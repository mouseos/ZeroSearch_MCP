//! Tool-argument helpers shared by several tool modules.
//!
//! Only helpers whose behaviour was identical in every module that
//! used them live here. Modules with different semantics (Baidu's
//! `String` query and strict integers, the shopping tools' `i32`
//! integers, ...) keep their own local helpers.

use anyhow::{anyhow, Result};
use serde_json::Value;

/// A string argument, trimmed; `""` when absent or not a string.
pub(crate) fn str_param(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// A string argument as given (not trimmed); `""` when absent or not
/// a string.
pub(crate) fn raw_str_param(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// The first non-empty (trimmed) argument among `keys`, or an
/// argument error saying `query` is required.
pub(crate) fn query_param_from(args: &Value, keys: &[&str]) -> Result<String> {
    for key in keys {
        let v = str_param(args, key);
        if !v.is_empty() {
            return Ok(v);
        }
    }
    Err(anyhow!(
        "`query` is required and must be a non-empty string"
    ))
}

/// The query, accepting the aliases agents most often reach for.
pub(crate) fn query_param(args: &Value) -> Result<String> {
    query_param_from(args, &["query", "q", "keyword"])
}

/// An integer argument, also accepted as a numeric string.
pub(crate) fn int_param(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key)
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(default)
}

/// An integer argument (JSON numbers only), if present.
pub(crate) fn opt_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}

/// Error kind from the first `reqwest::Error` in the chain, if any —
/// the classification every web-engine tool module shares. With
/// `restricted_statuses`, HTTP 403/429 count as `access_restricted`
/// (Bing) instead of `http_error`.
pub(crate) fn reqwest_error_kind(
    e: &anyhow::Error,
    restricted_statuses: bool,
) -> Option<&'static str> {
    let re = e.chain().find_map(|c| c.downcast_ref::<reqwest::Error>())?;
    Some(if re.is_timeout() {
        "timeout"
    } else if re.is_connect() {
        "connection_error"
    } else if re.is_status() {
        let status = re.status().map(|s| s.as_u16());
        if restricted_statuses && matches!(status, Some(403) | Some(429)) {
            "access_restricted"
        } else {
            "http_error"
        }
    } else if re.is_decode() || re.is_body() {
        "parse_error"
    } else {
        "http_error"
    })
}

/// The unified error result of the web-engine tools.
pub(crate) fn engine_error_result(
    provider: &str,
    vertical: &str,
    kind: &str,
    e: &anyhow::Error,
) -> serde_json::Value {
    serde_json::json!({
        "provider": provider,
        "ok": false,
        "vertical": vertical,
        "error": {
            "kind": kind,
            "message": e.to_string(),
        }
    })
}
