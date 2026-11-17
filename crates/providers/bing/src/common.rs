//! Shared helpers: HTTP behaviour, HTML/entity cleanup, URL handling.

use anyhow::anyhow;
use base64::Engine;
use regex::Regex;
use std::sync::LazyLock;
use zs_common::TAG_RE;
pub use zs_common::{clamp_limit, decode_entities, urlencode};

/// Desktop Chrome UA — the web / video / news pages only render their
/// results server-side for a desktop-class agent (APK analysis, round 1).
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// Android mobile UA — image search only returns server-rendered results
/// to a mobile agent combined with the app-style parameters (round 2).
pub const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";

pub const ACCEPT_LANGUAGE: &str = "ja,en-US;q=0.9,en;q=0.8";

static DIGITS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9][0-9,]*").unwrap());

/// Turn an HTTP status into the unified error vocabulary. 403/429 (and any
/// CAPTCHA wall, which Bing serves as 403/429) become `access_restricted`;
/// the client never retries or tries to evade them.
pub fn status_error(status: reqwest::StatusCode) -> anyhow::Error {
    if status.as_u16() == 403 || status.as_u16() == 429 {
        anyhow!("access_restricted: HTTP {status} from Bing (rate limited or blocked)")
    } else {
        anyhow!("http_error: HTTP {status} from Bing")
    }
}

/// Normalise a piece of page text: strip tags and Bing's highlight markers
/// (web titles use `<strong>`, image descriptions use the private-use
/// characters U+E000/U+E001), decode entities and collapse whitespace.
/// Collapse DOM text (or an attribute value) to a single line. The
/// HTML parser has already decoded entities there, so unlike
/// [`clean_text`] nothing is decoded or stripped again — doing so
/// turned a page's literal `&lt;b&gt;` into `<b>` and then removed it.
pub fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn clean_text(s: &str) -> String {
    let without_tags = TAG_RE.replace_all(s, "");
    let without_markers = without_tags.replace(['\u{e000}', '\u{e001}'], "");
    let decoded = decode_entities(&without_markers);
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

/// Bing wraps most web-result links in a click-tracking redirect
/// (`https://www.bing.com/ck/a?...&u=a1<base64>`); the `u` parameter carries
/// the real destination, base64-encoded with an `a1` prefix. Decode it when
/// present and return the destination; otherwise return the href unchanged.
pub fn decode_click_url(href: &str) -> String {
    if !href.contains("/ck/a") {
        return href.to_string();
    }
    let Some(u) = query_param(href, "u") else {
        return href.to_string();
    };
    let Some(raw) = u.strip_prefix("a1") else {
        return href.to_string();
    };
    let padded = {
        let mut s = raw.replace('-', "+").replace('_', "/");
        while s.len() % 4 != 0 {
            s.push('=');
        }
        s
    };
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(padded.as_bytes())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok());
    match decoded {
        Some(url) if url.starts_with("http://") || url.starts_with("https://") => url,
        _ => href.to_string(),
    }
}

/// Extract the total hit count from Bing's `.sb_count` text
/// (e.g. `70,500 件の検索結果` → `70500`).
pub fn parse_total_count(text: &str) -> Option<i64> {
    let caps = DIGITS_RE.captures(text)?;
    caps[0].replace(',', "").parse::<i64>().ok()
}

/// Guard shared by the HTML parsers: a page whose expected result container
/// is entirely absent means the layout changed (or a block page was served),
/// which must surface as an error rather than a silent empty result.
pub fn layout_error(vertical: &str) -> anyhow::Error {
    anyhow!("{vertical} result blocks not found (Bing page layout may have changed)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_whitespace_leaves_decoded_text_alone() {
        assert_eq!(
            collapse_whitespace(
                " a &lt;b&gt;
 c "
            ),
            "a &lt;b&gt; c"
        );
    }

    #[test]
    fn clean_text_strips_tags_markers_and_entities() {
        assert_eq!(
            clean_text("<strong>テスト</strong> &amp; 実験"),
            "テスト & 実験"
        );
        assert_eq!(clean_text("とは"), "とは");
        assert_eq!(clean_text("a\n b\t c"), "a b c");
    }

    #[test]
    fn decode_click_url_unwraps_bing_redirect() {
        // u = "a1" + base64("https://example.com/page")
        let href = "https://www.bing.com/ck/a?!&&p=abc&u=a1aHR0cHM6Ly9leGFtcGxlLmNvbS9wYWdl&ntb=1";
        assert_eq!(decode_click_url(href), "https://example.com/page");
        assert_eq!(
            decode_click_url("https://example.com/direct"),
            "https://example.com/direct"
        );
    }

    #[test]
    fn parse_total_count_reads_grouped_digits() {
        assert_eq!(parse_total_count("70,500 件の検索結果"), Some(70500));
        assert_eq!(parse_total_count("約 1,234 件"), Some(1234));
        assert_eq!(parse_total_count("no numbers"), None);
    }

    #[test]
    fn resolve_url_handles_relative_paths() {
        assert_eq!(
            resolve_url("/images/search?view=detailV2", "https://www.bing.com"),
            "https://www.bing.com/images/search?view=detailV2"
        );
        assert_eq!(
            resolve_url("//th.bing.com/th?id=1", "https://www.bing.com"),
            "https://th.bing.com/th?id=1"
        );
    }
}
