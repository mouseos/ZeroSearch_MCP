//! Signer provider abstraction — port of `goofish_client/signing.py`
//! and `goofish_client/unidbg_signer.py`. The SecurityGuard final
//! computation is native and is NOT reimplemented here; this module
//! assembles the INPUT string exactly and bridges to caller-owned
//! external signing processes. No fake signature is ever produced.

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::protocol::PUBLIC_APP_KEY;

// ---------------------------------------------------------------------------
// SignRequest / SignResult
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SignRequest {
    pub api: String,
    pub version: String,
    /// The request JSON string (the plan's `data_json()`).
    pub data: String,
    pub timestamp: String,
    pub utdid: Option<String>,
    pub device_id: Option<String>,
    pub ttid: Option<String>,
    pub uid: Option<String>,
    pub sid: Option<String>,
    pub app_key: String,
    pub reqbiz_ext: Option<String>,
    pub lat: Option<String>,
    pub lng: Option<String>,
    pub extdata: Option<String>,
    pub x_features: Option<String>,
    pub router_id: Option<String>,
    pub place_id: Option<String>,
    pub open_biz: Option<String>,
    pub mini_app_key: Option<String>,
    pub req_app_key: Option<String>,
    pub access_token: Option<String>,
    pub open_biz_data: Option<String>,
    /// Unified signing (pv 6.3) always includes the extdata slot;
    /// legacy signing (pv 6.2) includes it only when extdata non-empty.
    pub include_ext_data: bool,
    pub use_wua: bool,
    pub env: i64,
    pub request_id: Option<String>,
}

impl SignRequest {
    pub fn new(api: &str, version: &str, data: String, timestamp: String) -> Self {
        Self {
            api: api.to_string(),
            version: version.to_string(),
            data,
            timestamp,
            utdid: None,
            device_id: None,
            ttid: None,
            uid: None,
            sid: None,
            app_key: PUBLIC_APP_KEY.to_string(),
            reqbiz_ext: None,
            lat: None,
            lng: None,
            extdata: None,
            x_features: None,
            router_id: None,
            place_id: None,
            open_biz: None,
            mini_app_key: None,
            req_app_key: None,
            access_token: None,
            open_biz_data: None,
            include_ext_data: true,
            use_wua: false,
            env: 0,
            request_id: None,
        }
    }

    pub fn data_json(&self) -> String {
        self.data.clone()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SignResult {
    pub sign: String,
    pub x_umt: Option<String>,
    pub x_mini_wua: Option<String>,
    pub x_sgext: Option<String>,
    pub wua: Option<String>,
    pub extras: Map<String, Value>,
}

pub trait Signer: Send + Sync {
    fn sign(&self, request: &SignRequest) -> Result<SignResult>;
}

fn s(value: &Option<String>) -> String {
    value.clone().unwrap_or_default()
}

/// Assemble the SecurityGuard INPUT string (mtop-signing.md §2):
/// `&`-joined, null -> "", MD5 hex of the data JSON in place of data,
/// api/v lower-cased. Slot order:
/// utdid & uid & reqbiz-ext & appKey & MD5(data) & t & api & v & sid
/// & ttid & deviceId & lat & lng & [extdata &] & x-features & routerId
/// & placeId & openBiz & miniAppKey & reqAppKey & accessToken & openBizData
pub fn build_sign_input(request: &SignRequest, include_ext_data: Option<bool>) -> String {
    let include_ext_data = include_ext_data.unwrap_or(request.include_ext_data);
    let data_md5 = format!("{:x}", md5::compute(request.data_json().as_bytes()));
    let mut parts: Vec<String> = vec![
        s(&request.utdid),
        s(&request.uid),
        s(&request.reqbiz_ext),
        request.app_key.clone(),
        data_md5,
        request.timestamp.clone(),
        request.api.to_lowercase(),
        request.version.to_lowercase(),
        s(&request.sid),
        s(&request.ttid),
        s(&request.device_id),
        s(&request.lat),
        s(&request.lng),
    ];
    if include_ext_data
        || request
            .extdata
            .as_deref()
            .map(|e| !e.is_empty())
            .unwrap_or(false)
    {
        parts.push(s(&request.extdata));
    }
    parts.extend([
        s(&request.x_features),
        s(&request.router_id),
        s(&request.place_id),
        s(&request.open_biz),
        s(&request.mini_app_key),
        s(&request.req_app_key),
        s(&request.access_token),
        s(&request.open_biz_data),
    ]);
    parts.join("&")
}

/// The JSON object a signing bridge sends to an external signer.
pub fn sign_request_payload(request: &SignRequest) -> Value {
    let mut payload = Map::new();
    payload.insert("api".into(), json!(request.api));
    payload.insert("version".into(), json!(request.version));
    payload.insert("data".into(), json!(request.data_json()));
    payload.insert("t".into(), json!(request.timestamp));
    payload.insert("timestamp".into(), json!(request.timestamp));
    payload.insert("utdid".into(), opt_or_null(&request.utdid));
    payload.insert("device_id".into(), opt_or_null(&request.device_id));
    payload.insert("ttid".into(), opt_or_null(&request.ttid));
    payload.insert("uid".into(), opt_or_null(&request.uid));
    payload.insert("sid".into(), opt_or_null(&request.sid));
    payload.insert("app_key".into(), json!(request.app_key));
    payload.insert("input".into(), json!(build_sign_input(request, None)));
    payload.insert("include_ext_data".into(), json!(request.include_ext_data));
    payload.insert("use_wua".into(), json!(request.use_wua));
    payload.insert("env".into(), json!(request.env));
    let optional: Vec<(&str, &Option<String>)> = vec![
        ("reqbiz_ext", &request.reqbiz_ext),
        ("lat", &request.lat),
        ("lng", &request.lng),
        ("extdata", &request.extdata),
        ("x_features", &request.x_features),
        ("router_id", &request.router_id),
        ("place_id", &request.place_id),
        ("open_biz", &request.open_biz),
        ("mini_app_key", &request.mini_app_key),
        ("req_app_key", &request.req_app_key),
        ("access_token", &request.access_token),
        ("open_biz_data", &request.open_biz_data),
        ("request_id", &request.request_id),
    ];
    for (name, value) in optional {
        if let Some(v) = value {
            payload.insert(name.into(), json!(v));
        }
    }
    Value::Object(payload)
}

fn opt_or_null(value: &Option<String>) -> Value {
    match value {
        Some(v) => json!(v),
        None => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// UnavailableSigner
// ---------------------------------------------------------------------------

pub struct UnavailableSigner {
    pub reason: String,
}

impl Default for UnavailableSigner {
    fn default() -> Self {
        Self {
            reason: "no signing provider is configured: the SecurityGuard final computation is native and is not reproduced by this package".to_string(),
        }
    }
}

impl Signer for UnavailableSigner {
    fn sign(&self, request: &SignRequest) -> Result<SignResult> {
        Err(anyhow!(
            "refusing to sign {}/{}: {}",
            request.api,
            request.version,
            self.reason
        ))
    }
}

// ---------------------------------------------------------------------------
// SubprocessSigner (fresh process per call, JSON-lines)
// ---------------------------------------------------------------------------

pub struct SubprocessSigner {
    pub command: Vec<String>,
    pub timeout: Duration,
}

impl SubprocessSigner {
    pub fn new(command: Vec<String>) -> Result<Self> {
        Self::with_timeout(command, Duration::from_secs(10))
    }

    pub fn with_timeout(command: Vec<String>, timeout: Duration) -> Result<Self> {
        if command.is_empty() || command.iter().any(|c| c.is_empty()) {
            return Err(anyhow!(
                "SubprocessSigner command must be a non-empty sequence of strings"
            ));
        }
        if timeout.is_zero() {
            return Err(anyhow!("SubprocessSigner timeout must be positive"));
        }
        Ok(Self { command, timeout })
    }
}

impl Signer for SubprocessSigner {
    fn sign(&self, request: &SignRequest) -> Result<SignResult> {
        let payload_line = format!("{}\n", sign_request_payload(request));
        let mut child = Command::new(&self.command[0])
            .args(&self.command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                anyhow!(
                    "signing process could not be started ({}): startup",
                    class_name(&e)
                )
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(payload_line.as_bytes());
            // stdin dropped here -> closed, as the protocol requires.
        }
        let stdout = child.stdout.take();
        let (tx, rx) = channel::<String>();
        let reader = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(out) = stdout {
                let mut reader = BufReader::new(out);
                let _ = reader.read_line(&mut buf);
            }
            let _ = tx.send(buf);
        });
        let line = match rx.recv_timeout(self.timeout) {
            Ok(l) => l,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(anyhow!(
                    "signing process timed out after {}s: timeout",
                    self.timeout.as_secs_f64()
                ));
            }
        };
        let _ = reader.join();
        let status = child
            .wait()
            .map_err(|e| anyhow!("signing process wait failed: {e}"))?;
        if !status.success() {
            return Err(anyhow!(
                "signing process exited abnormally (returncode {}): exit",
                status.code().unwrap_or(-1)
            ));
        }
        let line = line.trim();
        if line.is_empty() {
            return Err(anyhow!("signing process produced no output: malformed"));
        }
        let doc: Value = serde_json::from_str(line)
            .map_err(|_| anyhow!("signing process returned malformed JSON: malformed"))?;
        let doc = doc.as_object().ok_or_else(|| {
            anyhow!("signing process returned a non-object JSON value: malformed")
        })?;
        let pick = |names: &[&str]| -> Option<String> {
            for name in names {
                if let Some(Value::String(v)) = doc.get(*name) {
                    if !v.is_empty() {
                        return Some(v.clone());
                    }
                }
            }
            None
        };
        let sign_value = pick(&["sign", "x-sign", "x_sign"])
            .ok_or_else(|| anyhow!("signing process returned no sign value: no_sign"))?;
        let known = [
            "sign",
            "x-sign",
            "x_sign",
            "x_umt",
            "x-umt",
            "x_mini_wua",
            "x-mini-wua",
            "x_sgext",
            "x-sgext",
            "wua",
        ];
        let mut extras = Map::new();
        for (k, v) in doc {
            if !known.contains(&k.as_str()) {
                extras.insert(k.clone(), v.clone());
            }
        }
        Ok(SignResult {
            sign: sign_value,
            x_umt: pick(&["x_umt", "x-umt"]),
            x_mini_wua: pick(&["x_mini_wua", "x-mini-wua"]),
            x_sgext: pick(&["x_sgext", "x-sgext"]),
            wua: pick(&["wua"]),
            extras,
        })
    }
}

fn class_name(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::NotFound => "FileNotFoundError",
        std::io::ErrorKind::PermissionDenied => "PermissionError",
        _ => "OSError",
    }
}

// ---------------------------------------------------------------------------
// UnidbgSigner (persistent serve process)
// ---------------------------------------------------------------------------

enum LineEvent {
    Line(String),
    Eof,
}

struct UnidbgInner {
    child: Option<Child>,
    rx: Option<Receiver<LineEvent>>,
    broken: Option<String>,
    closed: bool,
    /// Wall time the most recent harness start took (ready line
    /// received), for the phase breakdown in issue #2.
    startup_duration: Option<Duration>,
}

pub struct UnidbgSigner {
    pub command: Vec<String>,
    pub timeout: Duration,
    pub request_type: i64,
    pub auth_code: String,
    /// INPUT layout selector (default false = round-19 legacy path B, pv 6.2).
    pub include_ext_data: bool,
    inner: Mutex<UnidbgInner>,
}

impl UnidbgSigner {
    pub fn new(command: Vec<String>) -> Result<Self> {
        Self::with_options(command, Duration::from_secs(120), false)
    }

    pub fn with_options(
        command: Vec<String>,
        timeout: Duration,
        include_ext_data: bool,
    ) -> Result<Self> {
        if command.is_empty() || command.iter().any(|c| c.is_empty()) {
            return Err(anyhow!(
                "UnidbgSigner command must be a non-empty sequence of strings"
            ));
        }
        if timeout.is_zero() {
            return Err(anyhow!("UnidbgSigner timeout must be positive"));
        }
        Ok(Self {
            command,
            timeout,
            request_type: 7,
            auth_code: String::new(),
            include_ext_data,
            inner: Mutex::new(UnidbgInner {
                child: None,
                rx: None,
                broken: None,
                closed: false,
                startup_duration: None,
            }),
        })
    }

    fn read_line(inner: &UnidbgInner, timeout: Duration, phase: &str) -> Result<String> {
        let rx = inner.rx.as_ref().expect("line channel exists after start");
        match rx.recv_timeout(timeout) {
            Ok(LineEvent::Line(l)) => Ok(l),
            Ok(LineEvent::Eof) => Err(anyhow!(
                "unidbg signing harness closed its output during {phase}: exit"
            )),
            Err(_) => Err(anyhow!(
                "unidbg signing harness timed out during {phase} after {}s: timeout",
                timeout.as_secs_f64()
            )),
        }
    }

    fn start(&self, inner: &mut UnidbgInner) -> Result<()> {
        let started_at = Instant::now();
        let mut command = Command::new(&self.command[0]);
        command
            .args(&self.command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Orphan prevention (issue #2): the harness runs in its own
        // process group so shutdown can kill the whole tree, and on
        // Linux it gets PR_SET_PDEATHSIG so the kernel kills it even
        // when this process dies without running any cleanup (e.g.
        // SIGKILL after the MCP client vanished).
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            unsafe {
                command.pre_exec(|| {
                    // PR_SET_PDEATHSIG is Linux-only; on other Unix
                    // platforms the process-group kill above is the
                    // orphan-prevention mechanism.
                    #[cfg(target_os = "linux")]
                    {
                        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                        // Race: the parent may already be gone.
                        if libc::getppid() == 1 {
                            libc::_exit(1);
                        }
                    }
                    Ok(())
                });
            }
        }
        let mut child = command.spawn().map_err(|e| {
            anyhow!(
                "unidbg signing harness could not be started ({}): startup",
                class_name(&e)
            )
        })?;
        let stdout = child.stdout.take().expect("stdout piped");
        let (tx, rx) = channel::<LineEvent>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(LineEvent::Eof);
                        break;
                    }
                    Ok(_) => {
                        if tx.send(LineEvent::Line(line)).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        inner.child = Some(child);
        inner.rx = Some(rx);
        let line = Self::read_line(inner, self.timeout, "startup")?;
        let ready: Value = serde_json::from_str(line.trim())
            .map_err(|_| anyhow!("unidbg signing harness sent a malformed ready line: startup"))?;
        let ready_obj = ready
            .as_object()
            .ok_or_else(|| anyhow!("unidbg signing harness did not report ready: startup"))?;
        if ready_obj.get("ready").and_then(|v| v.as_bool()) != Some(true) {
            return Err(anyhow!(
                "unidbg signing harness did not report ready: startup"
            ));
        }
        // Round 28: ready=true alone does not mean usable — validate
        // initRc / appKey when present (absent keys are accepted).
        if let Some(init_rc) = ready_obj.get("initRc") {
            if init_rc.as_i64() != Some(0) {
                let detail = init_rc
                    .as_i64()
                    .map(|n| format!(" (initRc={n})"))
                    .unwrap_or_default();
                return Err(anyhow!(
                    "unidbg signing harness reported SG init failure{detail}: startup"
                ));
            }
        }
        if let Some(app_key) = ready_obj.get("appKey") {
            let ok = matches!(app_key, Value::String(s) if !s.is_empty());
            if !ok {
                return Err(anyhow!(
                    "unidbg signing harness reported no appKey at startup: startup"
                ));
            }
        }
        inner.startup_duration = Some(started_at.elapsed());
        Ok(())
    }

    /// Whether the harness process has been started (and not shut
    /// down since).
    pub fn is_started(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.child.is_some())
            .unwrap_or(false)
    }

    /// How long the most recent harness start took, if it has
    /// started at least once in this signer.
    pub fn last_startup_duration(&self) -> Option<Duration> {
        self.inner.lock().ok()?.startup_duration
    }

    fn shutdown(inner: &mut UnidbgInner) {
        inner.rx = None;
        if let Some(mut child) = inner.child.take() {
            let pid = child.id();
            if let Some(stdin) = child.stdin.take() {
                drop(stdin);
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        if Instant::now() > deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
            // The harness leads its own process group (see start()):
            // kill the group as well, so grandchildren (a Java
            // harness spawned under a wrapper shell that did not
            // exec, helper processes, ...) cannot outlive the signer
            // even when the direct child already exited cleanly.
            #[cfg(unix)]
            unsafe {
                libc::killpg(pid as libc::pid_t, libc::SIGKILL);
            }
            let _ = pid;
        }
    }

    pub fn close(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.closed = true;
        Self::shutdown(&mut inner);
    }

    fn sign_locked(&self, inner: &mut UnidbgInner, request: &SignRequest) -> Result<SignResult> {
        if inner.child.is_none() {
            self.start(inner)?;
        }
        {
            let child = inner.child.as_mut().expect("child after start");
            if let Ok(Some(_)) = child.try_wait() {
                return Err(anyhow!("unidbg signing harness is no longer running: exit"));
            }
        }
        let input_str = build_sign_input(request, Some(self.include_ext_data));
        let payload = json!({
            "input": input_str,
            "requestType": self.request_type,
            "authCode": self.auth_code,
        });
        {
            let child = inner.child.as_mut().expect("child after start");
            let stdin = child.stdin.as_mut().expect("stdin piped");
            stdin
                .write_all(format!("{payload}\n").as_bytes())
                .and_then(|_| stdin.flush())
                .map_err(|e| {
                    anyhow!(
                        "could not write to the unidbg signing harness ({}): exit",
                        class_name(&e)
                    )
                })?;
        }
        let line = Self::read_line(inner, self.timeout, "sign")?;
        let doc: Value = serde_json::from_str(line.trim())
            .map_err(|_| anyhow!("unidbg signing harness returned malformed JSON: malformed"))?;
        let doc = doc.as_object().ok_or_else(|| {
            anyhow!("unidbg signing harness returned a non-object JSON value: malformed")
        })?;
        if let Some(err) = doc.get("error") {
            let truthy = match err {
                Value::String(s) => !s.is_empty(),
                Value::Null => false,
                Value::Bool(b) => *b,
                _ => true,
            };
            if truthy {
                // The harness's error text is deliberately not propagated.
                return Err(anyhow!(
                    "unidbg signing harness reported an error for the sign request: protocol"
                ));
            }
        }
        let sign_value = match doc.get("sign") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            _ => {
                return Err(anyhow!(
                    "unidbg signing harness returned no sign value: no_sign"
                ))
            }
        };
        Ok(SignResult {
            sign: sign_value,
            ..Default::default()
        })
    }
}

impl Signer for UnidbgSigner {
    fn sign(&self, request: &SignRequest) -> Result<SignResult> {
        let mut inner = self.inner.lock().unwrap();
        if inner.closed {
            return Err(anyhow!("unidbg signer is closed"));
        }
        if let Some(broken) = &inner.broken {
            // No retry: a failed harness is never restarted silently.
            return Err(anyhow!("{broken}"));
        }
        match self.sign_locked(&mut inner, request) {
            Ok(r) => Ok(r),
            Err(e) => {
                let msg = e.to_string();
                inner.broken = Some(msg.clone());
                Self::shutdown(&mut inner);
                Err(anyhow!("{msg}"))
            }
        }
    }
}

impl Drop for UnidbgSigner {
    /// Dropping the signer (client dropped, server shutting down)
    /// must not leave the harness tree running (issue #2). The
    /// PDEATHSIG set at spawn covers abrupt process death; this
    /// covers every orderly path.
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.closed = true;
            Self::shutdown(&mut inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn req() -> SignRequest {
        let mut r = SignRequest::new(
            "mtop.taobao.idlemtopsearch.search.suggest",
            "4.0",
            "{\"inputWords\":\"x\"}".to_string(),
            "1700000000".to_string(),
        );
        r.utdid = Some("UTDID-1".into());
        r.device_id = Some("DEV-1".into());
        r.ttid = Some("TTID-1".into());
        r
    }

    // Ported from goofish-client/tests/test_round19_signing.py
    #[test]
    fn input_string_full_layout() {
        let md5 = format!("{:x}", md5::compute(b"{\"inputWords\":\"x\"}"));
        let mut expected_parts = vec![
            "UTDID-1",
            "",
            "",
            "21407387",
            md5.as_str(),
            "1700000000",
            "mtop.taobao.idlemtopsearch.search.suggest",
            "4.0",
            "",
            "TTID-1",
            "DEV-1",
            "",
            "",
            "", // lat, lng, extdata (unified: always present)
        ];
        expected_parts.extend(["", "", "", "", "", "", "", ""]); // x-features .. openBizData
        let expected = expected_parts.join("&");
        assert_eq!(expected.split('&').count(), 22);
        assert_eq!(build_sign_input(&req(), None), expected);
    }

    #[test]
    fn input_string_uses_md5_of_data() {
        let result = build_sign_input(&req(), None);
        assert!(result.contains(&format!("{:x}", md5::compute(b"{\"inputWords\":\"x\"}"))));
        assert!(!result.contains("inputWords"));
    }

    #[test]
    fn input_string_nulls_and_lowercase() {
        let mut r = req();
        r.utdid = None;
        r.device_id = None;
        r.ttid = None;
        let input = build_sign_input(&r, None);
        let parts: Vec<&str> = input.split('&').collect();
        assert_eq!(parts[0], "");
        assert_eq!(parts[9], "");
        assert_eq!(parts[10], "");
        assert_eq!(parts[3], "21407387");
        let mut r2 = req();
        r2.api = "MTOP.TAOBAO.UPPER".into();
        r2.version = "V2".into();
        let input2 = build_sign_input(&r2, None);
        let parts: Vec<&str> = input2.split('&').collect();
        assert_eq!(parts[6], "mtop.taobao.upper");
        assert_eq!(parts[7], "v2");
    }

    #[test]
    fn input_string_extdata_rules() {
        let with_slot = build_sign_input(&req(), Some(true));
        assert_eq!(with_slot.split('&').count(), 22);
        let mut r = req();
        r.extdata = Some("EXT".into());
        assert_eq!(
            build_sign_input(&r, Some(true))
                .split('&')
                .collect::<Vec<_>>()[13],
            "EXT"
        );
        let without = build_sign_input(&req(), Some(false));
        assert_eq!(without.split('&').count(), 21);
        assert_eq!(build_sign_input(&r, Some(false)).split('&').count(), 22);
    }

    #[test]
    fn input_string_all_optional_fields_in_order() {
        let mut r = req();
        r.uid = Some("UID".into());
        r.sid = Some("SID".into());
        r.reqbiz_ext = Some("RB".into());
        r.lat = Some("1.5".into());
        r.lng = Some("2.5".into());
        r.x_features = Some("123".into());
        r.router_id = Some("R".into());
        r.place_id = Some("P".into());
        r.open_biz = Some("OB".into());
        r.mini_app_key = Some("MAK".into());
        r.req_app_key = Some("RAK".into());
        r.access_token = Some("AT".into());
        r.open_biz_data = Some("OBD".into());
        r.extdata = Some("EX".into());
        let parts: Vec<String> = build_sign_input(&r, None)
            .split('&')
            .map(|s| s.to_string())
            .collect();
        assert_eq!(parts[1], "UID");
        assert_eq!(parts[8], "SID");
        assert_eq!(parts[2], "RB");
        assert_eq!(parts[11], "1.5");
        assert_eq!(parts[12], "2.5");
        assert_eq!(parts[13], "EX");
        assert_eq!(
            parts[14..],
            ["123", "R", "P", "OB", "MAK", "RAK", "AT", "OBD"]
        );
    }

    #[test]
    fn payload_carries_input_and_components() {
        let payload = sign_request_payload(&req());
        assert_eq!(payload["input"], json!(build_sign_input(&req(), None)));
        assert_eq!(
            payload["api"],
            json!("mtop.taobao.idlemtopsearch.search.suggest")
        );
        assert_eq!(payload["t"], json!("1700000000"));
        assert_eq!(payload["app_key"], json!("21407387"));
    }

    #[test]
    fn unavailable_signer_always_stops() {
        assert!(UnavailableSigner::default().sign(&req()).is_err());
    }

    fn write_script(body: &str, name: &str) -> Vec<String> {
        let dir =
            std::env::temp_dir().join(format!("gf_sign_test_{}_{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("script.py");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        vec!["python3".to_string(), path.to_string_lossy().to_string()]
    }

    const ECHO_SIGNER: &str = r#"
import json, sys
line = sys.stdin.readline()
req = json.loads(line)
assert req["input"], "payload must carry the INPUT string"
json.dump({"sign": "TESTSIGN-" + req["api"], "x-umt": "UMT-TEST", "extra_field": 7}, sys.stdout)
sys.stdout.write("\n")
"#;

    #[test]
    fn subprocess_signer_round_trip() {
        let signer = SubprocessSigner::new(write_script(ECHO_SIGNER, "echo")).unwrap();
        let result = signer.sign(&req()).unwrap();
        assert_eq!(
            result.sign,
            "TESTSIGN-mtop.taobao.idlemtopsearch.search.suggest"
        );
        assert_eq!(result.x_umt.as_deref(), Some("UMT-TEST"));
        assert_eq!(result.extras.get("extra_field"), Some(&json!(7)));
    }

    #[test]
    fn subprocess_signer_hyphenated_keys() {
        let body = "import json,sys; sys.stdin.readline(); print(json.dumps({\"x-sign\":\"HS\",\"x-mini-wua\":\"MW\"}))";
        let signer = SubprocessSigner::new(write_script(body, "hyph")).unwrap();
        let result = signer.sign(&req()).unwrap();
        assert_eq!(result.sign, "HS");
        assert_eq!(result.x_mini_wua.as_deref(), Some("MW"));
    }

    #[test]
    fn subprocess_signer_timeout() {
        let signer = SubprocessSigner::with_timeout(
            write_script("import time; time.sleep(30)", "slow"),
            Duration::from_millis(500),
        )
        .unwrap();
        let err = signer.sign(&req()).unwrap_err().to_string();
        assert!(err.contains("timed out"), "{err}");
    }

    #[test]
    fn subprocess_signer_abnormal_exit_leaks_no_secrets() {
        let signer =
            SubprocessSigner::new(write_script("import sys; sys.exit(3)", "exit3")).unwrap();
        let mut r = req();
        r.utdid = Some("UTDID-SECRET-VALUE".into());
        let err = signer.sign(&r).unwrap_err().to_string();
        assert!(err.contains("returncode 3"), "{err}");
        assert!(!err.contains("UTDID-SECRET-VALUE"));
    }

    #[test]
    fn subprocess_signer_missing_sign_and_malformed() {
        let signer = SubprocessSigner::new(write_script(
            "import sys; sys.stdin.readline(); print(\"{}\")",
            "nosign",
        ))
        .unwrap();
        assert!(signer.sign(&req()).is_err());
        let signer = SubprocessSigner::new(write_script(
            "import sys; sys.stdin.readline(); print(\"not json\")",
            "badjson",
        ))
        .unwrap();
        assert!(signer.sign(&req()).is_err());
        assert!(SubprocessSigner::new(vec![]).is_err());
    }

    fn write_shell_script(body: &str, name: &str) -> Vec<String> {
        let dir =
            std::env::temp_dir().join(format!("gf_unidbg_test_{}_{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("serve.sh");
        std::fs::write(&path, body).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        vec![path.to_string_lossy().to_string()]
    }

    #[test]
    fn unidbg_signer_fake_harness_round_trip() {
        // Fake serve harness: ready line, then one {"sign": ...} per request line.
        let script = "#!/bin/sh\necho '{\"ready\": true, \"initRc\": 0, \"appKey\": \"21407387\"}'\nwhile IFS= read -r line; do echo '{\"sign\": \"FAKESIGN\"}'; done\n";
        let signer = UnidbgSigner::with_options(
            write_shell_script(script, "ok"),
            Duration::from_secs(10),
            false,
        )
        .unwrap();
        let result = signer.sign(&req()).unwrap();
        assert_eq!(result.sign, "FAKESIGN");
        // persistent process: second sign reuses it
        let result2 = signer.sign(&req()).unwrap();
        assert_eq!(result2.sign, "FAKESIGN");
        signer.close();
    }

    #[test]
    fn unidbg_signer_ready_line_validation() {
        // initRc != 0 must be refused at startup, before any sign request.
        let script = "#!/bin/sh\necho '{\"ready\": true, \"initRc\": 1, \"appKey\": \"21407387\"}'\nwhile IFS= read -r line; do echo '{\"sign\": \"X\"}'; done\n";
        let signer = UnidbgSigner::with_options(
            write_shell_script(script, "badrc"),
            Duration::from_secs(10),
            false,
        )
        .unwrap();
        let err = signer.sign(&req()).unwrap_err().to_string();
        assert!(err.contains("SG init failure"), "{err}");
        // permanently broken: a second call fails immediately with the same error
        let err2 = signer.sign(&req()).unwrap_err().to_string();
        assert_eq!(err, err2);
        // empty appKey also refused
        let script2 = "#!/bin/sh\necho '{\"ready\": true, \"initRc\": 0, \"appKey\": \"\"}'\nwhile IFS= read -r line; do echo '{\"sign\": \"X\"}'; done\n";
        let signer2 = UnidbgSigner::with_options(
            write_shell_script(script2, "badkey"),
            Duration::from_secs(10),
            false,
        )
        .unwrap();
        let err = signer2.sign(&req()).unwrap_err().to_string();
        assert!(err.contains("appKey"), "{err}");
        // harness error reply -> protocol failure, sign value never surfaces
        let script3 = "#!/bin/sh\necho '{\"ready\": true}'\nwhile IFS= read -r line; do echo '{\"error\": \"boom\"}'; done\n";
        let signer3 = UnidbgSigner::with_options(
            write_shell_script(script3, "errreply"),
            Duration::from_secs(10),
            false,
        )
        .unwrap();
        let err = signer3.sign(&req()).unwrap_err().to_string();
        assert!(err.contains("protocol"), "{err}");
        assert!(!err.contains("boom"));
    }

    #[cfg(unix)]
    fn process_alive(pid: i32) -> bool {
        // Alive = /proc entry exists and is not a zombie.
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => match stat.rsplit_once(')') {
                Some((_, rest)) => !rest.trim_start().starts_with('Z'),
                None => true,
            },
            Err(_) => false,
        }
    }

    #[cfg(unix)]
    #[test]
    fn unidbg_signer_drop_kills_harness_tree() {
        // Fake harness that spawns a grandchild (sleep) and records
        // its pid: dropping the signer must kill the whole tree,
        // not just the direct child (issue #2 orphan processes).
        let name = "tree";
        let dir =
            std::env::temp_dir().join(format!("gf_unidbg_test_{}_{}", std::process::id(), name));
        let pid_file = dir.join("grandchild.pid");
        let body = format!(
            "#!/bin/sh\nsleep 300 &\necho $! > '{}'\necho '{{\"ready\": true, \"initRc\": 0, \"appKey\": \"21407387\"}}'\nwhile IFS= read -r line; do echo '{{\"sign\": \"FAKESIGN\"}}'; done\n",
            pid_file.display()
        );
        let signer = UnidbgSigner::with_options(
            write_shell_script(&body, name),
            Duration::from_secs(10),
            false,
        )
        .unwrap();
        let result = signer.sign(&req()).unwrap();
        assert_eq!(result.sign, "FAKESIGN");
        assert!(signer.is_started());
        assert!(signer.last_startup_duration().is_some());
        let grandchild: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            process_alive(grandchild),
            "grandchild should run while the signer lives"
        );
        drop(signer);
        let deadline = Instant::now() + Duration::from_secs(3);
        while process_alive(grandchild) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !process_alive(grandchild),
            "grandchild {grandchild} survived signer drop"
        );
    }
}
