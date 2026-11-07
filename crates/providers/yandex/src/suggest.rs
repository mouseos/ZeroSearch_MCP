//! Yandex autocomplete (`https://yandex.ru/suggest/suggest-ya.cgi`).
//!
//! The endpoint answers with a JSON array:
//! `[query, [["", suggestion, {sg_weight, tpah}], …], metadata]` — the
//! suggestion string is element 1 of each inner array.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::*;
use crate::urlbuild::build_search_url;
use crate::YandexSearchClient;

/// One autocomplete suggestion, with the web-search URL for that query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub keyword: String,
    pub search_url: String,
}

/// Response envelope for [`YandexSearchClient::suggest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestResult {
    // Owned strings, like `SearchResult`: `&'static str` here would make the
    // result impossible for downstream users to deserialise back.
    pub provider: String,
    pub vertical: String,
    pub ok: bool,
    pub query: String,
    pub count: usize,
    pub suggestions: Vec<Suggestion>,
}

/// Parse a suggest response body into the result envelope.
pub fn parse_suggest_body(body: &Value, query: &str, limit: usize) -> SuggestResult {
    let suggestions: Vec<Suggestion> = body
        .get(1)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let keyword = entry
                        .get(1)
                        .and_then(|v| v.as_str())
                        .map(clean_text)
                        .filter(|s| !s.is_empty())?;
                    let search_url = build_search_url(&keyword, "web").unwrap_or_default();
                    Some(Suggestion {
                        keyword,
                        search_url,
                    })
                })
                .take(limit)
                .collect()
        })
        .unwrap_or_default();

    SuggestResult {
        provider: crate::PROVIDER.to_string(),
        vertical: "suggest".to_string(),
        ok: true,
        query: query.to_string(),
        count: suggestions.len(),
        suggestions,
    }
}

impl YandexSearchClient {
    /// Get autocomplete suggestions for a (partial) query.
    /// `limit` is clamped to 20. An empty query is an error.
    pub async fn suggest(&self, query: &str, limit: i64) -> Result<SuggestResult> {
        let query = query.trim();
        if query.is_empty() {
            return Err(anyhow!("query must not be empty"));
        }
        let limit = clamp_limit(limit, 20);

        self.ensure_session(query).await?;
        let url = build_search_url(query, "suggest")?;
        let body_text = self.get_page(&url).await?;
        let body: Value = serde_json::from_str(&body_text)?;
        Ok(parse_suggest_body(&body, query, limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_measured_suggest_shape() {
        let body: Value = serde_json::from_str(
            r#"["test",[["","testcenter kz",{"sg_weight":0,"tpah":[0,4,10]}],["","test speed",{"sg_weight":0}]],{"r":84}]"#,
        )
        .unwrap();
        let result = parse_suggest_body(&body, "test", 10);
        assert_eq!(result.count, 2);
        assert_eq!(result.suggestions[0].keyword, "testcenter kz");
        assert_eq!(
            result.suggestions[0].search_url,
            "https://yandex.ru/search/touch/?text=testcenter%20kz"
        );
        assert_eq!(result.suggestions[1].keyword, "test speed");
    }

    #[test]
    fn tolerates_missing_or_malformed_entries() {
        let body: Value =
            serde_json::from_str(r#"["x",[["only-one-element"],[["nested"]],"junk"]]"#).unwrap();
        let result = parse_suggest_body(&body, "x", 10);
        assert_eq!(result.count, 0);
        let empty = parse_suggest_body(&Value::Null, "x", 10);
        assert_eq!(empty.count, 0);
    }
}
