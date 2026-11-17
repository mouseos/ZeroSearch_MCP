use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use yahoo_search::{YahooSearchClient, PROVIDER};

// ── Argument helpers ──

fn str_param(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// The query, accepting the aliases agents most often reach for.
fn query_param(args: &Value) -> Result<String> {
    for key in ["query", "q", "p", "keyword"] {
        let v = str_param(args, key);
        if !v.is_empty() {
            return Ok(v);
        }
    }
    Err(anyhow!(
        "`query` is required and must be a non-empty string"
    ))
}

fn int_param(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key)
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(default)
}

// ── Tool definitions ──

pub fn tool_definitions() -> Vec<Value> {
    DEFS.clone()
}

/// The tool definitions, kept as data (`defs/yahoo.json`) and
/// parsed once: building them with `json!` compiled to hundreds of
/// KiB of machine code for what is a constant document.
static DEFS: std::sync::LazyLock<Vec<Value>> =
    std::sync::LazyLock::new(|| super::parse_defs(include_str!("defs/yahoo.json")));

// ── Dispatch ──

// ── Error form (unified with Baidu_MCP) ──
//
// A search that *ran* but failed on Yahoo's side (HTTP error, timeout, a
// page layout change, …) is reported as a structured tool result —
// `{provider, ok:false, vertical, error:{kind, message}}` — the same form
// Baidu_MCP uses, so agents can branch on `ok` uniformly across search
// MCPs. Argument errors (missing query) and unknown tools are not search
// failures: they still surface as MCP `isError` results via `Err` here.
// Successful results keep their existing shape untouched.

/// Classify a provider error into a stable `kind` string.
fn error_kind(e: &anyhow::Error) -> &'static str {
    for cause in e.chain() {
        if let Some(re) = cause.downcast_ref::<reqwest::Error>() {
            if re.is_timeout() {
                return "timeout";
            }
            if re.is_connect() {
                return "connection_error";
            }
            if re.is_status() {
                return "http_error";
            }
            if re.is_decode() || re.is_body() {
                return "parse_error";
            }
            return "http_error";
        }
    }
    if e.to_string()
        .contains("not found (Yahoo page layout may have changed)")
    {
        "parse_error"
    } else {
        "search_error"
    }
}

/// The unified structured error result for a failed search.
fn error_result(vertical: &str, e: anyhow::Error) -> Value {
    json!({
        "provider": PROVIDER,
        "ok": false,
        "vertical": vertical,
        "error": {
            "kind": error_kind(&e),
            "message": e.to_string(),
        }
    })
}

pub async fn dispatch(name: &str, args: &Value, client: &YahooSearchClient) -> Result<Value> {
    match name {
        "yahoo_web_search" => match client
            .web_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
                &str_param(args, "updated"),
                &str_param(args, "site"),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("web", e)),
        },
        "yahoo_image_search" => match client
            .image_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 20),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("image", e)),
        },
        "yahoo_video_search" => match client
            .video_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 20),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("video", e)),
        },
        "yahoo_news_search" => match client
            .news_search(&query_param(args)?, int_param(args, "limit", 20))
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("news", e)),
        },
        "yahoo_realtime_search" => match client
            .realtime_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 20),
                &str_param(args, "sort"),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("realtime", e)),
        },
        "yahoo_chiebukuro_search" => match client
            .chiebukuro_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
                &str_param(args, "status"),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("chiebukuro", e)),
        },
        "yahoo_suggest_keywords" => match client
            .suggest_keywords(&query_param(args)?, int_param(args, "limit", 10))
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("suggest", e)),
        },
        "yahoo_list_verticals" => Ok(yahoo_search::verticals()),
        other => Err(anyhow!("Unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_param_accepts_aliases() {
        assert_eq!(query_param(&json!({"q": "ラーメン"})).unwrap(), "ラーメン");
        assert_eq!(query_param(&json!({"keyword": " 東京 "})).unwrap(), "東京");
        assert!(query_param(&json!({"query": "  "})).is_err());
        assert!(query_param(&json!({})).is_err());
    }

    #[test]
    fn int_param_parses_numeric_strings() {
        assert_eq!(int_param(&json!({"limit": "5"}), "limit", 10), 5);
        assert_eq!(int_param(&json!({"limit": 3}), "limit", 10), 3);
        assert_eq!(int_param(&json!({}), "limit", 10), 10);
    }

    #[test]
    fn error_result_uses_the_unified_form() {
        let v = error_result("web", anyhow!("boom"));
        assert_eq!(v["provider"], "yahoo_japan");
        assert_eq!(v["ok"], false);
        assert_eq!(v["vertical"], "web");
        assert_eq!(v["error"]["kind"], "search_error");
        assert_eq!(v["error"]["message"], "boom");
    }

    #[test]
    fn error_kind_classifies_layout_changes_as_parse_error() {
        let e = anyhow!("__NEXT_DATA__ block not found (Yahoo page layout may have changed)");
        assert_eq!(error_kind(&e), "parse_error");
        let v = error_result("news", e);
        assert_eq!(v["error"]["kind"], "parse_error");
    }

    #[test]
    fn every_tool_has_a_schema_and_description() {
        for tool in tool_definitions() {
            assert!(!tool["name"].as_str().unwrap().is_empty());
            assert!(tool["description"].as_str().unwrap().len() > 40);
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }
}
