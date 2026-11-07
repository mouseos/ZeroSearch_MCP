//! Search URL construction for every vertical (no network).
//!
//! The templates are the ones the Yahoo Scout app (package
//! `com.yahoo.mobile.client.android.search`) carries in its JS bundle and
//! that the live SERPs themselves link to; see the analysis report in the
//! project history. Pagination differs per vertical:
//!
//! - web: 7 results per page, `b = 1, 8, 15, …` with `pz=7`
//! - video: 60 results per page, `b = 1, 61, 121, …`
//! - news: 10 results per page, `b = 1, 11, 21, …`

use anyhow::{anyhow, Result};

use crate::common::urlencode;

/// Verticals for which a search URL can be built.
pub const URL_VERTICALS: [&str; 5] = ["web", "image", "video", "news", "suggest"];

fn page_offset(page: i64, per_page: i64) -> i64 {
    (page.max(1) - 1) * per_page + 1
}

/// Build the first-page (or `page`-th page) search URL for a vertical.
///
/// `vertical` is one of `web`, `image`, `video`, `news`, `suggest`
/// (case-insensitive). Note the image URL is only a template: image *search*
/// is not supported by this crate because Yahoo's image SERP returned
/// HTTP 500 from every environment tested (see the crate README).
pub fn build_search_url(query: &str, vertical: &str, page: i64) -> Result<String> {
    let q = urlencode(query.trim());
    let page = page.max(1);
    match vertical.trim().to_lowercase().as_str() {
        "web" => {
            let b = page_offset(page, crate::web::PER_PAGE);
            Ok(format!(
                "https://search.yahoo.com/search?p={q}&b={b}&pz={}",
                crate::web::PER_PAGE
            ))
        }
        "image" => Ok(format!(
            "https://images.search.yahoo.com/search/images?p={q}"
        )),
        "video" => {
            let b = page_offset(page, crate::video::PER_PAGE);
            Ok(format!(
                "https://video.search.yahoo.com/search/video?p={q}&b={b}"
            ))
        }
        "news" => {
            let b = page_offset(page, crate::news::PER_PAGE);
            Ok(format!("https://news.search.yahoo.com/search?p={q}&b={b}"))
        }
        "suggest" => Ok(format!(
            "https://search.yahoo.com/sugg/gossip/gossip-us-ura/?command={q}&output=json"
        )),
        other => Err(anyhow!(
            "unknown vertical: {other} (expected one of {})",
            URL_VERTICALS.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_url_uses_b_and_pz_paging() {
        assert_eq!(
            build_search_url("apple pie", "web", 1).unwrap(),
            "https://search.yahoo.com/search?p=apple%20pie&b=1&pz=7"
        );
        assert_eq!(
            build_search_url("apple", "web", 3).unwrap(),
            "https://search.yahoo.com/search?p=apple&b=15&pz=7"
        );
    }

    #[test]
    fn video_and_news_urls_use_their_own_offsets() {
        assert_eq!(
            build_search_url("apple", "video", 2).unwrap(),
            "https://video.search.yahoo.com/search/video?p=apple&b=61"
        );
        assert_eq!(
            build_search_url("apple", "news", 2).unwrap(),
            "https://news.search.yahoo.com/search?p=apple&b=11"
        );
    }

    #[test]
    fn suggest_url_is_the_gossip_endpoint() {
        assert_eq!(
            build_search_url("app", "suggest", 1).unwrap(),
            "https://search.yahoo.com/sugg/gossip/gossip-us-ura/?command=app&output=json"
        );
    }

    #[test]
    fn unknown_vertical_is_an_error() {
        assert!(build_search_url("x", "maps", 1).is_err());
    }
}
