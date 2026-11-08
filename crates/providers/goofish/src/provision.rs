//! First-use provisioning of the Goofish signing runtime.
//!
//! The release ships a `goofish-payload/` directory next to the
//! server executable: a Java 8 runtime and the compiled unidbg
//! signing harness (see `runtime/goofish/build-payload.sh`). The
//! one piece that cannot be shipped is the Goofish app package
//! itself (~269 MB, Alibaba-proprietary). On the first Goofish
//! call — normally from the server's background pre-warm, before
//! any MTOP request is even built — this module assembles the
//! runtime in the server's data directory:
//!
//! 1. download the pinned app package (idlefish 7.28.70, the build
//!    the harness emulates) from the app-store CDN URL recorded in
//!    `payload.json`, verifying its SHA-256;
//! 2. extract the SecurityGuard native libraries from it;
//! 3. generate the bundle's `serve.sh` / `serve.cmd`, pointing the
//!    harness at the payload's JRE and classes.
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
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::identity;

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
const DEFAULT_APK_SHA256: &str =
    "905718aff71859527c53b80c88a971fc47c7aa5e8c1a6d3d661b9e073b837530";
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PayloadInfo {
    pub dir: PathBuf,
    pub apk_version: String,
    pub apk_url: String,
    pub apk_sha256: String,
    pub apk_size: u64,
    pub main_so: String,
    pub jars: Vec<String>,
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
    let jars: Vec<String> = doc
        .get("harness")
        .and_then(|h| h.get("jars"))
        .and_then(|j| j.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if jars.is_empty() {
        return Err(format!(
            "payload.json in {} lists no harness jars",
            dir.display()
        ));
    }
    if !dir.join("jre").join("bin").is_dir() {
        return Err(format!("payload in {} has no jre/bin", dir.display()));
    }
    for jar in &jars {
        if !dir.join("harness").join("lib").join(jar).is_file() {
            return Err(format!(
                "payload in {} is missing harness/lib/{jar}",
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
        jars,
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

/// Runtime state check: marker written by a previous provisioning
/// run, the serve script, and an app package of the expected size.
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
    if marker.get("apk_sha256").and_then(|v| v.as_str()) != Some(payload.apk_sha256.as_str()) {
        return false;
    }
    if marker.get("payload_dir").and_then(|v| v.as_str())
        != Some(payload.dir.to_string_lossy().as_ref())
    {
        return false;
    }
    let apk = rt.join(APK_FILE_NAME);
    std::fs::metadata(&apk)
        .map(|m| m.is_file() && m.len() == payload.apk_size)
        .unwrap_or(false)
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
    std::fs::create_dir_all(rt.join("libs"))
        .and_then(|_| std::fs::create_dir_all(rt.join("logs")))
        .map_err(|e| format!("cannot create {}: {e}", rt.display()))?;

    // 1. The app package (download once; the harness re-verifies
    //    its SHA-256 at every boot via -DapkSha256).
    let apk_path = rt.join(APK_FILE_NAME);
    let apk_ok = std::fs::metadata(&apk_path)
        .map(|m| m.is_file() && m.len() == payload.apk_size)
        .unwrap_or(false);
    if !apk_ok {
        download_apk(&payload, &apk_path)?;
    }

    // 2. SecurityGuard libraries out of the package.
    let so_path = rt.join("libs").join(&payload.main_so);
    if !so_path.is_file() {
        extract_libraries(&apk_path, &rt.join("libs"))?;
    }
    if !so_path.is_file() {
        return Err(format!(
            "the downloaded app package did not contain {}",
            payload.main_so
        ));
    }

    // 3. The serve script + marker (regenerated whenever any input
    //    above changed, so a moved install re-points itself).
    write_serve_script(&rt, &payload)?;
    let marker = json!({
        "format": 1,
        "apk_version": payload.apk_version,
        "apk_sha256": payload.apk_sha256,
        "payload_dir": payload.dir.to_string_lossy(),
        "main_so": payload.main_so,
    });
    std::fs::write(
        rt.join(MARKER_FILE_NAME),
        format!("{}\n", serde_json::to_string_pretty(&marker).unwrap()),
    )
    .map_err(|e| format!("cannot write the provisioning marker: {e}"))?;
    log(&format!(
        "signing runtime ready in {} (app package {}, sha256 verified)",
        rt.display(),
        payload.apk_version
    ));
    Ok(rt)
}

fn download_apk(payload: &PayloadInfo, dest: &Path) -> Result<(), String> {
    let part = dest.with_extension("apk.part");
    let _ = std::fs::remove_file(&part);
    log(&format!(
        "first Goofish use: downloading the Goofish app package ({:.0} MB) from the app-store CDN",
        payload.apk_size as f64 / 1_000_000.0
    ));
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(1800))
        .build()
        .map_err(|e| format!("cannot build the download client: {e}"))?;
    let mut resp = client
        .get(&payload.apk_url)
        .send()
        .map_err(|e| format!("app package download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "app package download failed: HTTP {}",
            resp.status()
        ));
    }
    let mut file = std::fs::File::create(&part)
        .map_err(|e| format!("cannot create {}: {e}", part.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    let mut written: u64 = 0;
    let mut next_log: u64 = 32 * 1024 * 1024;
    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| format!("app package download failed mid-stream: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| format!("cannot write {}: {e}", part.display()))?;
        written += n as u64;
        if written >= next_log {
            log(&format!(
                "app package download: {written} / {} bytes",
                payload.apk_size
            ));
            next_log += 32 * 1024 * 1024;
        }
    }
    drop(file);
    if written != payload.apk_size {
        let _ = std::fs::remove_file(&part);
        return Err(format!(
            "app package download incomplete: got {written} bytes, expected {}",
            payload.apk_size
        ));
    }
    let digest = format!("{:x}", hasher.finalize());
    if digest != payload.apk_sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(format!(
            "app package failed SHA-256 verification (expected {}, got {digest}); refusing to use it",
            payload.apk_sha256
        ));
    }
    std::fs::rename(&part, dest)
        .map_err(|e| format!("cannot move the app package into place: {e}"))?;
    log("app package downloaded and SHA-256 verified");
    Ok(())
}

// ---------------------------------------------------------------------------
// Minimal ZIP reader (central directory), for pulling the .so files
// out of the app package. Stored and deflated entries only — all an
// APK uses. Sizes come from the central directory, so entries with
// data descriptors are handled.
// ---------------------------------------------------------------------------

struct ZipEntry {
    name: String,
    method: u16,
    comp_size: u64,
    local_offset: u64,
}

fn zip_entries(file: &mut std::fs::File) -> Result<Vec<ZipEntry>, String> {
    let len = file
        .metadata()
        .map_err(|e| format!("cannot stat the app package: {e}"))?
        .len();
    let tail_len = len.min(65_557);
    if tail_len < 22 {
        return Err("the app package is too small to be a ZIP".to_string());
    }
    let mut tail = vec![0u8; tail_len as usize];
    file.seek(SeekFrom::Start(len - tail_len))
        .and_then(|_| file.read_exact(&mut tail))
        .map_err(|e| format!("cannot read the app package: {e}"))?;
    let eocd = (0..tail_len as usize - 21)
        .rev()
        .find(|&i| tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or("the app package has no ZIP end-of-central-directory")?;
    let u32_at = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64;
    let cd_size = u32_at(&tail[eocd + 12..eocd + 16]);
    let cd_offset = u32_at(&tail[eocd + 16..eocd + 20]);
    let mut cd = vec![0u8; cd_size as usize];
    file.seek(SeekFrom::Start(cd_offset))
        .and_then(|_| file.read_exact(&mut cd))
        .map_err(|e| format!("cannot read the app package directory: {e}"))?;
    let mut entries = Vec::new();
    let mut i = 0usize;
    while i + 46 <= cd.len() {
        if cd[i..i + 4] != [0x50, 0x4b, 0x01, 0x02] {
            break;
        }
        let u16_at = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]) as usize;
        let method = u16::from_le_bytes([cd[i + 10], cd[i + 11]]);
        let name_len = u16_at(&cd[i + 28..i + 30]);
        let extra_len = u16_at(&cd[i + 30..i + 32]);
        let comment_len = u16_at(&cd[i + 32..i + 34]);
        let name = String::from_utf8_lossy(&cd[i + 46..i + 46 + name_len]).into_owned();
        entries.push(ZipEntry {
            name,
            method,
            comp_size: u32_at(&cd[i + 20..i + 24]),
            local_offset: u32_at(&cd[i + 42..i + 46]),
        });
        i += 46 + name_len + extra_len + comment_len;
    }
    Ok(entries)
}

fn zip_extract_one(
    file: &mut std::fs::File,
    entry: &ZipEntry,
    dest: &Path,
) -> Result<(), String> {
    let mut lh = [0u8; 30];
    file.seek(SeekFrom::Start(entry.local_offset))
        .and_then(|_| file.read_exact(&mut lh))
        .map_err(|e| format!("cannot read {} in the app package: {e}", entry.name))?;
    if lh[0..4] != [0x50, 0x4b, 0x03, 0x04] {
        return Err(format!("{} has a bad local header", entry.name));
    }
    let name_len = u16::from_le_bytes([lh[26], lh[27]]) as u64;
    let extra_len = u16::from_le_bytes([lh[28], lh[29]]) as u64;
    let data_start = entry.local_offset + 30 + name_len + extra_len;
    file.seek(SeekFrom::Start(data_start))
        .map_err(|e| format!("cannot seek in the app package: {e}"))?;
    let limited = file.take(entry.comp_size);
    let mut out = std::fs::File::create(dest)
        .map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    match entry.method {
        0 => {
            let mut r = limited;
            std::io::copy(&mut r, &mut out)
                .map_err(|e| format!("cannot extract {}: {e}", entry.name))?;
        }
        8 => {
            let mut decoder = flate2::read::DeflateDecoder::new(limited);
            std::io::copy(&mut decoder, &mut out)
                .map_err(|e| format!("cannot extract {}: {e}", entry.name))?;
        }
        other => return Err(format!("{} uses unsupported ZIP method {other}", entry.name)),
    }
    Ok(())
}

fn extract_libraries(apk_path: &Path, libs_dir: &Path) -> Result<(), String> {
    let mut file = std::fs::File::open(apk_path)
        .map_err(|e| format!("cannot open {}: {e}", apk_path.display()))?;
    let entries = zip_entries(&mut file)?;
    let mut extracted = 0usize;
    for entry in &entries {
        let Some(base) = entry.name.rsplit('/').next() else {
            continue;
        };
        if !entry.name.contains("/arm64-v8a/") {
            continue;
        }
        if !SO_PREFIXES.iter().any(|p| base.starts_with(p)) {
            continue;
        }
        zip_extract_one(&mut file, entry, &libs_dir.join(base))?;
        extracted += 1;
    }
    if extracted == 0 {
        return Err("the app package contains no SecurityGuard libraries".to_string());
    }
    log(&format!(
        "extracted {extracted} SecurityGuard libraries from the app package"
    ));
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
    let mut cp = format!(
        "{}/harness/classes-patched:{}/harness/classes",
        payload_str, payload_str
    );
    for jar in &payload.jars {
        cp.push_str(&format!(":{}/harness/lib/{jar}", payload_str));
    }
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
         exec \"$PAYLOAD/jre/bin/java\" \\\n\
         \x20 -Dapk=\"$RT/{apk}\" \\\n\
         \x20 -DapkSha256={sha} \\\n\
         \x20 -Dso=\"$RT/libs/{so}\" \\\n\
         \x20 -Xmx512m -cp \"{cp_esc}\" local.goofish.SgHarness serve \\\n\
         \x20 2>>\"$RT/logs/serve_stderr.log\"\n",
        payload_esc = sh_escape(&payload_str),
        apk = APK_FILE_NAME,
        sha = payload.apk_sha256,
        so = payload.main_so,
        cp_esc = sh_escape(&cp),
    )
}

pub(crate) fn serve_script_windows(rt: &Path, payload: &PayloadInfo) -> String {
    let payload_str = payload.dir.to_string_lossy().replace('/', "\\");
    let mut cp = format!(
        "{}\\harness\\classes-patched;{}\\harness\\classes",
        payload_str, payload_str
    );
    for jar in &payload.jars {
        cp.push_str(&format!(";{}\\harness\\lib\\{jar}", payload_str));
    }
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
         \"%PAYLOAD%\\jre\\bin\\java.exe\" -Dapk=\"%RT%{apk}\" -DapkSha256={sha} -Dso=\"%RT%libs\\{so}\" -Xmx512m -cp \"{cp}\" local.goofish.SgHarness serve 2>>\"%RT%logs\\serve_stderr.log\"\n",
        apk = APK_FILE_NAME,
        sha = payload.apk_sha256,
        so = payload.main_so,
        cp = cp,
    )
}

fn write_serve_script(rt: &Path, payload: &PayloadInfo) -> Result<(), String> {
    let path = rt.join(serve_file_name());
    let body = if cfg!(windows) {
        serve_script_windows(rt, payload)
    } else {
        serve_script_unix(rt, payload)
    };
    std::fs::write(&path, body)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
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
            jars: vec!["a.jar".to_string(), "b.jar".to_string()],
        }
    }

    #[test]
    fn payload_json_round_trip() {
        let dir = std::env::temp_dir().join(format!("gf_prov_test_{}", std::process::id()));
        let payload_dir = dir.join("goofish-payload");
        std::fs::create_dir_all(payload_dir.join("jre").join("bin")).unwrap();
        std::fs::create_dir_all(payload_dir.join("harness").join("lib")).unwrap();
        std::fs::write(payload_dir.join("harness").join("lib").join("a.jar"), b"x").unwrap();
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
        let info = load_payload(&payload_dir).unwrap();
        assert_eq!(info.apk_sha256, DEFAULT_APK_SHA256);
        assert_eq!(info.apk_size, DEFAULT_APK_SIZE);
        assert_eq!(info.jars, vec!["a.jar".to_string()]);
        // A payload whose jar is gone is refused, not half-used.
        std::fs::remove_file(payload_dir.join("harness").join("lib").join("a.jar")).unwrap();
        assert!(load_payload(&payload_dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serve_script_unix_shape() {
        let payload = fake_payload(Path::new("/opt/zero search/goofish-payload"));
        let script = serve_script_unix(Path::new("/data/goofish-runtime"), &payload);
        // Patched classes precede the harness classes, which precede
        // the dependency jars (the verified classpath order).
        let patched = script.find("classes-patched").unwrap();
        let classes = script.find("classes:").unwrap();
        let jar = script.find("lib/a.jar").unwrap();
        assert!(patched < classes && classes < jar, "{script}");
        assert!(script.contains(&format!("-DapkSha256={DEFAULT_APK_SHA256}")));
        assert!(script.contains("-Dso=\"$RT/libs/libsgmainso-6.7.260202.so\""));
        assert!(script.contains("\"$PAYLOAD/jre/bin/java\""));
        // The space in the payload path is safe inside the quoting.
        assert!(script.contains("PAYLOAD=\"/opt/zero search/goofish-payload\""));
    }

    #[test]
    fn zip_extract_stored_and_deflated() {
        // Build a tiny ZIP in memory with one stored and one
        // deflated entry and pull them back out by name.
        use flate2::write::DeflateEncoder;
        use flate2::Compression;
        let dir = std::env::temp_dir().join(format!("gf_zip_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let apk = dir.join("mini.apk");
        let stored_data = b"STORED-DATA".to_vec();
        let deflated_data = b"DEFLATED-DATA-DEFLATED-DATA".to_vec();
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&deflated_data).unwrap();
        let deflated_comp = enc.finish().unwrap();
        // (name, method, data-as-stored)
        let entries: Vec<(&str, u16, Vec<u8>)> = vec![
            ("lib/arm64-v8a/libsgmainso-1.so", 0, stored_data.clone()),
            ("lib/arm64-v8a/libsgcore.so", 8, deflated_comp.clone()),
            ("classes.dex", 0, b"IGNORED".to_vec()),
        ];
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, method, data) in &entries {
            let offset = out.len() as u32;
            let crc = 0u32; // the reader under test never checks CRC
            out.extend_from_slice(&0x04034b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes()); // version
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // time
            out.extend_from_slice(&0u16.to_le_bytes()); // date
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            let plain_len = if *method == 8 {
                deflated_data.len()
            } else {
                data.len()
            };
            out.extend_from_slice(&(plain_len as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            // Central directory entry, field offsets per APPNOTE:
            // method @10, comp size @20, size @24, name len @28,
            // extra @30, comment @32, local header offset @42.
            central.extend_from_slice(&0x02014b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes()); // version made by
            central.extend_from_slice(&20u16.to_le_bytes()); // version needed
            central.extend_from_slice(&0u16.to_le_bytes()); // flags
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // time
            central.extend_from_slice(&0u16.to_le_bytes()); // date
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(plain_len as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // extra len
            central.extend_from_slice(&0u16.to_le_bytes()); // comment len
            central.extend_from_slice(&0u16.to_le_bytes()); // disk number
            central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x06054b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        std::fs::write(&apk, &out).unwrap();

        let libs = dir.join("libs");
        std::fs::create_dir_all(&libs).unwrap();
        extract_libraries(&apk, &libs).unwrap();
        assert_eq!(
            std::fs::read(libs.join("libsgmainso-1.so")).unwrap(),
            stored_data
        );
        assert_eq!(
            std::fs::read(libs.join("libsgcore.so")).unwrap(),
            deflated_data
        );
        assert!(!libs.join("classes.dex").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
