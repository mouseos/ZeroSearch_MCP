use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use naver_search::{NaverSearchClient, PROVIDER};

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
    json!({"type": "string", "description": "Search query. Korean works best; Naver indexes Korean-language content far more deeply than other engines."})
}

fn page_prop() -> Value {
    json!({"type": "integer", "description": "1-based page number.", "default": 1})
}

// ── Tool definitions ──

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "naver_search",
            "Run a Naver integrated search and get the subject sections (news, images, blog, dictionary, web documents, ...) in page order, each with its own results. Best first tool when the right vertical is unclear, or for a broad overview of a Korean topic.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "limit": {"type": "integer", "description": "Max results to keep per section (1-10).", "default": 5}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "naver_web_search",
            "Search Naver's web-results view (external web documents). Returns title, URL, snippet and Naver's rank. The tab serves one top-selection batch, so only page 1 exists — asking for page 2 returns an explicit error rather than a repeat of page 1.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": page_prop(),
                    "limit": {"type": "integer", "description": "Max results to return (1-20).", "default": 10}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "naver_image_search",
            "Search Naver images. Returns the original image URL (recovered from behind Naver's thumbnail proxy), a thumbnail, the source page, the source label and pixel dimensions. Pages follow the more-API chain the page itself issues (up to page 10).",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": page_prop(),
                    "limit": {"type": "integer", "description": "Max images to return (1-50).", "default": 10}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "naver_news_search",
            "Search Naver News for Korean news articles. Returns headline, article URL, publisher, relative publish time, snippet and thumbnail. Pages follow the more-API chain the page itself issues (up to page 10).",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": page_prop(),
                    "limit": {"type": "integer", "description": "Max articles to return (1-30).", "default": 10}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "naver_video_search",
            "Search Naver videos across YouTube and other hosts. Returns title, watch URL, hosting platform, channel, duration, relative upload time, view count and thumbnail. Pages follow the more-API chain the page itself issues (up to page 10).",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": page_prop(),
                    "limit": {"type": "integer", "description": "Max videos to return (1-48).", "default": 10}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "naver_blog_search",
            "Search Naver Blog posts. Returns title, post URL, blog name and id, relative publish time, snippet and thumbnail. Pages follow the more-API chain the page itself issues (up to page 10).",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": page_prop(),
                    "limit": {"type": "integer", "description": "Max posts to return (1-30).", "default": 10}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "naver_cafe_search",
            "Search the Naver Cafe tab (community posts). Returns title, post URL, cafe name, relative publish time and snippet. Only the first page exists as a server-rendered page (the app's continuation is a native API this build does not reproduce), so page > 1 is an explicit error.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": page_prop(),
                    "limit": {"type": "integer", "description": "Max posts to return (1-30).", "default": 10}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "naver_kin_search",
            "Search the Naver 지식iN (knowledge Q&A) tab. Returns question title, URL, relative publish time, snippet, answer count, category and the status badge (e.g. 답변완료). Only the first page exists as a server-rendered page, so page > 1 is an explicit error.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": page_prop(),
                    "limit": {"type": "integer", "description": "Max questions to return (1-30).", "default": 10}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "naver_suggest",
            "Get Naver autocomplete suggestions for a partial query. Useful for discovering how Korean users actually phrase a topic before running a full search.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Partial query to complete."}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "naver_build_search_url",
            "Build the Naver search URL for a query and vertical (integrated, web, image, news, video, blog) exactly as the client fetches it — no network access. Useful for handing a human a link to open.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "vertical": {
                        "type": "string",
                        "description": "Which search page to build the URL for.",
                        "enum": ["integrated", "web", "image", "news", "video", "blog"],
                        "default": "integrated"
                    }
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "naver_list_verticals",
            "List the Naver search verticals this server exposes, with the tool name and paging behaviour of each, plus the tabs that exist but are not supported. Call this to decide which search tool fits a question.",
            json!({"type": "object", "properties": {}}),
        ),
    ]
}

// ── Dispatch ──

// ── Error form (unified with Baidu_MCP / Yahoo-Japan-Search-MCP) ──
//
// A search that *ran* but failed on Naver's side (HTTP error, timeout, a
// page layout change, an unavailable deep page, …) is reported as a
// structured tool result — `{provider, ok:false, vertical,
// error:{kind, message}}` — the same form the other search MCPs use, so
// agents can branch on `ok` uniformly. Argument errors (missing query)
// and unknown tools are not search failures: they still surface as MCP
// `isError` results via `Err` here. Successful results keep the standard
// envelope shape untouched.

/// Classify a provider error into a stable `kind` string. The provider
/// prefixes its messages with the kind (`"timeout: …"`,
/// `"access_restricted: …"`, `"parse_error: …"`, `"unsupported_page: …"`);
/// unprefixed errors fall back to reqwest downcasting.
fn error_kind(e: &anyhow::Error) -> &'static str {
    let msg = e.to_string();
    for kind in [
        "access_restricted",
        "unsupported_page",
        "timeout",
        "connection_error",
        "http_error",
        "parse_error",
    ] {
        if msg.starts_with(&format!("{kind}:")) {
            return match kind {
                "access_restricted" => "access_restricted",
                "unsupported_page" => "unsupported_page",
                "timeout" => "timeout",
                "connection_error" => "connection_error",
                "http_error" => "http_error",
                _ => "parse_error",
            };
        }
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

pub async fn dispatch(name: &str, args: &Value, client: &NaverSearchClient) -> Result<Value> {
    match name {
        "naver_search" => match client
            .search(&query_param(args)?, int_param(args, "limit", 5))
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("integrated", e)),
        },
        "naver_web_search" => match client
            .web_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("web", e)),
        },
        "naver_image_search" => match client
            .image_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("image", e)),
        },
        "naver_news_search" => match client
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
        "naver_video_search" => match client
            .video_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("video", e)),
        },
        "naver_blog_search" => match client
            .blog_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("blog", e)),
        },
        "naver_cafe_search" => match client
            .cafe_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("cafe", e)),
        },
        "naver_kin_search" => match client
            .kin_search(
                &query_param(args)?,
                int_param(args, "page", 1),
                int_param(args, "limit", 10),
            )
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("kin", e)),
        },
        "naver_suggest" => match client.suggest(&query_param(args)?).await {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("suggest", e)),
        },
        "naver_build_search_url" => {
            let vertical = {
                let v = str_param(args, "vertical");
                if v.is_empty() {
                    "integrated".to_string()
                } else {
                    v
                }
            };
            match naver_search::build_search_url(&query_param(args)?, &vertical) {
                Ok(url) => Ok(json!({
                    "provider": PROVIDER,
                    "ok": true,
                    "vertical": naver_search::urlbuild::normalize_vertical(&vertical),
                    "url": url,
                })),
                Err(e) => Ok(error_result(
                    naver_search::urlbuild::normalize_vertical(&vertical),
                    e,
                )),
            }
        }
        "naver_list_verticals" => Ok(naver_search::verticals()),
        other => Err(anyhow!("Unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_param_accepts_aliases() {
        assert_eq!(query_param(&json!({"q": "테스트"})).unwrap(), "테스트");
        assert_eq!(query_param(&json!({"keyword": " 서울 "})).unwrap(), "서울");
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
        assert_eq!(v["provider"], "naver");
        assert_eq!(v["ok"], false);
        assert_eq!(v["vertical"], "web");
        assert_eq!(v["error"]["kind"], "search_error");
        assert_eq!(v["error"]["message"], "boom");
    }

    #[test]
    fn error_kind_reads_provider_prefixes() {
        let e = anyhow!("unsupported_page: the blog tab serves only its first batch");
        assert_eq!(error_kind(&e), "unsupported_page");
        let v = error_result("blog", e);
        assert_eq!(v["error"]["kind"], "unsupported_page");

        let e = anyhow!(
            "parse_error: web result blocks not found (Naver page layout may have changed)"
        );
        assert_eq!(error_kind(&e), "parse_error");

        let e = anyhow!("access_restricted: HTTP 429 Too Many Requests from Naver");
        assert_eq!(error_kind(&e), "access_restricted");
    }

    #[test]
    fn every_tool_has_a_schema_and_description() {
        for tool in tool_definitions() {
            assert!(!tool["name"].as_str().unwrap().is_empty());
            assert!(tool["description"].as_str().unwrap().len() > 40);
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
        assert_eq!(tool_definitions().len(), 11);
    }
}
