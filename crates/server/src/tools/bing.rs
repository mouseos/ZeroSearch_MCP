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

fn tool_def(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema,
    })
}

fn query_prop() -> Value {
    json!({"type": "string", "description": "Search query. Bing handles Japanese and English well."})
}

fn market_props() -> Value {
    json!({
        "mkt": {"type": "string", "description": "Bing market, e.g. \"ja-JP\" or \"en-US\". Pinning the market is required — without it Bing returns an empty result set. Defaults to \"ja-JP\".", "default": "ja-JP"},
        "setlang": {"type": "string", "description": "UI language code, e.g. \"ja\" or \"en\". Defaults to \"ja\".", "default": "ja"}
    })
}

fn object_schema(mut properties: Value, required: &[&str]) -> Value {
    properties["query"] = query_prop();
    json!({"type": "object", "properties": properties, "required": required})
}

fn paged_schema(limit_default: i64, limit_max: i64, extra: Value) -> Value {
    let mut props = market_props();
    props["page"] = json!({"type": "integer", "description": "1-based page number.", "default": 1});
    props["limit"] = json!({"type": "integer", "description": format!("Max results to return (1-{limit_max})."), "default": limit_default});
    let extra_obj = extra.as_object().cloned().unwrap_or_default();
    for (k, v) in extra_obj {
        props[k] = v;
    }
    object_schema(props, &["query"])
}

// ── Tool definitions ──

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "bing_web_search",
            "Search the Microsoft Bing web index via Bing's native v7 search API (the endpoint embedded in the Bing app). Returns title, direct destination URL, the displayed URL and a snippet, plus the reported total hit count and related queries. 10 results per page.",
            paged_schema(10, 10, json!({})),
        ),
        tool_def(
            "bing_image_search",
            "Search Bing images. Returns the original image URL, a Bing thumbnail, the page the image appears on and a title. Fetched the way the Bing app receives it (mobile user agent + app-style parameters), the only form where Bing server-renders image results. 12 results per page; this form has no total count.",
            paged_schema(12, 12, json!({})),
        ),
        tool_def(
            "bing_video_search",
            "Search Bing videos across YouTube, TikTok, Niconico and other hosts. Returns title, the real watch URL (taken from the result link's churl parameter), hosting site, channel, duration, view count and upload date text.",
            paged_schema(10, 35, json!({})),
        ),
        tool_def(
            "bing_news_search",
            "Search Bing News for news articles. Returns headline, article URL, publisher, publish-time text and a snippet. Bing's news page does not paginate, so this returns the first page only.",
            object_schema(
                {
                    let mut props = market_props();
                    props["limit"] =
                        json!({"type": "integer", "description": "Max articles to return (1-50).", "default": 10});
                    props
                },
                &["query"],
            ),
        ),
        tool_def(
            "bing_suggest",
            "Get Bing autocomplete suggestions for a partial query (the Bing app's own suggestions API). Useful for discovering how people phrase a topic before running a full search. Pass vertical \"images\" or \"videos\" for candidates in those verticals.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Partial query to complete."},
                    "vertical": {"type": "string", "description": "\"web\" (default), \"images\" or \"videos\".", "enum": ["web", "images", "videos"], "default": "web"}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "bing_build_search_url",
            "Build the Bing search URL for a query and vertical (web, image, video, news) without performing any search. Works offline.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "vertical": {"type": "string", "description": "\"web\" (default), \"image\", \"video\" or \"news\".", "enum": ["web", "image", "video", "news"], "default": "web"},
                    "mkt": {"type": "string", "description": "Bing market. Defaults to \"ja-JP\".", "default": "ja-JP"},
                    "setlang": {"type": "string", "description": "UI language code. Defaults to \"ja\".", "default": "ja"}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "bing_list_verticals",
            "List the Bing search verticals this server exposes, with the tool name for each. Call this to decide which search tool fits a question.",
            json!({"type": "object", "properties": {}}),
        ),
    ]
}

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
                    "",
                    "",
                )
                .await
            {
                Ok(result) => ok_value(result),
                Err(e) => Ok(error_result("suggest", e)),
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
    fn there_are_seven_tools_with_schemas() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 7);
        let names: Vec<&str> = defs.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "bing_web_search",
                "bing_image_search",
                "bing_video_search",
                "bing_news_search",
                "bing_suggest",
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
