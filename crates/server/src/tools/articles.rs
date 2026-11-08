//! Articles-category tools: CSDN — a technical article / blog
//! platform reached through the app's own signed gateway API, as a
//! guest, no login.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

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

fn tool_def(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema,
    })
}

// ── Tool definitions ──

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "csdn_search_articles",
            "Search CSDN articles through the CSDN app's own gateway API (the same assembled search the Android app uses). Returns article titles, authors, digests, URLs and view counts. Works without login.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search keywords. Chinese works best; CSDN is a Chinese developer platform."},
                    "page": {"type": "integer", "minimum": 1, "description": "1-based page (default 1)"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50, "description": "Results per page (default 20)"},
                    "sort": {"type": "string", "description": "Optional sort key passed to the app's second_rank_name_sort field"}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "csdn_get_article",
            "Fetch a CSDN blog article (title, author, full body HTML and text, tags, counts) through the CSDN app's BlogPhoenix API. Works without login.",
            json!({
                "type": "object",
                "properties": {
                    "article_id": {"type": "string", "description": "Numeric article id, or a blog.csdn.net article URL"}
                },
                "required": ["article_id"]
            }),
        ),
    ]
}

// ── Dispatch ──

pub async fn dispatch(name: &str, args: &Value, p: &Providers) -> Result<Value> {
    match name {
        "csdn_search_articles" => {
            let query = query_param(args)?;
            let page = int_param(args, "page", 1).max(1) as u32;
            let limit = int_param(args, "limit", 20).clamp(1, 50) as u32;
            let sort = args.get("sort").and_then(Value::as_str);
            Ok(csdn_search::search_articles(&p.csdn, &query, page, limit, sort).await)
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
