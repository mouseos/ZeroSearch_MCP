//! Tool routing for the unified server.
//!
//! Each engine's handler module is the standalone server's
//! `tools.rs`, ported unchanged (tool names, arguments and result
//! JSON are identical to the standalone servers). This module only
//! concatenates their definitions, routes calls by tool-name
//! prefix, and adds the one ZeroSearch-native tool,
//! `zero_list_categories`.

pub mod baidu;
pub mod bing;
pub mod media;
pub mod naver;
pub mod shopping;
pub mod yahoo;
pub mod yahoo_us;
pub mod yandex;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::Providers;

/// All tool definitions exposed by the unified server: the six web
/// engines' tools, the shopping tools, the media tools, and
/// `zero_list_categories`.
pub fn tool_definitions() -> Vec<Value> {
    let mut defs = Vec::new();
    defs.extend(baidu::tool_definitions());
    defs.extend(yahoo::tool_definitions());
    defs.extend(bing::tool_definitions());
    defs.extend(naver::tool_definitions());
    defs.extend(yahoo_us::tool_definitions());
    defs.extend(yandex::tool_definitions());
    defs.extend(shopping::tool_definitions());
    defs.extend(media::tool_definitions());
    defs.push(zero_list_categories_definition());
    defs
}

fn tool_names(defs: &[Value]) -> Vec<String> {
    defs.iter()
        .filter_map(|d| d.get("name").and_then(Value::as_str).map(str::to_string))
        .collect()
}

fn names_with_prefix(defs: &[Value], prefix: &str) -> Vec<String> {
    tool_names(defs)
        .into_iter()
        .filter(|n| n.starts_with(prefix))
        .collect()
}

fn zero_list_categories_definition() -> Value {
    json!({
        "name": "zero_list_categories",
        "description": "List the ZeroSearch engine categories (web / shopping / media), the engines in each category, and the MCP tools that belong to each engine. Takes no arguments.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }
    })
}

/// The payload of the `zero_list_categories` tool: categories,
/// their engines, and each engine's tools, derived from the live
/// tool definitions so it can never drift from `tools/list`.
pub fn categories() -> Value {
    let shopping_defs = shopping::tool_definitions();
    let shopping_shared = vec!["shopping_list_sources".to_string()];
    let media_defs = media::tool_definitions();
    let engine = |id: &str, tools: Vec<String>| json!({ "id": id, "tools": tools });
    json!({
        "server": "ZeroSearch_MCP",
        "categories": [
            {
                "id": "web",
                "label": "Web search",
                "engines": [
                    engine("baidu", tool_names(&baidu::tool_definitions())),
                    engine("yahoo_jp", tool_names(&yahoo::tool_definitions())),
                    engine("bing", tool_names(&bing::tool_definitions())),
                    engine("naver", tool_names(&naver::tool_definitions())),
                    engine("yahoo_us", tool_names(&yahoo_us::tool_definitions())),
                    engine("yandex", tool_names(&yandex::tool_definitions())),
                ],
            },
            {
                "id": "shopping",
                "label": "Shopping search",
                "engines": [
                    engine("aliexpress", names_with_prefix(&shopping_defs, "aliexpress_")),
                    engine("goofish", names_with_prefix(&shopping_defs, "goofish_")),
                    engine("mercari", names_with_prefix(&shopping_defs, "mercari_")),
                    engine("yahoo_auctions", names_with_prefix(&shopping_defs, "yauction_")),
                    engine("yahoo_fleamarket", names_with_prefix(&shopping_defs, "yflea_")),
                    engine("rakuma", names_with_prefix(&shopping_defs, "rakuma_")),
                    engine("rakuten", names_with_prefix(&shopping_defs, "rakuten_")),
                    engine("qoo10", names_with_prefix(&shopping_defs, "qoo10_")),
                    engine("yahoo_shopping", names_with_prefix(&shopping_defs, "yshopping_")),
                    engine("amazon", names_with_prefix(&shopping_defs, "amazon_")),
                ],
                "shared_tools": shopping_shared,
            },
            {
                "id": "media",
                "label": "Media platforms",
                "engines": [
                    engine("youtube", names_with_prefix(&media_defs, "youtube_")),
                    engine("bilibili", names_with_prefix(&media_defs, "bilibili_")),
                    engine("csdn", names_with_prefix(&media_defs, "csdn_")),
                ],
            },
        ],
        "tool_count": tool_definitions().len(),
    })
}

/// True for the shopping tools (including `shopping_list_sources`).
/// The unified server reproduces the standalone Shopping server's
/// extra result block (the keep-product-URLs-verbatim note) for
/// exactly these tools.
pub fn is_shopping_tool(name: &str) -> bool {
    [
        "aliexpress_",
        "goofish_",
        "mercari_",
        "yauction_",
        "yflea_",
        "rakuma_",
        "rakuten_",
        "qoo10_",
        "yshopping_",
        "amazon_",
        "shopping_",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

/// Route one tool call to its engine module. Routing is by name
/// ownership (prefix), never by trial: `yahoo_us_` is tested before
/// `yahoo_` because the latter is a prefix of the former.
pub async fn dispatch(name: &str, args: &Value, p: &Providers) -> Result<Value> {
    match name {
        "zero_list_categories" => Ok(categories()),
        n if n.starts_with("yahoo_us_") => yahoo_us::dispatch(n, args, &p.yahoo_us).await,
        n if n.starts_with("baidu_") => baidu::dispatch(n, args, p).await,
        n if n.starts_with("yahoo_") => yahoo::dispatch(n, args, &p.yahoo).await,
        n if n.starts_with("bing_") => bing::dispatch(n, args, &p.bing).await,
        n if n.starts_with("naver_") => naver::dispatch(n, args, &p.naver).await,
        n if n.starts_with("yandex_") => yandex::dispatch(n, args, &p.yandex).await,
        n if n.starts_with("youtube_") || n.starts_with("bilibili_") || n.starts_with("csdn_") => {
            media::dispatch(n, args, p).await
        }
        n if is_shopping_tool(n) => shopping::dispatch(n, args, p).await,
        other => Err(anyhow!("Unknown tool: {other}")),
    }
}
