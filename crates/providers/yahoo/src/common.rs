//! Shared helpers: SSR payload extraction, HTML/entity cleanup, query building.

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;
use zs_common::TAG_RE;
pub use zs_common::{clamp_limit, decode_entities, urlencode};

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
pub const ACCEPT_LANGUAGE: &str = "ja,en-US;q=0.9,en;q=0.8";

static NEXT_DATA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)<script id="__NEXT_DATA__"[^>]*>(.*?)</script>"#).unwrap());

static PRELOADED_STATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)__PRELOADED_STATE__\s*=\s*(\{.*?\});?\s*</script>"#).unwrap()
});

static WINDOW_PROPS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)window\.PROPS\s*=\s*(\{.*?\});?\s*(?:window\.|</script>)"#).unwrap()
});

/// `<script id="__NEXT_DATA__">` — used by web / image / video / realtime search.
pub fn extract_next_data(html: &str) -> Result<Value> {
    let caps = NEXT_DATA_RE.captures(html).ok_or_else(|| {
        anyhow!("__NEXT_DATA__ block not found (Yahoo page layout may have changed)")
    })?;
    Ok(serde_json::from_str(&caps[1])?)
}

/// `__PRELOADED_STATE__ = {...}` — used by Yahoo! News search.
pub fn extract_preloaded_state(html: &str) -> Result<Value> {
    let caps = PRELOADED_STATE_RE.captures(html).ok_or_else(|| {
        anyhow!("__PRELOADED_STATE__ block not found (Yahoo page layout may have changed)")
    })?;
    Ok(serde_json::from_str(&caps[1])?)
}

/// `window.PROPS = {...}` — used by Yahoo! Chiebukuro search.
pub fn extract_window_props(html: &str) -> Result<Value> {
    let caps = WINDOW_PROPS_RE.captures(html).ok_or_else(|| {
        anyhow!("window.PROPS block not found (Yahoo page layout may have changed)")
    })?;
    Ok(serde_json::from_str(&caps[1])?)
}

/// Strip markup, decode entities and drop Yahoo's private highlight markers.
///
/// Yahoo marks matched terms three different ways depending on the vertical:
/// `<b>`/`<em>` tags (web, chiebukuro), `\u{2}`..`\u{3}` (news) and
/// `\tSTART\t`..`\tEND\t` (realtime). All of them are noise for an LLM.
pub fn clean_text(s: &str) -> String {
    let without_tags = TAG_RE.replace_all(s, "");
    let without_markers = without_tags
        .replace("\u{2}", "")
        .replace("\u{3}", "")
        .replace("\tSTART\t", "")
        .replace("\tEND\t", "");
    decode_entities(&without_markers).trim().to_string()
}

pub fn get_str(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(clean_text)
        .filter(|s| !s.is_empty())
}

/// Like [`get_str`] but keeps the value verbatim — for URLs, IDs and other
/// fields where entity decoding would corrupt the value.
pub fn get_raw(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

pub fn get_i64(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

pub fn get_path<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_strips_tags_and_entities() {
        assert_eq!(clean_text("<b>ラーメン</b> &middot; 店"), "ラーメン · 店");
        assert_eq!(clean_text("a &amp;lt; b"), "a &lt; b");
        assert_eq!(clean_text("&#x3042;&#12356;"), "あい");
    }

    #[test]
    fn clean_text_strips_highlight_markers() {
        assert_eq!(clean_text("東京の\u{2}ラーメン\u{3}屋"), "東京のラーメン屋");
        assert_eq!(
            clean_text("好きな\tSTART\tラーメン\tEND\t屋"),
            "好きなラーメン屋"
        );
    }

    #[test]
    fn urlencode_handles_multibyte() {
        assert_eq!(
            urlencode("ラーメン"),
            "%E3%83%A9%E3%83%BC%E3%83%A1%E3%83%B3"
        );
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
    }

    #[test]
    fn clamp_limit_bounds() {
        assert_eq!(clamp_limit(0, 50), 1);
        assert_eq!(clamp_limit(999, 50), 50);
        assert_eq!(clamp_limit(10, 50), 10);
    }
}
