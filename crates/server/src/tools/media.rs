//! Media-category tools: YouTube and Bilibili — video platforms
//! reached through each app's own protocol (YouTube's InnerTube,
//! Bilibili's polymer gRPC search), all as guests, no login.
//! (CSDN, a text article platform, lives in the articles category.)
//!
//! Scope notes (measured, see the provider crates' reports):
//! - YouTube's ANDROID player response carries no publish date or
//!   like count; those fields come back null.
//! - Bilibili exposes search only. The app's View / RelatesFeed
//!   endpoints refuse requests without the app's device context
//!   (business error -400 / empty feed), and the metadata carrying
//!   that context cannot cross the service's public edge, so no
//!   detail or related tools are offered.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use youtube_search::filters::{Duration, ResultType, SearchFilters, SearchSort, UploadDate};
use youtube_search::YoutubeError;

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

fn youtube_error_result(err: YoutubeError) -> Value {
    json!({
        "provider": "youtube",
        "ok": false,
        "error": err.to_value(),
    })
}

/// Accept a bare video id, a watch URL or a Shorts URL and return
/// the bare id.
fn video_id_param(args: &Value) -> Result<String> {
    let raw = str_param(args, "video_id");
    let raw = if raw.is_empty() {
        str_param(args, "url")
    } else {
        raw
    };
    if raw.is_empty() {
        return Err(anyhow!(
            "`video_id` is required and must be a non-empty string"
        ));
    }
    if let Some(idx) = raw.find("watch?v=") {
        let rest = &raw[idx + "watch?v=".len()..];
        return Ok(rest.split(['&', '#']).next().unwrap_or(rest).to_string());
    }
    for marker in ["youtu.be/", "/shorts/", "/embed/"] {
        if let Some(idx) = raw.find(marker) {
            let rest = &raw[idx + marker.len()..];
            return Ok(rest
                .split(['?', '&', '/', '#'])
                .next()
                .unwrap_or(rest)
                .to_string());
        }
    }
    Ok(raw)
}

// ── Tool definitions ──

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "youtube_search_videos",
            "Search YouTube videos through the YouTube Android app's own InnerTube API (guest session, no login). Returns video id, title, watch URL, channel, view count and duration, plus the estimated total and a continuation token. Pass `continuation` (from a previous call) to fetch the next page; filters and sort are the app's own search parameters.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search keywords. Required unless `continuation` is given."},
                    "continuation": {"type": "string", "description": "Continuation token from a previous youtube_search_videos result (`next_continuation`); fetches the next page of that search."},
                    "sort": {"type": "string", "enum": ["relevance", "rating", "upload_date", "view_count"], "description": "Sort order (default relevance)."},
                    "upload_date": {"type": "string", "enum": ["hour", "today", "week", "month", "year"], "description": "Only videos uploaded within this window."},
                    "type": {"type": "string", "enum": ["video", "channel", "playlist", "movie"], "description": "Result type filter. Note: only video results are extracted into items."},
                    "duration": {"type": "string", "enum": ["short", "medium", "long"], "description": "Duration filter: short (under 4 min), medium (4-20 min), long (over 20 min)."}
                },
                "required": []
            }),
        ),
        tool_def(
            "youtube_get_video",
            "Get one YouTube video's details through the app's InnerTube player API: title, channel, length, view count, description, keywords, thumbnail, caption-track availability. Note: the Android client's player response carries no publish date or like count, so those fields are null.",
            json!({
                "type": "object",
                "properties": {
                    "video_id": {"type": "string", "description": "Video id (e.g. \"5C_HPTJg5ek\"), or a watch / Shorts / youtu.be URL."}
                },
                "required": ["video_id"]
            }),
        ),
        tool_def(
            "youtube_get_related",
            "Get the related videos shown next to a YouTube video (the app's watch-next list, about 20 entries with title, channel, view count and duration), plus the continuation token that opens the comments section — pass it to youtube_get_comments.",
            json!({
                "type": "object",
                "properties": {
                    "video_id": {"type": "string", "description": "Video id, or a watch / Shorts / youtu.be URL."}
                },
                "required": ["video_id"]
            }),
        ),
        tool_def(
            "youtube_get_comments",
            "Get one page of a YouTube video's comments (about 20, with author, text, like count text and reply count) through the app's InnerTube next API. The first page's token comes from youtube_get_related (`comments_continuation`); the result's `next_continuation` fetches the following page.",
            json!({
                "type": "object",
                "properties": {
                    "continuation": {"type": "string", "description": "Comments continuation token from youtube_get_related or a previous youtube_get_comments result."}
                },
                "required": ["continuation"]
            }),
        ),
        tool_def(
            "bilibili_search_videos",
            "Search Bilibili videos through the Bilibili Android app's own gRPC search (SearchAll), as a guest — no login. Returns title, BV id, author, cover, duration, play and danmaku counts and the watch URL, plus a `next` token for paging. Search only: the app's detail/related endpoints refuse requests without the app's device context, so they are not offered.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search keywords. Chinese works best; Bilibili is a Chinese platform."},
                    "limit": {"type": "integer", "description": "Page size (results per page).", "default": 20},
                    "next": {"type": "string", "description": "Pagination token from a previous result's `next` field; omit for the first page."},
                    "order": {"type": "integer", "description": "Sort order code of the app's SearchAll request (0 = comprehensive, the app default). Passed through unchanged.", "default": 0},
                    "tid_list": {"type": "string", "description": "Region (tid) filter, in the app's own list format. Passed through unchanged."},
                    "duration_list": {"type": "string", "description": "Duration filter, in the app's own list format. Passed through unchanged."},
                    "pub_time_begin_s": {"type": "integer", "description": "Only videos published at or after this unix timestamp (seconds)."},
                    "pub_time_end_s": {"type": "integer", "description": "Only videos published at or before this unix timestamp (seconds)."}
                },
                "required": ["query"]
            }),
        ),
    ]
}

// ── Dispatch ──

pub async fn dispatch(name: &str, args: &Value, p: &Providers) -> Result<Value> {
    match name {
        "youtube_search_videos" => youtube_search(args, p).await,
        "youtube_get_video" => {
            let id = video_id_param(args)?;
            match p.youtube.get_video(&id).await {
                Ok(details) => Ok(details.to_value()),
                Err(e) => Ok(youtube_error_result(e)),
            }
        }
        "youtube_get_related" => {
            let id = video_id_param(args)?;
            match p.youtube.get_watch_next(&id).await {
                Ok(next) => Ok(next.to_value()),
                Err(e) => Ok(youtube_error_result(e)),
            }
        }
        "youtube_get_comments" => {
            let token = str_param(args, "continuation");
            if token.is_empty() {
                return Err(anyhow!(
                    "`continuation` is required and must be a non-empty string"
                ));
            }
            match p.youtube.get_comments(&token).await {
                Ok(page) => Ok(page.to_value()),
                Err(e) => Ok(youtube_error_result(e)),
            }
        }
        "bilibili_search_videos" => bilibili_search(args, p).await,
        other => Err(anyhow!("Unknown tool: {other}")),
    }
}

async fn youtube_search(args: &Value, p: &Providers) -> Result<Value> {
    let continuation = str_param(args, "continuation");
    if !continuation.is_empty() {
        return match p.youtube.search_continuation(&continuation).await {
            Ok(page) => Ok(page.to_value(&str_param(args, "query"))),
            Err(e) => Ok(youtube_error_result(e)),
        };
    }
    let query = query_param(args)?;
    let mut filters = SearchFilters::default();
    let sort = str_param(args, "sort");
    if !sort.is_empty() {
        filters.sort =
            SearchSort::from_str(&sort).ok_or_else(|| anyhow!("unknown sort: {sort}"))?;
    }
    let upload_date = str_param(args, "upload_date");
    if !upload_date.is_empty() {
        filters.upload_date = Some(
            UploadDate::from_str(&upload_date)
                .ok_or_else(|| anyhow!("unknown upload_date: {upload_date}"))?,
        );
    }
    let result_type = str_param(args, "type");
    if !result_type.is_empty() {
        filters.result_type = Some(
            ResultType::from_str(&result_type)
                .ok_or_else(|| anyhow!("unknown type: {result_type}"))?,
        );
    }
    let duration = str_param(args, "duration");
    if !duration.is_empty() {
        filters.duration = Some(
            Duration::from_str(&duration).ok_or_else(|| anyhow!("unknown duration: {duration}"))?,
        );
    }
    match p.youtube.search_videos(&query, &filters).await {
        Ok(page) => Ok(page.to_value(&query)),
        Err(e) => Ok(youtube_error_result(e)),
    }
}

async fn bilibili_search(args: &Value, p: &Providers) -> Result<Value> {
    use bilibili_provider::proto::{Pagination, SearchAllRequest};
    let query = query_param(args)?;
    let page_size = int_param(args, "limit", 20);
    let req = SearchAllRequest {
        keyword: query.clone(),
        order: int_param(args, "order", 0) as i32,
        tid_list: str_param(args, "tid_list"),
        duration_list: str_param(args, "duration_list"),
        from_source: "search".to_string(),
        pagination: Some(Pagination {
            page_size: if page_size <= 0 { 20 } else { page_size as i32 },
            next: str_param(args, "next"),
        }),
        pub_time_begin_s: int_param(args, "pub_time_begin_s", 0),
        pub_time_end_s: int_param(args, "pub_time_end_s", 0),
        ..Default::default()
    };
    match p.bilibili.search_all(req).await {
        Ok(resp) => Ok(bilibili_provider::search_response_json(&query, &resp)),
        Err(e) => Ok(bilibili_provider::error_json(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_id_param_extracts_ids() {
        assert_eq!(
            video_id_param(&json!({"video_id": "5C_HPTJg5ek"})).unwrap(),
            "5C_HPTJg5ek"
        );
        assert_eq!(
            video_id_param(&json!({"video_id": "https://www.youtube.com/watch?v=5C_HPTJg5ek&t=3"}))
                .unwrap(),
            "5C_HPTJg5ek"
        );
        assert_eq!(
            video_id_param(&json!({"video_id": "https://youtu.be/5C_HPTJg5ek"})).unwrap(),
            "5C_HPTJg5ek"
        );
        assert_eq!(
            video_id_param(&json!({"url": "https://www.youtube.com/shorts/5C_HPTJg5ek"})).unwrap(),
            "5C_HPTJg5ek"
        );
        assert!(video_id_param(&json!({})).is_err());
    }

    #[test]
    fn every_tool_has_a_schema_and_description() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);
        for tool in defs {
            assert!(!tool["name"].as_str().unwrap().is_empty());
            assert!(tool["description"].as_str().unwrap().len() > 40);
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }
}
