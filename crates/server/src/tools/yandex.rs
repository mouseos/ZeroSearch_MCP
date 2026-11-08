use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use yandex_search::{YandexSearchClient, ACCESS_RESTRICTED_PREFIX, PROVIDER};

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
    json!({"type": "string", "description": "Search query. Russian works best; Yandex indexes Russian-language content far more deeply than other engines."})
}

// ── Tool definitions ──

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "yandex_web_search",
            "Search the Yandex web index. Best general-purpose tool for finding Russian-language web pages, shops, official sites and documentation. Returns title, destination URL, displayed path and snippet. About 10 organic results per page; page is 1-based (the session flow is verified live for the first two pages). With include_special_cards the result also carries the page's non-organic cards (AI answer, ad blocks, related searches) under special_cards.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": {"type": "integer", "description": "1-based page number.", "default": 1},
                    "limit": {"type": "integer", "description": "Max results to return (1-10).", "default": 10},
                    "include_special_cards": {"type": "boolean", "description": "Also return the page's special cards (AI answer, ads, related searches).", "default": false}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "yandex_image_search",
            "Search Yandex images. Returns the original image URL, a Yandex thumbnail, pixel dimensions and the source page each image appears on. 20 results per page; page is 1-based.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": {"type": "integer", "description": "1-based page number.", "default": 1},
                    "limit": {"type": "integer", "description": "Max results to return (1-20).", "default": 20}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "yandex_video_search",
            "Search Yandex videos across VK, YouTube, RuTube, OK and other hosts. Returns title, the watch URL on the hosting service, duration, provider and publish date.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "page": {"type": "integer", "description": "1-based page number.", "default": 1},
                    "limit": {"type": "integer", "description": "Max results to return (1-20).", "default": 20}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "yandex_suggest",
            "Get Yandex autocomplete suggestions for a partial query. Useful for discovering how Russian users actually phrase a topic before running a full search.",
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
            "yandex_suggest_endings",
            "Get Yandex query-ending suggestions: the completions the search box offers for how a query could end (the suggest service's endings mode), as opposed to yandex_suggest's whole-query completions.",
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
            "yandex_build_search_url",
            "Build the Yandex search URL for a query and vertical (web, image, video, suggest) without performing a search. Useful for handing a link to the user. News has no URL builder: it moved to dzen.ru and requires Yandex SSO.",
            json!({
                "type": "object",
                "properties": {
                    "query": query_prop(),
                    "vertical": {
                        "type": "string",
                        "description": "Which search the URL opens.",
                        "enum": ["web", "image", "video", "suggest"],
                        "default": "web"
                    }
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "yandex_list_verticals",
            "List the Yandex search verticals this server exposes, with the tool name and paging behaviour of each. News is listed as unsupported, with the reason. Call this to decide which search tool fits a question.",
            json!({"type": "object", "properties": {}}),
        ),
    ]
}

// ── Error form (unified with the sibling search MCPs) ──
//
// A search that *ran* but failed on Yandex's side (HTTP error, timeout, a
// page layout change, restricted access, …) is reported as a structured
// tool result — `{provider, ok:false, vertical, error:{kind, message}}` —
// so agents can branch on `ok` uniformly across search MCPs. Argument
// errors (missing query) and unknown tools are not search failures: they
// still surface as MCP `isError` results via `Err` here. Successful results
// keep their existing shape untouched.

/// Classify a provider error into a stable `kind` string.
fn error_kind(e: &anyhow::Error) -> &'static str {
    let message = e.to_string();
    if message.starts_with(ACCESS_RESTRICTED_PREFIX) {
        return "access_restricted";
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
    if message.contains("not found (Yandex page layout may have changed)") {
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

pub async fn dispatch(name: &str, args: &Value, client: &YandexSearchClient) -> Result<Value> {
    match name {
        "yandex_web_search" => {
            let query = query_param(args)?;
            let page = int_param(args, "page", 1);
            let limit = int_param(args, "limit", 10);
            let special = args
                .get("include_special_cards")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if special {
                match client.web_search_detailed(&query, page, limit).await {
                    Ok(result) => Ok(serde_json::to_value(result)?),
                    Err(e) => Ok(error_result("web", e)),
                }
            } else {
                match client.web_search(&query, page, limit).await {
                    Ok(result) => Ok(serde_json::to_value(result)?),
                    Err(e) => Ok(error_result("web", e)),
                }
            }
        }
        "yandex_image_search" => match client
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
        "yandex_video_search" => match client
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
        "yandex_suggest" => match client
            .suggest(&query_param(args)?, int_param(args, "limit", 10))
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("suggest", e)),
        },
        "yandex_suggest_endings" => match client
            .suggest_endings(&query_param(args)?, int_param(args, "limit", 10))
            .await
        {
            Ok(result) => Ok(serde_json::to_value(result)?),
            Err(e) => Ok(error_result("suggest_endings", e)),
        },
        "yandex_build_search_url" => {
            let query = query_param(args)?;
            let vertical = {
                let v = str_param(args, "vertical");
                if v.is_empty() {
                    "web".to_string()
                } else {
                    v
                }
            };
            match yandex_search::build_search_url(&query, &vertical) {
                Ok(url) => Ok(json!({
                    "provider": PROVIDER,
                    "ok": true,
                    "vertical": vertical,
                    "query": query,
                    "url": url,
                })),
                Err(e) => Ok(error_result("build_search_url", e)),
            }
        }
        "yandex_list_verticals" => Ok(yandex_search::verticals()),
        other => Err(anyhow!("Unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_param_accepts_aliases() {
        assert_eq!(query_param(&json!({"q": "тест"})).unwrap(), "тест");
        assert_eq!(query_param(&json!({"keyword": " тест "})).unwrap(), "тест");
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
        assert_eq!(v["provider"], "yandex");
        assert_eq!(v["ok"], false);
        assert_eq!(v["vertical"], "web");
        assert_eq!(v["error"]["kind"], "search_error");
        assert_eq!(v["error"]["message"], "boom");
    }

    #[test]
    fn error_kind_classifies_restricted_access() {
        let e = anyhow!("access_restricted: Yandex restricted access (HTTP 429)");
        assert_eq!(error_kind(&e), "access_restricted");
        let v = error_result("web", e);
        assert_eq!(v["error"]["kind"], "access_restricted");
    }

    #[test]
    fn error_kind_classifies_layout_changes_as_parse_error() {
        let e =
            anyhow!("initial state (data-state) not found (Yandex page layout may have changed)");
        assert_eq!(error_kind(&e), "parse_error");
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
