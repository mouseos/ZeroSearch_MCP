//! YouTube client library reproducing the Android app's own InnerTube
//! API — usable standalone and as the future YouTube provider of
//! ZeroSearch_MCP.
//!
//! The client speaks to the same production InnerTube backend the app
//! uses (`https://youtubei.googleapis.com`, `defpackage/akyp.java` in
//! the analysed APK), with the API key the app embeds
//! (`defpackage/ixl.java::a()`), the `ANDROID` client name and the app
//! version as client version. Sessions are guest sessions: the
//! `visitorData` is issued by the server and replayed from
//! `responseContext`, exactly as the app's visitor-data provider does.
//! No login, no user account, no fabricated dynamic values.
//!
//! # Library usage
//!
//! ```toml
//! [dependencies]
//! youtube_search = { package = "youtube-mcp-provider", path = "..." }
//! tokio = { version = "1", features = ["full"] }
//! ```
//!
//! ```no_run
//! # fn main() {
//! # let rt = tokio::runtime::Builder::new_current_thread()
//! #     .enable_all().build().unwrap();
//! # rt.block_on(async {
//! use youtube_search::{SearchFilters, YoutubeClient};
//!
//! let client = YoutubeClient::new();
//! let page = client
//!     .search_videos("rust programming", &SearchFilters::default())
//!     .await
//!     .unwrap();
//! for item in &page.items {
//!     println!("{} — {}", item.title, item.url);
//! }
//! let details = client.get_video(&page.items[0].video_id).await.unwrap();
//! println!("{} views", details.view_count.unwrap_or(0));
//! # });
//! # }
//! ```
//!
//! Tool mapping when integrated into ZeroSearch_MCP (provider
//! `youtube`, standard `{provider, ok, ...}` envelope):
//! `youtube_search_videos` → [`YoutubeClient::search_videos`] /
//! [`YoutubeClient::search_continuation`],
//! `youtube_get_video` → [`YoutubeClient::get_video`] +
//! [`YoutubeClient::get_watch_next`],
//! `youtube_get_comments` → [`YoutubeClient::get_comments`].

// The filter enums expose inherent `from_str` constructors returning
// `Option<Self>` (fallible-name lookup, not parsing) — the same
// convention as the other ZeroSearch providers — so the
// `should_implement_trait` lint does not apply to them.
#![allow(clippy::should_implement_trait)]

pub mod client;
pub mod errors;
pub mod filters;
pub mod next;
pub mod parse;
pub mod player;
pub mod search;

pub use client::{YoutubeClient, APP_VERSION, CLIENT_NAME, INNERTUBE_API_KEY, INNERTUBE_BASE};
pub use errors::YoutubeError;
pub use filters::{Duration, ResultType, SearchFilters, SearchSort, UploadDate};
pub use next::{CommentItem, CommentsPage, WatchNext};
pub use parse::VideoItem;
pub use player::VideoDetails;
pub use search::SearchPage;
