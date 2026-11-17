use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::args::{int_param, query_param, str_param};

use yandex_search::{YandexSearchClient, ACCESS_RESTRICTED_PREFIX, PROVIDER};

// ── Tool definitions ──

pub fn tool_definitions() -> Vec<Value> {
    DEFS.clone()
}

/// The tool definitions, kept as data (`defs/yandex.json`) and
/// parsed once: building them with `json!` compiled to hundreds of
/// KiB of machine code for what is a constant document.
static DEFS: std::sync::LazyLock<Vec<Value>> =
    std::sync::LazyLock::new(|| super::parse_defs(include_str!("defs/yandex.json")));

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
    if let Some(kind) = super::args::reqwest_error_kind(e, false) {
        return kind;
    }
    if message.contains("not found (Yandex page layout may have changed)") {
        "parse_error"
    } else {
        "search_error"
    }
}

/// The unified structured error result for a failed search.
fn error_result(vertical: &str, e: anyhow::Error) -> Value {
    super::args::engine_error_result(PROVIDER, vertical, error_kind(&e), &e)
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
