//! Offline parser tests against real InnerTube responses captured
//! during live verification (guest session, ANDROID client,
//! 2026-11-07). Fixtures are unmodified server responses.

use serde_json::Value;
use youtube_search::next::{parse_comments_response, parse_next_response};
use youtube_search::player::parse_player_response;
use youtube_search::search::parse_search_response;

fn load(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(&path).expect("fixture readable");
    serde_json::from_str(&raw).expect("fixture is valid JSON")
}

#[test]
fn search_fixture_parses_compact_renderers() {
    let page = parse_search_response(&load("search.json"));
    assert!(page.items.len() >= 15, "got {} items", page.items.len());
    let first = &page.items[0];
    assert_eq!(first.video_id, "5C_HPTJg5ek");
    assert_eq!(first.title, "Rust in 100 Seconds");
    assert_eq!(first.channel_name, "Fireship");
    assert!(first.view_count.unwrap_or(0) > 2_000_000);
    assert_eq!(first.duration_secs, Some(149));
    assert!(page.estimated_results.unwrap_or(0) > 1_000_000);
    assert!(
        page.next_continuation.is_some(),
        "search continuation present"
    );
    assert!(page
        .items
        .iter()
        .all(|i| !i.video_id.is_empty() && !i.title.is_empty()));
}

#[test]
fn next_fixture_parses_lockup_related_and_comments_token() {
    let wn = parse_next_response(&load("next.json"), "5C_HPTJg5ek");
    assert_eq!(wn.related.len(), 20, "related lockups parsed");
    assert!(wn
        .related
        .iter()
        .any(|i| i.video_id == "0y6RKiIk6cs" && i.title.contains("Rust for Dummies")));
    let sample = wn
        .related
        .iter()
        .find(|i| i.video_id == "0y6RKiIk6cs")
        .expect("sample present");
    assert_eq!(sample.channel_name, "Let's Get Rusty");
    assert_eq!(sample.duration_secs, Some(750));
    assert!(sample.view_count.unwrap_or(0) > 400_000);
    assert!(
        wn.comments_continuation.is_some(),
        "comments reload token extracted from the comment panel"
    );
}

#[test]
fn comments_fixture_parses_entities_in_thread_order() {
    let page = parse_comments_response(&load("comments.json"));
    assert_eq!(page.comments.len(), 20, "one page of comments");
    let first = &page.comments[0];
    assert_eq!(first.author, "@sznio");
    assert!(first.text.contains("Don't call rustc directly"));
    assert_eq!(first.comment_id, "UgwbOfn7CaLOqLDbF3t4AaABAg");
    assert!(page
        .comments
        .iter()
        .all(|c| !c.comment_id.is_empty() && !c.text.is_empty()));
    assert!(
        page.next_continuation.is_some(),
        "comments page-2 token present"
    );
}

#[test]
fn player_fixture_parses_core_details() {
    let d = parse_player_response(&load("player.json")).expect("playable");
    assert_eq!(d.video_id, "5C_HPTJg5ek");
    assert_eq!(d.title, "Rust in 100 Seconds");
    assert_eq!(d.channel_name, "Fireship");
    assert_eq!(d.length_secs, Some(149));
    assert!(d.view_count.unwrap_or(0) > 2_000_000);
    assert!(d.stream_format_count > 0, "streaming formats parsed");
    assert!(d.has_caption_tracks);
    // The ANDROID player response has no microformat section, so
    // publish date / like count are legitimately absent here.
    assert!(d.published_date.is_none());
    assert!(d.like_count.is_none());
}
