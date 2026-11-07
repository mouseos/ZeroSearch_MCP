//! `youtubei/v1/search`: query search and continuation-based paging.

use anyhow::Result;
use serde_json::{json, Value};

use crate::client::YoutubeClient;
use crate::errors::YoutubeError;
use crate::filters::SearchFilters;
use crate::parse::{video_from_renderer, VideoItem};

/// One page of search results.
pub struct SearchPage {
    pub items: Vec<VideoItem>,
    /// Estimated total result count reported by YouTube
    /// (`estimatedResults`), when present.
    pub estimated_results: Option<u64>,
    /// Token for the next page; pass to
    /// [`YoutubeClient::search_continuation`].
    pub next_continuation: Option<String>,
    pub visitor_data: Option<String>,
}

impl SearchPage {
    pub fn to_value(&self, query: &str) -> Value {
        json!({
            "provider": "youtube",
            "ok": true,
            "query": query,
            "count": self.items.len(),
            "estimated_results": self.estimated_results,
            "items": self.items.iter().map(|i| i.to_value()).collect::<Vec<_>>(),
            "next_continuation": self.next_continuation,
        })
    }
}

/// Collect video items in document order. The Android client's
/// search results are `compactVideoRenderer`s; `videoRenderer` is
/// handled too for layout variants.
fn collect_video_items(resp: &Value) -> Vec<VideoItem> {
    fn walk(v: &Value, out: &mut Vec<VideoItem>) {
        match v {
            Value::Object(map) => {
                for key in ["compactVideoRenderer", "videoRenderer"] {
                    if let Some(r) = map.get(key) {
                        if let Some(item) = video_from_renderer(r) {
                            out.push(item);
                        }
                    }
                }
                for child in map.values() {
                    walk(child, out);
                }
            }
            Value::Array(arr) => {
                for child in arr {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(resp, &mut out);
    out
}

/// The page-level continuation token of a search response. The
/// Android layout carries it as
/// `sectionListRenderer.continuations[].nextContinuationData.continuation`;
/// continuation responses and other layouts also use
/// `continuationItemRenderer`. The last one in document order wins.
fn collect_next_continuation(resp: &Value) -> Option<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                if let Some(tok) = map
                    .get("nextContinuationData")
                    .and_then(|n| n.get("continuation"))
                    .and_then(|t| t.as_str())
                {
                    out.push(tok.to_string());
                }
                if let Some(tok) = map
                    .get("continuationItemRenderer")
                    .and_then(|c| c.get("continuationEndpoint"))
                    .and_then(|c| c.get("continuationCommand"))
                    .and_then(|c| c.get("token"))
                    .and_then(|t| t.as_str())
                {
                    out.push(tok.to_string());
                }
                for child in map.values() {
                    walk(child, out);
                }
            }
            Value::Array(arr) => {
                for child in arr {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(resp, &mut out);
    out.into_iter().last()
}

/// Extract items + continuation from a search-shaped response
/// (`contents` for a fresh search, `continuationContents` /
/// `onResponseReceivedCommands` for a continuation response).
pub fn parse_search_response(resp: &Value) -> SearchPage {
    let items = collect_video_items(resp);
    let next_continuation = collect_next_continuation(resp);

    SearchPage {
        items,
        estimated_results: resp
            .get("estimatedResults")
            .and_then(|e| e.as_str())
            .and_then(|s| s.parse::<u64>().ok()),
        next_continuation,
        visitor_data: resp
            .pointer("/responseContext/visitorData")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

impl YoutubeClient {
    /// Search videos. `filters` carries sort order and the
    /// upload-date / type / duration filters.
    pub async fn search_videos(
        &self,
        query: &str,
        filters: &SearchFilters,
    ) -> Result<SearchPage, YoutubeError> {
        let mut body = json!({ "query": query });
        if let Some(params) = filters.encode_params() {
            body["params"] = json!(params);
        }
        let resp = self.post("search", body).await?;
        Ok(parse_search_response(&resp))
    }

    /// Fetch the next page of a previous search using its continuation
    /// token.
    pub async fn search_continuation(
        &self,
        continuation: &str,
    ) -> Result<SearchPage, YoutubeError> {
        let resp = self
            .post("search", json!({ "continuation": continuation }))
            .await?;
        Ok(parse_search_response(&resp))
    }
}
