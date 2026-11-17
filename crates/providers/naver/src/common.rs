//! Shared helpers: HTTP behaviour, HTML/entity cleanup, URL handling and
//! the small DOM utilities the vertical parsers share.

use anyhow::anyhow;
use regex::Regex;
use scraper::{ElementRef, Selector};
use std::sync::LazyLock;
use zs_common::TAG_RE;
pub use zs_common::{clamp_limit, decode_entities, urlencode};

/// Android mobile UA — Naver's mobile search host only serves
/// server-rendered results to a mobile agent (APK analysis, round 1).
pub const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";

pub const ACCEPT_LANGUAGE: &str = "ko,en-US;q=0.9,en;q=0.8";

/// Referer sent with search page fetches (the mobile search home).
pub const SEARCH_REFERER: &str = "https://m.search.naver.com/";

/// Korean relative-time stamps as Naver renders them (`8시간 전`, `4주 전`,
/// `방금 전`) plus the absolute `2025.09.16.` form.
static TIME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+\s*(?:분|시간|일|주|개월|년)\s*전|방금\s*전|\d{4}\.\d{1,2}\.\d{1,2}\.?)")
        .unwrap()
});

static DURATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{1,3}:\d{2}(?::\d{2})?$").unwrap());

static VIEWS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"조회수\s*[0-9.,]+\s*(?:만|억|천)?").unwrap());

/// Turn an HTTP status into the unified error vocabulary. 403/429 (and any
/// CAPTCHA wall) become `access_restricted`; the client never retries or
/// tries to evade them. The `kind:` prefix is how the server classifies the
/// error into the unified `{kind, message}` form.
pub fn status_error(status: reqwest::StatusCode) -> anyhow::Error {
    if status.as_u16() == 403 || status.as_u16() == 429 {
        anyhow!("access_restricted: HTTP {status} from Naver (rate limited or blocked)")
    } else {
        anyhow!("http_error: HTTP {status} from Naver")
    }
}

/// Map a reqwest failure onto the same `kind:` prefix vocabulary.
pub fn map_reqwest_error(e: reqwest::Error) -> anyhow::Error {
    if e.is_timeout() {
        anyhow!("timeout: {e}")
    } else if e.is_connect() {
        anyhow!("connection_error: {e}")
    } else if e.is_decode() || e.is_body() {
        anyhow!("parse_error: {e}")
    } else {
        anyhow!("http_error: {e}")
    }
}

/// Guard shared by the HTML parsers: a page whose expected result markup is
/// entirely absent means the layout changed (or a block page was served),
/// which must surface as an error rather than a silent empty result.
pub fn layout_error(vertical: &str) -> anyhow::Error {
    anyhow!("parse_error: {vertical} result blocks not found (Naver page layout may have changed)")
}

/// Error for `page` values that cannot be served: tab pages ignore
/// page parameters (deeper batches come from the page-issued more-API
/// chain, not a URL), the integrated view has no deeper pages, and
/// chained paging is capped at `more::MAX_PAGE`.
pub fn unsupported_page(vertical: &str, page: i64) -> anyhow::Error {
    anyhow!("unsupported_page: page {page} is not available for the {vertical} search")
}

/// Normalise a piece of page text: strip tags, decode entities and
/// collapse whitespace.
/// Collapse DOM text (or an attribute value) to a single line. The
/// HTML parser has already decoded entities there, so unlike
/// [`clean_text`] nothing is decoded or stripped again — doing so
/// turned a page's literal `&lt;b&gt;` into `<b>` and then removed it.
pub fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn clean_text(s: &str) -> String {
    let without_tags = TAG_RE.replace_all(s, "");
    let decoded = decode_entities(&without_tags);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Look up one query parameter in a URL (percent-decoded by the URL parser).
pub fn query_param(url: &str, key: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// Resolve a possibly-relative href against a base origin.
pub fn resolve_url(href: &str, base: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if let Some(rest) = href.strip_prefix("//") {
        return format!("https://{rest}");
    }
    if href.starts_with('/') {
        return format!("{base}{href}");
    }
    href.to_string()
}

/// All text under an element, normalised with [`clean_text`].
pub fn element_text(el: ElementRef) -> String {
    collapse_whitespace(&el.text().collect::<String>())
}

/// First Korean relative/absolute time stamp in `text`, if any.
pub fn find_time(text: &str) -> Option<String> {
    TIME_RE
        .captures(text)
        .map(|c| c[1].split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Whether `text` is a bare play duration (`08:03`, `1:02:03`).
pub fn is_duration(text: &str) -> bool {
    DURATION_RE.is_match(text.trim())
}

/// First view-count phrase (`조회수 449`, `조회수 1.2만`) in `text`, if any.
pub fn find_views(text: &str) -> Option<String> {
    VIEWS_RE
        .captures(text)
        .map(|c| c[0].split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Climb from `el` to the smallest ancestor that still contains only the
/// item `el` belongs to — i.e. keep climbing while the ancestor's subtree
/// holds at most one element matching `marker`. This locates the result
/// "card" around a link without depending on Naver's hashed CSS classes.
/// At most `max_levels` ancestors are considered; the starting element
/// itself is the fallback.
pub fn card_for<'a>(el: ElementRef<'a>, marker: &Selector, max_levels: usize) -> ElementRef<'a> {
    let mut card = el;
    let mut node = el.parent();
    for _ in 0..max_levels {
        let Some(n) = node else { break };
        let Some(er) = ElementRef::wrap(n) else {
            node = n.parent();
            continue;
        };
        if er.select(marker).count() > 1 {
            break;
        }
        card = er;
        node = n.parent();
    }
    card
}

/// Like [`card_for`], but the item boundary is "at most one *distinct*
/// value of `attr` among elements matching `marker`" — for groups where one
/// item owns several marker elements (video cards carry several
/// `a[data-api]` anchors for the same video).
pub fn card_for_unique<'a>(
    el: ElementRef<'a>,
    marker: &Selector,
    attr: &str,
    max_levels: usize,
) -> ElementRef<'a> {
    fn distinct(er: ElementRef, marker: &Selector, attr: &str) -> usize {
        let mut seen = Vec::new();
        for m in er.select(marker) {
            if let Some(v) = m.value().attr(attr) {
                if !seen.contains(&v) {
                    seen.push(v);
                }
            }
        }
        seen.len()
    }
    let mut card = el;
    let mut node = el.parent();
    for _ in 0..max_levels {
        let Some(n) = node else { break };
        let Some(er) = ElementRef::wrap(n) else {
            node = n.parent();
            continue;
        };
        if distinct(er, marker, attr) > 1 {
            break;
        }
        card = er;
        node = n.parent();
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_text_does_not_decode_entities_twice() {
        // The page shows the literal text "&lt;b&gt;" (escaped once more
        // in the source); the parser already decoded one level.
        let doc = scraper::Html::parse_fragment("<p> a &amp;lt;b&amp;gt;  c </p>");
        let sel = scraper::Selector::parse("p").unwrap();
        let el = doc.select(&sel).next().unwrap();
        assert_eq!(element_text(el), "a &lt;b&gt; c");
    }

    #[test]
    fn clean_text_strips_tags_and_entities() {
        assert_eq!(
            clean_text("<mark>테스트</mark> &amp; 실험"),
            "테스트 & 실험"
        );
        assert_eq!(clean_text("a\n b\t c"), "a b c");
    }

    #[test]
    fn find_time_reads_korean_stamps() {
        assert_eq!(
            find_time("조선비즈8시간 전네이버뉴스").as_deref(),
            Some("8시간 전")
        );
        assert_eq!(find_time("4주 전").as_deref(), Some("4주 전"));
        assert_eq!(find_time("방금 전").as_deref(), Some("방금 전"));
        assert_eq!(find_time("2025.09.16.").as_deref(), Some("2025.09.16."));
        assert_eq!(find_time("no time here"), None);
    }

    #[test]
    fn duration_and_views_patterns() {
        assert!(is_duration("08:03"));
        assert!(is_duration("1:02:03"));
        assert!(!is_duration("8시간 전"));
        assert_eq!(find_views("조회수 449").as_deref(), Some("조회수 449"));
        assert_eq!(
            find_views("조회수 1.2만 회").as_deref(),
            Some("조회수 1.2만")
        );
    }

    #[test]
    fn query_param_decodes_proxy_src() {
        let u = "https://search.pstatic.net/common/?src=http%3A%2F%2Fblogfiles.naver.net%2Fa.jpg&type=f54_54";
        assert_eq!(
            query_param(u, "src").as_deref(),
            Some("http://blogfiles.naver.net/a.jpg")
        );
        assert_eq!(query_param(u, "type").as_deref(), Some("f54_54"));
    }

    #[test]
    fn urlencode_handles_multibyte() {
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
        assert_eq!(urlencode("네이버"), "%EB%84%A4%EC%9D%B4%EB%B2%84");
    }

    #[test]
    fn clamp_limit_bounds() {
        assert_eq!(clamp_limit(0, 50), 1);
        assert_eq!(clamp_limit(999, 50), 50);
        assert_eq!(clamp_limit(10, 50), 10);
    }
}
