use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

const BASE: &str = "https://fril.jp";
const ITEM_BASE: &str = "https://item.fril.jp";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

static RAT_ITEM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"<a\s+href="(https://item\.fril\.jp/([a-f0-9]+))"[^>]*data-rat-itemid="([^"]+)"[^>]*data-rat-item_name="([^"]+)"[^>]*data-rat-igenre="([^"]+)"[^>]*data-rat-price="([^"]+)""#
    ).unwrap()
});
static RAT_TOTAL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"data-rat-cp-totalresults="([^"]+)""#).unwrap());
static IMG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<img[^>]*(?:data-original|src)="(https://img\.fril\.jp/img/(\d+)/m/[^"]+)""#)
        .unwrap()
});
static JSONLD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<script type="application/ld\+json">(.*?)</script>"#).unwrap()
});
static DETAIL_IMG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""(https://img\.fril\.jp/img/\d+/l/[^"]+)""#).unwrap());
static SELLER_HASH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"href="https://fril\.jp/shop/([a-f0-9]{32})""#).unwrap());
static REVIEW_ITEM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)<article[^>]*class="[^"]*review-item[^"]*"[^>]*>.*?<p[^>]*class="[^"]*review-item-title[^"]*"[^>]*>.*?</i>(.*?)</p>.*?<div[^>]*class="[^"]*review-item-text[^"]*"[^>]*>(.*?)</div>\s*<div[^>]*class="row"[^>]*>\s*<div[^>]*class="[^"]*review-item-name[^"]*"[^>]*>\s*(.*?)\s*</div>.*?<p[^>]*class="[^"]*review-item-date[^"]*"[^>]*>(.*?)</p>"#
    ).unwrap()
});

fn sort_params(s: &str) -> (&'static str, &'static str) {
    match s.to_lowercase().as_str() {
        "" | "default" | "relevance" => ("", ""),
        "price_asc" | "lowest_price" => ("sell_price", "asc"),
        "price_desc" | "highest_price" => ("sell_price", "desc"),
        "newest" | "new" => ("created_at", "desc"),
        "likes" | "popular" => ("like_count", "desc"),
        _ => ("", ""),
    }
}

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
    pub product_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    pub title: String,
    pub url: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SellerInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_hash: Option<String>,
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

#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub query: String,
    pub limit: i32,
    pub page: i32,
    pub sort: String,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub category_id: String,
    pub brand_id: String,
}

pub struct RakumaClient {
    client: Client,
}

impl RakumaClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent(USER_AGENT)
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub async fn search(&self, params: SearchParams) -> Result<SearchResult> {
        let (sort_key, order) = sort_params(&params.sort);
        let limit = if params.limit > 0 { params.limit } else { 40 };
        let page = if params.page > 0 { params.page } else { 1 };

        let mut qp: Vec<(String, String)> = vec![("query".into(), params.query.clone())];
        if !sort_key.is_empty() {
            qp.push(("sort".into(), sort_key.into()));
            qp.push(("order".into(), order.into()));
        }
        if page > 1 {
            qp.push(("page".into(), page.to_string()));
        }
        if let Some(v) = params.min_price {
            qp.push(("min".into(), v.to_string()));
        }
        if let Some(v) = params.max_price {
            qp.push(("max".into(), v.to_string()));
        }
        if !params.category_id.is_empty() {
            qp.push(("category_id".into(), params.category_id.clone()));
        }
        if !params.brand_id.is_empty() {
            qp.push(("brand_id".into(), params.brand_id.clone()));
        }

        let url = format!("{BASE}/s");
        let resp = self.client.get(&url).query(&qp).send().await?;
        let html = if resp.status().is_server_error() && sort_key.is_empty() {
            // fril.jp's relevance (default) search path returns HTTP 500
            // for some keywords (measured 2026-10-31: query=バッグ fails
            // with the site's own sort=relevance&order=desc URLs too,
            // while explicit sorts return the same result set, ~6.4M
            // items). Retry once with an explicit newest-first sort,
            // which only changes ordering, not the result set.
            drop(resp);
            let mut retry_qp = qp.clone();
            retry_qp.push(("sort".into(), "created_at".into()));
            retry_qp.push(("order".into(), "desc".into()));
            let retry_resp = self.client.get(&url).query(&retry_qp).send().await?;
            retry_resp.error_for_status()?.text().await?
        } else {
            resp.error_for_status()?.text().await?
        };

        let (items, total) = extract_search_items(&html);
        let count = items.len().min(limit as usize);

        Ok(SearchResult {
            provider: "rakuma".into(),
            ok: true,
            query: params.query,
            page,
            limit,
            sort: params.sort,
            items: items.into_iter().take(limit as usize).collect(),
            count,
            total_available: total,
        })
    }

    pub async fn get_item(&self, item_hash: &str) -> Result<ItemResult> {
        let url = format!("{ITEM_BASE}/{item_hash}");
        let resp = self.client.get(&url).send().await?.error_for_status()?;
        let html = resp.text().await?;

        match extract_detail_item(&html, item_hash) {
            Some(item) => Ok(ItemResult {
                provider: "rakuma".into(),
                ok: true,
                item: Some(item),
                error: None,
            }),
            None => Ok(ItemResult {
                provider: "rakuma".into(),
                ok: false,
                item: None,
                error: Some("no_jsonld_data".into()),
            }),
        }
    }

    pub async fn get_reviews(&self, seller_hash: &str, _page: i32, limit: i32) -> Result<Value> {
        let limit = if limit > 0 { limit } else { 20 };

        // Step 1: Fetch shop page for aggregate rating from JSON-LD
        let shop_url = format!("{BASE}/shop/{seller_hash}");
        let shop_resp = self
            .client
            .get(&shop_url)
            .send()
            .await?
            .error_for_status()?;
        let shop_html = shop_resp.text().await?;

        let (aggregate_rating, seller_name) = extract_aggregate_rating(&shop_html);

        // Step 2: Fetch review page for individual reviews
        let review_url = format!("{BASE}/shop/{seller_hash}/review");
        let review_resp = self
            .client
            .get(&review_url)
            .send()
            .await?
            .error_for_status()?;
        let review_html = review_resp.text().await?;

        let reviews = extract_reviews(&review_html, limit);
        let count = reviews.len();

        Ok(json!({
            "provider": "rakuma",
            "ok": true,
            "seller_hash": seller_hash,
            "seller_name": seller_name.unwrap_or_default(),
            "aggregate_rating": aggregate_rating,
            "reviews": reviews,
            "count": count,
        }))
    }
}

impl Default for RakumaClient {
    fn default() -> Self {
        Self::new()
    }
}

fn extract_search_items(html: &str) -> (Vec<Item>, Option<i64>) {
    let mut img_map: HashMap<String, String> = HashMap::new();
    for caps in IMG_RE.captures_iter(html) {
        img_map.insert(caps[2].to_string(), caps[1].to_string());
    }

    let total = RAT_TOTAL_RE
        .captures(html)
        .and_then(|c| c[1].parse::<i64>().ok());

    let mut seen = HashSet::new();
    let mut items = vec![];

    for caps in RAT_ITEM_RE.captures_iter(html) {
        let item_url = &caps[1];
        let item_hash = &caps[2];
        let rat_id = &caps[3];
        let name = &caps[4];
        let genre = &caps[5];
        let price_str = &caps[6];

        if !seen.insert(item_hash.to_string()) {
            continue;
        }

        let parts: Vec<&str> = rat_id.split('/').collect();
        let (seller_id, item_id) = if parts.len() > 1 {
            (Some(parts[0].to_string()), parts[1].to_string())
        } else {
            (None, parts[0].to_string())
        };

        let price_val = price_str.parse::<i64>().unwrap_or(0);
        let image = img_map.get(&item_id).cloned();

        items.push(Item {
            product_id: item_hash.to_string(),
            item_id: Some(item_id),
            title: name.to_string(),
            url: item_url.to_string(),
            source: "rakuma".into(),
            price: Some(Price::yen(price_val)),
            category_id: Some(genre.to_string()),
            seller: seller_id.map(|id| SellerInfo {
                id: Some(id),
                name: None,
                seller_hash: None,
            }),
            image,
            ..Default::default()
        });
    }

    (items, total)
}

fn extract_detail_item(html: &str, item_hash: &str) -> Option<Item> {
    let mut product: Option<Value> = None;
    for caps in JSONLD_RE.captures_iter(html) {
        if let Ok(data) = serde_json::from_str::<Value>(&caps[1]) {
            if data["@type"].as_str() == Some("Product") {
                product = Some(data);
                break;
            }
        }
    }

    let prod = product?;
    let offers = &prod["offers"];

    let price_val = offers["price"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .or_else(|| offers["price"].as_i64());

    let condition_url = offers["itemCondition"].as_str().unwrap_or_default();
    let condition = if condition_url.contains("NewCondition") {
        Some("新品".into())
    } else if condition_url.contains("UsedCondition") {
        Some("中古".into())
    } else if condition_url.contains("RefurbishedCondition") {
        Some("再生品".into())
    } else {
        None
    };

    let availability_url = offers["availability"].as_str().unwrap_or_default();
    let availability = if availability_url.contains("InStock") {
        Some("販売中".into())
    } else if availability_url.contains("SoldOut") || availability_url.contains("OutOfStock") {
        Some("売り切れ".into())
    } else {
        None
    };

    let description = prod["description"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let main_image = prod["image"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);

    let all_images: Vec<String> = {
        let mut set = BTreeSet::new();
        for caps in DETAIL_IMG_RE.captures_iter(html) {
            set.insert(caps[1].to_string());
        }
        set.into_iter().collect()
    };

    let seller_name = offers["seller"]["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);

    let seller_hash = SELLER_HASH_RE.captures(html).map(|c| c[1].to_string());

    let seller = if seller_name.is_some() || seller_hash.is_some() {
        Some(SellerInfo {
            id: None,
            name: seller_name,
            seller_hash,
        })
    } else {
        None
    };

    Some(Item {
        product_id: item_hash.to_string(),
        title: prod["name"].as_str().unwrap_or_default().to_string(),
        url: format!("{ITEM_BASE}/{item_hash}"),
        source: "rakuma".into(),
        price: price_val.map(Price::yen),
        condition,
        availability,
        description,
        image: main_image,
        images: if all_images.is_empty() {
            None
        } else {
            Some(all_images)
        },
        seller,
        ..Default::default()
    })
}

fn extract_aggregate_rating(html: &str) -> (Value, Option<String>) {
    for caps in JSONLD_RE.captures_iter(html) {
        if let Ok(data) = serde_json::from_str::<Value>(&caps[1]) {
            if data["@type"].as_str() == Some("Store") {
                let agg = &data["aggregateRating"];
                let seller_name = data["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                let rating = json!({
                    "rating_value": agg["ratingValue"].as_str().unwrap_or("0"),
                    "rating_count": agg["ratingCount"].as_str()
                        .and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
                    "best_rating": agg["bestRating"].as_str().unwrap_or("5"),
                    "worst_rating": agg["worstRating"].as_str().unwrap_or("0"),
                });
                return (rating, seller_name);
            }
        }
    }
    (json!(null), None)
}

fn strip_html_tags(s: &str) -> String {
    static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
    let text = s
        .replace("<br />", "\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n");
    TAG_RE.replace_all(&text, "").trim().to_string()
}

fn extract_reviews(html: &str, limit: i32) -> Vec<Value> {
    let mut reviews = Vec::new();

    for caps in REVIEW_ITEM_RE.captures_iter(html) {
        if reviews.len() >= limit as usize {
            break;
        }

        let rating = caps[1].trim().to_string();
        let message = strip_html_tags(&caps[2]);
        let reviewer_name = caps[3].trim().to_string();
        let date = caps[4].trim().to_string();

        reviews.push(json!({
            "rating": rating,
            "message": message,
            "reviewer_name": reviewer_name,
            "date": date,
        }));
    }

    reviews
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_params() {
        assert_eq!(sort_params(""), ("", ""));
        assert_eq!(sort_params("price_asc"), ("sell_price", "asc"));
        assert_eq!(sort_params("newest"), ("created_at", "desc"));
        assert_eq!(sort_params("likes"), ("like_count", "desc"));
    }

    #[test]
    fn test_sort_params_case_insensitive() {
        assert_eq!(sort_params("PRICE_ASC"), ("sell_price", "asc"));
        assert_eq!(sort_params("Price_Desc"), ("sell_price", "desc"));
        assert_eq!(sort_params("unknown_sort"), ("", ""));
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search() {
        let client = RakumaClient::new();
        let result = client
            .search(SearchParams {
                query: "バッグ".into(),
                limit: 5,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok, "search must succeed");
        assert_eq!(result.provider, "rakuma");
        assert!(
            !result.items.is_empty(),
            "search returned 0 items — API may be broken"
        );
        assert_eq!(result.count, result.items.len());
        for item in &result.items {
            assert!(!item.product_id.is_empty());
            assert!(!item.title.is_empty());
            assert!(item.url.contains("fril.jp"), "bad url: {}", item.url);
            if let Some(price) = &item.price {
                assert_eq!(price.currency, "JPY");
                assert!(price.value > 0);
            }
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_multiword() {
        let client = RakumaClient::new();
        let result = client
            .search(SearchParams {
                query: "ワイヤレス イヤホン".into(),
                limit: 3,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok, "multi-word search must succeed");
        assert!(
            !result.items.is_empty(),
            "multi-word search returned 0 items"
        );
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_then_get_item() {
        let client = RakumaClient::new();
        let search = client
            .search(SearchParams {
                query: "カメラ".into(),
                limit: 3,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(search.ok);
        assert!(!search.items.is_empty(), "need items to test get_item");
        let first = &search.items[0];
        let detail = client.get_item(&first.product_id).await.unwrap();
        assert!(
            detail.ok,
            "get_item FAILED for {}: {:?}",
            first.product_id, detail
        );
        assert_eq!(detail.provider, "rakuma");
        let item = detail.item.expect("item must be present");
        assert!(!item.title.is_empty());
        if let Some(price) = &item.price {
            assert_eq!(price.currency, "JPY");
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_get_reviews() {
        let client = RakumaClient::new();

        // Search to get an item
        let search = client
            .search(SearchParams {
                query: "バッグ".into(),
                limit: 3,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(search.ok);
        assert!(!search.items.is_empty(), "need items to find seller");

        // Get item detail to extract seller_hash
        let first = &search.items[0];
        let detail = client.get_item(&first.product_id).await.unwrap();
        assert!(detail.ok, "get_item must succeed");
        let item = detail.item.expect("item must be present");
        let seller = item.seller.expect("seller info must be present");
        let seller_hash = seller
            .seller_hash
            .expect("seller_hash must be present in item detail");
        assert_eq!(seller_hash.len(), 32, "seller_hash must be 32-char hex");

        // Fetch reviews
        let reviews = client.get_reviews(&seller_hash, 1, 5).await.unwrap();
        assert_eq!(reviews["provider"], "rakuma");
        assert_eq!(reviews["ok"], true);
        assert_eq!(reviews["seller_hash"], seller_hash);
        assert!(
            !reviews["seller_name"].as_str().unwrap_or("").is_empty(),
            "seller_name must be present"
        );

        // Check aggregate rating
        let agg = &reviews["aggregate_rating"];
        if !agg.is_null() {
            assert!(agg["rating_count"].as_i64().unwrap_or(0) >= 0);
        }

        // Check individual reviews
        let review_arr = reviews["reviews"]
            .as_array()
            .expect("reviews must be array");
        assert!(!review_arr.is_empty(), "should have at least one review");
        assert!(review_arr.len() <= 5, "should respect limit");
        for r in review_arr {
            assert!(
                !r["rating"].as_str().unwrap_or("").is_empty(),
                "rating must not be empty"
            );
            assert!(
                !r["date"].as_str().unwrap_or("").is_empty(),
                "date must not be empty"
            );
        }
    }
}
