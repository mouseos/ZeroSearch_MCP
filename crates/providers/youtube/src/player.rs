//! `youtubei/v1/player`: video details for one video id.

use serde_json::{json, Value};

use crate::client::YoutubeClient;
use crate::errors::{YoutubeError, KIND_NOT_FOUND};
use crate::parse::{parse_count_text, text_of};

/// Details of one video, from the player response.
pub struct VideoDetails {
    pub video_id: String,
    pub title: String,
    pub url: String,
    pub channel_name: String,
    pub channel_id: Option<String>,
    pub length_secs: Option<u64>,
    pub view_count: Option<u64>,
    pub like_count: Option<u64>,
    pub published_date: Option<String>,
    pub upload_date: Option<String>,
    pub category: Option<String>,
    pub is_live: bool,
    pub is_private: bool,
    pub short_description: String,
    pub keywords: Vec<String>,
    pub thumbnail: Option<String>,
    pub playability_status: String,
    pub stream_format_count: usize,
    pub has_caption_tracks: bool,
}

impl VideoDetails {
    pub fn to_value(&self) -> Value {
        json!({
            "provider": "youtube",
            "ok": true,
            "video_id": self.video_id,
            "title": self.title,
            "url": self.url,
            "channel_name": self.channel_name,
            "channel_id": self.channel_id,
            "length_secs": self.length_secs,
            "view_count": self.view_count,
            "like_count": self.like_count,
            "published_date": self.published_date,
            "upload_date": self.upload_date,
            "category": self.category,
            "is_live": self.is_live,
            "is_private": self.is_private,
            "short_description": self.short_description,
            "keywords": self.keywords,
            "thumbnail": self.thumbnail,
            "playability_status": self.playability_status,
            "stream_format_count": self.stream_format_count,
            "has_caption_tracks": self.has_caption_tracks,
        })
    }
}

/// Parse a player response into VideoDetails.
pub fn parse_player_response(resp: &Value) -> Result<VideoDetails, YoutubeError> {
    let vd = resp.get("videoDetails").cloned().unwrap_or(Value::Null);
    let micro = resp
        .pointer("/microformat/playerMicroformatRenderer")
        .cloned()
        .unwrap_or(Value::Null);
    let status = resp
        .pointer("/playabilityStatus/status")
        .and_then(|s| s.as_str())
        .unwrap_or("UNKNOWN")
        .to_string();
    let video_id = vd
        .get("videoId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if video_id.is_empty() && status != "OK" {
        return Err(YoutubeError::new(
            KIND_NOT_FOUND,
            format!("video not playable: playabilityStatus={status}"),
        ));
    }
    let view_count = vd
        .get("viewCount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| {
            micro
                .get("viewCount")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
        });
    let like_count = micro
        .get("likeCount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok());
    let formats = resp
        .pointer("/streamingData/formats")
        .and_then(|f| f.as_array())
        .map(|a| a.len())
        .unwrap_or(0)
        + resp
            .pointer("/streamingData/adaptiveFormats")
            .and_then(|f| f.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
    let has_caption_tracks = resp
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(|c| c.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    let thumbnail = vd
        .pointer("/thumbnail/thumbnails")
        .and_then(|t| t.as_array())
        .and_then(|a| a.last())
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());
    let _ = parse_count_text; // (kept for text-count fallbacks)
    let _ = text_of;
    Ok(VideoDetails {
        url: format!("https://www.youtube.com/watch?v={video_id}"),
        video_id,
        title: vd
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string(),
        channel_name: vd
            .get("author")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string(),
        channel_id: vd
            .get("channelId")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string()),
        length_secs: vd
            .get("lengthSeconds")
            .and_then(|t| t.as_str())
            .and_then(|s| s.parse::<u64>().ok()),
        view_count,
        like_count,
        published_date: micro
            .get("publishDate")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string()),
        upload_date: micro
            .get("uploadDate")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string()),
        category: micro
            .get("category")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string()),
        is_live: vd
            .get("isLiveContent")
            .and_then(|t| t.as_bool())
            .unwrap_or(false),
        is_private: vd
            .get("isPrivate")
            .and_then(|t| t.as_bool())
            .unwrap_or(false),
        short_description: vd
            .get("shortDescription")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string(),
        keywords: vd
            .get("keywords")
            .and_then(|k| k.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|k| k.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        thumbnail,
        playability_status: status,
        stream_format_count: formats,
        has_caption_tracks,
    })
}

impl YoutubeClient {
    /// Fetch details for one video via the player endpoint.
    pub async fn get_video(&self, video_id: &str) -> Result<VideoDetails, YoutubeError> {
        let resp = self
            .post(
                "player",
                json!({
                    "videoId": video_id,
                    "playbackContext": {
                        "contentPlaybackContext": { "vis": 0, "lactMilliseconds": "-1" }
                    },
                    "racyCheckOk": true,
                    "contentCheckOk": true,
                }),
            )
            .await?;
        parse_player_response(&resp)
    }
}
