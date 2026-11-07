//! Rakuten Ichiba (楽天市場) provider.
//!
//! Reproduces the official Android app's guest BFF traffic
//! (`app-ichiba-gateway.rakuten.co.jp/mobile-bff/`, all POST + JSON).
//! No login, token or device registration is involved: the app
//! itself routes guests to this host with only the fixed
//! `x-clientid: ichiba_android_long` header.
//!
//! Search uses `dynamic/search/v1` (the search-results screen API,
//! the only route that carries sort / filter parameters). Product
//! detail uses `itemscreen/get/v16`, reviews use `review/list/v3`.
//! The detail endpoint answers HTTP 207 (BFF partial success) when
//! member-only features fail; that is a normal response shape and
//! success is judged by the core features carrying data.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const BASE: &str = "https://app-ichiba-gateway.rakuten.co.jp/mobile-bff/";
const CLIENT_ID: &str = "ichiba_android_long";
const USER_AGENT: &str = "okhttp/4.12.0";

/// Sort values for `rakuten_search_products` (the `allValue` forms
/// the app sends for a normal guest search).
pub const SORTS: &[&str] = &[
    "relevancy",
    "price_low_to_high_all",
    "price_high_to_low_all",
    "newest",
    "review_count_all",
    "review_score_all",
    "price_plus_shippingfee_low_to_high_all",
    "price_plus_shippingfee_high_to_low_all",
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
    /// `"{shop_id}_{item_id}"` — pass the parts to
    /// `rakuten_get_product` / `rakuten_get_reviews`.
    pub product_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shop_id: Option<i64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller: Option<SellerInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipping_price: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stock_quantity: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipping_methods: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub provider: String,
    pub ok: bool,
    pub query: String,
    pub offset: i32,
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
// Search (dynamic/search/v1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub query: String,
    /// One of [`SORTS`]; empty means the app default (`relevancy`).
    pub sort: String,
    pub price_min: Option<i64>,
    pub price_max: Option<i64>,
    /// Genre id (`category` in the app's parameter map).
    pub category: String,
    pub offset: i32,
    /// Flag filters, comma-joined the way the app sends them
    /// (fixed values: `free_shipping`, `out_of_stock`, `super_deal`,
    /// `subscription`, `group_distribution`; other values are the
    /// server-distributed CommonFilter option values).
    pub filters: String,
    pub gift: String,
    /// `new` or `used`.
    pub condition: String,
    /// Review score lower bound (3.0 / 3.5 / 4.0 / 4.5).
    pub review_min: Option<f64>,
    pub excluded_keyword: String,
    pub brand_tags: String,
    pub shop_id: String,
    pub shop_code: String,
    /// Prefecture id; 48 (overseas) pairs with `country`.
    pub region: Option<i64>,
    pub country: String,
    pub postal_code: String,
    pub tags: String,
    pub tag_groups: String,
    pub event: Option<i64>,
    pub ran: String,
    pub inshop_pure_lexical_search: Option<bool>,
    pub image_search_items: String,
    pub similar_image_search_item: String,
    /// Response modules; empty means the measured default set.
    pub modules: String,
    pub ab: String,
    /// Client-side cap on returned items (page size is the
    /// server's, measured 45).
    pub limit: i32,
}

/// The gateway wraps every response in `{body, status}`; the
/// parsers work on the inner body. Pure; unit-tested.
fn unwrap_body(parsed: Value) -> Value {
    match parsed.get("body") {
        Some(body) if body.is_object() => body.clone(),
        _ => parsed,
    }
}

fn envelope(feature: &str, feature_params: Value) -> Value {
    envelope_multi(&[(feature, feature_params)], json!({}))
}

fn envelope_multi(features: &[(&str, Value)], common_params: Value) -> Value {
    let mut feats = Map::new();
    for (name, params) in features {
        feats.insert(
            name.to_string(),
            json!({ "params": params, "include": [], "exclude": [] }),
        );
    }
    json!({
        "common": { "params": common_params, "include": [], "exclude": [] },
        "features": Value::Object(feats),
    })
}

/// Build the `dynamic/search/v1` request body. Pure; unit-tested.
pub fn build_search_body(params: &SearchParams) -> Value {
    let mut p = Map::new();
    p.insert("config".into(), json!("ichiba_search_android"));
    p.insert("lang".into(), json!("ja"));
    p.insert("v".into(), json!(4));
    p.insert("query".into(), json!(params.query));
    let sort = if params.sort.is_empty() {
        "relevancy"
    } else {
        params.sort.as_str()
    };
    p.insert("sort".into(), json!(sort));
    // Price band: sent only when set. Sending -1 for "unset"
    // makes the live server answer numFound=0 (measured
    // 2026-11-07; round2's static note said the app sends -1,
    // but the server's measured behaviour wins).
    if let Some(v) = params.price_min {
        p.insert("priceMin".into(), json!(v));
    }
    if let Some(v) = params.price_max {
        p.insert("priceMax".into(), json!(v));
    }
    p.insert("offset".into(), json!(params.offset.max(0)));
    let modules = if params.modules.is_empty() {
        "ichiba_search,dynamic_filter,facet_conditions"
    } else {
        params.modules.as_str()
    };
    p.insert("modules".into(), json!(modules));
    if !params.category.is_empty() {
        p.insert("category".into(), json!(params.category));
    }
    if !params.filters.is_empty() {
        p.insert("filters".into(), json!(params.filters));
    }
    if !params.gift.is_empty() {
        p.insert("gift".into(), json!(params.gift));
    }
    if !params.condition.is_empty() {
        p.insert("condition".into(), json!(params.condition));
    }
    if let Some(v) = params.review_min {
        p.insert("review".into(), json!(v));
    }
    if !params.excluded_keyword.is_empty() {
        p.insert("excludedKeyword".into(), json!(params.excluded_keyword));
    }
    if !params.brand_tags.is_empty() {
        p.insert("brandTags".into(), json!(params.brand_tags));
    }
    if !params.shop_id.is_empty() {
        p.insert("shopId".into(), json!(params.shop_id));
    }
    if !params.shop_code.is_empty() {
        p.insert("shopCode".into(), json!(params.shop_code));
    }
    if let Some(v) = params.region {
        p.insert("region".into(), json!(v));
    }
    if !params.country.is_empty() {
        p.insert("country".into(), json!(params.country));
    }
    if !params.postal_code.is_empty() {
        p.insert("postalCode".into(), json!(params.postal_code));
    }
    if !params.tags.is_empty() {
        p.insert("tags".into(), json!(params.tags));
    }
    if !params.tag_groups.is_empty() {
        p.insert("tagGroups".into(), json!(params.tag_groups));
    }
    if let Some(v) = params.event {
        p.insert("event".into(), json!(v));
    }
    if !params.ran.is_empty() {
        p.insert("ran".into(), json!(params.ran));
    }
    if let Some(v) = params.inshop_pure_lexical_search {
        p.insert("inshopPureLexicalSearch".into(), json!(v));
    }
    if !params.image_search_items.is_empty() {
        p.insert("imageSearchItems".into(), json!(params.image_search_items));
    }
    if !params.similar_image_search_item.is_empty() {
        p.insert(
            "similarImageSearchItem".into(),
            json!(params.similar_image_search_item),
        );
    }
    if !params.ab.is_empty() {
        p.insert("ab".into(), json!(params.ab));
    }
    envelope("dynamicSearchInfo", Value::Object(p))
}

fn image_url(img: &Value) -> Option<String> {
    if let Some(s) = img.as_str() {
        return Some(s.to_string());
    }
    if let Some(u) = img.get("url").and_then(Value::as_str) {
        if !u.contains("${") {
            return Some(u.to_string());
        }
    }
    img.get("resizableUrl")
        .and_then(Value::as_str)
        .map(|u| u.replace("${width}x${height}", "400x400"))
}

/// Parse a `dynamic/search/v1` response body into items + total.
/// Pure; unit-tested against the captured live response.
pub fn parse_search_response(body: &Value) -> (Vec<Item>, Option<i64>) {
    let modules = body
        .pointer("/dynamicSearchInfo/data/modules")
        .and_then(Value::as_array);
    let mut items = Vec::new();
    let mut total = None;
    if let Some(modules) = modules {
        for module in modules {
            if module.get("id").and_then(Value::as_str) != Some("ichiba_search") {
                continue;
            }
            let data = &module["data"];
            total = data.pointer("/pagination/numFound").and_then(Value::as_i64);
            if let Some(arr) = data.get("items").and_then(Value::as_array) {
                for it in arr {
                    items.push(parse_search_item(it));
                }
            }
        }
    }
    (items, total)
}

fn parse_search_item(it: &Value) -> Item {
    let shop = &it["shop"];
    let shop_id = shop.get("id").and_then(Value::as_i64);
    let code = it
        .get("code")
        .and_then(|c| {
            c.as_i64()
                .map(|n| n.to_string())
                .or_else(|| c.as_str().map(str::to_string))
        })
        .unwrap_or_default();
    let images: Vec<String> = it
        .get("images")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(image_url).collect())
        .unwrap_or_default();
    let price = it
        .get("price")
        .and_then(|p| p.as_i64().or_else(|| p.as_f64().map(|f| f.round() as i64)));
    Item {
        product_id: match shop_id {
            Some(sid) => format!("{sid}_{code}"),
            None => code.clone(),
        },
        item_id: Some(code),
        shop_id,
        title: it["name"].as_str().unwrap_or_default().to_string(),
        url: it["url"].as_str().unwrap_or_default().to_string(),
        source: "rakuten".into(),
        price: price.map(Price::yen),
        availability: match it["isSoldOut"].as_bool() {
            Some(true) => Some("sold_out".into()),
            Some(false) => Some("in_stock".into()),
            None => None,
        },
        image: images.first().cloned(),
        images: if images.is_empty() {
            None
        } else {
            Some(images)
        },
        seller: Some(SellerInfo {
            id: shop_id.map(|s| s.to_string()),
            name: shop["name"].as_str().map(str::to_string),
            url: shop["url"].as_str().map(str::to_string),
        }),
        review_score: it.pointer("/review/score").and_then(Value::as_f64),
        review_count: it.pointer("/review/numReviews").and_then(Value::as_i64),
        shipping_price: it.pointer("/shipping/price").and_then(Value::as_i64),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Product detail (itemscreen/get/v16)
// ---------------------------------------------------------------------------

/// How to address one product. The app resolves the first form
/// that is fully specified, in this order: shop_id + item_id,
/// item_code, shop_id + manage_number, item_url.
#[derive(Debug, Clone, Default)]
pub struct ProductRef {
    pub shop_id: Option<i64>,
    pub item_id: Option<i64>,
    /// `"shopCode:itemId"` form is split into shop + item by the app.
    pub item_code: Option<String>,
    pub manage_number: Option<String>,
    pub item_url: Option<String>,
    pub variant_id: Option<String>,
    pub postal_code: Option<String>,
    pub prefecture_id: Option<i64>,
}

/// Build the `itemscreen/get/v16` request body. Pure; unit-tested.
pub fn build_detail_body(pref_: &ProductRef) -> Result<Value> {
    let mut common = Map::new();
    if let (Some(shop), Some(item)) = (pref_.shop_id, pref_.item_id) {
        common.insert("shopId".into(), json!(shop));
        common.insert("itemId".into(), json!(item));
    } else if let Some(code) = &pref_.item_code {
        common.insert("itemCode".into(), json!(code));
    } else if let (Some(shop), Some(mn)) = (pref_.shop_id, &pref_.manage_number) {
        common.insert("shopId".into(), json!(shop));
        common.insert("manageNumber".into(), json!(mn));
    } else if let Some(url) = &pref_.item_url {
        common.insert("itemUrl".into(), json!(url));
    } else {
        return Err(anyhow!(
            "rakuten_get_product needs shop_id+item_id, item_code, shop_id+manage_number, or item_url"
        ));
    }
    if let Some(v) = &pref_.variant_id {
        common.insert("variantId".into(), json!(v));
    }
    common.insert("locale".into(), json!("ja-JP"));
    common.insert("deviceType".into(), json!("sp"));

    let mut item_info = Map::new();
    if let Some(pc) = &pref_.postal_code {
        item_info.insert("postalCode".into(), json!(pc));
    }
    if let Some(pf) = pref_.prefecture_id {
        item_info.insert("prefectureId".into(), json!(pf));
    }
    let mut shipping_info = Map::new();
    if let Some(pc) = &pref_.postal_code {
        shipping_info.insert("postalCode".into(), json!(pc));
    }
    if let Some(pf) = pref_.prefecture_id {
        shipping_info.insert("prefectureId".into(), json!(pf));
    }
    Ok(envelope_multi(
        &[
            ("itemInfo", Value::Object(item_info)),
            ("shopInfo", json!({ "hits": 20 })),
            ("shippingInfo", Value::Object(shipping_info)),
            ("disabledShopInfo", json!({ "isInflow": false })),
            ("topicsInfo", json!({ "hits": 4 })),
            ("consumptionTaxInfo", json!({})),
        ],
        Value::Object(common),
    ))
}

/// Parse an `itemscreen/get/v16` response body. Success is judged
/// by the core `itemInfo` feature carrying data (the endpoint's
/// HTTP 207 partial-success form fails member-only features only).
/// Pure; unit-tested against the captured live response.
pub fn parse_detail_response(body: &Value) -> ItemResult {
    let data = body.pointer("/itemInfo/data");
    let Some(data) = data.filter(|d| d.is_object()) else {
        return ItemResult {
            provider: "rakuten".into(),
            ok: false,
            item: None,
            error: Some("itemInfo feature returned no data".into()),
        };
    };
    let variant = data
        .get("variants")
        .and_then(Value::as_array)
        .and_then(|v| v.first())
        .cloned()
        .unwrap_or(Value::Null);
    let price = variant
        .get("taxIncludedStandardPrice")
        .and_then(|p| p.as_f64().map(|f| f.round() as i64).or_else(|| p.as_i64()))
        .or_else(|| {
            variant.get("standardPrice").and_then(|p| {
                p.as_i64()
                    .or_else(|| p.as_str().and_then(|s| s.parse().ok()))
            })
        });
    let images: Vec<String> = data
        .get("images")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|i| i.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let shop = body
        .pointer("/shopInfo/data")
        .cloned()
        .unwrap_or(Value::Null);
    let shipping_methods: Vec<String> = body
        .pointer("/shippingInfo/data/standard/deliveryInfo")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|d| d["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let item_id = data.get("itemId").and_then(Value::as_i64);
    let description = data["productDescription"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| data["salesDescription"].as_str().filter(|s| !s.is_empty()))
        .map(str::to_string);
    ItemResult {
        provider: "rakuten".into(),
        ok: true,
        item: Some(Item {
            product_id: item_id.map(|i| i.to_string()).unwrap_or_default(),
            item_id: item_id.map(|i| i.to_string()),
            title: data["title"].as_str().unwrap_or_default().to_string(),
            url: data["itemUrl"].as_str().unwrap_or_default().to_string(),
            source: "rakuten".into(),
            price: price.map(Price::yen),
            availability: data["availability"].as_i64().map(|a| {
                if a > 0 {
                    "in_stock".into()
                } else {
                    "sold_out".into()
                }
            }),
            description,
            image: images.first().cloned(),
            images: if images.is_empty() {
                None
            } else {
                Some(images)
            },
            seller: Some(SellerInfo {
                id: shop["shopCode"].as_str().map(str::to_string),
                name: shop["shopName"].as_str().map(str::to_string),
                url: shop["shopURL"].as_str().map(str::to_string),
            }),
            review_score: data.pointer("/review/average").and_then(Value::as_f64),
            review_count: data.pointer("/review/count").and_then(Value::as_i64),
            stock_quantity: variant.get("quantity").and_then(Value::as_i64),
            shipping_methods: if shipping_methods.is_empty() {
                None
            } else {
                Some(shipping_methods)
            },
            ..Default::default()
        }),
        error: None,
    }
}

// ---------------------------------------------------------------------------
// Reviews (review/list/v3)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ReviewParams {
    pub shop_id: i64,
    pub item_id: i64,
    pub page: i32,
    /// Page size for the review list (server cap 30).
    pub hits: i32,
    /// `-regtime` (newest) / `-evaluation` / `-judgment`.
    pub sort: String,
    pub check_display_condition: bool,
    /// 0 = male, 1 = female.
    pub sex: Option<i64>,
    pub min_evaluation: Option<i64>,
    pub max_evaluation: Option<i64>,
    pub min_age: Option<i64>,
    pub max_age: Option<i64>,
    /// 0 = all reviews, 1 = media-only.
    pub image_flag: i32,
}

impl Default for ReviewParams {
    fn default() -> Self {
        Self {
            shop_id: 0,
            item_id: 0,
            page: 1,
            hits: 10,
            sort: String::new(),
            check_display_condition: true,
            sex: None,
            min_evaluation: None,
            max_evaluation: None,
            min_age: None,
            max_age: None,
            image_flag: 0,
        }
    }
}

/// Build the `review/list/v3` request body. Pure; unit-tested.
pub fn build_reviews_body(params: &ReviewParams) -> Value {
    let mut common = Map::new();
    common.insert("shopId".into(), json!(params.shop_id));
    common.insert("itemId".into(), json!(params.item_id));
    if !params.sort.is_empty() {
        common.insert("sort".into(), json!(params.sort));
    }
    common.insert(
        "checkDisplayCondition".into(),
        json!(params.check_display_condition),
    );
    if let Some(v) = params.sex {
        common.insert("sex".into(), json!(v));
    }
    if let Some(v) = params.min_evaluation {
        common.insert("minEvaluation".into(), json!(v));
    }
    if let Some(v) = params.max_evaluation {
        common.insert("maxEvaluation".into(), json!(v));
    }
    if let Some(v) = params.min_age {
        common.insert("minAge".into(), json!(v));
    }
    if let Some(v) = params.max_age {
        common.insert("maxAge".into(), json!(v));
    }
    envelope_multi(
        &[
            (
                "reviewListInfo",
                json!({
                    "page": params.page.max(1),
                    "hits": params.hits.clamp(1, 30),
                    "imageFlag": params.image_flag,
                }),
            ),
            (
                "mediaReviewsInfo",
                json!({
                    "page": params.page.max(1),
                    "hits": params.hits.clamp(1, 20),
                    "imageFlag": 1,
                }),
            ),
            ("reviewAISummaryInfo", json!({})),
        ],
        Value::Object(common),
    )
}

fn normalize_review(r: &Value) -> Value {
    json!({
        "body": r["body"].as_str().unwrap_or_default(),
        "rating": r["userEvaluation"].as_str().map(str::to_string)
            .or_else(|| r["userEvaluation"].as_i64().map(|n| n.to_string())),
        "nickname": r["nickname"].as_str().unwrap_or_default(),
        "age": r["age"].as_str().unwrap_or_default(),
        "sex": r["sex"].as_str().unwrap_or_default(),
        "buy_flag": r["buyFlag"].as_i64(),
        "reg_time": r["regTime"].as_str().unwrap_or_default(),
        "judgement_ok_count": r["judgementOkCount"].as_i64().unwrap_or(0),
        "evaluation_details": r["evaluationDetails"],
    })
}

/// Parse a `review/list/v3` response body. Pure; unit-tested.
pub fn parse_reviews_response(body: &Value, shop_id: i64, item_id: i64) -> Value {
    let data = body.pointer("/reviewListInfo/data");
    let Some(data) = data.filter(|d| d.is_object()) else {
        return json!({
            "provider": "rakuten",
            "ok": false,
            "shop_id": shop_id,
            "item_id": item_id,
            "error": "reviewListInfo feature returned no data",
        });
    };
    let reviews: Vec<Value> = data
        .get("reviews")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(normalize_review).collect())
        .unwrap_or_default();
    let media: Vec<Value> = body
        .pointer("/mediaReviewsInfo/data/reviews")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(normalize_review).collect())
        .unwrap_or_default();
    let summary = &data["summary"];
    json!({
        "provider": "rakuten",
        "ok": true,
        "shop_id": shop_id,
        "item_id": item_id,
        "total": summary["total"].as_i64(),
        "page": summary["page"].as_i64(),
        "page_count": summary["pageCount"].as_i64(),
        "evaluation_totals": summary["evaluationTotals"],
        "reviews": reviews,
        "count": reviews.len(),
        "media_reviews": media,
        "media_total": body.pointer("/mediaReviewsInfo/data/summary/total").and_then(Value::as_i64),
    })
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct RakutenClient {
    client: Client,
}

impl RakutenClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent(USER_AGENT)
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    async fn post_bff(&self, path: &str, body: &Value) -> Result<Value> {
        let resp = self
            .client
            .post(format!("{BASE}{path}"))
            .header("x-clientid", CLIENT_ID)
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        // itemscreen / review answer 207 (partial success) as a
        // normal shape; treat it as success and let the parsers
        // judge by feature data.
        if !(status.is_success() || status.as_u16() == 207) {
            return Err(anyhow!("rakuten BFF {path} returned HTTP {status}"));
        }
        let parsed: Value = resp.json().await?;
        Ok(unwrap_body(parsed))
    }

    pub async fn search_products(&self, params: SearchParams) -> Result<SearchResult> {
        let body = build_search_body(&params);
        let resp = self.post_bff("dynamic/search/v1", &body).await?;
        let (mut items, total) = parse_search_response(&resp);
        let limit = if params.limit > 0 {
            params.limit as usize
        } else {
            items.len()
        };
        items.truncate(limit);
        Ok(SearchResult {
            provider: "rakuten".into(),
            ok: true,
            query: params.query.clone(),
            offset: params.offset,
            sort: if params.sort.is_empty() {
                "relevancy".into()
            } else {
                params.sort.clone()
            },
            count: items.len(),
            items,
            total_available: total,
        })
    }

    pub async fn get_product(&self, pref_: ProductRef) -> Result<ItemResult> {
        let body = build_detail_body(&pref_)?;
        let resp = self.post_bff("itemscreen/get/v16", &body).await?;
        Ok(parse_detail_response(&resp))
    }

    pub async fn get_reviews(&self, params: ReviewParams) -> Result<Value> {
        let body = build_reviews_body(&params);
        let resp = self.post_bff("review/list/v3", &body).await?;
        Ok(parse_reviews_response(
            &resp,
            params.shop_id,
            params.item_id,
        ))
    }
}

impl Default for RakutenClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let raw = match name {
            "dynamic" => include_str!("../tests/fixtures/r2-dynamic.json"),
            "detail" => include_str!("../tests/fixtures/r2-detail.json"),
            "review" => include_str!("../tests/fixtures/r2-review.json"),
            _ => unreachable!(),
        };
        let saved: Value = serde_json::from_str(raw).unwrap();
        saved["body"].clone()
    }

    #[test]
    fn unwrap_body_strips_gateway_wrapper() {
        // The fixtures are the raw gateway responses ({status,
        // body}); the client must parse the inner body.
        let raw = include_str!("../tests/fixtures/r2-dynamic.json");
        let saved: Value = serde_json::from_str(raw).unwrap();
        assert!(saved.get("dynamicSearchInfo").is_none());
        let body = unwrap_body(saved);
        let (items, total) = parse_search_response(&body);
        assert!(!items.is_empty());
        assert!(total.unwrap_or(0) > 0);
        // Unwrapped payloads pass through unchanged.
        assert_eq!(unwrap_body(json!({"a": 1})), json!({"a": 1}));
    }

    #[test]
    fn search_body_has_fixed_and_optional_params() {
        let body = build_search_body(&SearchParams {
            query: "コーヒー豆".into(),
            sort: "price_low_to_high_all".into(),
            price_min: Some(1000),
            price_max: Some(3000),
            condition: "new".into(),
            review_min: Some(4.0),
            ..Default::default()
        });
        let p = &body["features"]["dynamicSearchInfo"]["params"];
        assert_eq!(p["config"], json!("ichiba_search_android"));
        assert_eq!(p["lang"], json!("ja"));
        assert_eq!(p["v"], json!(4));
        assert_eq!(p["query"], json!("コーヒー豆"));
        assert_eq!(p["sort"], json!("price_low_to_high_all"));
        assert_eq!(p["priceMin"], json!(1000));
        assert_eq!(p["priceMax"], json!(3000));
        assert_eq!(p["condition"], json!("new"));
        assert_eq!(p["review"], json!(4.0));
        assert_eq!(
            p["modules"],
            json!("ichiba_search,dynamic_filter,facet_conditions")
        );
        assert!(p.get("category").is_none());
        assert_eq!(body["common"]["params"], json!({}));
    }

    #[test]
    fn search_body_omits_unset_price_band() {
        // Sending -1 for an unset band makes the live server
        // answer numFound=0 (measured 2026-11-07), so the keys
        // must be absent when unset.
        let body = build_search_body(&SearchParams {
            query: "x".into(),
            ..Default::default()
        });
        let p = &body["features"]["dynamicSearchInfo"]["params"];
        assert!(p.get("priceMin").is_none());
        assert!(p.get("priceMax").is_none());
        assert_eq!(p["sort"], json!("relevancy"));
        assert_eq!(p["offset"], json!(0));
    }

    #[test]
    fn parse_search_fixture() {
        let (items, total) = parse_search_response(&fixture("dynamic"));
        assert_eq!(total, Some(63486));
        assert_eq!(items.len(), 45);
        let first = &items[0];
        assert_eq!(first.shop_id, Some(355510));
        assert_eq!(first.item_id.as_deref(), Some("10000010"));
        assert_eq!(first.product_id, "355510_10000010");
        assert_eq!(first.price.as_ref().unwrap().value, 1000);
        assert_eq!(first.review_count, Some(11803));
        assert_eq!(first.review_score, Some(4.32));
        assert!(first.url.contains("item.rakuten.co.jp"));
        assert!(first.seller.as_ref().unwrap().name.is_some());
        assert!(first.image.is_some());
    }

    #[test]
    fn detail_body_prefers_shop_and_item() {
        let body = build_detail_body(&ProductRef {
            shop_id: Some(355510),
            item_id: Some(10000010),
            item_code: Some("ignored:1".into()),
            ..Default::default()
        })
        .unwrap();
        let c = &body["common"]["params"];
        assert_eq!(c["shopId"], json!(355510));
        assert_eq!(c["itemId"], json!(10000010));
        assert_eq!(c["locale"], json!("ja-JP"));
        assert_eq!(c["deviceType"], json!("sp"));
        assert!(body["features"].get("itemInfo").is_some());
        assert!(body["features"].get("shopInfo").is_some());
        assert!(body["features"].get("shippingInfo").is_some());
    }

    #[test]
    fn detail_body_falls_back_to_item_code() {
        let body = build_detail_body(&ProductRef {
            item_code: Some("kyowashokken:10000010".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            body["common"]["params"]["itemCode"],
            json!("kyowashokken:10000010")
        );
    }

    #[test]
    fn detail_body_requires_an_identifier() {
        assert!(build_detail_body(&ProductRef::default()).is_err());
    }

    #[test]
    fn parse_detail_fixture() {
        let result = parse_detail_response(&fixture("detail"));
        assert!(result.ok, "detail must parse as ok: {result:?}");
        let item = result.item.unwrap();
        assert!(item.title.contains("麹とコンブチャ"));
        assert_eq!(item.price.as_ref().unwrap().value, 1000);
        assert_eq!(item.review_count, Some(11803));
        assert_eq!(item.review_score, Some(4.32));
        assert_eq!(item.stock_quantity, Some(1415));
        assert_eq!(
            item.seller.as_ref().unwrap().name.as_deref(),
            Some("協和食研 楽天市場店")
        );
        assert!(item.images.as_ref().unwrap().len() >= 4);
        assert!(item.shipping_methods.as_ref().unwrap().len() >= 2);
    }

    #[test]
    fn reviews_body_shape() {
        let body = build_reviews_body(&ReviewParams {
            shop_id: 355510,
            item_id: 10000010,
            sort: "-regtime".into(),
            min_evaluation: Some(4),
            ..Default::default()
        });
        let c = &body["common"]["params"];
        assert_eq!(c["shopId"], json!(355510));
        assert_eq!(c["itemId"], json!(10000010));
        assert_eq!(c["sort"], json!("-regtime"));
        assert_eq!(c["checkDisplayCondition"], json!(true));
        assert_eq!(c["minEvaluation"], json!(4));
        assert!(c.get("sex").is_none());
        let f = &body["features"];
        assert_eq!(f["reviewListInfo"]["params"]["hits"], json!(10));
        assert_eq!(f["mediaReviewsInfo"]["params"]["imageFlag"], json!(1));
    }

    #[test]
    fn parse_reviews_fixture() {
        let out = parse_reviews_response(&fixture("review"), 355510, 10000010);
        assert_eq!(out["ok"], json!(true));
        assert_eq!(out["total"], json!(11803));
        assert_eq!(out["count"], json!(10));
        let reviews = out["reviews"].as_array().unwrap();
        assert_eq!(reviews[0]["rating"], json!("5"));
        assert!(reviews[0]["body"].as_str().unwrap().contains("飲んでます"));
        assert_eq!(out["media_total"], json!(63));
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search() {
        let client = RakutenClient::new();
        let result = client
            .search_products(SearchParams {
                query: "コーヒー豆".into(),
                sort: "price_low_to_high_all".into(),
                price_min: Some(1000),
                price_max: Some(3000),
                limit: 5,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok);
        assert!(!result.items.is_empty());
        for item in &result.items {
            assert!(item.price.as_ref().unwrap().value >= 1000);
            assert!(item.price.as_ref().unwrap().value <= 3000);
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_detail_and_reviews() {
        let client = RakutenClient::new();
        let detail = client
            .get_product(ProductRef {
                shop_id: Some(355510),
                item_id: Some(10000010),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(detail.ok, "{detail:?}");
        let reviews = client
            .get_reviews(ReviewParams {
                shop_id: 355510,
                item_id: 10000010,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(reviews["ok"], json!(true));
        assert!(reviews["total"].as_i64().unwrap() > 0);
    }
}
