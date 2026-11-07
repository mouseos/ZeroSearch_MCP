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
//! carry rating aggregates only) and the suggest endpoint's
//! parameters are not established, so neither is provided here.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const BASE: &str = "https://shopping-appliproxy.yahooapis.jp";
const APPID: &str = "dj00aiZpPWtHdjlEQk16eVA1ZCZzPWNvbnN1bWVyc2VjcmV0Jng9OGM-";
const APP_VERSION: &str = "16.12.0";
const USER_AGENT: &str = "okhttp/4.12.0";
const IMAGE_BASE: &str = "https://item-shopping.c.yimg.jp/i/k/";

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
