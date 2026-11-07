use anyhow::Result;
use serde_json::{json, Map, Value};

use crate::Providers;

fn str_param(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn opt_str_param(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Search query with the unified aliases: `query` is canonical,
/// `q` / `keyword` are also accepted (Yahoo convention, spec §2).
fn query_param(args: &Value) -> String {
    for key in ["query", "q", "keyword"] {
        let v = str_param(args, key);
        if !v.is_empty() {
            return v;
        }
    }
    String::new()
}

fn opt_query_param(args: &Value) -> Option<String> {
    let q = query_param(args);
    if q.is_empty() {
        None
    } else {
        Some(q)
    }
}

fn int_param(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
}

fn opt_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}

/// Prepend `provider` / `ok` to a provider payload object.
fn with_provider(mut value: Value, ok: bool) -> Value {
    let mut out = Map::new();
    out.insert("provider".to_string(), json!("baidu"));
    out.insert("ok".to_string(), json!(ok));
    if let Value::Object(m) = std::mem::take(&mut value) {
        for (k, v) in m {
            out.insert(k, v);
        }
    }
    Value::Object(out)
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "baidu_search",
            "Search Baidu through the APK-derived (baiduboxapp) anonymous search flow and return the aggregated multi-page result JSON (same shape as the Python baidu_search client, plus provider/ok). Per-page failures are recorded in diagnostics.errors and already-fetched pages are kept.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query (aliases: q, keyword)"},
                    "max_pages": {"type": "integer", "default": 3, "description": "Maximum number of result pages to fetch"}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "baidu_search_page",
            "Fetch and parse exactly one Baidu search-result page. With page_url omitted the first-page URL is built from query; otherwise page_url is fetched as-is (server-issued pagination URL). Failures return {provider, ok:false, error:{kind,message,url,status_code}} with the Python client's error kind.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query (aliases: q, keyword)"},
                    "page_url": {"type": "string", "description": "Optional server-issued page URL to fetch as-is"}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "baidu_parse_search_results",
            "Parse a saved Baidu (APK/wise) search-result HTML page into structured JSON, offline (no network). Returns the Python baidu_parser page dict plus provider/ok.",
            json!({
                "type": "object",
                "properties": {
                    "html": {"type": "string", "description": "Search-result HTML"},
                    "query": {"type": "string", "description": "Search query the page belongs to"},
                    "page_url": {"type": "string", "description": "URL the HTML was fetched from (for relative-URL resolution and pn detection)"}
                },
                "required": ["html"]
            }),
        ),
        tool_def(
            "baidu_build_search_url",
            "Build the first-page Baidu search URL for a query (APK bq0.m reproduction: pu merges, https upgrade, ant_ct, t_samp). now_ms pins the clock for reproducibility; omit for the current time.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query (aliases: q, keyword)"},
                    "now_ms": {"type": "integer", "description": "Optional epoch milliseconds to pin the generated URL"}
                },
                "required": ["query"]
            }),
        ),
        tool_def(
            "baidu_list_sources",
            "List the search providers and tools supported by Baidu_MCP.",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
    ]
}

fn tool_def(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
    })
}

pub const TOOL_NAMES: &[&str] = &[
    "baidu_search",
    "baidu_search_page",
    "baidu_parse_search_results",
    "baidu_build_search_url",
    "baidu_list_sources",
];

pub async fn dispatch(tool: &str, args: &Value, p: &Providers) -> Result<Value> {
    match tool {
        "baidu_search" => {
            let query = query_param(args);
            if query.is_empty() {
                anyhow::bail!("query is required");
            }
            let max_pages = int_param(args, "max_pages", 3);
            let agg = p
                .baidu
                .search(&query, max_pages)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let ok = agg.pages_fetched >= 1;
            Ok(with_provider(agg.to_value(), ok))
        }
        "baidu_search_page" => {
            let query = query_param(args);
            if query.is_empty() {
                anyhow::bail!("query is required");
            }
            let page_url = opt_str_param(args, "page_url").filter(|s| !s.is_empty());
            match p.baidu.search_page(&query, page_url.as_deref()).await {
                Ok(single) => Ok(with_provider(single.to_value(), true)),
                Err(e) => Ok(json!({
                    "provider": "baidu",
                    "ok": false,
                    "error": {
                        "kind": e.kind,
                        "message": e.message,
                        "url": e.url,
                        "status_code": e.status_code,
                    }
                })),
            }
        }
        "baidu_parse_search_results" => {
            let html = str_param(args, "html");
            if html.is_empty() {
                anyhow::bail!("html is required");
            }
            let query = opt_query_param(args);
            let page_url = opt_str_param(args, "page_url");
            let page =
                baidu_search::parse_search_results(&html, query.as_deref(), page_url.as_deref());
            Ok(with_provider(page.to_value(), true))
        }
        "baidu_build_search_url" => {
            let query = query_param(args);
            if query.is_empty() {
                anyhow::bail!("query is required");
            }
            let url =
                baidu_search::search_url::build_initial_search_url(&query, opt_i64(args, "now_ms"));
            Ok(json!({
                "provider": "baidu",
                "ok": true,
                "query": query,
                "url": url,
                "user_agent": baidu_search::search_url::user_agent(),
            }))
        }
        "baidu_list_sources" => Ok(json!({
            "provider": "baidu",
            "ok": true,
            "sources": [{
                "name": "baidu",
                "description": "Baidu (百度) mobile search via the Android app (baiduboxapp) flow: APK-derived URL generation with ant_ct, server-issued pagination, and structured parsing of wise result pages. Anonymous; no login or API key.",
                "tools": TOOL_NAMES,
            }]
        })),
        _ => anyhow::bail!("Unknown tool: {}", tool),
    }
}
