//! Naver search suggestions.
//!
//! The public autocomplete endpoint `ac.search.naver.com/nx/ac` answers
//! JSON anonymously (verified live during the APK analysis); candidates
//! live in `items[0]` as `[keyword, kind]` pairs. No key or signature is
//! involved.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::*;
use crate::urlbuild::{build_search_url, suggest_url};
use crate::NaverSearchClient;

/// One autocomplete suggestion, with the integrated-search URL for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub keyword: String,
    pub search_url: String,
}

/// Response envelope for [`NaverSearchClient::suggest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestResult {
    pub provider: String,
    pub vertical: String,
    pub ok: bool,
    pub query: String,
    pub count: usize,
    pub suggestions: Vec<Suggestion>,
}

/// Parse the suggestions API JSON into suggestion entries
/// (offline-testable). Anything malformed yields an empty list.
pub fn parse_suggest_json(value: &Value) -> Vec<Suggestion> {
    let mut out = Vec::new();
    let Some(first) = value
        .get("items")
        .and_then(|i| i.as_array())
        .and_then(|groups| groups.first())
        .and_then(|g| g.as_array())
    else {
        return out;
    };
    for entry in first {
        let Some(keyword) = entry
            .as_array()
            .and_then(|pair| pair.first())
            .and_then(|k| k.as_str())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        out.push(Suggestion {
            keyword: keyword.to_string(),
            search_url: build_search_url(keyword, "integrated").unwrap_or_default(),
        });
    }
    out
}

impl NaverSearchClient {
    /// Fetch autocomplete suggestions for a (partial) query.
    pub async fn suggest(&self, query: &str) -> Result<SuggestResult> {
        let query = query.trim();
        if query.is_empty() {
            return Err(anyhow::anyhow!("query must not be empty"));
        }
        let url = suggest_url(query);
        let json = self.get_json(&url, "https://search.naver.com/").await?;
        if json.get("items").is_none() {
            return Err(layout_error("suggest"));
        }
        let suggestions = parse_suggest_json(&json);
        Ok(SuggestResult {
            provider: crate::PROVIDER.to_string(),
            vertical: "suggest".to_string(),
            ok: true,
            query: query.to_string(),
            count: suggestions.len(),
            suggestions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_first_items_group() {
        let value: Value = serde_json::from_str(
            r#"{"query":["네이"],"items":[[["네이버웹툰","0"],["네이트","1"]]]}"#,
        )
        .unwrap();
        let out = parse_suggest_json(&value);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].keyword, "네이버웹툰");
        assert!(out[0].search_url.contains("m.search.naver.com"));
    }

    #[test]
    fn parse_tolerates_missing_items() {
        assert!(parse_suggest_json(&serde_json::json!({})).is_empty());
    }
}
