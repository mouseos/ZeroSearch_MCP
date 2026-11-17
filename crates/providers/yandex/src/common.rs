//! Shared helpers: app-identical request identity, embedded-state extraction,
//! HTML/entity cleanup and query building.

use anyhow::{anyhow, Result};
use serde_json::Value;
use zs_common::TAG_RE;
pub use zs_common::{clamp_limit, decode_entities, urlencode};

/// User agent of the official Yandex app (Yandex Start) web view, as measured
/// against the live service. Yandex serves the touch result pages — and only
/// issues a working session — for this request shape.
pub const USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
pub const ACCEPT_LANGUAGE: &str = "ru-RU,ru;q=0.9";
/// Package name of the Yandex app; sent by the app's web view on every load.
pub const REQUESTED_WITH: &str = "ru.yandex.searchplugin";
/// The app's `X-Yandex-Flags` header value
/// (`WebVerticalsUtils.getXYandexFlagsHeaderValue()` in the APK
/// analysis; audit W-39).
pub const YANDEX_FLAGS: &str = "appsearch-header=1;disable_intents=1;accept-api=1";
/// The `ui` query parameter the app attaches to its search page
/// loads (`WebVerticalsUtils.b()`; audit W-39).
pub const UI_PARAM: &str = "ui=webmobileapp.yandex";

/// Extract the touch app's initial state: a JSON document stored,
/// HTML-entity-escaped, in a `data-state="…"` attribute. Several elements
/// carry `data-state`; the search state is the one mentioning `serpList`.
pub fn extract_data_state(html: &str) -> Result<Value> {
    let mut rest = html;
    while let Some(pos) = rest.find("data-state=\"") {
        let value_start = pos + "data-state=\"".len();
        let Some(value_end) = rest[value_start..].find('"') else {
            break;
        };
        let raw = &rest[value_start..value_start + value_end];
        if raw.contains("serpList") {
            let decoded = decode_entities(raw);
            return Ok(serde_json::from_str(&decoded)?);
        }
        rest = &rest[value_start + value_end..];
    }
    Err(anyhow!(
        "initial state (data-state) not found (Yandex page layout may have changed)"
    ))
}

/// Extract a JSON object embedded in a page: finds `marker`, then parses the
/// balanced `{…}` block that follows it, honouring string literals and
/// escapes (used for the video page's duplicate-metadata map). If the marker
/// itself ends with `{`, that brace is the object's opening brace.
pub fn extract_json_after(html: &str, marker: &str) -> Option<Value> {
    let mpos = html.find(marker)?;
    let obj_start = if marker.ends_with('{') {
        mpos + marker.len() - 1
    } else {
        let after = mpos + marker.len();
        after + html[after..].find('{')?
    };
    let bytes = html.as_bytes();
    let mut i = obj_start;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return serde_json::from_str(&html[obj_start..=i]).ok();
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Make a Yandex-relative or protocol-relative URL absolute.
pub fn absolutize(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("//") {
        format!("https://{rest}")
    } else if url.starts_with('/') {
        format!("https://yandex.ru{url}")
    } else {
        url.to_string()
    }
}

/// Strip markup, decode entities and drop Yandex's highlight markers.
///
/// Matched terms are wrapped in `<b>` tags in the HTML results, and marked
/// with `\u{7}` in the embedded JSON metadata. Both are noise for an LLM.
pub fn clean_text(s: &str) -> String {
    let without_tags = TAG_RE.replace_all(s, "");
    let without_markers = without_tags.replace('\u{7}', "");
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
    fn clean_text_strips_tags_entities_and_markers() {
        assert_eq!(
            clean_text("<b>тест</b> &middot; страница"),
            "тест · страница"
        );
        assert_eq!(clean_text("a &amp;lt; b"), "a &lt; b");
        assert_eq!(clean_text("\u{7}[Тест\u{7}] на IQ"), "[Тест] на IQ");
    }

    #[test]
    fn urlencode_handles_multibyte() {
        assert_eq!(urlencode("тест"), "%D1%82%D0%B5%D1%81%D1%82");
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
    }

    #[test]
    fn clamp_limit_bounds() {
        assert_eq!(clamp_limit(0, 50), 1);
        assert_eq!(clamp_limit(999, 50), 50);
        assert_eq!(clamp_limit(10, 50), 10);
    }

    #[test]
    fn absolutize_handles_relative_forms() {
        assert_eq!(
            absolutize("//avatars.mds.yandex.net/i?id=1"),
            "https://avatars.mds.yandex.net/i?id=1"
        );
        assert_eq!(
            absolutize("/video/touch/preview/42"),
            "https://yandex.ru/video/touch/preview/42"
        );
        assert_eq!(absolutize("https://example.com/"), "https://example.com/");
    }

    #[test]
    fn extract_data_state_picks_the_serp_list_state() {
        let html = r#"<div data-state="{&quot;a&quot;:1}"></div><div data-state="{&quot;initialState&quot;:{&quot;serpList&quot;:{&quot;items&quot;:{}}}}"></div>"#;
        let state = extract_data_state(html).unwrap();
        assert!(state["initialState"]["serpList"].is_object());
        assert!(extract_data_state("<html>no state</html>").is_err());
    }

    #[test]
    fn extract_json_after_parses_balanced_object() {
        let html = r#"{"dups":{"1":{"title":"a } brace in string","n":2},"2":{}}}"#;
        let dups = extract_json_after(html, "\"dups\":").unwrap();
        assert_eq!(dups["1"]["title"], "a } brace in string");
        assert_eq!(dups["1"]["n"], 2);
        assert!(extract_json_after(html, "\"missing\":").is_none());
    }
}
