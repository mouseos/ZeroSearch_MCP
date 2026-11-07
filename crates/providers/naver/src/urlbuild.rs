//! Search URL generation, reproducing what the Naver Android app builds.
//!
//! The app (APK analysis, round 1) opens
//! `https://m.search.naver.com/search.naver?where=m&ie=utf8&query=...&sm=...`
//! in its WebView for integrated search; the vertical tabs use the same
//! host with an `ssc=tab.m_*.all` section code (plus `where=m_*` where the
//! tab needs it). The desktop host only ships a JavaScript shell over
//! plain HTTP, so everything here stays on the mobile host.
//!
//! Paging, as measured against the live pages: every tab page renders
//! one batch and ignores its URL's `start`/`page` parameters (page 1
//! and page 2 URLs return identical content). Deeper batches of the
//! image / news / video / blog tabs are served by the "more" API whose
//! URL each page itself issues — see the `more` module; those pages
//! therefore cannot be expressed as a single URL and are an error
//! here. The web tab has no such API and its batch never changes, so
//! its deeper pages are an error too
//! ([`crate::common::unsupported_page`]), never a silent repeat of
//! page 1.

use anyhow::{anyhow, Result};

use crate::common::{unsupported_page, urlencode};

/// Mobile search host + path, the app's search entry point.
pub const SEARCH_BASE: &str = "https://m.search.naver.com/search.naver";
/// Suggestions endpoint (the public `nx/ac` JSON API, verified live).
pub const SUGGEST_BASE: &str = "https://ac.search.naver.com/nx/ac";

/// Items in the blog tab's first batch.
pub const BLOG_PER_PAGE: i64 = 30;
/// Items in the news tab's first batch.
pub const NEWS_PER_PAGE: i64 = 15;
/// Items the video tab's "more" config asks for per batch.
pub const VIDEO_PER_PAGE: i64 = 48;

/// Normalise a vertical name for display purposes.
pub fn normalize_vertical(vertical: &str) -> &str {
    match vertical.trim().to_lowercase().as_str() {
        "integrated" | "all" | "m" => "integrated",
        "web" | "url" | "ur" => "web",
        "image" | "images" => "image",
        "news" => "news",
        "video" | "videos" => "video",
        "blog" => "blog",
        "suggest" | "suggestions" => "suggest",
        _ => "integrated",
    }
}

/// Strictly resolve a caller-supplied vertical for URL building: unknown
/// names are an error, never a silent fall-back.
fn strict_vertical(vertical: &str) -> Result<&'static str> {
    match vertical.trim().to_lowercase().as_str() {
        "integrated" | "all" => Ok("integrated"),
        "web" => Ok("web"),
        "image" | "images" => Ok("image"),
        "news" => Ok("news"),
        "video" | "videos" => Ok("video"),
        "blog" => Ok("blog"),
        other => Err(anyhow!("unsupported vertical: {other}")),
    }
}

/// Build the search URL for a 1-based `page` of a vertical — exactly the
/// URL the client fetches.
pub fn build_page_url(query: &str, vertical: &str, page: i64) -> Result<String> {
    let vertical = strict_vertical(vertical)?;
    let page = page.max(1);
    let q = urlencode(query.trim());
    let url = match vertical {
        "integrated" => {
            if page > 1 {
                return Err(unsupported_page("integrated", page));
            }
            // The app's own integrated-search URL (InAppBrowser).
            format!("{SEARCH_BASE}?where=m&ie=utf8&query={q}&sm=mob_hty.sch")
        }
        "web" => {
            if page > 1 {
                return Err(unsupported_page("web", page));
            }
            format!("{SEARCH_BASE}?ssc=tab.m_ur.all&sm=mtb_jum&query={q}")
        }
        "image" => {
            if page > 1 {
                return Err(unsupported_page("image", page));
            }
            format!("{SEARCH_BASE}?ssc=tab.m_image.all&where=m_image&sm=mtb_jum&query={q}")
        }
        "news" => {
            if page > 1 {
                return Err(unsupported_page("news", page));
            }
            format!("{SEARCH_BASE}?ssc=tab.m_news.all&where=m_news&sm=mtb_jum&query={q}")
        }
        "video" => {
            if page > 1 {
                return Err(unsupported_page("video", page));
            }
            format!("{SEARCH_BASE}?ssc=tab.m_video.all&where=m_video&sm=mtb_jum&query={q}")
        }
        "blog" => {
            if page > 1 {
                return Err(unsupported_page("blog", page));
            }
            format!("{SEARCH_BASE}?ssc=tab.m_blog.all&sm=mtb_jum&query={q}")
        }
        _ => unreachable!("strict_vertical only returns known verticals"),
    };
    Ok(url)
}

/// Build the first-page search URL for a vertical (the
/// `naver_build_search_url` tool; no network access).
pub fn build_search_url(query: &str, vertical: &str) -> Result<String> {
    build_page_url(query, vertical, 1)
}

/// Build the suggestions API URL for a query.
pub fn suggest_url(query: &str) -> String {
    format!(
        "{SUGGEST_BASE}?q={}&q_enc=UTF-8&st=100&frm=nv&r_format=json&r_enc=UTF-8&r_unicode=0&t_koreng=1&ans=2&rev=4",
        urlencode(query.trim())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrated_url_matches_the_app_template() {
        let url = build_search_url("테스트", "integrated").unwrap();
        assert_eq!(
            url,
            "https://m.search.naver.com/search.naver?where=m&ie=utf8&query=%ED%85%8C%EC%8A%A4%ED%8A%B8&sm=mob_hty.sch"
        );
    }

    #[test]
    fn deep_pages_are_not_expressible_as_urls() {
        // Tab pages ignore page/start parameters; deeper batches come
        // from the page-issued more-API chain (module `more`), and the
        // web/integrated views have no deeper pages at all.
        for v in ["web", "image", "news", "video", "blog", "integrated"] {
            let err = build_page_url("x", v, 2).unwrap_err();
            assert!(
                err.to_string().starts_with("unsupported_page:"),
                "{v}: {err}"
            );
        }
        for v in ["web", "image", "news", "video", "blog"] {
            assert!(build_page_url("x", v, 1).is_ok(), "{v} page 1");
        }
    }

    #[test]
    fn suggest_url_uses_the_public_endpoint() {
        let url = suggest_url("네이");
        assert!(url.starts_with("https://ac.search.naver.com/nx/ac?q="));
        assert!(url.contains("&r_format=json"));
    }

    #[test]
    fn unknown_vertical_is_an_error() {
        assert!(build_search_url("x", "cafe").is_err());
        assert!(build_search_url("x", "kin").is_err());
    }
}
