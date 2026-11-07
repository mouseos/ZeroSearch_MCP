//! Yahoo! Shopping Japan (jp.co.yahoo.android.yshopping)
//! provider.
//!
//! Reproduces the current app's (16.12.0) native API traffic on
//! `shopping-appliproxy.yahooapis.jp`. Guests need no
//! authorization: the app sends only its built-in `appid`
//! constant. Search is `POST /searchList/v4/items`, detail is
//! `GET /item-detail/app/v2` addressed by the search result's
//! `itemId` (the `{storeId}_{itemCode}` id the app passes as its
//! `itemId` Bundle key).
//!
//! The current API has no review-list endpoint (search results
//! carry rating aggregates only). Reviews are served the way the
//! app itself shows them: `ItemReviewDeepLinkRouter` opens
//! `https://shopping.yahoo.co.jp/review/item/list` in
//! `WebViewActivity`, and that page's server-rendered data block
//! carries the review list, so `get_reviews` reads it from there.
//! The suggest endpoint's parameters are not established, so it
//! is not provided here.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const BASE: &str = "https://shopping-appliproxy.yahooapis.jp";
const APPID: &str = "dj00aiZpPWtHdjlEQk16eVA1ZCZzPWNvbnN1bWVyc2VjcmV0Jng9OGM-";
const APP_VERSION: &str = "16.12.0";
const USER_AGENT: &str = "okhttp/4.12.0";
const IMAGE_BASE: &str = "https://item-shopping.c.yimg.jp/i/k/";
/// The item review page the app opens in its WebView
/// (`ItemReviewDeepLinkRouter` builds this URL from the store id
/// and page key).
const REVIEW_PAGE_BASE: &str = "https://shopping.yahoo.co.jp/review/item/list";
/// A standard Android WebView user agent, used only for the
/// review page (the API calls keep the okhttp agent above). The
/// mobile variant of the page carries the review data.
const WEBVIEW_USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";

/// Sort orders for `yshopping_search_products` (the app's
/// `SearchResultV2Options$Order$Sort` enum names, sent verbatim).
pub const ORDERS: &[&str] = &[
    "Recommend",
    "Price",
    "TotalPrice",
    "ReviewCount",
    "ReviewScoreAverage",
    "DiscountRate",
    "CatalogReleaseDate",
    "RegistrationDate",
    "GoodDelivery",
];

const MODULE_LIST: &[&str] = &[
    "GenreCategories",
    "Brand",
    "Municipalities",
    "SiblingGenreCategories",
    "SiblingSpecs",
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
pub struct SellerInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Item {
    /// The `{storeId}_{itemCode}` id; pass it to
    /// `yshopping_get_product`.
    pub product_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    pub title: String,
    pub url: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Raw image ids from the detail response (the API hands out
    /// ids, not URLs, for detail images).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller: Option<SellerInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_count: Option<i64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_available: Option<i64>,
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
// Search (POST /searchList/v4/items)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SearchParams {
    pub query: String,
    /// Page size (`results`, app default 48).
    pub limit: i32,
    /// 1-based (`startIndex`).
    pub start_index: i32,
    /// One of [`ORDERS`].
    pub order: String,
    pub asc: bool,
    pub price_from: Option<i64>,
    pub price_to: Option<i64>,
    pub discounted: Option<bool>,
    pub discount_from: Option<i64>,
    pub discount_rate_from: Option<i64>,
    pub category_ids: Vec<String>,
    pub brand_id_list: Vec<String>,
    pub seller_id: String,
    pub seller_rating_from: Option<f64>,
    /// Passed through verbatim (the app's UsedStatus value).
    pub used_status: String,
    pub jan_code: String,
    pub include_sold_out: Option<bool>,
    pub good_delivery: Option<bool>,
    pub asutsuku: Option<bool>,
    pub furusato: Option<bool>,
    pub has_coupon: Option<bool>,
    pub has_premium_price: Option<bool>,
    pub is_verified: Option<bool>,
    /// Prefecture code (`deliveryArea`, app default "13").
    pub delivery_area: String,
    pub shipping_service_list: Vec<String>,
    pub conditional_free_shipping_price_to: Option<i64>,
    pub payment_list: Vec<String>,
    pub kspec_id_list: Vec<String>,
    pub sp_codes: Vec<String>,
    pub municipality_code: String,
    pub seller_event_group_id: String,
    pub campaign_tab_id: String,
}

impl Default for SearchParams {
    fn default() -> Self {
        Self {
            query: String::new(),
            limit: 48,
            start_index: 1,
            order: "Recommend".into(),
            asc: false,
            price_from: None,
            price_to: None,
            discounted: None,
            discount_from: None,
            discount_rate_from: None,
            category_ids: Vec::new(),
            brand_id_list: Vec::new(),
            seller_id: String::new(),
            seller_rating_from: None,
            used_status: String::new(),
            jan_code: String::new(),
            include_sold_out: None,
            good_delivery: None,
            asutsuku: None,
            furusato: None,
            has_coupon: None,
            has_premium_price: None,
            is_verified: None,
            delivery_area: "13".into(),
            shipping_service_list: Vec::new(),
            conditional_free_shipping_price_to: None,
            payment_list: Vec::new(),
            kspec_id_list: Vec::new(),
            sp_codes: Vec::new(),
            municipality_code: String::new(),
            seller_event_group_id: String::new(),
            campaign_tab_id: String::new(),
        }
    }
}

/// Build the search request body. The core keys are the ones the
/// app's builder (`SearchResultV2Options.a`) always emits for a
/// guest search; optional filter keys are added when set. Pure;
/// unit-tested.
pub fn build_search_body(params: &SearchParams) -> Value {
    let mut b = Map::new();
    b.insert("referer".into(), json!("shopping_search"));
    b.insert("mallSellerType".into(), json!("All"));
    b.insert("mallDisplayType".into(), json!("Shopping"));
    b.insert("query".into(), json!(params.query));
    b.insert("results".into(), json!(params.limit.max(1)));
    b.insert("startIndex".into(), json!(params.start_index.max(1)));
    b.insert("moduleList".into(), json!(MODULE_LIST));
    b.insert("ranking".into(), json!("shopping_search"));
    b.insert(
        "order".into(),
        json!(if params.order.is_empty() {
            "Recommend"
        } else {
            params.order.as_str()
        }),
    );
    b.insert("asc".into(), json!(params.asc));
    b.insert(
        "deliveryArea".into(),
        json!(if params.delivery_area.is_empty() {
            "13"
        } else {
            params.delivery_area.as_str()
        }),
    );
    b.insert("stampRallyRankCode".into(), json!("2000"));
    if let Some(v) = params.price_from {
        b.insert("priceFrom".into(), json!(v));
    }
    if let Some(v) = params.price_to {
        b.insert("priceTo".into(), json!(v));
    }
    if let Some(v) = params.discounted {
        b.insert("discounted".into(), json!(v));
    }
    if let Some(v) = params.discount_from {
        b.insert("discountFrom".into(), json!(v));
    }
    if let Some(v) = params.discount_rate_from {
        b.insert("discountRateFrom".into(), json!(v));
    }
    if !params.category_ids.is_empty() {
        b.insert("categoryIds".into(), json!(params.category_ids));
    }
    if !params.brand_id_list.is_empty() {
        b.insert("brandIdList".into(), json!(params.brand_id_list));
    }
    if !params.seller_id.is_empty() {
        b.insert("sellerId".into(), json!(params.seller_id));
    }
    if let Some(v) = params.seller_rating_from {
        b.insert("sellerRatingFrom".into(), json!(v.to_string()));
    }
    if !params.used_status.is_empty() {
        b.insert("usedStatus".into(), json!(params.used_status));
    }
    if !params.jan_code.is_empty() {
        b.insert("janCode".into(), json!(params.jan_code));
    }
    if let Some(v) = params.include_sold_out {
        b.insert("includeSoldOut".into(), json!(v));
    }
    if let Some(v) = params.good_delivery {
        b.insert("goodDelivery".into(), json!(v));
    }
    if let Some(v) = params.asutsuku {
        b.insert("asutsuku".into(), json!(v));
    }
    if let Some(v) = params.furusato {
        b.insert("furusato".into(), json!(v));
    }
    if let Some(v) = params.has_coupon {
        b.insert("hasCoupon".into(), json!(v));
    }
    if let Some(v) = params.has_premium_price {
        b.insert("hasPremiumPrice".into(), json!(v));
    }
    if let Some(v) = params.is_verified {
        b.insert("isVerified".into(), json!(v));
    }
    if !params.shipping_service_list.is_empty() {
        b.insert(
            "shippingServiceList".into(),
            json!(params.shipping_service_list),
        );
    }
    if let Some(v) = params.conditional_free_shipping_price_to {
        b.insert("conditionalFreeShippingPriceTo".into(), json!(v));
    }
    if !params.payment_list.is_empty() {
        b.insert("paymentList".into(), json!(params.payment_list));
    }
    if !params.kspec_id_list.is_empty() {
        b.insert("kspecIdList".into(), json!(params.kspec_id_list));
    }
    if !params.sp_codes.is_empty() {
        b.insert("spCodes".into(), json!(params.sp_codes));
    }
    if !params.municipality_code.is_empty() {
        b.insert("municipalityCode".into(), json!(params.municipality_code));
    }
    if !params.seller_event_group_id.is_empty() {
        b.insert(
            "sellerEventGroupId".into(),
            json!(params.seller_event_group_id),
        );
    }
    if !params.campaign_tab_id.is_empty() {
        b.insert("campaignTabId".into(), json!(params.campaign_tab_id));
    }
    Value::Object(b)
}

/// Parse a search response body. Pure; unit-tested against the
/// captured live response.
pub fn parse_search_response(body: &Value) -> Vec<Item> {
    let results = body
        .pointer("/result/searchResult/results")
        .and_then(Value::as_array);
    let mut items = Vec::new();
    if let Some(results) = results {
        for it in results {
            let item_id = it["itemId"].as_str().unwrap_or_default().to_string();
            let image_id = it["imageId"].as_str().unwrap_or_default();
            items.push(Item {
                product_id: item_id.clone(),
                item_id: Some(item_id),
                title: it["name"].as_str().unwrap_or_default().to_string(),
                url: it["url"].as_str().unwrap_or_default().to_string(),
                source: "yahoo_shopping".into(),
                price: it
                    .pointer("/price/displayPrice")
                    .and_then(Value::as_i64)
                    .map(Price::yen),
                availability: match it["inStock"].as_bool() {
                    Some(true) => Some("in_stock".into()),
                    Some(false) => Some("sold_out".into()),
                    None => None,
                },
                image: if image_id.is_empty() {
                    None
                } else {
                    Some(format!("{IMAGE_BASE}{image_id}"))
                },
                seller: Some(SellerInfo {
                    id: it
                        .pointer("/store/id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    name: it
                        .pointer("/store/name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    url: it
                        .pointer("/store/url")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                }),
                review_score: it.pointer("/review/rating").and_then(Value::as_f64),
                review_count: it.pointer("/review/rateCount").and_then(Value::as_i64),
                ..Default::default()
            });
        }
    }
    items
}

// ---------------------------------------------------------------------------
// Detail (GET /item-detail/app/v2)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct DetailParams {
    /// `{storeId}_{itemCode}` — the search result's `itemId`.
    pub item_id: String,
    pub prefectures: String,
    pub sku_id: String,
    pub donation_id: String,
}

/// Build the detail query parameters (without appid /
/// android_version, which the client adds). Pure; unit-tested.
pub fn build_detail_query(params: &DetailParams) -> Vec<(String, String)> {
    let mut q = vec![
        ("itemId".to_string(), params.item_id.clone()),
        ("isSoftBankCarrier".to_string(), "false".to_string()),
        ("giftCard".to_string(), "true".to_string()),
        ("isTablet".to_string(), "false".to_string()),
        ("usePtahSsr".to_string(), "true".to_string()),
    ];
    if !params.prefectures.is_empty() {
        q.push(("prefectures".to_string(), params.prefectures.clone()));
    }
    if !params.sku_id.is_empty() {
        q.push(("skuId".to_string(), params.sku_id.clone()));
    }
    if !params.donation_id.is_empty() {
        q.push(("donationId".to_string(), params.donation_id.clone()));
    }
    q
}

/// Parse a detail response body. Pure; unit-tested against the
/// captured live response.
pub fn parse_detail_response(body: &Value) -> ItemResult {
    let item = body.pointer("/result/item");
    let Some(item) = item.filter(|i| i.is_object()) else {
        return ItemResult {
            provider: "yahoo_shopping".into(),
            ok: false,
            item: None,
            error: Some("detail response carried no result.item".into()),
        };
    };
    let ysr_id = item["ysrId"].as_str().unwrap_or_default().to_string();
    let price = item
        .pointer("/price/applicablePrice")
        .and_then(Value::as_i64)
        .or_else(|| item.pointer("/price/price1/price").and_then(Value::as_i64))
        .or_else(|| item.pointer("/price/regularPrice").and_then(Value::as_i64));
    let mut image_ids: Vec<String> = Vec::new();
    if let Some(id) = item.pointer("/images/mainImage/id").and_then(Value::as_str) {
        image_ids.push(id.to_string());
    }
    if let Some(list) = item
        .pointer("/images/detailImageList")
        .and_then(Value::as_array)
    {
        for e in list {
            if let Some(id) = e["id"].as_str() {
                image_ids.push(id.to_string());
            }
        }
    }
    let seller = body
        .pointer("/result/seller")
        .cloned()
        .unwrap_or(Value::Null);
    ItemResult {
        provider: "yahoo_shopping".into(),
        ok: true,
        item: Some(Item {
            product_id: ysr_id.clone(),
            item_id: item["srId"].as_str().map(str::to_string),
            title: item["name"].as_str().unwrap_or_default().to_string(),
            url: item["url"].as_str().unwrap_or_default().to_string(),
            source: "yahoo_shopping".into(),
            price: price.map(Price::yen),
            availability: match item.pointer("/stock/isAvailable").and_then(Value::as_bool) {
                Some(true) => Some("in_stock".into()),
                Some(false) => Some("sold_out".into()),
                None => None,
            },
            description: item["caption"].as_str().map(str::to_string),
            image: if ysr_id.is_empty() {
                None
            } else {
                Some(format!("{IMAGE_BASE}{ysr_id}"))
            },
            image_ids: if image_ids.is_empty() {
                None
            } else {
                Some(image_ids)
            },
            seller: Some(SellerInfo {
                id: seller["sellerId"].as_str().map(str::to_string),
                name: seller["name"].as_str().map(str::to_string),
                url: None,
            }),
            review_score: item
                .pointer("/review/reviewSummary/average")
                .and_then(Value::as_f64),
            review_count: item
                .pointer("/review/reviewSummary/count")
                .and_then(Value::as_i64),
        }),
        error: None,
    }
}

/// Extract the `{storeId}_{itemCode}` id from a product URL or
/// pass an id through.
pub fn extract_item_id(product: &str) -> String {
    let s = product.trim();
    if s.contains("store.shopping.yahoo.co.jp") {
        // https://store.shopping.yahoo.co.jp/<store>/<code>.html
        // addresses the item as "{store}_{code}".
        let tail = s.trim_end_matches(".html").trim_end_matches('/');
        let mut parts = tail.rsplit('/');
        let code = parts.next().unwrap_or_default();
        let store = parts.next().unwrap_or_default();
        if !store.is_empty() && !code.is_empty() {
            return format!("{store}_{code}");
        }
        return code.to_string();
    }
    s.to_string()
}

// ---------------------------------------------------------------------------
// Reviews (the app's WebView review page)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ReviewParams {
    /// `{storeId}_{itemCode}` — the search result's `itemId`.
    pub item_id: String,
    /// Cap on returned reviews. The page itself carries 20.
    pub limit: i32,
}

impl Default for ReviewParams {
    fn default() -> Self {
        Self {
            item_id: String::new(),
            limit: 20,
        }
    }
}

/// Build the review page URL from a `{storeId}_{itemCode}` id
/// (split at the first `_`, the same split the app applies to
/// ysrId). Returns `None` when either side is empty. Pure;
/// unit-tested.
pub fn build_review_url(item_id: &str) -> Option<String> {
    let (store_id, page_key) = item_id.split_once('_')?;
    if store_id.is_empty() || page_key.is_empty() {
        return None;
    }
    Some(format!(
        "{REVIEW_PAGE_BASE}?store_id={store_id}&page_key={page_key}"
    ))
}

/// Format an epoch-milliseconds timestamp as a `YYYY-MM-DD` date
/// in JST (UTC+9, no DST). Pure; unit-tested.
fn jst_date(epoch_ms: i64) -> String {
    let days = (epoch_ms + 9 * 3600 * 1000).div_euclid(86_400_000);
    // Civil-from-days algorithm (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Extract the JSON of the page's `<script id="__NEXT_DATA__">`
/// block. The review page is server-side rendered, so the data
/// is present in the static HTML. Pure; unit-tested.
pub fn extract_next_data(html: &str) -> Option<Value> {
    let marker = html.find("id=\"__NEXT_DATA__\"")?;
    let start = html[marker..].find('>')? + marker + 1;
    let end = html[start..].find("</script>")? + start;
    serde_json::from_str(&html[start..end]).ok()
}

fn normalize_review(r: &Value) -> Value {
    let posted_time = r["postedTime"].as_i64().unwrap_or(0);
    let variant: Vec<String> = r["subCodeOptions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|o| o["value"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let aspect_ratings: Vec<Value> = r["subReviews"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|s| {
                    json!({
                        "name": s["name"].as_str().unwrap_or_default(),
                        "value": s["valueName"].as_str().unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let images: Vec<String> = r["images"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|i| i["url"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "id": r["id"].as_str().unwrap_or_default(),
        "author": r["maskYid"].as_str().unwrap_or_default(),
        "rating": r["rating"].as_i64().unwrap_or(0),
        "title": r["title"].as_str().unwrap_or_default(),
        "body": r["body"].as_str().unwrap_or_default(),
        "posted_time": posted_time,
        "posted_date": jst_date(posted_time),
        "helpful_count": r["referenceCount"].as_i64().unwrap_or(0),
        "edited": r["edited"].as_bool().unwrap_or(false),
        "variant": variant,
        "aspect_ratings": aspect_ratings,
        "images": images,
        "store_reply": r["storeReply"].clone(),
    })
}

/// Parse a review page's HTML into the tool's result JSON. Pure;
/// unit-tested against a captured live page.
pub fn parse_reviews_page(html: &str, item_id: &str, limit: i32) -> Value {
    let url = build_review_url(item_id).unwrap_or_default();
    let fail = |error: &str| {
        json!({
            "provider": "yahoo_shopping",
            "ok": false,
            "item_id": item_id,
            "url": url,
            "error": error,
        })
    };
    let Some(data) = extract_next_data(html) else {
        return fail("review page carried no __NEXT_DATA__ block");
    };
    let item_review = data.pointer("/props/pageProps/reviewEntity/itemReview");
    let Some(item_review) = item_review.filter(|v| v.is_object()) else {
        return fail("review page carried no reviewEntity.itemReview");
    };
    let reviews: Vec<Value> = item_review["reviews"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(limit.clamp(1, 100) as usize)
                .map(normalize_review)
                .collect()
        })
        .unwrap_or_default();
    let summary = &item_review["reviewSummary"];
    json!({
        "provider": "yahoo_shopping",
        "ok": true,
        "item_id": item_id,
        "url": url,
        "total": summary["count"].as_i64(),
        "average_rating": summary["average"].as_f64(),
        "rating_counts": {
            "1": summary["one"].as_i64().unwrap_or(0),
            "2": summary["two"].as_i64().unwrap_or(0),
            "3": summary["three"].as_i64().unwrap_or(0),
            "4": summary["four"].as_i64().unwrap_or(0),
            "5": summary["five"].as_i64().unwrap_or(0),
        },
        "reviews": reviews,
        "count": reviews.len(),
        "next_offset": item_review["nextOffset"].as_i64(),
    })
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct YahooShoppingClient {
    client: Client,
}

impl YahooShoppingClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent(USER_AGENT)
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub async fn search_products(&self, params: SearchParams) -> Result<SearchResult> {
        let body = build_search_body(&params);
        let resp = self
            .client
            .post(format!("{BASE}/searchList/v4/items"))
            .query(&[("appid", APPID), ("android_version", APP_VERSION)])
            .header("X-Referer", "shopping_search")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow!("yahoo shopping search returned HTTP {status}"));
        }
        let parsed: Value = resp.json().await?;
        let items = parse_search_response(&parsed);
        Ok(SearchResult {
            provider: "yahoo_shopping".into(),
            ok: true,
            query: params.query.clone(),
            page: params.start_index,
            limit: params.limit,
            sort: params.order.clone(),
            count: items.len(),
            items,
            total_available: None,
        })
    }

    pub async fn get_product(&self, params: DetailParams) -> Result<ItemResult> {
        let mut query = vec![
            ("appid".to_string(), APPID.to_string()),
            ("android_version".to_string(), APP_VERSION.to_string()),
        ];
        query.extend(build_detail_query(&params));
        let resp = self
            .client
            .get(format!("{BASE}/item-detail/app/v2"))
            .query(&query)
            .header("X-Referer", "item")
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow!("yahoo shopping detail returned HTTP {status}"));
        }
        let parsed: Value = resp.json().await?;
        Ok(parse_detail_response(&parsed))
    }

    /// Fetch item reviews from the review page the app opens in
    /// its WebView (the native API has no review-list endpoint).
    pub async fn get_reviews(&self, params: ReviewParams) -> Result<Value> {
        let item_id = extract_item_id(&params.item_id);
        let url = build_review_url(&item_id)
            .ok_or_else(|| anyhow!("invalid item id for reviews: {item_id}"))?;
        let resp = self
            .client
            .get(&url)
            .header("User-Agent", WEBVIEW_USER_AGENT)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow!("yahoo shopping review page returned HTTP {status}"));
        }
        let html = resp.text().await?;
        Ok(parse_reviews_page(&html, &item_id, params.limit))
    }
}

impl Default for YahooShoppingClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let raw = match name {
            "search" => include_str!("../tests/fixtures/search.json"),
            "detail" => include_str!("../tests/fixtures/detail.json"),
            _ => unreachable!(),
        };
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn search_body_core_keys() {
        let body = build_search_body(&SearchParams {
            query: "コーヒー豆".into(),
            order: "Price".into(),
            asc: true,
            price_from: Some(1000),
            price_to: Some(5000),
            ..Default::default()
        });
        assert_eq!(body["referer"], json!("shopping_search"));
        assert_eq!(body["mallSellerType"], json!("All"));
        assert_eq!(body["mallDisplayType"], json!("Shopping"));
        assert_eq!(body["query"], json!("コーヒー豆"));
        assert_eq!(body["results"], json!(48));
        assert_eq!(body["startIndex"], json!(1));
        assert_eq!(body["order"], json!("Price"));
        assert_eq!(body["asc"], json!(true));
        assert_eq!(body["priceFrom"], json!(1000));
        assert_eq!(body["priceTo"], json!(5000));
        assert_eq!(body["deliveryArea"], json!("13"));
        assert_eq!(body["stampRallyRankCode"], json!("2000"));
        assert_eq!(
            body["moduleList"],
            json!([
                "GenreCategories",
                "Brand",
                "Municipalities",
                "SiblingGenreCategories",
                "SiblingSpecs"
            ])
        );
        assert!(body.get("janCode").is_none());
    }

    #[test]
    fn parse_search_fixture() {
        let items = parse_search_response(&fixture("search"));
        assert_eq!(items.len(), 48);
        let first = &items[0];
        assert_eq!(first.product_id, "dripcoffee_house-500-4");
        assert_eq!(first.price.as_ref().unwrap().value, 3980);
        assert_eq!(
            first.image.as_deref(),
            Some("https://item-shopping.c.yimg.jp/i/k/dripcoffee_house-500-4")
        );
        assert_eq!(
            first.seller.as_ref().unwrap().name.as_deref(),
            Some("DripCoffeeFactory")
        );
        assert_eq!(first.review_count, Some(584));
        assert_eq!(first.review_score, Some(4.5));
    }

    #[test]
    fn detail_query_shape() {
        let q = build_detail_query(&DetailParams {
            item_id: "dripcoffee_house-500-4".into(),
            sku_id: "sku1".into(),
            ..Default::default()
        });
        let map: std::collections::HashMap<_, _> = q.into_iter().collect();
        assert_eq!(map["itemId"], "dripcoffee_house-500-4");
        assert_eq!(map["isSoftBankCarrier"], "false");
        assert_eq!(map["giftCard"], "true");
        assert_eq!(map["isTablet"], "false");
        assert_eq!(map["usePtahSsr"], "true");
        assert_eq!(map["skuId"], "sku1");
        assert!(!map.contains_key("prefectures"));
    }

    #[test]
    fn parse_detail_fixture() {
        let result = parse_detail_response(&fixture("detail"));
        assert!(result.ok, "{result:?}");
        let item = result.item.unwrap();
        assert_eq!(item.product_id, "dripcoffee_house-500-4");
        assert_eq!(item.item_id.as_deref(), Some("house-500-4"));
        assert_eq!(item.price.as_ref().unwrap().value, 3980);
        assert_eq!(item.review_count, Some(584));
        assert_eq!(item.review_score, Some(4.5));
        assert_eq!(
            item.seller.as_ref().unwrap().name.as_deref(),
            Some("DripCoffeeFactory")
        );
        assert!(item.image_ids.as_ref().unwrap().len() >= 3);
    }

    #[test]
    fn extract_item_id_from_url() {
        assert_eq!(
            extract_item_id("https://store.shopping.yahoo.co.jp/dripcoffee/house-500-4.html"),
            "dripcoffee_house-500-4"
        );
        assert_eq!(
            extract_item_id("dripcoffee_house-500-4"),
            "dripcoffee_house-500-4"
        );
    }

    #[test]
    fn review_url_shape() {
        assert_eq!(
            build_review_url("dripcoffee_rrich-500-4").as_deref(),
            Some(
                "https://shopping.yahoo.co.jp/review/item/list?store_id=dripcoffee&page_key=rrich-500-4"
            )
        );
        // The page key itself may contain further underscores and
        // hyphens; only the first `_` separates the store.
        assert_eq!(
            build_review_url("store_a_b-c").as_deref(),
            Some("https://shopping.yahoo.co.jp/review/item/list?store_id=store&page_key=a_b-c")
        );
        assert_eq!(build_review_url("noseparator"), None);
        assert_eq!(build_review_url("_code"), None);
        assert_eq!(build_review_url("store_"), None);
    }

    #[test]
    fn jst_date_shape() {
        assert_eq!(jst_date(1_763_360_207_730), "2025-11-17");
        assert_eq!(jst_date(1_718_369_771_054), "2024-06-14");
        assert_eq!(jst_date(0), "1970-01-01");
    }

    #[test]
    fn parse_reviews_fixture() {
        let html = include_str!("../tests/fixtures/reviews.html");
        let out = parse_reviews_page(html, "dripcoffee_rrich-500-4", 20);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["total"], json!(7394));
        assert_eq!(out["average_rating"], json!(4.5));
        assert_eq!(out["rating_counts"]["5"], json!(4653));
        assert_eq!(out["rating_counts"]["1"], json!(58));
        assert_eq!(out["next_offset"], json!(20));
        assert_eq!(out["count"], json!(20));
        let reviews = out["reviews"].as_array().unwrap();
        let first = &reviews[0];
        assert_eq!(first["author"], json!("meg********"));
        assert_eq!(first["rating"], json!(5));
        assert_eq!(first["title"], json!("少し苦みのあるブラジル系で美味しい"));
        assert!(first["body"].as_str().unwrap().contains("酸味が低い"));
        assert_eq!(first["posted_date"], json!("2025-11-17"));
        assert_eq!(first["helpful_count"], json!(27));
        assert_eq!(first["edited"], json!(true));
        assert_eq!(first["variant"][1], json!("【4571621610115】豆のまま"));
        assert_eq!(first["aspect_ratings"][0]["name"], json!("香り"));
        assert_eq!(first["aspect_ratings"][0]["value"], json!("非常に良い"));
        assert!(first["images"][0]
            .as_str()
            .unwrap()
            .starts_with("https://review-shopping.c.yimg.jp/"));
    }

    #[test]
    fn parse_reviews_limit_and_missing_data() {
        let html = include_str!("../tests/fixtures/reviews.html");
        let out = parse_reviews_page(html, "dripcoffee_rrich-500-4", 3);
        assert_eq!(out["count"], json!(3));
        assert_eq!(out["reviews"].as_array().unwrap().len(), 3);

        let out = parse_reviews_page("<html><body>no data</body></html>", "a_b", 20);
        assert_eq!(out["ok"], json!(false));
        assert!(out["error"].as_str().unwrap().contains("__NEXT_DATA__"));
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_reviews() {
        let client = YahooShoppingClient::new();
        let out = client
            .get_reviews(ReviewParams {
                item_id: "dripcoffee_rrich-500-4".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out["total"].as_i64().unwrap() > 0);
        assert!(!out["reviews"].as_array().unwrap().is_empty());
        assert!(!out["reviews"][0]["body"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search() {
        let client = YahooShoppingClient::new();
        let result = client
            .search_products(SearchParams {
                query: "コーヒー豆".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok);
        assert!(!result.items.is_empty());
        assert!(result.items[0].price.is_some());
    }
}
