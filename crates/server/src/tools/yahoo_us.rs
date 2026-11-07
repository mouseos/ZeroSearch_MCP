use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use yahoo_us_search::{YahooUsSearchClient, PROVIDER};

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

fn tool_def(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema,
    })
}

fn query_prop() -> Value {
    json!({"type": "string", "description": "Search query for Yahoo (US). English works best; this is the US edition of Yahoo Search, separate from Yahoo! JAPAN."})
}

// ── Tool definitions ──

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "yahoo_us_web_search",
            "Search the Yahoo (US) web index. Returns title, real destination URL (Yahoo redirect links are unwrapped), snippet and source site. 7 results per page. Note: this is Yahoo US (search.yahoo.com), not Yahoo! JAPAN.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": {"type": "integer", "description": "1-based page number.", "default": 1},
                    "limit": {"type": "integer", "description": "Max results to return (1-7).", "default": 7}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "yahoo_us_video_search",
            "Search Yahoo (US) videos across YouTube and other hosts. Returns title, watch URL, duration, source site, description and thumbnail. 60 results per page.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": {"type": "integer", "description": "1-based page number.", "default": 1},
                    "limit": {"type": "integer", "description": "Max results to return (1-60).", "default": 20}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "yahoo_us_image_search",
            "Search Yahoo (US) images. Returns title, the page the image appears on, the original image URL, a thumbnail, dimensions and the source site. 60 results per page. Pages are fetched by the bundled helper process with a browser-grade client because Yahoo's image host refuses plain HTTP requests.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": {"type": "integer", "description": "1-based page number.", "default": 1},
                    "limit": {"type": "integer", "description": "Max results to return (1-60).", "default": 20}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "yahoo_us_news_search",
            "Search Yahoo (US) news articles. Returns headline, article URL, publisher, relative publish time and a snippet. 10 results per page.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": {"type": "integer", "description": "1-based page number.", "default": 1},
                    "limit": {"type": "integer", "description": "Max articles to return (1-10).", "default": 10}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "yahoo_us_suggest",
            "Get Yahoo (US) autocomplete suggestions for a partial query. Useful for discovering how users actually phrase a topic before running a full search.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Partial query to complete."},
                    "limit": {"type": "integer", "description": "Max suggestions to return (1-20).", "default": 10}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "yahoo_us_build_search_url",
            "Build the Yahoo (US) search URL for a query and vertical (web, image, video, news, suggest) without performing any search. Useful for handing a user a link to open themselves.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "vertical": {
                        "type": "string",
                        "description": "Which search vertical the URL should open.",
                        "enum": ["web", "image", "video", "news", "suggest"],
                        "default": "web"
                    },
                    "page": {"type": "integer", "description": "1-based page number the URL should open.", "default": 1}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "yahoo_us_list_verticals",
            "List the Yahoo (US) search verticals this server exposes, with the tool name and paging behaviour of each. Call this to decide which search tool fits a question.",
            json!({"type": "object", "properties": {}}),
        ),
    ]
}

// ── Dispatch ──

// ── Error form (unified with Baidu_MCP / Yahoo-Japan-Search-MCP) ──
//
// A search that *ran* but failed on Yahoo's side (HTTP error, timeout, a
// page layout change, …) is reported as a structured tool result —
// `{provider, ok:false, vertical, error:{kind, message}}` — so agents can
// branch on `ok` uniformly across search MCPs. Argument errors (missing
// query) and unknown tools are not search failures: they still surface as
// MCP `isError` results via `Err` here. Successful results keep their
// existing shape untouched.

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
    } else if e.to_string().contains("returned HTTP") {
        // The browser-grade (wreq) path reports bare status errors with
        // this wording instead of a typed reqwest error.
        "http_error"
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

pub async fn dispatch(name: &str, args: &Value, client: &YahooUsSearchClient) -> Result<Value> {
    match name {
        "yahoo_us_web_search" => match client
            .web_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 7),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("web", e)),
        },
        "yahoo_us_video_search" => match client
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
        "yahoo_us_image_search" => match client
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
        "yahoo_us_news_search" => match client
            .news_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("news", e)),
        },
        "yahoo_us_suggest" => match client
            .suggest(&query_param(args)?, int_param(args, "limit", 10))
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("suggest", e)),
        },
        "yahoo_us_build_search_url" => {
            let query = query_param(args)?;
            let vertical = {
                let v = str_param(args, "vertical");
                if v.is_empty() {
                    "web".to_string()
                } else {
                    v
                }
            };
            let page = int_param(args, "page", 1);
            match yahoo_us_search::build_search_url(&query, &vertical, page) {
                Ok(url) => Ok(json!({
                    "provider": PROVIDER,
                    "ok": true,
                    "vertical": vertical,
                    "query": query,
                    "page": page.max(1),
                    "url": url,
                })),
                Err(e) => Ok(error_result(&vertical, e)),
            }
        }
        "yahoo_us_list_verticals" => Ok(yahoo_us_search::verticals()),
        other => Err(anyhow!("Unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_param_accepts_aliases() {
        assert_eq!(query_param(&json!({"q": "apple"})).unwrap(), "apple");
        assert_eq!(query_param(&json!({"keyword": " pie "})).unwrap(), "pie");
        assert_eq!(query_param(&json!({"p": "apple"})).unwrap(), "apple");
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
        assert_eq!(v["provider"], "yahoo_us");
        assert_eq!(v["ok"], false);
        assert_eq!(v["vertical"], "web");
        assert_eq!(v["error"]["kind"], "search_error");
        assert_eq!(v["error"]["message"], "boom");
    }

    #[test]
    fn error_kind_classifies_layout_changes_as_parse_error() {
        let e = anyhow!("web results container not found (Yahoo page layout may have changed)");
        assert_eq!(error_kind(&e), "parse_error");
        let v = error_result("web", e);
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
