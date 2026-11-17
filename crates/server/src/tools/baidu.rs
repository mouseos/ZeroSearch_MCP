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
    DEFS.clone()
}

/// The tool definitions, kept as data (`defs/baidu.json`) and
/// parsed once: building them with `json!` compiled to hundreds of
/// KiB of machine code for what is a constant document.
static DEFS: std::sync::LazyLock<Vec<Value>> =
    std::sync::LazyLock::new(|| super::parse_defs(include_str!("defs/baidu.json")));

pub const TOOL_NAMES: &[&str] = &[
    "baidu_search",
    "baidu_search_page",
    "baidu_suggest",
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
            if let Some(page) = opt_i64(args, "page") {
                return match p.baidu.search_page_at(&query, page).await {
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
                };
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
        "baidu_suggest" => {
            let query = query_param(args);
            if query.is_empty() {
                anyhow::bail!("query is required");
            }
            let system = opt_str_param(args, "system").filter(|s| !s.is_empty());
            let want = |name: &str| system.as_deref().map(|s| s == name).unwrap_or(true);
            let mut suggestions: Vec<Value> = Vec::new();
            let mut systems = Map::new();
            let mut any_ok = false;
            let mut first_error: Option<Value> = None;
            let record_err = |systems: &mut Map<String, Value>,
                              name: &str,
                              e: baidu_search::BaiduError,
                              first_error: &mut Option<Value>| {
                let err = json!({
                    "kind": e.kind,
                    "message": e.message,
                    "url": e.url,
                    "status_code": e.status_code,
                });
                systems.insert(name.to_string(), json!({"ok": false, "error": err.clone()}));
                if first_error.is_none() {
                    *first_error = Some(err);
                }
            };
            if want("classic") {
                match p.baidu_suggest.suggest(&query).await {
                    Ok(entries) => {
                        any_ok = true;
                        systems.insert(
                            "classic".to_string(),
                            json!({"ok": true, "count": entries.len()}),
                        );
                        for e in &entries {
                            let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
                            if let Value::Object(m) = &mut v {
                                m.insert("system".to_string(), json!("classic"));
                            }
                            suggestions.push(v);
                        }
                    }
                    Err(e) => record_err(&mut systems, "classic", e, &mut first_error),
                }
            }
            if want("recommendations") {
                match p.baidu_suggest.suggest_recommendations(&query).await {
                    Ok(entries) => {
                        any_ok = true;
                        systems.insert(
                            "recommendations".to_string(),
                            json!({"ok": true, "count": entries.len()}),
                        );
                        for e in &entries {
                            let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
                            if let Value::Object(m) = &mut v {
                                m.insert("system".to_string(), json!("recommendations"));
                            }
                            suggestions.push(v);
                        }
                    }
                    Err(e) => record_err(&mut systems, "recommendations", e, &mut first_error),
                }
            }
            if want("inputbox") {
                match p.baidu_suggest.suggest_inputbox(&query).await {
                    Ok(ib) => {
                        any_ok = true;
                        let count = ib.chat_sug.len()
                            + ib.content_groups
                                .iter()
                                .map(|(_, g)| g.len())
                                .sum::<usize>();
                        systems.insert("inputbox".to_string(), json!({"ok": true, "count": count}));
                        for e in &ib.chat_sug {
                            let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
                            if let Value::Object(m) = &mut v {
                                m.insert("system".to_string(), json!("inputbox"));
                                m.insert("group".to_string(), json!("chat_sug"));
                            }
                            suggestions.push(v);
                        }
                        for (group, entries) in &ib.content_groups {
                            for e in entries {
                                let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
                                if let Value::Object(m) = &mut v {
                                    m.insert("system".to_string(), json!("inputbox"));
                                    m.insert("group".to_string(), json!(group));
                                }
                                suggestions.push(v);
                            }
                        }
                    }
                    Err(e) => record_err(&mut systems, "inputbox", e, &mut first_error),
                }
            }
            if !any_ok {
                return Ok(json!({
                    "provider": "baidu",
                    "ok": false,
                    "query": query,
                    "systems": Value::Object(systems),
                    "error": first_error.unwrap_or_else(|| json!({"kind": "no_systems", "message": "no suggestion system was queried"})),
                }));
            }
            Ok(json!({
                "provider": "baidu",
                "ok": true,
                "query": query,
                "suggestions": suggestions,
                "systems": Value::Object(systems),
            }))
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
