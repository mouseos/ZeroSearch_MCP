//! Amazon.co.jp (Amazon Shopping app) provider.
//!
//! The app has no native search API: search and product pages
//! are WebView loads. This provider reproduces what the app's
//! WebView sends — the app-format User-Agent
//! (`Amazon.com/<version> (Android/<release>/<model>)`, not a
//! browser UA), the pre-navigation cookies the app syncs into the
//! WebView (`mobile-device-info`, `amzn-app-id`), and plain
//! `/s` / `/dp/<ASIN>` URLs — and extracts the product data from
//! the server-rendered HTML. No browser engine is involved.
//! `amzn-app-ctxt` is deliberately not sent: live verification
//! succeeded without it, and its value embeds device serials.
//!
//! The fetches themselves are made with libcurl (the `curl`
//! crate): the same requests sent through a different HTTP
//! client stack are answered with a robot check (HTTP 503),
//! while libcurl's requests — the requests the curl tool makes —
//! are served. Only this provider uses libcurl; the rest of the
//! workspace stays on reqwest.
//!
//! Note: Amazon serves these pages only to Japan-side access in
//! practice; from other egress the request typically times out
//! or is answered with a robot check.

use anyhow::{anyhow, Result};
use curl::easy::{Easy, HttpVersion, List};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const HOST: &str = "https://www.amazon.co.jp";
const USER_AGENT: &str = "Amazon.com/32.20.2.100 (Android/14/Pixel 8)";
/// `mobile-device-info`: `dpi:<dpi>|w:<width>|h:<height>` built
/// from the device profile, as the app generates it.
const COOKIE_DEVICE_INFO: &str = "dpi:420|w:1080|h:2400";
/// `amzn-app-id`: `<app name>/<versionName>/<build_revision>`.
/// App name from the app's strings, build revision from the
/// APK's `res/raw/build_revision.txt` (32.20.2.100).
const COOKIE_APP_ID: &str = "AmazonShopping/32.20.2.100/18.0.700537.0";

/// Sort values for the `s` parameter (Amazon web vocabulary):
/// empty / `relevancerank`, `price-asc-rank`, `price-desc-rank`,
/// `review-rank`, `date-desc-rank`.
pub const SORTS: &[&str] = &[
    "relevancerank",
    "price-asc-rank",
    "price-desc-rank",
    "review-rank",
    "date-desc-rank",
];

// ---------------------------------------------------------------------------
// Shared result types (same shape as the other shopping providers)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Price {
    pub value: i64,
    pub currency: String,
    pub display: String,
}

impl Price {
    fn yen(v: i64) -> Self {
        Self {
            value: v,
            currency: "JPY".into(),
            display: format!("¥{v}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Item {
    /// The ASIN.
    pub product_id: String,
    pub title: String,
    pub url: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sponsored: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub provider: String,
    pub ok: bool,
    pub query: String,
    pub page: i32,
    pub limit: i32,
    pub sort: String,
    pub items: Vec<Item>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemResult {
    pub provider: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<Item>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Search URL construction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub query: String,
    /// Web-side `page` parameter.
    pub page: i32,
    /// Client-side cap on returned items.
    pub limit: i32,
    /// `s` parameter; see [`SORTS`]. Empty omits it.
    pub sort: String,
    /// `rank` parameter (the app's sort hand-off field).
    pub rank: String,
    /// Department alias → rh `i:`.
    pub department: String,
    /// Category node id → rh `n:`.
    pub category_node: String,
    /// Price band in JPY → rh `p_36:<min>-<max>`.
    pub price_min: Option<i64>,
    pub price_max: Option<i64>,
    /// Prime-eligible only → rh `p_85`.
    pub prime_only: bool,
    /// In-stock only → rh `p_n_availability`.
    pub in_stock_only: bool,
    /// Minimum star rating (1..=4) → rh `p_72`.
    pub min_rating: Option<i32>,
    /// Brand name → rh `p_89:`.
    pub brand: String,
    /// Seller id → `me` parameter.
    pub seller_id: String,
    /// Raw extra `rh` entries (`key:value`, comma-joined),
    /// appended after the structured ones.
    pub rh: String,
}

/// Assemble the `rh` refinement string the way the app's
/// `getRefinementsString` does (`key:value` pairs, comma-joined).
/// The facet values follow Amazon's web vocabulary (the app
/// passes the web UI's refinement map through unchanged).
pub fn build_rh(params: &SearchParams) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !params.department.is_empty() {
        parts.push(format!("i:{}", params.department));
    }
    if !params.category_node.is_empty() {
        parts.push(format!("n:{}", params.category_node));
    }
    if params.price_min.is_some() || params.price_max.is_some() {
        parts.push(format!(
            "p_36:{}-{}",
            params.price_min.unwrap_or(0),
            params.price_max.unwrap_or(i32::MAX as i64)
        ));
    }
    if params.prime_only {
        parts.push("p_85:2470954011".into());
    }
    if params.in_stock_only {
        parts.push("p_n_availability:2494232051".into());
    }
    if let Some(r) = params.min_rating {
        let v = match r {
            4 => "2661618011",
            3 => "2661619011",
            2 => "2661620011",
            _ => "2661621011",
        };
        parts.push(format!("p_72:{v}"));
    }
    if !params.brand.is_empty() {
        parts.push(format!("p_89:{}", params.brand));
    }
    if !params.rh.is_empty() {
        parts.push(params.rh.clone());
    }
    parts.join(",")
}

/// Build the search URL. Pure; unit-tested.
pub fn build_search_url(params: &SearchParams) -> String {
    let mut url = format!("{HOST}/s?k={}", url_encode(&params.query));
    if !params.sort.is_empty() {
        url.push_str(&format!("&s={}", url_encode(&params.sort)));
    }
    if !params.rank.is_empty() {
        url.push_str(&format!("&rank={}", url_encode(&params.rank)));
    }
    let rh = build_rh(params);
    if !rh.is_empty() {
        url.push_str(&format!("&rh={}", url_encode(&rh)));
    }
    if !params.seller_id.is_empty() {
        url.push_str(&format!("&me={}", url_encode(&params.seller_id)));
    }
    if params.page > 1 {
        url.push_str(&format!("&page={}", params.page));
    }
    url
}

pub fn build_detail_url(asin: &str) -> String {
    format!("{HOST}/dp/{asin}")
}

/// Extract an ASIN from a `/dp/<ASIN>` URL or pass a bare ASIN
/// through.
pub fn extract_asin(product: &str) -> String {
    static DP_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"/(?:dp|gp/product)/([A-Z0-9]{10})").unwrap());
    let s = product.trim();
    if let Some(c) = DP_RE.captures(s) {
        return c[1].to_string();
    }
    s.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(s)
        .to_string()
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// HTML extraction
// ---------------------------------------------------------------------------

static ASIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"data-asin="([A-Z0-9]{10})""#).unwrap());
static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<h2[^>]*aria-label="([^"]+)""#).unwrap());
static PRICE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"a-price-whole">([0-9,]+)"#).unwrap());
static S_IMAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<img[^>]*class="s-image"[^>]*src="([^"]+)""#).unwrap());
static RATING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"5つ星のうち([0-9.]+)").unwrap());
static DOC_TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<title>(.*?)</title>").unwrap());
static LANDING_IMG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""landingImageUrl":"(https://[^"]+)""#).unwrap());
static OG_IMAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"property="og:image" content="([^"]+)""#).unwrap());
static REVIEW_COUNT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"id="acrCustomerReviewText"[^>]*>([0-9,]+)"#).unwrap());

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// True when the page is a robot check / apology page rather
/// than search results.
pub fn is_block_page(html: &str) -> bool {
    html.contains("validateCaptcha")
        || html.contains("Robot Check")
        || html.contains("ロボット") && html.contains("チェック")
}

/// Parse a search results page. Pure; unit-tested against the
/// captured live page.
pub fn parse_search_html(html: &str, limit: usize) -> Vec<Item> {
    let mut items = Vec::new();
    for chunk in html
        .split("data-component-type=\"s-search-result\"")
        .skip(1)
    {
        if items.len() >= limit {
            break;
        }
        let Some(asin) = ASIN_RE.captures(chunk).map(|c| c[1].to_string()) else {
            continue;
        };
        let raw_title = TITLE_RE
            .captures(chunk)
            .map(|c| html_unescape(&c[1]))
            .unwrap_or_default();
        let sponsored = chunk.contains("AdHolder") || raw_title.starts_with("スポンサー広告");
        let title = raw_title
            .strip_prefix("スポンサー広告 - ")
            .unwrap_or(&raw_title)
            .to_string();
        let price = PRICE_RE
            .captures(chunk)
            .and_then(|c| c[1].replace(',', "").parse::<i64>().ok());
        items.push(Item {
            product_id: asin.clone(),
            title,
            url: build_detail_url(&asin),
            source: "amazon".into(),
            price: price.map(Price::yen),
            image: S_IMAGE_RE.captures(chunk).map(|c| c[1].to_string()),
            review_score: RATING_RE
                .captures(chunk)
                .and_then(|c| c[1].parse::<f64>().ok()),
            sponsored: Some(sponsored),
            ..Default::default()
        });
    }
    items
}

/// Parse a product detail page. Pure; unit-tested against
/// segments of the captured live page.
pub fn parse_detail_html(html: &str, asin: &str) -> Option<Item> {
    let doc_title = DOC_TITLE_RE
        .captures(html)?
        .get(1)?
        .as_str()
        .trim()
        .to_string();
    let mut title = doc_title
        .strip_prefix("Amazon.co.jp:")
        .map(|s| s.trim().to_string())
        .unwrap_or(doc_title);
    // The <title> form is "<product title> : <department>".
    if let Some(idx) = title.rfind(" : ") {
        title = title[..idx].trim().to_string();
    }
    if title.is_empty() || title.contains("Robot Check") {
        return None;
    }
    // Prefer the price inside the core price display block.
    let price = html
        .find("corePriceDisplay")
        .and_then(|i| PRICE_RE.captures(&html[i..]))
        .or_else(|| PRICE_RE.captures(html))
        .and_then(|c| c[1].replace(',', "").parse::<i64>().ok());
    let image = LANDING_IMG_RE
        .captures(html)
        .map(|c| c[1].to_string())
        .or_else(|| OG_IMAGE_RE.captures(html).map(|c| c[1].to_string()));
    Some(Item {
        product_id: asin.to_string(),
        title,
        url: build_detail_url(asin),
        source: "amazon".into(),
        price: price.map(Price::yen),
        image,
        review_score: RATING_RE
            .captures(html)
            .and_then(|c| c[1].parse::<f64>().ok()),
        review_count: REVIEW_COUNT_RE
            .captures(html)
            .and_then(|c| c[1].replace(',', "").parse::<i64>().ok()),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Client (manual cookie jar, mirroring the app's WebView cookies)
// ---------------------------------------------------------------------------

pub struct AmazonClient {
    cookies: Mutex<HashMap<String, String>>,
}

impl AmazonClient {
    pub fn new() -> Self {
        let mut jar = HashMap::new();
        // The cookies the app syncs into the WebView before the
        // first navigation. Server-issued cookies (session-id
        // etc.) are captured from responses and added to the jar.
        jar.insert(
            "mobile-device-info".to_string(),
            COOKIE_DEVICE_INFO.to_string(),
        );
        jar.insert("amzn-app-id".to_string(), COOKIE_APP_ID.to_string());
        Self {
            cookies: Mutex::new(jar),
        }
    }

    fn cookie_header(&self) -> String {
        let jar = self.cookies.lock().unwrap();
        let mut pairs: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
        pairs.sort();
        pairs.join("; ")
    }

    /// Store the cookies from a response's `Set-Cookie` header
    /// values (the part after the header name, one per entry).
    fn store_response_cookies(&self, set_cookie_values: &[String]) {
        let mut jar = self.cookies.lock().unwrap();
        for s in set_cookie_values {
            let first = s.split(';').next().unwrap_or("");
            if let Some((k, v)) = first.split_once('=') {
                jar.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }

    async fn get_html(&self, url: &str) -> Result<String> {
        let cookie = self.cookie_header();
        let url = url.to_string();
        let (status, set_cookies, html) =
            tokio::task::spawn_blocking(move || fetch_blocking(&url, &cookie))
                .await
                .map_err(|e| anyhow!("amazon fetch task failed: {e}"))??;
        self.store_response_cookies(&set_cookies);
        if status == 503 {
            return Err(anyhow!(
                "amazon returned HTTP 503 (robot check; Japan-side access is required in practice)"
            ));
        }
        if !(200..300).contains(&status) {
            return Err(anyhow!("amazon returned HTTP {status}"));
        }
        Ok(html)
    }

    pub async fn search_products(&self, params: SearchParams) -> Result<SearchResult> {
        let url = build_search_url(&params);
        let html = self.get_html(&url).await?;
        if is_block_page(&html) {
            return Err(anyhow!(
                "amazon answered with a robot check page (Japan-side access is required in practice)"
            ));
        }
        let limit = if params.limit > 0 {
            params.limit as usize
        } else {
            usize::MAX
        };
        let items = parse_search_html(&html, limit);
        Ok(SearchResult {
            provider: "amazon".into(),
            ok: true,
            query: params.query.clone(),
            page: params.page,
            limit: params.limit,
            sort: params.sort.clone(),
            count: items.len(),
            items,
        })
    }

    pub async fn get_product(&self, product: &str) -> Result<ItemResult> {
        let asin = extract_asin(product);
        let url = build_detail_url(&asin);
        let html = self.get_html(&url).await?;
        if is_block_page(&html) {
            return Ok(ItemResult {
                provider: "amazon".into(),
                ok: false,
                item: None,
                error: Some(
                    "amazon answered with a robot check page (Japan-side access is required in practice)"
                        .into(),
                ),
            });
        }
        match parse_detail_html(&html, &asin) {
            Some(item) => Ok(ItemResult {
                provider: "amazon".into(),
                ok: true,
                item: Some(item),
                error: None,
            }),
            None => Ok(ItemResult {
                provider: "amazon".into(),
                ok: false,
                item: None,
                error: Some("amazon detail: no product data in the returned page".into()),
            }),
        }
    }
}

/// One blocking libcurl fetch. Returns the final HTTP status,
/// the response's `Set-Cookie` values, and the body as text.
///
/// The request is deliberately the request the curl tool makes:
/// libcurl's own TLS and HTTP stacks, HTTP/1.1 (the
/// measured-working shape from the round2 verification fetches;
/// HTTP/2 negotiation stalls against this service in practice),
/// and no proxy configuration of ours — libcurl applies its
/// standard environment-based proxy behaviour exactly like the
/// curl tool does.
fn fetch_blocking(url: &str, cookie_header: &str) -> Result<(u32, Vec<String>, String)> {
    let mut easy = Easy::new();
    easy.url(url)?;
    easy.follow_location(true)?;
    easy.http_version(HttpVersion::V11)?;
    // The curl crate defaults to CURLOPT_NOSIGNAL, so timeouts
    // are not delivered as signals inside the multi-threaded
    // server process.
    easy.timeout(std::time::Duration::from_secs(60))?;
    easy.useragent(USER_AGENT)?;
    let mut headers = List::new();
    headers.append("Accept-Language: ja-JP")?;
    headers.append("Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")?;
    headers.append(&format!("Cookie: {cookie_header}"))?;
    easy.http_headers(headers)?;

    let mut body: Vec<u8> = Vec::new();
    let mut set_cookies: Vec<String> = Vec::new();
    {
        let mut transfer = easy.transfer();
        transfer.write_function(|data| {
            body.extend_from_slice(data);
            Ok(data.len())
        })?;
        transfer.header_function(|line| {
            let line = String::from_utf8_lossy(line);
            let line = line.trim_end();
            if let Some((name, value)) = line.split_once(':') {
                if name.trim().eq_ignore_ascii_case("set-cookie") {
                    set_cookies.push(value.trim().to_string());
                }
            }
            true
        })?;
        transfer.perform()?;
    }
    let status = easy.response_code()?;
    let html = String::from_utf8_lossy(&body).into_owned();
    Ok((status, set_cookies, html))
}

impl Default for AmazonClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_url_minimal() {
        let url = build_search_url(&SearchParams {
            query: "ワイヤレスイヤホン".into(),
            ..Default::default()
        });
        assert!(url.starts_with("https://www.amazon.co.jp/s?k="));
        assert!(!url.contains("&s="));
        assert!(!url.contains("&page="));
    }

    #[test]
    fn search_url_with_sort_filters_and_page() {
        let url = build_search_url(&SearchParams {
            query: "イヤホン".into(),
            page: 2,
            sort: "price-asc-rank".into(),
            price_min: Some(1000),
            price_max: Some(5000),
            prime_only: true,
            min_rating: Some(4),
            brand: "Anker".into(),
            seller_id: "A1EXAMPLE".into(),
            ..Default::default()
        });
        assert!(url.contains("&s=price-asc-rank"));
        assert!(url.contains("&page=2"));
        assert!(url.contains("&me=A1EXAMPLE"));
        let rh = build_rh(&SearchParams {
            price_min: Some(1000),
            price_max: Some(5000),
            prime_only: true,
            min_rating: Some(4),
            brand: "Anker".into(),
            ..Default::default()
        });
        assert_eq!(
            rh,
            "p_36:1000-5000,p_85:2470954011,p_72:2661618011,p_89:Anker"
        );
    }

    #[test]
    fn parse_search_fixture() {
        let html = include_str!("../tests/fixtures/search.html");
        let items = parse_search_html(html, 50);
        assert!(items.len() >= 5, "got {} items", items.len());
        let first = &items[0];
        assert_eq!(first.product_id.len(), 10);
        assert!(!first.title.is_empty());
        assert!(!first.title.starts_with("スポンサー広告"));
        assert_eq!(first.sponsored, Some(true));
        assert!(first.url.ends_with(&first.product_id));
        assert!(first.price.is_some());
        assert!(first.image.as_ref().unwrap().contains("media-amazon.com"));
    }

    #[test]
    fn parse_detail_fixture() {
        let html = include_str!("../tests/fixtures/detail.html");
        let item = parse_detail_html(html, "B0FNRS5WB2").expect("detail must parse");
        assert!(item.title.contains("Anker Soundcore P31i"));
        assert_eq!(item.price.as_ref().unwrap().value, 4990);
        assert!(item.image.as_ref().unwrap().contains("media-amazon.com"));
        assert_eq!(item.review_score, Some(4.4));
    }

    #[test]
    fn asin_extraction() {
        assert_eq!(
            extract_asin("https://www.amazon.co.jp/dp/B0FNRS5WB2"),
            "B0FNRS5WB2"
        );
        assert_eq!(
            extract_asin("https://www.amazon.co.jp/gp/product/B0FNRS5WB2/ref=x"),
            "B0FNRS5WB2"
        );
        assert_eq!(extract_asin("B0FNRS5WB2"), "B0FNRS5WB2");
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored (requires Japan-side access)"]
    async fn test_live_search() {
        let client = AmazonClient::new();
        let result = client
            .search_products(SearchParams {
                query: "ワイヤレスイヤホン".into(),
                limit: 5,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok);
        assert!(!result.items.is_empty());
    }
}
