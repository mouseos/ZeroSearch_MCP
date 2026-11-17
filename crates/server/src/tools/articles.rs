//! Articles-category tools: CSDN — a technical article / blog
//! platform reached through the app's own signed gateway API, as a
//! guest, no login.

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::Providers;

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

/// The tool definitions, kept as data (`defs/articles.json`) and
/// parsed once: building them with `json!` compiled to hundreds of
/// KiB of machine code for what is a constant document.
static DEFS: std::sync::LazyLock<Vec<Value>> =
    std::sync::LazyLock::new(|| super::parse_defs(include_str!("defs/articles.json")));

// ── Dispatch ──

pub async fn dispatch(name: &str, args: &Value, p: &Providers) -> Result<Value> {
    match name {
        "csdn_search_articles" => {
            let query = query_param(args)?;
            let page = int_param(args, "page", 1).max(1) as u32;
            let limit = int_param(args, "limit", 20).clamp(1, 50) as u32;
            let category_name = str_param(args, "category");
            let category = if category_name.is_empty() {
                csdn_search::SearchCategory::All
            } else {
                csdn_search::SearchCategory::parse(&category_name).ok_or_else(|| {
                    anyhow!(
                        "unknown category: {category_name} (valid: all, blog, ask, download, course, mall, ebook, user, vip, special_column; the app has no forum search)"
                    )
                })?
            };
            let sort = args.get("sort").and_then(Value::as_str);
            let filter = args.get("filter").and_then(Value::as_str);
            let subfilter = args.get("subfilter").and_then(Value::as_str);
            let search_user_name = args.get("search_user_name").and_then(Value::as_str);
            let mut options = csdn_search::SearchOptions::new(&query);
            options.page = page;
            options.limit = limit;
            options.category = category;
            options.sort = sort;
            options.filter = filter;
            options.subfilter = subfilter;
            options.search_user_name = search_user_name;
            Ok(csdn_search::search_with_options(&p.csdn, &options).await)
        }
        "csdn_get_article" => {
            let article_id = str_param(args, "article_id");
            if article_id.is_empty() {
                return Err(anyhow!(
                    "`article_id` is required and must be a non-empty string"
                ));
            }
            Ok(csdn_search::get_article(&p.csdn, &article_id).await)
        }
        other => Err(anyhow!("Unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_schema_and_description() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 2);
        for tool in defs {
            assert!(!tool["name"].as_str().unwrap().is_empty());
            assert!(tool["description"].as_str().unwrap().len() > 40);
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }
}
