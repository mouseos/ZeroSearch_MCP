// Offline parsing tests. The fixtures are trimmed REAL gateway
// responses captured live on 2026-11-07 (query "rust"), not
// synthetic data: ../live/raw-search.json and ../live/raw-detail.json
// hold the untouched captures.
use csdn_search::{extract_article_id, parse_article_response, parse_search_response};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let raw = match name {
        "search" => include_str!("fixtures/search.json"),
        "detail" => include_str!("fixtures/detail.json"),
        _ => unreachable!(),
    };
    serde_json::from_str(raw).unwrap()
}

#[test]
fn search_response_maps_items() {
    let out = parse_search_response(&fixture("search"), "rust", 1, 20);
    assert_eq!(out["provider"], "csdn");
    assert_eq!(out["ok"], true);
    assert_eq!(out["count"], 2);
    // The live v6 response carries no per-item total.
    assert!(out["total_available"].is_null());
    let first = &out["items"][0];
    assert_eq!(first["id"], "152301757");
    // Titles carry the server's <em> highlight markup verbatim.
    assert!(first["title"].as_str().unwrap().contains("<em>Rust</em>"));
    assert_eq!(first["author"], "gitblog_00700");
    assert_eq!(
        first["url"],
        "https://blog.csdn.net/gitblog_00700/article/details/152301757"
    );
    // viewcount arrives as a JSON string in the live response.
    assert_eq!(first["view_count"], 919);
    assert_eq!(first["published_at"], "2025-09-30 00:00:00");
    assert_eq!(out["items"][1]["id"], "87755263");
}

#[test]
fn article_response_maps_body() {
    let out = parse_article_response(&fixture("detail"), "152301757");
    assert_eq!(out["ok"], true);
    let item = &out["item"];
    assert_eq!(item["id"], "152301757");
    assert!(item["title"].as_str().unwrap().contains("Rust Enhanced"));
    assert_eq!(item["author"], "gitblog_00700");
    assert_eq!(item["author_name"], "成旭涛Strange");
    let html = item["content_html"].as_str().unwrap();
    assert!(html.len() > 10_000);
    let text = item["content_text"].as_str().unwrap();
    assert!(text.contains("Rust Enhanced"));
    assert!(!text.contains("<article"));
    assert_eq!(item["view_count"], 919);
    assert_eq!(item["comment_count"], 0);
    assert_eq!(item["digg_count"], 23);
    assert_eq!(item["collect_count"], 21);
    assert!(item["tags"].is_array());
}

#[test]
fn article_id_extraction() {
    assert_eq!(extract_article_id("152301757"), "152301757");
    assert_eq!(
        extract_article_id("https://blog.csdn.net/gitblog_00700/article/details/152301757"),
        "152301757"
    );
    assert_eq!(
        extract_article_id(
            "https://blog.csdn.net/gitblog_00700/article/details/152301757?spm=1001.2014.3001.5501"
        ),
        "152301757"
    );
}

#[test]
fn html_to_text_handles_blocks_and_entities() {
    let text = csdn_search::html_to_text("<p>a</p><p>b&amp;c</p><pre>x&lt;y</pre>");
    assert!(text.contains("a\nb&c"));
    assert!(text.contains("x<y"));
}
