//! Yahoo (US) search client — usable both as the search engine of the
//! Yahoo_US_MCP server and as a standalone Rust library.
//!
//! The verticals are read from the same public search pages the Yahoo
//! Scout app opens in its WebView (`search.yahoo.com` and friends). The US
//! SERP ships no JSON payload, so results are parsed from the
//! server-rendered HTML, and result links are unwrapped from Yahoo's
//! `r.search.yahoo.com` redirect URLs into real destination URLs. Nothing
//! here needs an API key, a login, cookies or any user configuration.
//!
//! Image search is deliberately not implemented: Yahoo's image SERP
//! (`images.search.yahoo.com`) returned HTTP 500 from every environment
//! tested during the APK analysis (desktop/mobile UA, with/without
//! cookies), so there is no verified way to read it over plain HTTP.
//!
//! # Library usage
//!
//! Add the provider crate to your `Cargo.toml` (the library is imported as
//! `yahoo_us_search`, after the `[lib]` target name):
//!
//! ```toml
//! [dependencies]
//! yahoo_us_search = { git = "https://github.com/mouseos/Yahoo_US_MCP", package = "yahoo-us-mcp-provider" }
//! tokio = { version = "1", features = ["full"] }
//! anyhow = "1"
//! ```
//!
//! All searches go through [`YahooUsSearchClient`]; every method returns
//! `anyhow::Result`, wrapping either a [`SearchResult`] of the vertical's
//! item type or, for autocomplete, a [`SuggestResult`]:
//!
//! ```no_run
//! # fn main() {
//! # let rt = tokio::runtime::Builder::new_current_thread()
//! #     .enable_all().build().unwrap();
//! # rt.block_on(async {
//! use yahoo_us_search::YahooUsSearchClient;
//!
//! let client = YahooUsSearchClient::new();
//! // Web search: query, page (1-based), limit.
//! let result = client.web_search("apple pie recipe", 1, 5).await.unwrap();
//! assert!(result.ok);
//! for item in &result.items {
//!     println!("{} — {}", item.title, item.url);
//! }
//! // Search suggestions.
//! let suggest = client.suggest("app", 10).await.unwrap();
//! for s in &suggest.suggestions {
//!     println!("{}", s.keyword);
//! }
//! # });
//! # }
//! ```
//!
//! The parsers also work offline on saved pages, which is how the test
//! suite verifies them against recorded fixtures:
//!
//! ```
//! use yahoo_us_search::{build_search_url, parse_web_results, PROVIDER};
//!
//! assert_eq!(PROVIDER, "yahoo_us");
//! assert_eq!(
//!     build_search_url("apple", "web", 2).unwrap(),
//!     "https://search.yahoo.com/search?p=apple&b=8&pz=7"
//! );
//! let html = r#"<div id="web"></div>"#;
//! assert!(parse_web_results(html, 7).is_empty());
//! ```

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub mod common;
pub mod news;
pub mod suggest;
pub mod urlbuild;
pub mod video;
pub mod web;

// ── Root re-exports: the stable library surface ──
//
// Library users should only need `use yahoo_us_search::{...}`. The
// per-vertical modules stay public, but every item type, the suggest
// types, the offline parsers and the text/URL helpers are re-exported
// here so callers never have to reach into a module path.

/// Text and URL helpers shared by all verticals.
pub use common::{clean_text, decode_entities, decode_redirect_url, urldecode, urlencode};
/// News article item returned by [`YahooUsSearchClient::news_search`].
pub use news::NewsItem;
/// Offline news parsers for saved SERP pages.
pub use news::{news_has_next_page, parse_news_results};
/// Offline suggest-response parser for a decoded gossip JSON body.
pub use suggest::parse_suggest_response;
/// Autocomplete types returned by [`YahooUsSearchClient::suggest`].
pub use suggest::{SuggestResult, Suggestion};
/// Search URL builder (no network).
pub use urlbuild::build_search_url;
/// Video item returned by [`YahooUsSearchClient::video_search`].
pub use video::VideoItem;
/// Offline video parsers for saved SERP pages.
pub use video::{parse_video_next_offset, parse_video_results, parse_video_total};
/// Web item returned by [`YahooUsSearchClient::web_search`].
pub use web::WebItem;
/// Offline web parsers for saved SERP pages.
pub use web::{parse_web_results, web_has_next_page};

/// Provider identifier used in every result envelope (`"yahoo_us"`).
pub const PROVIDER: &str = "yahoo_us";

/// A vertical-agnostic search response envelope.
///
/// `provider` / `vertical` are owned `String`s (not `&'static str`) so that
/// downstream users can deserialise results back — e.g. from
/// `serde_json::Value` or a cache file — which a borrowed field makes
/// impossible outside `'static` lifetimes. The JSON form is unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult<T> {
    pub provider: String,
    pub vertical: String,
    pub ok: bool,
    pub query: String,
    pub page: i64,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_hits: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_queries: Vec<String>,
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl<T> SearchResult<T> {
    /// Build a successful envelope for `vertical` from already-fetched items.
    /// `count` is derived from `items.len()`; chain the `with_*` builders to
    /// attach totals and paging.
    pub fn new(vertical: &str, query: &str, page: i64, items: Vec<T>) -> Self {
        Self {
            provider: PROVIDER.to_string(),
            vertical: vertical.to_string(),
            ok: true,
            query: query.to_string(),
            page,
            count: items.len(),
            total_hits: None,
            next_page: None,
            related_queries: Vec::new(),
            items,
            note: None,
        }
    }

    /// Attach the total hit count reported by Yahoo, if known.
    pub fn with_total(mut self, total: Option<i64>) -> Self {
        self.total_hits = total;
        self
    }

    /// Attach the next 1-based page number, if a further page exists.
    pub fn with_next_page(mut self, next: Option<i64>) -> Self {
        self.next_page = next;
        self
    }

    /// Attach related queries suggested alongside the results.
    pub fn with_related(mut self, related: Vec<String>) -> Self {
        self.related_queries = related;
        self
    }

    /// Attach a human-readable note about the result (e.g. a paging caveat).
    pub fn with_note(mut self, note: &str) -> Self {
        self.note = Some(note.to_string());
        self
    }
}

/// Shared HTTP client. Cheap to clone; holds a connection pool. No cookie
/// jar is needed — the US SERPs answer anonymous requests as-is (verified
/// during the APK analysis).
#[derive(Clone)]
pub struct YahooUsSearchClient {
    http: Client,
}

impl Default for YahooUsSearchClient {
    fn default() -> Self {
        Self::new()
    }
}

impl YahooUsSearchClient {
    /// Create a client with the shared connection pool.
    /// No configuration, API key or login is required. Cheap to clone and
    /// safe to share across tasks; one client per process is the norm.
    pub fn new() -> Self {
        let http = Client::builder()
            .user_agent(common::USER_AGENT)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");

        Self { http }
    }

    /// GET a Yahoo page as text, with the headers their edge expects.
    pub(crate) async fn get_html(&self, url: &str, referer: &str) -> Result<String> {
        let resp = self
            .http
            .get(url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", common::ACCEPT_LANGUAGE)
            .header("Referer", referer)
            .header("Upgrade-Insecure-Requests", "1")
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.text().await?)
    }

    pub(crate) async fn get_json(&self, url: &str, referer: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(url)
            .header("Accept", "application/json,text/javascript,*/*;q=0.01")
            .header("Accept-Language", common::ACCEPT_LANGUAGE)
            .header("Referer", referer)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }
}

/// Static description of the searchable verticals (web, video, news,
/// suggest — plus image, marked unsupported) as a JSON value — powers the
/// `yahoo_us_list_verticals` MCP tool so an agent can pick the right one,
/// and doubles as machine-readable documentation for library users.
pub fn verticals() -> serde_json::Value {
    serde_json::json!({
        "provider": PROVIDER,
        "auth": "none",
        "config_required": false,
        "verticals": [
            {
                "id": "web",
                "name": "Yahoo Search (Web)",
                "tool": "yahoo_us_web_search",
                "description": "General web search. 7 results per page; result URLs are unwrapped to their real destinations.",
                "paginated": true,
                "supported": true
            },
            {
                "id": "video",
                "name": "Yahoo Video Search",
                "tool": "yahoo_us_video_search",
                "description": "Video search across YouTube and other hosts, with duration, source and thumbnails. 60 results per page.",
                "paginated": true,
                "supported": true
            },
            {
                "id": "news",
                "name": "Yahoo News Search",
                "tool": "yahoo_us_news_search",
                "description": "News article search with publisher and relative publish time. 10 results per page.",
                "paginated": true,
                "supported": true
            },
            {
                "id": "suggest",
                "name": "Yahoo Search Suggestions",
                "tool": "yahoo_us_suggest",
                "description": "Autocomplete suggestions for a partial query (gossip API).",
                "paginated": false,
                "supported": true
            },
            {
                "id": "image",
                "name": "Yahoo Image Search",
                "tool": null,
                "description": "Not supported: Yahoo's image SERP returned HTTP 500 from every environment tested (desktop/mobile UA, with/without cookies), so it cannot be read over plain HTTP.",
                "paginated": false,
                "supported": false
            }
        ]
    })
}
