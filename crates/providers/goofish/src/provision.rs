//! First-use provisioning of the Goofish signing runtime.
//!
//! The release ships a `goofish-payload/` directory next to the
//! server executable: the Rust signing harness (`sgharness`, which
//! runs the app's own SecurityGuard native library under the
//! Unicorn engine — no JVM) and the Android system libraries it
//! loads the library against (see `runtime/goofish/build-payload.sh`).
//! The one piece that cannot be shipped is the Goofish app package
//! itself (~269 MB, Alibaba-proprietary) — but the runtime uses only
//! a few MB of it. On the first Goofish call — normally from the
//! server's background pre-warm, before any MTOP request is even
//! built — this module assembles the runtime in the server's data
//! directory:
//!
//! 1. fetch, with HTTP range requests against the app-store CDN URL
//!    recorded in `payload.json` (idlefish 7.28.70, the build the
//!    harness emulates), only the package's central directory, the
//!    SecurityGuard libraries, SecurityGuard's configuration image
//!    (`res/drawable/yw_1222.jpg`), the v1 certificate and the APK
//!    Signing Block — about 3 MB in all (see `slimapk`), each piece
//!    checked against a pinned SHA-256;
//! 2. extract the SecurityGuard native libraries into `libs/`;
//! 3. stage the harness's root filesystem with a slim package (the
//!    image, the certificate and the signing block, ~55 KB) at the
//!    app's install path — it signs byte-identically to the full
//!    package;
//! 4. generate the bundle's `serve.sh` / `serve.cmd`, pointing the
//!    harness at the payload's signer binary and system libraries.
//!
//! A runtime assembled by an older version (which downloaded the
//! whole package) is re-assembled from its local `goofish.apk`
//! without any network access.
//!
//! The assembled bundle lands in the same data-directory location
//! the resolver has always discovered (`<data dir>/goofish-runtime/
//! serve.sh`), so no discovery behaviour changes. Provisioning is
//! idempotent, runs at most once per process (later calls re-check
//! a marker file), needs no user action, configuration file or
//! environment variable, and only ever runs when a payload is
//! actually present — deployments without one (developer builds,
//! explicit `GOOFISH_SG_SERVE` setups) are untouched.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::identity;
use crate::slimapk;

pub(crate) const PAYLOAD_DIR_NAME: &str = "goofish-payload";
pub(crate) const RUNTIME_DIR_NAME: &str = "goofish-runtime";
const APK_FILE_NAME: &str = "goofish.apk";
const MARKER_FILE_NAME: &str = "provision.json";

/// Fallback app-package coordinates, used only when a payload's
/// `payload.json` is missing fields. `build-payload.sh` writes the
/// same values into every payload it assembles; the two must move
/// together. The URL is Tencent's app-store CDN, content-addressed
/// by the package's MD5 — it serves exactly this build (verified
/// 2026-11-08: the full download hashes to `apk_sha256`).
const DEFAULT_APK_VERSION: &str = "7.28.70";
const DEFAULT_APK_URL: &str =
    "https://imtt.dd.qq.com/16891/apk/361C15B2C9BC291581DBE4FF8194A899.apk";
const DEFAULT_APK_SHA256: &str = "905718aff71859527c53b80c88a971fc47c7aa5e8c1a6d3d661b9e073b837530";
const DEFAULT_APK_SIZE: u64 = 268_743_306;
const DEFAULT_MAIN_SO: &str = "libsgmainso-6.7.260202.so";
/// APK entries extracted into the runtime's `libs/`: the
/// SecurityGuard library set the harness and the plugin load (the
/// same set `bootstrap.sh` extracted).
const SO_PREFIXES: [&str; 5] = [
    "libsgmainso",
    "libsgmain.so",
    "libsgcore",
    "libsgmisc",
    "libsgnocaptcha",
];

/// Marker value naming the signer generation a runtime was
/// assembled for. Runtimes assembled by the retired JVM payload
/// carry no such field (or a different one) and are re-assembled:
/// their `serve.sh` points into a payload layout that no longer
/// exists. The app package itself is reused — re-assembly only
/// rewrites the scripts and the staged root filesystem.
pub(crate) const SIGNER_KIND: &str = "rust-sgharness";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PayloadInfo {
    pub dir: PathBuf,
    pub apk_version: String,
    pub apk_url: String,
    pub apk_sha256: String,
    pub apk_size: u64,
    pub main_so: String,
    /// File name of the signer binary inside `signer/`
    /// (`sgharness`, or `sgharness.exe` on Windows).
    pub signer_binary: String,
    /// Android system libraries inside `syslibs/` that the signer
    /// loads the SecurityGuard library against.
    pub syslibs: Vec<String>,
}

fn log(msg: &str) {
    eprintln!("[goofish] {msg}");
}

/// The payload directory shipped next to the server executable, if
/// present and readable.
pub(crate) fn payload_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.join(PAYLOAD_DIR_NAME);
    if dir.join("payload.json").is_file() {
        Some(dir)
    } else {
        None
    }
}

pub(crate) fn payload_available() -> bool {
    payload_dir().is_some()
}

/// Read and validate `payload.json` in `dir`.
pub(crate) fn load_payload(dir: &Path) -> Result<PayloadInfo, String> {
    let text = std::fs::read_to_string(dir.join("payload.json"))
        .map_err(|e| format!("cannot read {}: {e}", dir.join("payload.json").display()))?;
    let doc: Value = serde_json::from_str(&text)
        .map_err(|e| format!("payload.json in {} is malformed: {e}", dir.display()))?;
    let pick_str = |key: &str| -> Option<String> {
        doc.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    };
    let apk = doc.get("apk");
    let apk_str = |key: &str| -> Option<String> {
        apk.and_then(|a| a.get(key))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    };
    let signer = doc.get("signer");
    if signer.is_none() {
        // A payload without a signer section is the retired JVM
        // layout (jre/ + harness/); it cannot drive this server.
        return Err(format!(
            "payload in {} is not a Rust-signer payload (no signer section)",
            dir.display()
        ));
    }
    let signer_binary = signer
        .and_then(|s| s.get("binary"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("payload.json in {} names no signer binary", dir.display()))?
        .to_string();
    if !dir.join("signer").join(&signer_binary).is_file() {
        return Err(format!(
            "payload in {} is missing signer/{signer_binary}",
            dir.display()
        ));
    }
    let syslibs: Vec<String> = signer
        .and_then(|s| s.get("syslibs"))
        .and_then(|j| j.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if syslibs.is_empty() {
        return Err(format!(
            "payload.json in {} lists no signer syslibs",
            dir.display()
        ));
    }
    for lib in &syslibs {
        if !dir.join("syslibs").join(lib).is_file() {
            return Err(format!(
                "payload in {} is missing syslibs/{lib}",
                dir.display()
            ));
        }
    }
    Ok(PayloadInfo {
        dir: dir.to_path_buf(),
        apk_version: apk_str("version").unwrap_or_else(|| DEFAULT_APK_VERSION.to_string()),
        apk_url: apk_str("url").unwrap_or_else(|| DEFAULT_APK_URL.to_string()),
        apk_sha256: apk_str("sha256")
            .unwrap_or_else(|| DEFAULT_APK_SHA256.to_string())
            .to_lowercase(),
        apk_size: apk
            .and_then(|a| a.get("size"))
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_APK_SIZE),
        main_so: pick_str("main_so").unwrap_or_else(|| DEFAULT_MAIN_SO.to_string()),
        signer_binary,
        syslibs,
    })
}

/// Where the assembled runtime lives (the data-directory bundle
/// location the resolver already discovers).
pub(crate) fn runtime_dir() -> Option<PathBuf> {
    identity::data_dir().map(|d| d.join(RUNTIME_DIR_NAME))
}

fn serve_file_name() -> &'static str {
    if cfg!(windows) {
        "serve.cmd"
    } else {
        "serve.sh"
    }
}

/// Where the staged app package lives inside the harness rootfs:
/// the install path the emulated environment reports for the app,
/// which the SecurityGuard init opens and verifies in place.
fn staged_apk_path(rt: &Path) -> PathBuf {
    rt.join("rootfs")
        .join("data")
        .join("app")
        .join("com.taobao.idlefish-1")
        .join("base.apk")
}

/// Package entries the signing runtime uses besides the libraries:
/// SecurityGuard's configuration image (it carries the encrypted
/// appKey table; without it init fails with
/// `SEC_ERROR_INIT_PARSE_USER_CONFIG_ERROR` = 125, or "succeeds"
/// with no appKey) and the v1 signing certificate. The APK Signing
/// Block is kept too. Measured 2026-11-17 against idlefish 7.28.70
/// under the Linux harness: a package holding only these (plus the
/// signing block) signs byte-identically to the full package; the
/// image alone without any certificate source yields no appKey.
const STAGED_ENTRIES: [&str; 2] = ["res/drawable/yw_1222.jpg", "META-INF/CERT.RSA"];

/// Which package entries are fetched: the SecurityGuard libraries
/// (extracted into `libs/`) and [`STAGED_ENTRIES`]. Everything else
/// (dex, resources, assets — over 98% of the package) is never
/// fetched.
fn slim_wanted(name: &str) -> bool {
    if let Some(base) = name.strip_prefix("lib/arm64-v8a/") {
        return SO_PREFIXES.iter().any(|p| base.starts_with(p));
    }
    STAGED_ENTRIES.contains(&name)
}

/// SHA-256 of every fetched entry (decompressed) and of the signing
/// block, for the pinned package [`DEFAULT_APK_SHA256`]. They replace
/// the whole-package SHA-256 check now that the whole package is no
/// longer downloaded: each piece used is still authenticated.
const PINNED_PARTS_SHA256: [(&str, &str); 11] = [
    (
        "META-INF/CERT.RSA",
        "6da9296d0db8ad816402c42f7c887b8bac06dbdaf0418b83c8025b14e380b2f3",
    ),
    (
        "lib/arm64-v8a/libsgcore.so",
        "2757f8a62a6ad9436d570d4b895db0e54e32fd93032a81d3f9ec266a7bde8957",
    ),
    (
        "lib/arm64-v8a/libsgmain.so",
        "396d9cda681f28ff2f92ae68c329f0967d27cb51dcdb3b3d33f38b0af1fe1921",
    ),
    (
        "lib/arm64-v8a/libsgmainso-6.7.260202.so",
        "7d2ae8a83d8b0e567805079b68f876397341330d3efbd2b86c5d957863843400",
    ),
    (
        "lib/arm64-v8a/libsgmainso.version.so",
        "ec25ccafe8896e3b55a4ccd014790a7b98f6c2274822035aaeb4222a8c252a71",
    ),
    (
        "lib/arm64-v8a/libsgmisc.so",
        "deb277ae144520853ea779516a38657c4279c4e00976268c4417bd890c617a1c",
    ),
    (
        "lib/arm64-v8a/libsgmiscso-6.5.9.so",
        "d1f6c43da92119f6b9183edfa818f01c22610ff4a0dd15658624ddd494a3bc3e",
    ),
    (
        "lib/arm64-v8a/libsgnocaptcha.so",
        "14de624829a30d72f9145f7c8ad88c34fddbad8ec3e0dc1747073e24562e8f86",
    ),
    (
        "lib/arm64-v8a/libsgnocaptchaso-6.5.8.so",
        "1b74eba2c62c0251110ba69637dbcf492c1ee0606243e4a2976d2c1120633e47",
    ),
    (
        "res/drawable/yw_1222.jpg",
        "deda6274114c3ad35c809df7bd8dc83a5c49c74fe48f3188a13fc13fd9382063",
    ),
    (
        SIGNING_BLOCK_KEY,
        "4914e849f5ff27a9037c96dbe7b4c1fb8669fcd7b4eb0591d439b9960482f389",
    ),
];
const SIGNING_BLOCK_KEY: &str = "<APK Signing Block>";

/// Check the fetched parts against [`PINNED_PARTS_SHA256`] when the
/// payload names the pinned package. Every pinned part must be
/// present and match; a payload naming another package build has
/// only the per-entry CRC-32 check.
fn verify_parts(
    payload: &PayloadInfo,
    kept: &[slimapk::KeptEntry],
    signing_block: &[u8],
) -> Result<(), String> {
    if payload.apk_sha256 != DEFAULT_APK_SHA256 {
        return Ok(());
    }
    for (name, want) in PINNED_PARTS_SHA256 {
        let got = if name == SIGNING_BLOCK_KEY {
            slimapk::sha256_hex(signing_block)
        } else {
            match kept.iter().find(|k| k.entry.name == name) {
                Some(k) => slimapk::sha256_hex(&k.plain),
                None => return Err(format!("the app package is missing {name}")),
            }
        };
        if got != want {
            return Err(format!(
                "{name} failed SHA-256 verification (expected {want}, got {got}); refusing to use it"
            ));
        }
    }
    Ok(())
}

/// Package layout recorded in the marker: the staged package is
/// the slim one assembled from range reads.
const APK_LAYOUT_SLIM: &str = "slim-v1";

/// Runtime state check: marker written by a previous provisioning
/// run for this package, signer generation and layout, the serve
/// script, the main library, and the staged package with the size
/// the marker recorded. A runtime assembled by an older version
/// (full package download, or the retired JVM payload) is
/// re-assembled; its downloaded package is reused as the source, so
/// nothing is fetched again.
fn runtime_complete(rt: &Path, payload: &PayloadInfo) -> bool {
    if !rt.join(serve_file_name()).is_file() {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(rt.join(MARKER_FILE_NAME)) else {
        return false;
    };
    let Ok(marker) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    let field = |k: &str| marker.get(k).and_then(|v| v.as_str()).map(str::to_string);
    if field("apk_sha256").as_deref() != Some(payload.apk_sha256.as_str())
        || field("payload_dir").as_deref() != Some(payload.dir.to_string_lossy().as_ref())
        || field("signer").as_deref() != Some(SIGNER_KIND)
        || field("apk_layout").as_deref() != Some(APK_LAYOUT_SLIM)
    {
        return false;
    }
    let Some(staged_size) = marker.get("staged_size").and_then(|v| v.as_u64()) else {
        return false;
    };
    let staged_ok = std::fs::metadata(staged_apk_path(rt))
        .map(|m| m.is_file() && m.len() == staged_size)
        .unwrap_or(false);
    staged_ok && rt.join("libs").join(&payload.main_so).is_file()
}

/// True when `serve_command` is the serve script of the runtime this
/// module assembles in the data directory, a payload is shipped, and
/// that runtime is not the current one (an older version's
/// full-package or JVM-era runtime, whose serve script is still
/// present). Without this check the old script was discovered first
/// and the runtime was never re-assembled.
pub(crate) fn data_dir_runtime_stale(serve_command: &str) -> bool {
    let (Some(dir), Some(rt)) = (payload_dir(), runtime_dir()) else {
        return false;
    };
    let Ok(payload) = load_payload(&dir) else {
        return false;
    };
    let in_runtime = Path::new(serve_command)
        .parent()
        .is_some_and(|p| same_dir(p, &rt));
    in_runtime && !runtime_complete(&rt, &payload)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

static PROVISION_LOCK: Mutex<()> = Mutex::new(());

/// Assemble the runtime in the data directory. Idempotent and
/// serialised: concurrent callers (pre-warm thread, first live
/// call) wait for the one run and then re-check its result.
pub(crate) fn ensure_provisioned() -> Result<PathBuf, String> {
    let _guard = PROVISION_LOCK.lock().unwrap();
    let Some(payload_dir) = payload_dir() else {
        return Err(format!(
            "no {PAYLOAD_DIR_NAME} directory next to the server executable"
        ));
    };
    let payload = load_payload(&payload_dir)?;
    let Some(rt) = runtime_dir() else {
        return Err("no data directory available for the Goofish runtime".to_string());
    };
    if runtime_complete(&rt, &payload) {
        return Ok(rt);
    }
    assemble(&payload, &rt)?;
    Ok(rt)
}

/// Assemble (or re-assemble) the runtime for `payload` in `rt`.
fn assemble(payload: &PayloadInfo, rt: &Path) -> Result<(), String> {
    let rt = rt.to_path_buf();
    std::fs::create_dir_all(rt.join("libs"))
        .and_then(|_| std::fs::create_dir_all(rt.join("logs")))
        .map_err(|e| format!("cannot create {}: {e}", rt.display()))?;

    // 1. The needed entries + signing block. A full package left by
    //    an older version is read locally; otherwise only the needed
    //    byte ranges are fetched from the CDN (a few MB instead of
    //    the whole package).
    let legacy_apk = rt.join(APK_FILE_NAME);
    let legacy_ok = std::fs::metadata(&legacy_apk)
        .map(|m| m.is_file() && m.len() == payload.apk_size)
        .unwrap_or(false);
    let (kept, signing_block) = if legacy_ok {
        let file = std::fs::File::open(&legacy_apk)
            .map_err(|e| format!("cannot open {}: {e}", legacy_apk.display()))?;
        slimapk::fetch_entries(&mut slimapk::FileRanges(file), &slim_wanted)?
    } else {
        log(&format!(
            "first Goofish use: fetching the signing-relevant parts of the Goofish app package {} \
             (range requests; the {:.0} MB package itself is not downloaded)",
            payload.apk_version,
            payload.apk_size as f64 / 1_000_000.0
        ));
        let mut src = slimapk::HttpRanges::new(&payload.apk_url, payload.apk_size)?;
        let out = slimapk::fetch_entries(&mut src, &slim_wanted)?;
        log(&format!(
            "fetched {:.1} MB of the app package",
            src.fetched as f64 / 1_000_000.0
        ));
        out
    };
    verify_parts(payload, &kept, &signing_block)?;

    // 2. SecurityGuard libraries.
    let mut extracted = 0usize;
    for k in &kept {
        if let Some(base) = k.entry.name.strip_prefix("lib/arm64-v8a/") {
            let dest = rt.join("libs").join(base);
            std::fs::write(&dest, &k.plain)
                .map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
            extracted += 1;
        }
    }
    let main_so = rt.join("libs").join(&payload.main_so);
    if extracted == 0 || !main_so.is_file() {
        return Err(format!(
            "the app package did not contain {}",
            payload.main_so
        ));
    }

    // 3. The harness root filesystem with the slim package (the
    //    configuration image, the certificate and the signing block;
    //    the libraries are loaded from `libs/`) staged at the app's
    //    install path.
    let staged_entries: Vec<slimapk::KeptEntry> = kept
        .into_iter()
        .filter(|k| STAGED_ENTRIES.contains(&k.entry.name.as_str()))
        .collect();
    let slim = slimapk::build_zip(&staged_entries, &signing_block);
    stage_rootfs(&rt, &slim)?;

    // 4. The serve script + marker.
    write_serve_script(&rt, payload)?;
    let marker = json!({
        "format": 3,
        "signer": SIGNER_KIND,
        "apk_layout": APK_LAYOUT_SLIM,
        "apk_version": payload.apk_version,
        "apk_sha256": payload.apk_sha256,
        "staged_size": slim.len(),
        "staged_sha256": slimapk::sha256_hex(&slim),
        "payload_dir": payload.dir.to_string_lossy(),
        "main_so": payload.main_so,
    });
    std::fs::write(
        rt.join(MARKER_FILE_NAME),
        format!("{}\n", serde_json::to_string_pretty(&marker).unwrap()),
    )
    .map_err(|e| format!("cannot write the provisioning marker: {e}"))?;
    log(&format!(
        "signing runtime ready in {} (app package {}, {} libraries + {} staged entries, verified)",
        rt.display(),
        payload.apk_version,
        extracted,
        staged_entries.len()
    ));
    Ok(())
}

/// Stage the harness root filesystem: the directory skeleton the
/// emulated environment expects, with the slim package written at
/// the app's install path. The signer only reads the staged package
/// — SecurityGuard opens it to read the app's signing certificate —
/// and writes its own state under the app's data dirs.
fn stage_rootfs(rt: &Path, package: &[u8]) -> Result<(), String> {
    let rootfs = rt.join("rootfs");
    let app_dir = rootfs
        .join("data")
        .join("app")
        .join("com.taobao.idlefish-1");
    let data_dir = rootfs.join("data").join("data").join("com.taobao.idlefish");
    for dir in [
        app_dir.clone(),
        data_dir.join("files"),
        data_dir.join("cache"),
        rootfs.join("tmp"),
        rootfs.join("system"),
    ] {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let staged = app_dir.join("base.apk");
    // Remove first: an older runtime's staged copy is a hard link to
    // the full package, and writing through it would truncate that.
    let _ = std::fs::remove_file(&staged);
    std::fs::write(&staged, package)
        .map_err(|e| format!("cannot stage the app package into the rootfs: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Serve script generation
// ---------------------------------------------------------------------------

fn sh_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

pub(crate) fn serve_script_unix(rt: &Path, payload: &PayloadInfo) -> String {
    let payload_str = payload.dir.to_string_lossy();
    let _ = rt;
    format!(
        "#!/bin/sh\n\
         # Assembled by zero-search-mcp on first Goofish use — do not edit;\n\
         # it is regenerated when the bundled signing payload changes.\n\
         set -u\n\
         RT=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n\
         PAYLOAD=\"{payload_esc}\"\n\
         mkdir -p \"$RT/logs\" \"$RT/rootfs\"\n\
         cd \"$RT\"\n\
         exec \"$PAYLOAD/signer/{bin}\" serve \\\n\
         \x20 --rootfs \"$RT/rootfs\" \\\n\
         \x20 --sg \"$RT/libs/{so}\" \\\n\
         \x20 --syslibs \"$PAYLOAD/syslibs\" \\\n\
         \x20 2>>\"$RT/logs/serve_stderr.log\"\n",
        payload_esc = sh_escape(&payload_str),
        bin = payload.signer_binary,
        so = payload.main_so,
    )
}

pub(crate) fn serve_script_windows(rt: &Path, payload: &PayloadInfo) -> String {
    let payload_str = payload.dir.to_string_lossy().replace('/', "\\");
    let _ = rt;
    format!(
        "@echo off\n\
         rem Assembled by zero-search-mcp on first Goofish use — do not edit;\n\
         rem it is regenerated when the bundled signing payload changes.\n\
         set \"RT=%~dp0\"\n\
         set \"PAYLOAD={payload_str}\"\n\
         if not exist \"%RT%logs\" mkdir \"%RT%logs\"\n\
         if not exist \"%RT%rootfs\" mkdir \"%RT%rootfs\"\n\
         cd /d \"%RT%\"\n\
         \"%PAYLOAD%\\signer\\{bin}\" serve --rootfs \"%RT%rootfs\" --sg \"%RT%libs\\{so}\" --syslibs \"%PAYLOAD%\\syslibs\" 2>>\"%RT%logs\\serve_stderr.log\"\n",
        bin = payload.signer_binary,
        so = payload.main_so,
    )
}

fn write_serve_script(rt: &Path, payload: &PayloadInfo) -> Result<(), String> {
    let path = rt.join(serve_file_name());
    let body = if cfg!(windows) {
        serve_script_windows(rt, payload)
    } else {
        serve_script_unix(rt, payload)
    };
    std::fs::write(&path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)
            .map_err(|e| format!("cannot stat {}: {e}", path.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms)
            .map_err(|e| format!("cannot mark {} executable: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_payload(dir: &Path) -> PayloadInfo {
        PayloadInfo {
            dir: dir.to_path_buf(),
            apk_version: "7.28.70".to_string(),
            apk_url: DEFAULT_APK_URL.to_string(),
            apk_sha256: DEFAULT_APK_SHA256.to_string(),
            apk_size: DEFAULT_APK_SIZE,
            main_so: DEFAULT_MAIN_SO.to_string(),
            signer_binary: "sgharness".to_string(),
            syslibs: vec!["libc.so".to_string(), "libz.so".to_string()],
        }
    }

    #[test]
    fn payload_json_round_trip() {
        let dir = std::env::temp_dir().join(format!("gf_prov_test_{}", std::process::id()));
        let payload_dir = dir.join("goofish-payload");
        std::fs::create_dir_all(payload_dir.join("signer")).unwrap();
        std::fs::create_dir_all(payload_dir.join("syslibs")).unwrap();
        std::fs::write(payload_dir.join("signer").join("sgharness"), b"x").unwrap();
        std::fs::write(payload_dir.join("syslibs").join("libc.so"), b"x").unwrap();
        std::fs::write(
            payload_dir.join("payload.json"),
            json!({
                "format": 2,
                "platform": "linux-x64",
                "signer": {"kind": SIGNER_KIND, "binary": "sgharness",
                           "syslibs": ["libc.so"]},
                "apk": {"version": "7.28.70", "url": DEFAULT_APK_URL,
                        "sha256": DEFAULT_APK_SHA256, "size": DEFAULT_APK_SIZE},
                "main_so": DEFAULT_MAIN_SO,
            })
            .to_string(),
        )
        .unwrap();
        let info = load_payload(&payload_dir).unwrap();
        assert_eq!(info.apk_sha256, DEFAULT_APK_SHA256);
        assert_eq!(info.apk_size, DEFAULT_APK_SIZE);
        assert_eq!(info.signer_binary, "sgharness");
        assert_eq!(info.syslibs, vec!["libc.so".to_string()]);
        // A payload whose syslib is gone is refused, not half-used.
        std::fs::remove_file(payload_dir.join("syslibs").join("libc.so")).unwrap();
        assert!(load_payload(&payload_dir).is_err());
        // The retired JVM layout (no signer section) is refused too.
        std::fs::write(
            payload_dir.join("payload.json"),
            json!({
                "format": 1,
                "platform": "linux-x64",
                "apk": {"version": "7.28.70", "url": DEFAULT_APK_URL,
                        "sha256": DEFAULT_APK_SHA256, "size": DEFAULT_APK_SIZE},
                "main_so": DEFAULT_MAIN_SO,
                "harness": {"main_class": "local.goofish.SgHarness", "jars": ["a.jar"]},
            })
            .to_string(),
        )
        .unwrap();
        assert!(load_payload(&payload_dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serve_script_unix_shape() {
        let payload = fake_payload(Path::new("/opt/zero search/goofish-payload"));
        let script = serve_script_unix(Path::new("/data/goofish-runtime"), &payload);
        // The signer binary is exec'd with the runtime's rootfs, the
        // extracted SecurityGuard library, and the payload's syslibs.
        assert!(
            script.contains("exec \"$PAYLOAD/signer/sgharness\" serve"),
            "{script}"
        );
        assert!(script.contains("--rootfs \"$RT/rootfs\""), "{script}");
        assert!(
            script.contains("--sg \"$RT/libs/libsgmainso-6.7.260202.so\""),
            "{script}"
        );
        assert!(
            script.contains("--syslibs \"$PAYLOAD/syslibs\""),
            "{script}"
        );
        // The space in the payload path is safe inside the quoting.
        assert!(script.contains("PAYLOAD=\"/opt/zero search/goofish-payload\""));
        // No JVM anywhere in the generated script.
        assert!(!script.contains("java"), "{script}");
    }

    #[test]
    fn serve_script_windows_shape() {
        let payload = fake_payload(Path::new("C:\\zero search\\goofish-payload"));
        let script = serve_script_windows(Path::new("D:\\data\\goofish-runtime"), &payload);
        assert!(script.contains("\\signer\\sgharness"), "{script}");
        assert!(script.contains("--rootfs \"%RT%rootfs\""), "{script}");
        assert!(
            script.contains("libs\\libsgmainso-6.7.260202.so"),
            "{script}"
        );
        assert!(!script.contains("java"), "{script}");
    }

    #[test]
    fn stage_rootfs_places_the_package() {
        let dir = std::env::temp_dir().join(format!("gf_stage_test_{}", std::process::id()));
        let rt = dir.join("goofish-runtime");
        std::fs::create_dir_all(&rt).unwrap();
        // An older runtime's staged copy is a hard link to the full
        // package: re-staging must not write through it.
        std::fs::write(rt.join(APK_FILE_NAME), b"FULLPACKAGE").unwrap();
        stage_rootfs(&rt, b"OLD").unwrap();
        let staged = staged_apk_path(&rt);
        std::fs::remove_file(&staged).unwrap();
        std::fs::hard_link(rt.join(APK_FILE_NAME), &staged).unwrap();
        stage_rootfs(&rt, b"SLIM").unwrap();
        assert_eq!(std::fs::read(&staged).unwrap(), b"SLIM");
        assert_eq!(
            std::fs::read(rt.join(APK_FILE_NAME)).unwrap(),
            b"FULLPACKAGE"
        );
        assert!(rt
            .join("rootfs")
            .join("data")
            .join("data")
            .join("com.taobao.idlefish")
            .join("files")
            .is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Assemble a runtime from a real payload. Set
    /// GOOFISH_TEST_PAYLOAD (a goofish-payload directory) and
    /// GOOFISH_TEST_RT (the runtime directory to assemble; put the
    /// full package there as goofish.apk to assemble offline, or
    /// leave it out to fetch the parts from the CDN).
    #[test]
    #[ignore = "needs a real payload; fetches from the CDN unless a local package is provided"]
    fn assemble_from_real_payload() {
        let payload =
            load_payload(Path::new(&std::env::var("GOOFISH_TEST_PAYLOAD").unwrap())).unwrap();
        let rt = PathBuf::from(std::env::var("GOOFISH_TEST_RT").unwrap());
        assemble(&payload, &rt).unwrap();
        assert!(runtime_complete(&rt, &payload));
    }

    #[test]
    fn slim_selection() {
        assert!(slim_wanted("lib/arm64-v8a/libsgmainso-6.7.260202.so"));
        assert!(slim_wanted("lib/arm64-v8a/libsgcore.so"));
        assert!(slim_wanted("META-INF/CERT.RSA"));
        assert!(slim_wanted("res/drawable/yw_1222.jpg"));
        assert!(!slim_wanted("AndroidManifest.xml"));
        assert!(!slim_wanted("META-INF/CERT.SF"));
        assert!(!slim_wanted("lib/armeabi-v7a/libsgmainso-6.7.260202.so"));
        assert!(!slim_wanted("lib/arm64-v8a/libflutter.so"));
        assert!(!slim_wanted("classes.dex"));
        assert!(!slim_wanted("resources.arsc"));
    }
}
