//! Walking helpers for InnerTube renderer trees and the shared
//! extraction of video items from `videoRenderer` (search) and
//! `compactVideoRenderer` (watch-next related) objects.

use serde_json::{json, Value};

/// Concatenate a `simpleText` or `runs[]` text object into a String.
pub fn text_of(v: &Value) -> String {
    if let Some(s) = v.get("simpleText").and_then(|t| t.as_str()) {
        return s.to_string();
    }
    if let Some(runs) = v.get("runs").and_then(|r| r.as_array()) {
        let mut out = String::new();
        for run in runs {
            if let Some(t) = run.get("text").and_then(|t| t.as_str()) {
                out.push_str(t);
            }
        }
        return out;
    }
    String::new()
}

/// First URL of a thumbnail container (`thumbnail.thumbnails[]`),
/// preferring the largest (last) entry.
pub fn thumbnail_url(v: &Value) -> Option<String> {
    let thumbs = v
        .get("thumbnail")
        .and_then(|t| t.get("thumbnails"))
        .and_then(|t| t.as_array())?;
    thumbs
        .last()
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
}

/// Parse "12:34" / "1:02:03" into seconds.
pub fn parse_duration_secs(text: &str) -> Option<u64> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut secs: u64 = 0;
    for p in parts {
        let n: u64 = p.parse().ok()?;
        secs = secs * 60 + n;
    }
    Some(secs)
}

/// Parse a view-count text like "1.2M views" / "345K views" / "No views"
/// into an approximate count. Returns None when unparsable.
pub fn parse_count_text(text: &str) -> Option<u64> {
    let t = text.trim().to_lowercase();
    if t.starts_with("no views") {
        return Some(0);
    }
    let num_part: String = t
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    if num_part.is_empty() {
        return None;
    }
    let mult = if t[num_part.len()..].starts_with('k') {
        1_000f64
    } else if t[num_part.len()..].starts_with('m') {
        1_000_000f64
    } else if t[num_part.len()..].starts_with('b') {
        1_000_000_000f64
    } else {
        1f64
    };
    let cleaned: String = num_part.chars().filter(|c| *c != ',').collect();
    cleaned.parse::<f64>().ok().map(|n| (n * mult) as u64)
}

/// A video item as returned by search and by watch-next related lists.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VideoItem {
    pub video_id: String,
    pub title: String,
    pub url: String,
    pub channel_name: String,
    pub channel_id: Option<String>,
    pub view_count_text: Option<String>,
    pub view_count: Option<u64>,
    pub duration_text: Option<String>,
    pub duration_secs: Option<u64>,
    pub published_time: Option<String>,
    pub thumbnail: Option<String>,
    pub is_live: bool,
}

impl VideoItem {
    pub fn to_value(&self) -> Value {
        json!({
            "video_id": self.video_id,
            "title": self.title,
            "url": self.url,
            "channel_name": self.channel_name,
            "channel_id": self.channel_id,
            "view_count_text": self.view_count_text,
            "view_count": self.view_count,
            "duration_text": self.duration_text,
            "duration_secs": self.duration_secs,
            "published_time": self.published_time,
            "thumbnail": self.thumbnail,
            "is_live": self.is_live,
        })
    }
}

/// Extract a VideoItem from a `videoRenderer` object.
pub fn video_from_renderer(r: &Value) -> Option<VideoItem> {
    let video_id = r.get("videoId")?.as_str()?.to_string();
    let title = text_of(r.get("title")?);
    let byline = r.get("longBylineText").or_else(|| r.get("ownerText"));
    let (channel_name, channel_id) = match byline {
        Some(b) => {
            let name = text_of(b);
            let cid = b
                .get("runs")
                .and_then(|x| x.as_array())
                .and_then(|a| a.first())
                .and_then(|run| run.get("navigationEndpoint"))
                .and_then(|ne| ne.get("browseEndpoint"))
                .and_then(|be| be.get("browseId"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            (name, cid)
        }
        None => (String::new(), None),
    };
    let view_count_text = r
        .get("viewCountText")
        .map(text_of)
        .filter(|s| !s.is_empty());
    let duration_text = r.get("lengthText").map(text_of).filter(|s| !s.is_empty());
    let is_live = duration_text.is_none()
        && r.get("badges")
            .and_then(|b| b.as_array())
            .map(|badges| {
                badges.iter().any(|b| {
                    b.pointer("/metadataBadgeRenderer/label")
                        .and_then(|l| l.as_str())
                        == Some("LIVE")
                })
            })
            .unwrap_or(false);
    Some(VideoItem {
        url: format!("https://www.youtube.com/watch?v={video_id}"),
        video_id,
        title,
        channel_name,
        channel_id,
        view_count: view_count_text.as_deref().and_then(parse_count_text),
        view_count_text,
        duration_secs: duration_text.as_deref().and_then(parse_duration_secs),
        duration_text,
        published_time: r
            .get("publishedTimeText")
            .map(text_of)
            .filter(|s| !s.is_empty()),
        thumbnail: thumbnail_url(r),
        is_live,
    })
}

/// Depth-first search for the first string value stored under `key`
/// anywhere in the tree (used for continuation tokens that move between
/// renderer layouts).
pub fn find_string<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    match v {
        Value::Object(map) => {
            if let Some(s) = map.get(key).and_then(|x| x.as_str()) {
                return Some(s);
            }
            for child in map.values() {
                if let Some(s) = find_string(child, key) {
                    return Some(s);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(|child| find_string(child, key)),
        _ => None,
    }
}

/// Collect every object in the tree that has the given renderer key,
/// in document order.
pub fn collect_renderers(v: &Value, renderer_key: &str, out: &mut Vec<Value>) {
    match v {
        Value::Object(map) => {
            if let Some(inner) = map.get(renderer_key) {
                out.push(inner.clone());
            }
            for child in map.values() {
                collect_renderers(child, renderer_key, out);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                collect_renderers(child, renderer_key, out);
            }
        }
        _ => {}
    }
}
