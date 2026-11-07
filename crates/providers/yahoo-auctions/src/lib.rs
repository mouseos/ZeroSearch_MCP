use anyhow::{Context, Result};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

/// Web base, used only for canonical item URLs and the seller rating page
/// (reviews have no app-API equivalent and stay on the Web route).
const WEB_BASE: &str = "https://auctions.yahoo.co.jp";
/// Official app API base (APK 8.48.1, interface `hm1`).
const API_BASE: &str = "https://auctions.yahooapis.jp/app/";
/// Public app identifier the app's `nd1` interceptor attaches to every API
/// request. Without it the gateway returns 401. It is not a user credential:
/// no Authorization header is sent for anonymous access.
const APPID: &str = "dj00aiZpPXVkeDRaNE5IY1B1OCZzPWNvbnN1bWVyc2VjcmV0Jng9ZmM-";
const APP_USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8 Build/PSR1.180720.093; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/126.0.0.0 Mobile Safari/537.36 YJApp-ANDROID jp.co.yahoo.android.yauction/8.48.1";
const WEB_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

static REVIEW_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"(?s)<!--\s*RATING INFO\s*-->"#,
        r#".*?<IMG[^>]*alt="([^"]*)"[^>]*/?>.*?"#,
        r#"(出品者です|落札者です)"#,
        r#".*?評価者：[^<]*?</b>\s*([^（(\s<]+)\s*[（(](\d+)[)）]"#,
        r#".*?<!--\s*ITEM INFO\s*-->"#,
        r#".*?/auction/([^"]*)"[^>]*>([^<]*)</A>"#,
        r#".*?<!--\s*COMMENT\s*-->"#,
        r#".*?<td[^>]*>：</td>\s*<td[^>]*><small>(.*?)(?:\(評価日時：([^)]*)\))?\s*</small>"#,
    ))
    .unwrap()
});

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Seller {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub product_id: String,
    pub auction_id: String,
    pub title: String,
    pub url: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy_now_price: Option<Price>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bid_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watchers: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_path: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_shipping: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller: Option<Seller>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipping_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub rating: String,
    pub role: String,
    pub message: String,
    pub reviewer_name: String,
    pub reviewer_score: i64,
    pub item_title: String,
    pub auction_id: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewsResult {
    pub provider: String,
    pub ok: bool,
    pub seller_id: String,
    pub reviews: Vec<Review>,
    pub count: usize,
    pub page: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub provider: String,
    pub ok: bool,
    pub query: String,
    pub page: i32,
    pub limit: i32,
    pub sort: String,
    pub status: String,
    pub items: Vec<Item>,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_available: Option<i64>,
    pub search_mode: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub keyword: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_path: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestResult {
    pub provider: String,
    pub ok: bool,
    pub query: String,
    pub suggestions: Vec<Suggestion>,
    pub count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub query: String,
    pub limit: i32,
    pub page: i32,
    pub sort: String,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub category_id: String,
    pub status: String,
    pub keywords_and: String,
    pub keywords_not: String,
    pub seller_id: String,
    pub buynow_only: bool,
    pub free_shipping: bool,
    pub condition: String,
}

/// Map Shopping_MCP sort vocabulary to the app API `sort` values
/// (SortOrder.Search in the APK). `newest` differs by status: open sorts by
/// first start time, closed by end time.
fn api_sort(s: &str, is_closed: bool) -> &'static str {
    match s.to_lowercase().as_str() {
        "price_asc" | "lowest_price" => "+PRICE",
        "price_desc" | "highest_price" => "-PRICE",
        "bids" => "-BID_COUNT",
        "end_time" | "ending_soonest" | "end" => "+END_TIME",
        "newest" | "new" => {
            if is_closed {
                "-END_TIME"
            } else {
                "-FIRST_START_TIME"
            }
        }
        _ => "-RANKING",
    }
}

/// Map Shopping_MCP condition vocabulary to the app API `conditions`
/// values (ItemCondition ids in the APK).
fn api_condition(s: &str) -> Option<&'static str> {
    match s.to_lowercase().as_str() {
        "new" | "新品" => Some("NEW"),
        "unused" | "未使用" => Some("USED10"),
        "nearly_new" | "目立った傷や汚れなし" => Some("USED20"),
        "used" | "中古" => Some("USED40"),
        "slightly_used" | "やや傷や汚れあり" => Some("USED40"),
        "used_fair" | "傷や汚れあり" => Some("USED60"),
        "junk" | "全体的に状態が悪い" => Some("USED80"),
        _ => None,
    }
}

/// The API returns the seller good-rating as a ratio (0.992); present it
/// the way the app/Web do ("99.2%").
fn format_good_ratio(ratio: f64) -> String {
    format!("{:.1}%", ratio * 100.0)
}

fn seller_is_store(seller_type: &str) -> Option<bool> {
    match seller_type {
        "STORE" => Some(true),
        "AUCTION" => Some(false),
        _ => None,
    }
}

pub struct YahooAuctionsClient {
    api_client: Client,
    web_client: Client,
}

impl YahooAuctionsClient {
    pub fn new() -> Self {
        let mut api_headers = HeaderMap::new();
        api_headers.insert("x-z-os", HeaderValue::from_static("android"));
        api_headers.insert("x-z-app-version", HeaderValue::from_static("8.48.1"));
        api_headers.insert("x-z-os-version", HeaderValue::from_static("34"));
        api_headers.insert("x-z-service", HeaderValue::from_static("auction"));
        Self {
            api_client: Client::builder()
                .user_agent(APP_USER_AGENT)
                .default_headers(api_headers)
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .expect("failed to build API HTTP client"),
            web_client: Client::builder()
                .user_agent(WEB_USER_AGENT)
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .expect("failed to build Web HTTP client"),
        }
    }

    /// GET an app-API path with the mandatory `appid` query attached.
    /// On HTTP errors, returns an error carrying the status and the head
    /// of the body (the API answers `{error:{code,message,name}}` or the
    /// gateway form `{Error:{Message}}`).
    async fn api_get(&self, path: &str, query: &[(String, String)]) -> Result<Value> {
        let url = format!("{API_BASE}{path}");
        let mut qp: Vec<(String, String)> = query.to_vec();
        qp.push(("appid".into(), APPID.into()));
        let resp = self.api_client.get(&url).query(&qp).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            let head: String = text.chars().take(300).collect();
            anyhow::bail!("yahoo app API HTTP {status} for {path}: {head}");
        }
        let value: Value =
            serde_json::from_str(&text).with_context(|| format!("invalid JSON from {path}"))?;
        if let Some(err) = value.get("error") {
            let msg = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown API error");
            let code = err.get("code").and_then(|v| v.as_str()).unwrap_or("");
            anyhow::bail!("yahoo app API error for {path}: {code} {msg}");
        }
        if let Some(err) = value.get("Error") {
            let msg = err
                .get("Message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown gateway error");
            anyhow::bail!("yahoo app API gateway error for {path}: {msg}");
        }
        Ok(value)
    }

    pub async fn search(&self, params: SearchParams) -> Result<SearchResult> {
        let is_closed = params.status == "closed";
        let limit = if params.limit > 0 { params.limit } else { 20 };
        let page = if params.page > 0 { params.page } else { 1 };

        // The API has no separate AND/NOT keyword fields: fold them into
        // the query string the way the app's search syntax does.
        let mut query = params.query.trim().to_string();
        if !params.keywords_and.trim().is_empty() {
            if !query.is_empty() {
                query.push(' ');
            }
            query.push_str(params.keywords_and.trim());
        }
        for word in params.keywords_not.split_whitespace() {
            if !query.is_empty() {
                query.push(' ');
            }
            query.push('-');
            query.push_str(word);
        }

        let mut qp: Vec<(String, String)> = Vec::new();
        if !query.is_empty() {
            qp.push(("query".into(), query.clone()));
        }
        qp.push(("offset".into(), ((page - 1) * limit).to_string()));
        qp.push(("limit".into(), limit.to_string()));
        qp.push(("sort".into(), api_sort(&params.sort, is_closed).into()));
        qp.push((
            "status".into(),
            if is_closed { "CLOSED" } else { "OPEN" }.into(),
        ));
        qp.push(("isEnableSpeller".into(), "true".into()));
        if !params.category_id.is_empty() {
            qp.push(("categoryIds".into(), params.category_id.clone()));
        }
        if !params.seller_id.is_empty() {
            qp.push(("aucUserIds".into(), params.seller_id.clone()));
        }
        if let Some(v) = params.min_price {
            qp.push(("minPrice".into(), v.to_string()));
        }
        if let Some(v) = params.max_price {
            qp.push(("maxPrice".into(), v.to_string()));
        }
        if params.free_shipping {
            qp.push(("isFreeShipping".into(), "true".into()));
        }
        if params.buynow_only {
            qp.push(("submitType".into(), "FIXED".into()));
        }
        if let Some(c) = api_condition(&params.condition) {
            qp.push(("conditions".into(), c.into()));
        }

        let resp = self.api_get("list/v2/search/items", &qp).await?;
        let (items, total) = parse_search_response(&resp);
        let count = items.len().min(limit as usize);
        Ok(SearchResult {
            provider: "yahoo_auctions".into(),
            ok: true,
            query: params.query.clone(),
            page,
            limit,
            sort: params.sort.clone(),
            status: if is_closed { "closed" } else { "open" }.into(),
            items: items.into_iter().take(limit as usize).collect(),
            count,
            total_available: total,
            search_mode: "appApi".into(),
        })
    }

    pub async fn get_item(&self, auction_id: &str) -> Result<ItemResult> {
        let path = format!("detail/v2/items/{auction_id}");
        let qp = vec![("collectHistory".into(), "false".into())];
        let resp = self.api_get(&path, &qp).await?;
        if resp["auctionId"].is_null() {
            return Ok(ItemResult {
                provider: "yahoo_auctions".into(),
                ok: false,
                item: None,
                error: Some("no_item_data".into()),
            });
        }
        Ok(ItemResult {
            provider: "yahoo_auctions".into(),
            ok: true,
            item: Some(normalize_detail_item(&resp)),
            error: None,
        })
    }

    pub async fn suggest_keywords(&self, query: &str, limit: i32) -> Result<SuggestResult> {
        let limit = if limit > 0 { limit } else { 10 };
        let qp = vec![
            ("query".into(), query.to_string()),
            ("limit".into(), limit.to_string()),
            ("categoryLimit".into(), "5".into()),
            ("brandLimit".into(), "5".into()),
        ];
        let resp = self.api_get("search/v2/suggest/queries", &qp).await?;
        let suggestions = parse_suggestions(&resp);
        let count = suggestions.len();
        Ok(SuggestResult {
            provider: "yahoo_auctions".into(),
            ok: true,
            query: query.to_string(),
            suggestions,
            count,
        })
    }

    /// Seller reviews have no app-API equivalent (only a rating summary
    /// exists there), so this stays on the Web rating page as before.
    pub async fn get_reviews(
        &self,
        seller_id: &str,
        page: i32,
        limit: i32,
    ) -> Result<ReviewsResult> {
        let page = if page > 0 { page } else { 1 };
        let limit = if limit > 0 { limit } else { 25 };

        let url = format!("{WEB_BASE}/jp/show/rating");
        let resp = self
            .web_client
            .get(&url)
            .query(&[("auc_user_id", seller_id), ("apg", &page.to_string())])
            .send()
            .await?
            .error_for_status()?;
        let html = resp.text().await?;

        let mut reviews: Vec<Review> = Vec::new();
        for caps in REVIEW_BLOCK_RE.captures_iter(&html) {
            let rating = normalize_rating(&caps[1]);
            let role = if caps[2].contains("出品者") {
                "seller"
            } else {
                "buyer"
            };
            let reviewer_name = caps[3].trim().to_string();
            let reviewer_score: i64 = caps[4].parse().unwrap_or(0);
            let auction_id = caps[5].to_string();
            let item_title = caps[6].trim().to_string();
            let message = caps[7].replace("&nbsp;", " ").trim().to_string();
            let date = caps
                .get(8)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();

            reviews.push(Review {
                rating: rating.to_string(),
                role: role.to_string(),
                message,
                reviewer_name,
                reviewer_score,
                item_title,
                auction_id,
                date,
            });

            if reviews.len() >= limit as usize {
                break;
            }
        }

        let count = reviews.len();
        Ok(ReviewsResult {
            provider: "yahoo_auctions".into(),
            ok: true,
            seller_id: seller_id.to_string(),
            reviews,
            count,
            page,
        })
    }
}

impl Default for YahooAuctionsClient {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_rating(alt: &str) -> &'static str {
    match alt {
        "非常に良い" => "very_good",
        "良い" => "good",
        "どちらでもない" => "neutral",
        "悪い" => "bad",
        "非常に悪い" => "very_bad",
        _ => "unknown",
    }
}

fn parse_search_response(resp: &Value) -> (Vec<Item>, Option<i64>) {
    let total = resp["totalResultsAvailable"].as_i64();
    let items = resp["items"]
        .as_array()
        .map(|arr| arr.iter().filter_map(normalize_search_item).collect())
        .unwrap_or_default();
    (items, total)
}

fn normalize_search_item(raw: &Value) -> Option<Item> {
    let aid = raw["auctionId"].as_str().unwrap_or_default().to_string();
    if aid.is_empty() {
        return None;
    }
    let title = raw["title"].as_str().unwrap_or_default().to_string();

    let price = raw["price"].as_i64().map(Price::yen);
    let buy_now = raw["buyNowPrice"]
        .as_i64()
        .filter(|&v| v > 0)
        .map(Price::yen);
    let bid_count = raw["bidCount"].as_i64();
    let watchers = raw["watchCount"].as_i64();
    let end_time = raw["endTime"].as_str().map(String::from);
    let start_time = raw["startTime"].as_str().map(String::from);
    let image = raw["imageUrl"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);
    // The app API returns categoryPath as bare numeric IDs with no names,
    // so category / category_path are intentionally left unset here
    // (the detail API does provide names).
    let condition = raw["itemCondition"].as_str().map(String::from);
    let free_shipping = raw["isFreeShipping"].as_bool();

    let seller = raw["seller"].as_object().map(|s| Seller {
        user_id: s
            .get("aucUserId")
            .and_then(|v| v.as_str())
            .map(String::from),
        display_name: s
            .get("displayName")
            .and_then(|v| v.as_str())
            .map(String::from),
        rating: s
            .get("rating")
            .and_then(|r| r.get("goodRatio"))
            .and_then(|v| v.as_f64())
            .map(format_good_ratio),
        is_store: s
            .get("type")
            .and_then(|v| v.as_str())
            .and_then(seller_is_store),
        location: None,
    });

    Some(Item {
        url: format!("{WEB_BASE}/jp/auction/{aid}"),
        product_id: aid.clone(),
        auction_id: aid,
        title,
        source: "yahoo_auctions".into(),
        price,
        buy_now_price: buy_now,
        bid_count,
        watchers,
        end_time,
        start_time,
        image,
        condition,
        free_shipping,
        seller,
        ..Default::default()
    })
}

fn normalize_detail_item(raw: &Value) -> Item {
    let aid = raw["auctionId"].as_str().unwrap_or_default().to_string();
    let title = raw["title"].as_str().unwrap_or_default().to_string();
    let price = raw["price"].as_i64().map(Price::yen);
    let buy_now = raw["buyNowPrice"]
        .as_i64()
        .filter(|&v| v > 0)
        .map(Price::yen);
    let bid_count = raw["bidCount"].as_i64();
    // The app API names this `watchCount` (the old Web payload used
    // `watchListNum`).
    let watchers = raw["watchCount"].as_i64();
    let end_time = raw["endTime"].as_str().map(String::from);
    let start_time = raw["startTime"].as_str().map(String::from);
    let status = raw["status"].as_str().map(String::from);
    let condition = raw["condition"].as_str().map(String::from);

    // Prefer the plain-text description over the HTML one.
    let description = raw["plainDescription"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| raw["description"].as_str().map(String::from));

    let (image, images) = if let Some(imgs) = raw["images"].as_array() {
        let all: Vec<String> = imgs
            .iter()
            .filter_map(|im| im["url"].as_str().map(String::from))
            .collect();
        let first = all.first().cloned();
        (first, if all.is_empty() { None } else { Some(all) })
    } else {
        (None, None)
    };

    let shipping = &raw["shippingInfo"];
    let seller = raw["seller"].as_object().map(|s| Seller {
        user_id: s
            .get("aucUserId")
            .and_then(|v| v.as_str())
            .map(String::from),
        display_name: s
            .get("displayName")
            .and_then(|v| v.as_str())
            .map(String::from),
        rating: s
            .get("rating")
            .and_then(|r| r.get("goodRatio"))
            .and_then(|v| v.as_f64())
            .map(format_good_ratio),
        is_store: None,
        // The shipping location lives on shippingInfo in the app API;
        // the old Web detail exposed it as the seller location too.
        location: shipping["location"].as_str().map(String::from),
    });

    let category_path = raw["categoryList"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|c| c["name"].as_str().map(String::from))
            .collect::<Vec<_>>()
    });
    let category = category_path.as_ref().and_then(|p| p.last().cloned());

    let brand = raw["brandList"]
        .as_array()
        .and_then(|arr| arr.last())
        .and_then(|b| b["name"].as_str())
        .map(String::from);

    let free_shipping = shipping["isFreeShipping"].as_bool();

    Item {
        url: format!("{WEB_BASE}/jp/auction/{aid}"),
        product_id: aid.clone(),
        auction_id: aid,
        title,
        source: "yahoo_auctions".into(),
        price,
        buy_now_price: buy_now,
        bid_count,
        watchers,
        end_time,
        start_time,
        status,
        condition,
        description,
        image,
        images,
        seller,
        category,
        category_path,
        brand,
        free_shipping,
        // No shipping-method list exists in the app detail payload.
        shipping_method: None,
    }
}

fn parse_suggestions(resp: &Value) -> Vec<Suggestion> {
    let Some(queries) = resp["queries"].as_array() else {
        return Vec::new();
    };
    queries
        .iter()
        .filter_map(|q| {
            let keyword = q["keyword"].as_str()?.to_string();
            let category = q.get("category");
            let category_id = category.and_then(|c| c["id"].as_i64());
            let category_path = category.and_then(|c| {
                c["namePath"].as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
            });
            let brand = q.get("brand").and_then(|b| {
                b.get("name")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| {
                        b["namePath"]
                            .as_array()
                            .and_then(|arr| arr.last().and_then(|v| v.as_str()).map(String::from))
                    })
            });
            Some(Suggestion {
                keyword,
                category_id,
                category_path,
                brand,
            })
        })
        .collect()
}

impl Default for Item {
    fn default() -> Self {
        Self {
            product_id: String::new(),
            auction_id: String::new(),
            title: String::new(),
            url: String::new(),
            source: "yahoo_auctions".into(),
            price: None,
            buy_now_price: None,
            bid_count: None,
            watchers: None,
            end_time: None,
            start_time: None,
            image: None,
            images: None,
            category: None,
            category_path: None,
            condition: None,
            free_shipping: None,
            seller: None,
            status: None,
            description: None,
            brand: None,
            shipping_method: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed copy of the verified live sample
    // (~/workspace/yauction-analysis/live/search-open-appid.json, 2026-11-01).
    fn sample_search_item() -> Value {
        serde_json::json!({
            "auctionId": "m1250380714",
            "title": "美品 iPhone 17 Pro 1TB",
            "imageUrl": "https://example.com/img.jpg",
            "price": 173005,
            "buyNowPrice": null,
            "isFixedPrice": false,
            "startTime": "2026-10-30T23:59:06+09:00",
            "endTime": "2026-11-01T00:23:42+09:00",
            "itemCondition": "USED40",
            "bidCount": 113,
            "watchCount": 36,
            "isFreeShipping": false,
            "categoryPath": [0, 23632, 23960, 2084317598, 2084317599],
            "seller": {"aucUserId": "BZHRJg8RuSfMn2oHCtUiWnmfAwjPw", "type": "AUCTION", "rating": {"goodRatio": 0.992}}
        })
    }

    #[test]
    fn test_api_sort() {
        assert_eq!(api_sort("", false), "-RANKING");
        assert_eq!(api_sort("relevance", false), "-RANKING");
        assert_eq!(api_sort("default", true), "-RANKING");
        assert_eq!(api_sort("price_asc", false), "+PRICE");
        assert_eq!(api_sort("price_desc", false), "-PRICE");
        assert_eq!(api_sort("bids", false), "-BID_COUNT");
        assert_eq!(api_sort("bids", true), "-BID_COUNT");
        assert_eq!(api_sort("end_time", false), "+END_TIME");
        assert_eq!(api_sort("ending_soonest", true), "+END_TIME");
        assert_eq!(api_sort("newest", false), "-FIRST_START_TIME");
        assert_eq!(api_sort("newest", true), "-END_TIME");
        assert_eq!(api_sort("unknown_sort", false), "-RANKING");
    }

    #[test]
    fn test_api_condition() {
        assert_eq!(api_condition("new"), Some("NEW"));
        assert_eq!(api_condition("新品"), Some("NEW"));
        assert_eq!(api_condition("unused"), Some("USED10"));
        assert_eq!(api_condition("nearly_new"), Some("USED20"));
        assert_eq!(api_condition("used"), Some("USED40"));
        assert_eq!(api_condition("slightly_used"), Some("USED40"));
        assert_eq!(api_condition("used_fair"), Some("USED60"));
        assert_eq!(api_condition("junk"), Some("USED80"));
        assert_eq!(api_condition(""), None);
        assert_eq!(api_condition("unknown"), None);
    }

    #[test]
    fn test_format_good_ratio() {
        assert_eq!(format_good_ratio(0.992), "99.2%");
        assert_eq!(format_good_ratio(1.0), "100.0%");
        assert_eq!(format_good_ratio(0.99), "99.0%");
        assert_eq!(format_good_ratio(0.946), "94.6%");
    }

    #[test]
    fn test_seller_is_store() {
        assert_eq!(seller_is_store("STORE"), Some(true));
        assert_eq!(seller_is_store("AUCTION"), Some(false));
        // Seen live (AUC_STORE / FLEAMARKET): not guessed at.
        assert_eq!(seller_is_store("AUC_STORE"), None);
        assert_eq!(seller_is_store("FLEAMARKET"), None);
    }

    #[test]
    fn test_normalize_search_item() {
        let item = normalize_search_item(&sample_search_item()).unwrap();
        assert_eq!(item.auction_id, "m1250380714");
        assert_eq!(item.product_id, "m1250380714");
        assert!(item.url.ends_with("/jp/auction/m1250380714"));
        assert_eq!(item.price.as_ref().unwrap().value, 173005);
        assert_eq!(item.price.as_ref().unwrap().currency, "JPY");
        assert!(
            item.buy_now_price.is_none(),
            "null buyNowPrice must stay None"
        );
        assert_eq!(item.bid_count, Some(113));
        assert_eq!(item.watchers, Some(36));
        assert_eq!(item.image.as_deref(), Some("https://example.com/img.jpg"));
        assert_eq!(item.condition.as_deref(), Some("USED40"));
        assert_eq!(item.free_shipping, Some(false));
        // categoryPath is bare numeric IDs in the app API: no names.
        assert!(item.category.is_none());
        assert!(item.category_path.is_none());
        let seller = item.seller.as_ref().unwrap();
        assert_eq!(
            seller.user_id.as_deref(),
            Some("BZHRJg8RuSfMn2oHCtUiWnmfAwjPw")
        );
        assert_eq!(seller.rating.as_deref(), Some("99.2%"));
        assert_eq!(seller.is_store, Some(false));
    }

    #[test]
    fn test_parse_search_response() {
        let resp = serde_json::json!({
            "totalResultsAvailable": 370056,
            "items": [sample_search_item(), {"auctionId": "x1", "title": "t", "price": 10, "buyNowPrice": 500, "seller": {"aucUserId": "s", "type": "AUC_STORE", "rating": {"goodRatio": 0.99}}}]
        });
        let (items, total) = parse_search_response(&resp);
        assert_eq!(total, Some(370056));
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].buy_now_price.as_ref().unwrap().value, 500);
        assert_eq!(items[1].seller.as_ref().unwrap().is_store, None);
        assert_eq!(
            items[1].seller.as_ref().unwrap().rating.as_deref(),
            Some("99.0%")
        );
    }

    #[test]
    fn test_normalize_detail_item() {
        // Trimmed copy of the verified live sample
        // (~/workspace/yauction-analysis/live/detail-appid.json, 2026-11-01).
        let raw = serde_json::json!({
            "auctionId": "m1250380714",
            "title": "美品 iPhone 17 Pro 1TB",
            "description": "<DIV>HTML説明</DIV>",
            "plainDescription": "プレーン説明",
            "images": [{"url": "https://example.com/1.jpg", "comment": null}, {"url": "https://example.com/2.jpg", "comment": null}],
            "price": 173005,
            "buyNowPrice": null,
            "status": "OPEN",
            "startTime": "2026-10-30T23:59:06+09:00",
            "endTime": "2026-11-01T00:23:42+09:00",
            "bidCount": 113,
            "watchCount": 36,
            "condition": "USED40",
            "seller": {
                "aucUserId": "BZHRJg8RuSfMn2oHCtUiWnmfAwjPw",
                "displayName": "rab********",
                "rating": {"total": 10539, "good": 10627, "bad": 88, "goodRatio": 0.992, "ratingStatus": "RATED"}
            },
            "categoryList": [{"id": 0, "name": "すべて"}, {"id": 23632, "name": "家電、AV、カメラ"}, {"id": 2084317599, "name": "iPhone"}],
            "brandList": [{"id": 100011, "name": "Apple"}, {"id": 205733, "name": "iPhone 17 Pro"}],
            "shippingInfo": {"isFreeShipping": false, "chargeForShipping": "BUYER", "location": "広島県"}
        });
        let item = normalize_detail_item(&raw);
        assert_eq!(item.auction_id, "m1250380714");
        assert_eq!(item.price.as_ref().unwrap().value, 173005);
        assert!(item.buy_now_price.is_none());
        assert_eq!(item.bid_count, Some(113));
        // app API key is watchCount, not the old Web watchListNum.
        assert_eq!(item.watchers, Some(36));
        assert_eq!(item.status.as_deref(), Some("OPEN"));
        assert_eq!(item.condition.as_deref(), Some("USED40"));
        // plainDescription wins over the HTML description.
        assert_eq!(item.description.as_deref(), Some("プレーン説明"));
        assert_eq!(item.image.as_deref(), Some("https://example.com/1.jpg"));
        assert_eq!(item.images.as_ref().unwrap().len(), 2);
        assert_eq!(item.category.as_deref(), Some("iPhone"));
        assert_eq!(
            item.category_path.as_ref().unwrap(),
            &vec![
                "すべて".to_string(),
                "家電、AV、カメラ".to_string(),
                "iPhone".to_string()
            ]
        );
        assert_eq!(item.brand.as_deref(), Some("iPhone 17 Pro"));
        assert_eq!(item.free_shipping, Some(false));
        assert!(item.shipping_method.is_none());
        let seller = item.seller.as_ref().unwrap();
        assert_eq!(
            seller.user_id.as_deref(),
            Some("BZHRJg8RuSfMn2oHCtUiWnmfAwjPw")
        );
        assert_eq!(seller.display_name.as_deref(), Some("rab********"));
        assert_eq!(seller.rating.as_deref(), Some("99.2%"));
        assert_eq!(seller.location.as_deref(), Some("広島県"));
    }

    #[test]
    fn test_normalize_detail_item_html_description_fallback() {
        let raw = serde_json::json!({
            "auctionId": "a1",
            "title": "t",
            "description": "<DIV>HTMLのみ</DIV>",
            "plainDescription": "",
            "price": 100
        });
        let item = normalize_detail_item(&raw);
        assert_eq!(item.description.as_deref(), Some("<DIV>HTMLのみ</DIV>"));
        assert!(item.images.is_none());
        assert!(item.seller.is_none());
    }

    #[test]
    fn test_parse_suggestions() {
        // Verified live sample shape
        // (~/workspace/yauction-analysis/live/suggest-appid.json, 2026-11-01).
        let resp = serde_json::json!({
            "queries": [
                {"keyword": "iphone", "category": {"id": 2084005061, "namePath": ["オークション", "家電、AV、カメラ", "携帯電話、スマートフォン", "アクセサリー"]}},
                {"keyword": "iphone"},
                {"keyword": "iphone17"}
            ]
        });
        let suggestions = parse_suggestions(&resp);
        assert_eq!(suggestions.len(), 3);
        assert_eq!(suggestions[0].keyword, "iphone");
        assert_eq!(suggestions[0].category_id, Some(2084005061));
        assert_eq!(suggestions[0].category_path.as_ref().unwrap().len(), 4);
        assert!(suggestions[1].category_id.is_none());
        assert!(suggestions[1].category_path.is_none());
        assert_eq!(suggestions[2].keyword, "iphone17");
        assert!(parse_suggestions(&serde_json::json!({})).is_empty());
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_closed() {
        let client = YahooAuctionsClient::new();
        let result = client
            .search(SearchParams {
                query: "カメラ".into(),
                limit: 5,
                page: 1,
                status: "closed".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok, "closed search must succeed");
        assert_eq!(result.provider, "yahoo_auctions");
        assert_eq!(result.status, "closed");
        assert_eq!(result.search_mode, "appApi");
        assert!(
            !result.items.is_empty(),
            "closed search returned 0 items — API may be broken"
        );
        assert_eq!(result.count, result.items.len());
        for item in &result.items {
            assert!(!item.auction_id.is_empty(), "auction_id empty");
            assert!(!item.title.is_empty(), "title empty");
            assert!(
                item.url.contains(&item.auction_id),
                "url must contain auction_id"
            );
            if let Some(price) = &item.price {
                assert_eq!(price.currency, "JPY");
                assert!(price.value >= 0);
            }
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_open() {
        let client = YahooAuctionsClient::new();
        let result = client
            .search(SearchParams {
                query: "iPhone".into(),
                limit: 5,
                page: 1,
                status: "open".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok, "open search must succeed");
        assert_eq!(result.provider, "yahoo_auctions");
        assert_eq!(result.status, "open");
        assert!(
            !result.items.is_empty(),
            "open search returned 0 items — API may be broken"
        );
        assert_eq!(
            result.search_mode, "appApi",
            "open search must use the app API"
        );
        assert!(
            result.total_available.unwrap_or(0) > 0,
            "total_available must be present from the API"
        );
        assert!(
            result.items.iter().any(|i| i.image.is_some()),
            "API items must carry images"
        );
        assert!(
            result.items.iter().any(|i| i.seller.is_some()),
            "API items must carry sellers"
        );
        for item in &result.items {
            assert!(!item.auction_id.is_empty(), "auction_id empty");
            assert!(!item.title.is_empty(), "title empty");
            assert!(
                item.url.contains("auctions.yahoo.co.jp"),
                "bad url: {}",
                item.url
            );
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_open_multiword() {
        let client = YahooAuctionsClient::new();
        let result = client
            .search(SearchParams {
                query: "NVIDIA GPU".into(),
                limit: 3,
                page: 1,
                status: "open".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok);
        // Multi-word search should work; 0 items only if genuinely no results
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_get_item_from_open() {
        let client = YahooAuctionsClient::new();
        let search = client
            .search(SearchParams {
                query: "カメラ".into(),
                limit: 5,
                page: 1,
                status: "open".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(search.ok);
        assert!(!search.items.is_empty(), "need items to test get_item");
        let mut found_detail = false;
        for candidate in &search.items {
            if let Ok(detail) = client.get_item(&candidate.auction_id).await {
                if detail.ok {
                    assert_eq!(detail.provider, "yahoo_auctions");
                    let item = detail.item.expect("item must be present");
                    assert_eq!(item.auction_id, candidate.auction_id);
                    assert!(!item.title.is_empty());
                    assert!(item.url.contains(&item.auction_id));
                    if let Some(price) = &item.price {
                        assert_eq!(price.currency, "JPY");
                    }
                    found_detail = true;
                    break;
                }
            }
        }
        assert!(
            found_detail,
            "get_item failed for ALL {} search results",
            search.items.len()
        );
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_with_sort() {
        let client = YahooAuctionsClient::new();
        let result = client
            .search(SearchParams {
                query: "レンズ".into(),
                limit: 5,
                page: 1,
                status: "closed".into(),
                sort: "price_asc".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok);
        assert_eq!(result.sort, "price_asc");
        assert!(!result.items.is_empty(), "sorted search returned 0 items");
        // price_asc on the API must actually come back ascending.
        let prices: Vec<i64> = result
            .items
            .iter()
            .filter_map(|i| i.price.as_ref().map(|p| p.value))
            .collect();
        assert!(
            prices.windows(2).all(|w| w[0] <= w[1]),
            "closed price_asc not ascending: {prices:?}"
        );
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_suggest_keywords() {
        let client = YahooAuctionsClient::new();
        let result = client.suggest_keywords("iphone", 10).await.unwrap();
        assert!(result.ok);
        assert_eq!(result.provider, "yahoo_auctions");
        assert!(
            !result.suggestions.is_empty(),
            "suggest returned 0 suggestions — API may be broken"
        );
        assert_eq!(result.count, result.suggestions.len());
        for s in &result.suggestions {
            assert!(!s.keyword.is_empty(), "suggestion keyword empty");
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_get_reviews() {
        let client = YahooAuctionsClient::new();
        // First search for an item to get a seller_id
        let search = client
            .search(SearchParams {
                query: "カメラ".into(),
                limit: 5,
                page: 1,
                status: "open".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(search.ok);
        assert!(!search.items.is_empty(), "need items to find a seller");

        // Get item detail to extract seller_id (hash-form aucUserId; the
        // Web rating page accepts it, verified 2026-11-01).
        let mut seller_id = String::new();
        for candidate in &search.items {
            if let Ok(detail) = client.get_item(&candidate.auction_id).await {
                if detail.ok {
                    if let Some(item) = &detail.item {
                        if let Some(seller) = &item.seller {
                            if let Some(uid) = &seller.user_id {
                                if !uid.is_empty() {
                                    seller_id = uid.clone();
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(
            !seller_id.is_empty(),
            "could not find a seller_id from search results"
        );

        let reviews = client.get_reviews(&seller_id, 1, 25).await.unwrap();
        assert!(reviews.ok);
        assert_eq!(reviews.provider, "yahoo_auctions");
        assert_eq!(reviews.seller_id, seller_id);
        assert_eq!(reviews.page, 1);
        // Seller may have reviews; if they do, validate structure
        for review in &reviews.reviews {
            assert!(
                ["very_good", "good", "neutral", "bad", "very_bad", "unknown"]
                    .contains(&review.rating.as_str()),
                "unexpected rating: {}",
                review.rating
            );
            assert!(
                review.role == "seller" || review.role == "buyer",
                "unexpected role: {}",
                review.role
            );
            assert!(!review.reviewer_name.is_empty(), "reviewer_name empty");
            assert!(!review.auction_id.is_empty(), "auction_id empty");
        }
    }
}
