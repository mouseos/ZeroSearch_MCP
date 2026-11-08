//! Media-category tools: YouTube and Bilibili — video platforms
//! reached through each app's own protocol (YouTube's InnerTube,
//! Bilibili's polymer gRPC search), all as guests, no login.
//! (CSDN, a text article platform, lives in the articles category.)
//!
//! Scope notes (measured, see the provider crates' reports):
//! - YouTube's ANDROID player response carries no publish date or
//!   like count; the like count is merged in from the watch-next
//!   response and the exact view count / relative upload time from
//!   `updated_metadata`. No response in the app's own flow carries
//!   an absolute calendar publish date (the app works with the
//!   relative form), so `published_date` stays null.
//! - Bilibili search, video detail (viewunite View) and related
//!   videos (RelatesFeed) all work as a guest: the `-bin` device
//!   metadata travels base64-encoded on this ingress (round-2
//!   correction of the round-1 "cannot cross the edge" reading).

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
            "Search YouTube through the YouTube Android app's own InnerTube API (guest session, no login). `items` holds the video results (id, title, watch URL, channel, view count, duration); `results` holds every result kind in page order — videos, Shorts, playlists and channels — each tagged with its kind. Also returns the estimated total and a continuation token. Pass `continuation` (from a previous call) to fetch the next page; filters and sort are the app's own search parameters.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search keywords. Required unless `continuation` is given."},
                    "continuation": {"type": "string", "description": "Continuation token from a previous youtube_search_videos result (`next_continuation`); fetches the next page of that search."},
                    "sort": {"type": "string", "enum": ["relevance", "rating", "upload_date", "view_count"], "description": "Sort order (default relevance)."},
                    "upload_date": {"type": "string", "enum": ["hour", "today", "week", "month", "year"], "description": "Only videos uploaded within this window."},
                    "type": {"type": "string", "enum": ["video", "channel", "playlist", "movie"], "description": "Result type filter (app search parameter)."},
                    "duration": {"type": "string", "enum": ["short", "medium", "long"], "description": "Duration filter: short (under 4 min), medium (4-20 min), long (over 20 min)."}
                },
                "required": []
            }),
        ),
        tool_def(
            "youtube_get_video",
            "Get one YouTube video's details through the app's InnerTube API: title, channel, length, view count, description, keywords, thumbnail, caption tracks. The like count is merged from the watch-next response and the exact view count plus the relative upload time (upload_time_text, e.g. \"5y ago\") from updated_metadata — the same entities the app renders. No response in the app's flow carries an absolute calendar publish date, so published_date stays null.",
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
            "Get the related videos shown next to a YouTube video (the app's watch-next list, about 20 entries with title, channel, view count and duration), the watched video's like count, and the continuation tokens: related_continuation pages this list (pass it back as `continuation`), comments_continuation opens the comments section — pass it to youtube_get_comments.",
            json!({
                "type": "object",
                "properties": {
                    "video_id": {"type": "string", "description": "Video id, or a watch / Shorts / youtu.be URL. Required unless `continuation` is given."},
                    "continuation": {"type": "string", "description": "Related-list continuation token from a previous youtube_get_related result (`related_continuation`)."}
                },
                "required": []
            }),
        ),
        tool_def(
            "youtube_get_captions",
            "Get a YouTube video's caption transcript through the app's own two-step caption flow (player-issued track URL, then the timedtext document). Returns the cues with timestamps plus the joined text. `language` selects a track by language code; without it a manual track is preferred over the automatic (ASR) one.",
            json!({
                "type": "object",
                "properties": {
                    "video_id": {"type": "string", "description": "Video id, or a watch / Shorts / youtu.be URL."},
                    "language": {"type": "string", "description": "Caption language code (e.g. \"en\", \"ja\")."}
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
            "bilibili_get_video",
            "Get one Bilibili video's details through the app's viewunite View call, as a guest — no login. Identify the video by BV id / video URL (video_id) or by numeric aid. Returns title, author, cover, description, duration, play/danmaku/like/coin/favorite/share counts, publish time and the part (分P) list.",
            json!({
                "type": "object",
                "properties": {
                    "video_id": {"type": "string", "description": "BV id (e.g. \"BV1xx411c7mD\") or a bilibili video URL."},
                    "aid": {"type": "integer", "description": "Numeric av id; alternative to video_id."}
                },
                "required": []
            }),
        ),
        tool_def(
            "bilibili_get_related",
            "Get the related videos Bilibili shows next to a video (viewunite RelatesFeed), as a guest — no login. Identify the video by BV id / video URL (video_id) or by numeric aid. `next` continues the feed.",
            json!({
                "type": "object",
                "properties": {
                    "video_id": {"type": "string", "description": "BV id or a bilibili video URL."},
                    "aid": {"type": "integer", "description": "Numeric av id; alternative to video_id."},
                    "limit": {"type": "integer", "description": "Page size.", "default": 20},
                    "next": {"type": "string", "description": "Pagination token from a previous result; omit for the first page."}
                },
                "required": []
            }),
        ),
        tool_def(
            "bilibili_search_videos",
            "Search Bilibili videos through the Bilibili Android app's own gRPC search (SearchAll), as a guest — no login. Returns title, BV id, author, cover, duration, play and danmaku counts and the watch URL, plus a `next` token for paging.",
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
                Ok(details) => {
                    let mut value = details.to_value();
                    // Best-effort enrichment from the two other
                    // surfaces the app itself renders: the
                    // watch-next like count and updated_metadata
                    // (exact view count, like count, relative
                    // upload time). Their failure never fails the
                    // detail call.
                    if value["like_count"].is_null() {
                        if let Ok(next) = p.youtube.get_watch_next(&id).await {
                            if let Some(likes) = next.like_count {
                                value["like_count"] = json!(likes);
                            }
                            if let Some(text) = &next.like_count_text {
                                value["like_count_text"] = json!(text);
                            }
                        }
                    }
                    if let Ok(meta) = p.youtube.get_updated_metadata(&id).await {
                        if let Some(views) = meta.view_count {
                            value["view_count"] = json!(views);
                        }
                        if value["like_count"].is_null() {
                            if let Some(likes) = meta.like_count {
                                value["like_count"] = json!(likes);
                            }
                        }
                        value["view_count_text"] = json!(meta.view_count_text);
                        if value
                            .get("like_count_text")
                            .map(|v| v.is_null())
                            .unwrap_or(true)
                        {
                            value["like_count_text"] = json!(meta.like_count_text);
                        }
                        value["upload_time_text"] = json!(meta.upload_time_text);
                        value["upload_time_a11y_text"] = json!(meta.upload_time_a11y_text);
                    }
                    Ok(value)
                }
                Err(e) => Ok(youtube_error_result(e)),
            }
        }
        "youtube_get_related" => {
            let token = str_param(args, "continuation");
            if !token.is_empty() {
                return match p.youtube.get_related_continuation(&token).await {
                    Ok(next) => Ok(next.to_value()),
                    Err(e) => Ok(youtube_error_result(e)),
                };
            }
            let id = video_id_param(args)?;
            match p.youtube.get_watch_next(&id).await {
                Ok(next) => Ok(next.to_value()),
                Err(e) => Ok(youtube_error_result(e)),
            }
        }
        "youtube_get_captions" => {
            let id = video_id_param(args)?;
            let language = str_param(args, "language");
            let language = if language.is_empty() {
                None
            } else {
                Some(language.as_str())
            };
            match p.youtube.get_captions(&id, language).await {
                Ok(transcript) => {
                    let mut value = transcript.to_value();
                    value["text"] = json!(transcript.full_text());
                    Ok(value)
                }
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
        "bilibili_get_video" => bilibili_get_video(args, p).await,
        "bilibili_get_related" => bilibili_get_related(args, p).await,
        other => Err(anyhow!("Unknown tool: {other}")),
    }
}

/// Extract a BV id from a bare id or a bilibili video URL.
fn bvid_param(args: &Value) -> Option<String> {
    let raw = str_param(args, "video_id");
    if raw.is_empty() {
        return None;
    }
    if let Some(idx) = raw.find("BV") {
        let rest = &raw[idx..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if !id.is_empty() {
            return Some(id);
        }
    }
    Some(raw)
}

async fn bilibili_get_video(args: &Value, p: &Providers) -> Result<Value> {
    let aid = args.get("aid").and_then(|v| v.as_u64());
    let result = match (bvid_param(args), aid) {
        (_, Some(aid)) => p.bilibili.get_video_by_aid(aid).await,
        (Some(bvid), None) => p.bilibili.get_video(&bvid).await,
        (None, None) => return Err(anyhow!("`video_id` (BV id or URL) or `aid` is required")),
    };
    match result {
        Ok(value) => Ok(value),
        Err(e) => Ok(bilibili_provider::error_json(&e)),
    }
}

async fn bilibili_get_related(args: &Value, p: &Providers) -> Result<Value> {
    let aid = args.get("aid").and_then(|v| v.as_u64());
    let page_size = int_param(args, "limit", 20) as i32;
    let next = str_param(args, "next");
    let result = match (bvid_param(args), aid) {
        (_, Some(aid)) => p.bilibili.get_related_by_aid(aid, page_size, &next).await,
        (Some(bvid), None) => p.bilibili.get_related(&bvid, page_size, &next).await,
        (None, None) => return Err(anyhow!("`video_id` (BV id or URL) or `aid` is required")),
    };
    match result {
        Ok(value) => Ok(value),
        Err(e) => Ok(bilibili_provider::error_json(&e)),
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
        assert_eq!(defs.len(), 8);
        for tool in defs {
            assert!(!tool["name"].as_str().unwrap().is_empty());
            assert!(tool["description"].as_str().unwrap().len() > 40);
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }
}
