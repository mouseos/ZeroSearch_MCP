use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use bing_search::{BingSearchClient, PROVIDER};

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
    for key in ["query", "q", "keyword"] {
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

/// The tool definitions, kept as data (`defs/bing.json`) and
/// parsed once: building them with `json!` compiled to hundreds of
/// KiB of machine code for what is a constant document.
static DEFS: std::sync::LazyLock<Vec<Value>> =
    std::sync::LazyLock::new(|| super::parse_defs(include_str!("defs/bing.json")));

// ── Error form (unified with Baidu_MCP / Yahoo) ──
//
// A search that *ran* but failed on Bing's side (HTTP error, timeout, a
// page layout change, an access restriction, …) is reported as a
// structured tool result — `{provider, ok:false, vertical,
// error:{kind, message}}` — so agents can branch on `ok` uniformly
// across search MCPs. Argument errors (missing query) and unknown tools
// are not search failures: they still surface as MCP `isError` results
// via `Err` here. Successful results carry `ok: true`.

/// Classify a provider error into a stable `kind` string.
fn error_kind(e: &anyhow::Error) -> &'static str {
    let message = e.to_string();
    if message.contains("access_restricted") {
        return "access_restricted";
    }
    if message.contains("may have changed") || message.contains("parse_error") {
        return "parse_error";
    }
    if message.contains("timeout") || message.contains("timed out") {
        return "timeout";
    }
    if message.contains("connection_error") {
        return "connection_error";
    }
    if message.contains("http_error") {
        return "http_error";
    }
    for cause in e.chain() {
        if let Some(re) = cause.downcast_ref::<reqwest::Error>() {
            if re.is_timeout() {
                return "timeout";
            }
            if re.is_connect() {
                return "connection_error";
            }
            if re.is_status() {
                let status = re.status().map(|s| s.as_u16());
                if matches!(status, Some(403) | Some(429)) {
                    return "access_restricted";
                }
                return "http_error";
            }
            if re.is_decode() || re.is_body() {
                return "parse_error";
            }
            return "http_error";
        }
    }
    "search_error"
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

/// Serialise a provider result and mark it `ok: true`.
fn ok_value<T: serde::Serialize>(result: T) -> Result<Value> {
    let mut value = serde_json::to_value(result)?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("ok".to_string(), json!(true));
    }
    Ok(value)
}

pub async fn dispatch(name: &str, args: &Value, client: &BingSearchClient) -> Result<Value> {
    match name {
        "bing_web_search" => match client
            .web_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
                &str_param(args, "mkt"),
                &str_param(args, "setlang"),
            )
            .await
        {
            Ok(result) => ok_value(result),
            Err(e) => Ok(error_result("web", e)),
        },
        "bing_image_search" => match client
            .image_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 12),
                &str_param(args, "mkt"),
                &str_param(args, "setlang"),
            )
            .await
        {
            Ok(result) => ok_value(result),
            Err(e) => Ok(error_result("image", e)),
        },
        "bing_video_search" => match client
            .video_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
                &str_param(args, "mkt"),
                &str_param(args, "setlang"),
            )
            .await
        {
            Ok(result) => ok_value(result),
            Err(e) => Ok(error_result("video", e)),
        },
        "bing_news_search" => match client
            .news_search(
                &query_param(args)?,
                int_param(args, "limit", 10),
                &str_param(args, "mkt"),
                &str_param(args, "setlang"),
            )
            .await
        {
            Ok(result) => ok_value(result),
            Err(e) => Ok(error_result("news", e)),
        },
        "bing_suggest" => {
            let vertical = str_param(args, "vertical");
            match client
                .suggest(
                    &query_param(args)?,
                    if vertical.is_empty() {
                        "web"
                    } else {
                        &vertical
                    },
                    &str_param(args, "mkt"),
                    &str_param(args, "setlang"),
                )
                .await
            {
                Ok(result) => ok_value(result),
                Err(e) => Ok(error_result("suggest", e)),
            }
        }
        "bing_shopping_search" => match client
            .shopping_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 20),
                &str_param(args, "mkt"),
                &str_param(args, "setlang"),
            )
            .await
        {
            Ok(result) => ok_value(result),
            Err(e) => Ok(error_result("shopping", e)),
        },
        "bing_trending_topics" => match client
            .trending_topics(
                int_param(args, "limit", 20),
                &str_param(args, "mkt"),
                &str_param(args, "setlang"),
            )
            .await
        {
            Ok(topics) => Ok(json!({
                "provider": PROVIDER,
                "ok": true,
                "vertical": "trending",
                "count": topics.len(),
                "topics": topics,
            })),
            Err(e) => Ok(error_result("trending", e)),
        },
        "bing_visual_search" => {
            let b64 = str_param(args, "image_base64");
            if b64.is_empty() {
                return Err(anyhow!(
                    "`image_base64` is required and must be a non-empty string"
                ));
            }
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64.as_bytes())
                .map_err(|e| anyhow!("`image_base64` is not valid base64: {e}"))?;
            match client
                .visual_search(&bytes, &str_param(args, "mkt"), &str_param(args, "setlang"))
                .await
            {
                Ok(tags) => Ok(json!({
                    "provider": PROVIDER,
                    "ok": true,
                    "vertical": "visual",
                    "count": tags.len(),
                    "tags": tags,
                })),
                Err(e) => Ok(error_result("visual", e)),
            }
        }
        "bing_build_search_url" => {
            let query = query_param(args)?;
            let vertical = str_param(args, "vertical");
            let vertical = if vertical.is_empty() {
                "web".to_string()
            } else {
                vertical
            };
            let mkt = str_param(args, "mkt");
            let setlang = str_param(args, "setlang");
            let url = bing_search::build_search_url(&query, &vertical, &mkt, &setlang)?;
            Ok(json!({
                "provider": PROVIDER,
                "ok": true,
                "vertical": bing_search::urlbuild::normalize_vertical(&vertical),
                "query": query,
                "url": url,
            }))
        }
        "bing_list_verticals" => {
            let mut value = bing_search::verticals();
            if let Some(obj) = value.as_object_mut() {
                obj.insert("ok".to_string(), json!(true));
            }
            Ok(value)
        }
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
        assert_eq!(v["provider"], "bing");
        assert_eq!(v["ok"], false);
        assert_eq!(v["vertical"], "web");
        assert_eq!(v["error"]["kind"], "search_error");
        assert_eq!(v["error"]["message"], "boom");
    }

    #[test]
    fn error_kind_classifies_known_failures() {
        assert_eq!(
            error_kind(&anyhow!("access_restricted: HTTP 429 Too Many Requests")),
            "access_restricted"
        );
        assert_eq!(
            error_kind(&anyhow!(
                "web result blocks not found (Bing page layout may have changed)"
            )),
            "parse_error"
        );
        assert_eq!(
            error_kind(&anyhow!("http_error: HTTP 500 from Bing")),
            "http_error"
        );
        assert_eq!(
            error_kind(&anyhow!("connection_error: dns failure")),
            "connection_error"
        );
    }

    #[test]
    fn there_are_ten_tools_with_schemas() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 10);
        let names: Vec<&str> = defs.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "bing_web_search",
                "bing_image_search",
                "bing_video_search",
                "bing_news_search",
                "bing_suggest",
                "bing_shopping_search",
                "bing_trending_topics",
                "bing_visual_search",
                "bing_build_search_url",
                "bing_list_verticals"
            ]
        );
        for tool in &defs {
            assert!(tool["description"].as_str().unwrap().len() > 40);
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }
}
