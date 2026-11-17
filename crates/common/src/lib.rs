//! Helpers that were copied verbatim into several providers.
//!
//! Every function here reproduces the exact output of the per-provider
//! copies it replaced; the tests pin that equivalence.

use regex::Regex;
use std::sync::LazyLock;

/// Any HTML/XML tag (`<...>`), for stripping markup before entity decoding.
pub static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").unwrap());

/// Named entities decoded by [`decode_entities`], other than `&amp;`.
const NAMED_ENTITIES: [(&str, &str); 16] = [
    ("&nbsp;", " "),
    ("&middot;", "·"),
    ("&hellip;", "…"),
    ("&mdash;", "—"),
    ("&ndash;", "–"),
    ("&laquo;", "«"),
    ("&raquo;", "»"),
    ("&lsquo;", "\u{2018}"),
    ("&rsquo;", "\u{2019}"),
    ("&ldquo;", "\u{201c}"),
    ("&rdquo;", "\u{201d}"),
    ("&quot;", "\""),
    ("&#39;", "'"),
    ("&apos;", "'"),
    ("&lt;", "<"),
    ("&gt;", ">"),
];

/// Output buffer that applies the final `&amp;` -> `&` substitution on
/// the fly, with `str::replace` semantics (leftmost, non-overlapping, no
/// rescanning of a replacement): the `&` a replacement leaves behind can
/// never start a new match, so `&amp;amp;` decodes one level only, to
/// `&amp;`. A `&` produced by a numeric reference (`&#38;`) does take
/// part in matching, exactly as it did when `&amp;` was replaced in a
/// separate pass after the numeric references.
struct AmpSink {
    out: String,
    /// Byte offset before which no `&amp;` match may start.
    floor: usize,
}

impl AmpSink {
    fn push(&mut self, c: char) {
        self.out.push(c);
        if c == ';' && self.out.len() >= self.floor + 5 && self.out.ends_with("&amp;") {
            let keep = self.out.len() - 4;
            self.out.truncate(keep);
            self.floor = keep;
        }
    }

    fn push_str(&mut self, s: &str) {
        for c in s.chars() {
            self.push(c);
        }
    }
}

/// Length of a numeric character reference `&#N;` / `&#xH;` at the start
/// of `s` (lowercase `x` only) and the character it names, if valid.
/// The length is returned even when the value is not a valid `char`
/// (overflow, surrogate, beyond U+10FFFF) so the caller keeps it verbatim.
fn numeric_entity(s: &str) -> Option<(usize, Option<char>)> {
    let rest = s.strip_prefix("&#")?;
    let (digits, radix, prefix) = match rest.strip_prefix('x') {
        Some(hex) => (hex, 16, 3),
        None => (rest, 10, 2),
    };
    let n = digits
        .bytes()
        .take_while(|b| {
            if radix == 16 {
                b.is_ascii_hexdigit()
            } else {
                b.is_ascii_digit()
            }
        })
        .count();
    if n == 0 || digits.as_bytes().get(n) != Some(&b';') {
        return None;
    }
    let code = u32::from_str_radix(&digits[..n], radix)
        .ok()
        .and_then(char::from_u32);
    Some((prefix + n + 1, code))
}

/// Decode the HTML entities the search pages use: a fixed set of named
/// entities, decimal/hex numeric references (invalid ones are kept
/// verbatim) and, last, `&amp;`, so `&amp;lt;` becomes `&lt;` rather
/// than `<`. Unknown entities are left as they are. Single pass.
pub fn decode_entities(s: &str) -> String {
    let mut sink = AmpSink {
        out: String::with_capacity(s.len()),
        floor: 0,
    };
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        sink.push_str(&rest[..pos]);
        rest = &rest[pos..];
        if let Some((name, value)) = NAMED_ENTITIES.iter().find(|(n, _)| rest.starts_with(n)) {
            sink.push_str(value);
            rest = &rest[name.len()..];
        } else if let Some((len, code)) = numeric_entity(rest) {
            match code {
                Some(c) => sink.push(c),
                None => sink.push_str(&rest[..len]),
            }
            rest = &rest[len..];
        } else {
            sink.push('&');
            rest = &rest[1..];
        }
    }
    sink.push_str(rest);
    sink.out
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// Percent-encode a string for use in a query value (RFC 3986 unreserved
/// set kept, every other byte as `%XX` with uppercase hex).
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                out.push('%');
                out.push(HEX_UPPER[(byte >> 4) as usize] as char);
                out.push(HEX_UPPER[(byte & 0x0f) as usize] as char);
            }
        }
    }
    out
}

/// Clamp a caller-supplied limit into `1..=max`.
pub fn clamp_limit(limit: i64, max: i64) -> usize {
    limit.clamp(1, max) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chained implementation every provider used before this crate.
    fn decode_entities_chained(s: &str) -> String {
        static NUMERIC_ENTITY_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"&#(x[0-9a-fA-F]+|[0-9]+);").unwrap());
        let named = s
            .replace("&nbsp;", " ")
            .replace("&middot;", "·")
            .replace("&hellip;", "…")
            .replace("&mdash;", "—")
            .replace("&ndash;", "–")
            .replace("&laquo;", "«")
            .replace("&raquo;", "»")
            .replace("&lsquo;", "\u{2018}")
            .replace("&rsquo;", "\u{2019}")
            .replace("&ldquo;", "\u{201c}")
            .replace("&rdquo;", "\u{201d}")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&apos;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
        let numeric = NUMERIC_ENTITY_RE.replace_all(&named, |caps: &regex::Captures| {
            let raw = &caps[1];
            let code = if let Some(hex) = raw.strip_prefix('x').or_else(|| raw.strip_prefix('X')) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                raw.parse::<u32>().ok()
            };
            code.and_then(char::from_u32)
                .map(String::from)
                .unwrap_or_else(|| caps[0].to_string())
        });
        numeric.replace("&amp;", "&")
    }

    /// The per-byte `format!` implementation the providers used before.
    fn urlencode_old(s: &str) -> String {
        let mut out = String::new();
        for byte in s.as_bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(*byte as char)
                }
                _ => out.push_str(&format!("%{:02X}", byte)),
            }
        }
        out
    }

    const TRICKY: &[&str] = &[
        "",
        "plain text",
        "&",
        "&&",
        "a & b",
        "trailing &",
        "&amp;",
        "&amp;amp;",
        "&amp;amp;amp;",
        "&amp;lt;",
        "&amp;#65;",
        "&amp;#x41;",
        "&lt;b&gt;bold&lt;/b&gt;",
        "&foo;",
        "&foo",
        "&;",
        "&#;",
        "&#x;",
        "&#65",
        "&#65;",
        "&#x41;",
        "&#X41;",
        "&#xAbC;",
        "&#39;&apos;&quot;",
        "&#0;",
        "&#xD800;",
        "&#55296;",
        "&#x110000;",
        "&#1114112;",
        "&#99999999999;",
        "&#xFFFFFFFFF;",
        "&#00065;",
        "&#38;",
        "&#38;amp;",
        "&#38;lt;",
        "&#x26;amp;amp;",
        "&&#97;mp;",
        "&am&#112;;",
        "&amp&#59;",
        "&#38;&amp;",
        "&#38;#65;",
        "&#38;#38;amp;",
        "&nbsp;&middot;&hellip;&mdash;&ndash;&laquo;&raquo;",
        "&lsquo;&rsquo;&ldquo;&rdquo;",
        "&NBSP;&Amp;&LT;",
        "日本語&amp;テキスト&#x3042;&#12354;",
        "한국어 &lt;검색&gt; &#xAC00;",
        "€&#8364;&#x20AC;🎉&#x1F389;",
        "&&amp;;&amp;&;",
        "x&lt&gt;y",
        "&#x&#65;;",
        "&#&#51;9;",
    ];

    #[test]
    fn decode_entities_matches_chained_implementation() {
        for input in TRICKY {
            assert_eq!(
                decode_entities(input),
                decode_entities_chained(input),
                "input: {input:?}"
            );
        }
    }

    /// Deterministic fuzzing over entity fragments, so references split
    /// across or produced by other references are covered as well.
    #[test]
    fn decode_entities_matches_chained_on_generated_inputs() {
        const TOKENS: &[&str] = &[
            "&", "amp;", "&amp;", "&#38;", "&#x26;", "&#", "x", "26", "38", ";", "a", "mp", "lt;",
            "&lt;", "&#39;", "&#xD800;", "#", "日", " ", "&nbsp;", "&#59;", "&#97;",
        ];
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let len = (next() % 9) as usize;
            let input: String = (0..len)
                .map(|_| TOKENS[(next() % TOKENS.len() as u64) as usize])
                .collect();
            assert_eq!(
                decode_entities(&input),
                decode_entities_chained(&input),
                "input: {input:?}"
            );
        }
    }

    #[test]
    fn decode_entities_examples() {
        assert_eq!(decode_entities("&amp;lt;"), "&lt;");
        assert_eq!(decode_entities("&amp;amp;"), "&amp;");
        assert_eq!(decode_entities("&foo;"), "&foo;");
        assert_eq!(decode_entities("&#x41;&#66;"), "AB");
        assert_eq!(decode_entities("&#xD800;"), "&#xD800;");
    }

    #[test]
    fn urlencode_matches_old_implementation() {
        let extra: &[&str] = &["a b+c/d?e=f", "-_.~", "\u{0}\u{7f}ÿ"];
        for input in TRICKY.iter().chain(extra) {
            assert_eq!(urlencode(input), urlencode_old(input), "input: {input:?}");
        }
        assert_eq!(urlencode("日本"), "%E6%97%A5%E6%9C%AC");
    }

    #[test]
    fn clamp_limit_bounds() {
        assert_eq!(clamp_limit(0, 10), 1);
        assert_eq!(clamp_limit(-5, 10), 1);
        assert_eq!(clamp_limit(5, 10), 5);
        assert_eq!(clamp_limit(50, 10), 10);
    }

    #[test]
    fn tag_re_strips_markup() {
        assert_eq!(TAG_RE.replace_all("<b>a</b> <i x=\"1\">b</i>", ""), "a b");
    }
}
