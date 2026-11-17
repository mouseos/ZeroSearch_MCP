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
