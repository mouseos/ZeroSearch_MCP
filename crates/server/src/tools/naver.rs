use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::args::{int_param, query_param, str_param};

use naver_search::{NaverSearchClient, PROVIDER};

// ── Tool definitions ──

pub fn tool_definitions() -> Vec<Value> {
    DEFS.clone()
}

/// The tool definitions, kept as data (`defs/naver.json`) and
/// parsed once: building them with `json!` compiled to hundreds of
/// KiB of machine code for what is a constant document.
static DEFS: std::sync::LazyLock<Vec<Value>> =
    std::sync::LazyLock::new(|| super::parse_defs(include_str!("defs/naver.json")));

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
        if msg
            .strip_prefix(kind)
            .is_some_and(|rest| rest.starts_with(':'))
        {
            return kind;
        }
    }
    if let Some(kind) = super::args::reqwest_error_kind(e, false) {
        return kind;
    }
    "search_error"
}

/// The unified structured error result for a failed search.
fn error_result(vertical: &str, e: anyhow::Error) -> Value {
    super::args::engine_error_result(PROVIDER, vertical, error_kind(&e), &e)
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
