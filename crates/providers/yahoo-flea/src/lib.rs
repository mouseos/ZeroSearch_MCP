use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::LazyLock;

const BASE: &str = "https://paypayfleamarket.yahoo.co.jp";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

static SSR_JSON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<script[^>]*id="__NEXT_DATA__"[^>]*>(.*?)</script>"#).unwrap());

fn sort_params(s: &str) -> (&'static str, &'static str) {
    match s.to_lowercase().as_str() {
        "" | "default" | "relevance" | "ranking" => ("ranking", "desc"),
        "price_asc" | "lowest_price" => ("price", "asc"),
        "price_desc" | "highest_price" => ("price", "desc"),
        "newest" | "new" => ("openTime", "desc"),
        _ => ("ranking", "desc"),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SellerInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_rating: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_rating: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Item {
    pub product_id: String,
    pub title: String,
    pub url: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub likes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub views: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_path: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller: Option<SellerInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_schedule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hashtags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub specs: Option<Vec<Spec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_down: Option<bool>,
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
}

pub struct YahooFleaClient {
    client: Client,
}

impl YahooFleaClient {
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

        let url = if params.query.is_empty() {
            format!("{BASE}/search/")
        } else {
            let encoded: String = params
                .query
                .bytes()
                .map(|b| {
                    if b == b' ' {
                        "+".to_string()
                    } else if b.is_ascii_alphanumeric()
                        || b == b'-'
                        || b == b'_'
                        || b == b'.'
                        || b == b'~'
                    {
                        format!("{}", b as char)
                    } else {
                        format!("%{:02X}", b)
                    }
                })
                .collect();
            format!("{BASE}/search/{encoded}")
        };

        let mut qp: Vec<(String, String)> = vec![];
        if sort_key != "ranking" {
            qp.push(("sort".into(), sort_key.into()));
            qp.push(("order".into(), order.into()));
        }
        if page > 1 {
            qp.push(("page".into(), page.to_string()));
        }
        if let Some(v) = params.min_price {
            qp.push(("minPrice".into(), v.to_string()));
        }
        if let Some(v) = params.max_price {
            qp.push(("maxPrice".into(), v.to_string()));
        }
        if !params.category_id.is_empty() {
            qp.push(("categoryId".into(), params.category_id.clone()));
        }

        let resp = self.client.get(&url).query(&qp).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(SearchResult {
                provider: "yahoo_flea".into(),
                ok: true,
                query: params.query,
                page,
                limit,
                sort: params.sort,
                items: vec![],
                count: 0,
                total_available: Some(0),
            });
        }
        let resp = resp.error_for_status()?;
        let html = resp.text().await?;

        let ssr = match extract_ssr_json(&html) {
            Some(v) => v,
            None => {
                return Ok(SearchResult {
                    provider: "yahoo_flea".into(),
                    ok: false,
                    query: params.query,
                    page,
                    limit,
                    sort: params.sort,
                    items: vec![],
                    count: 0,
                    total_available: None,
                })
            }
        };

        let search_data = &ssr["props"]["initialState"]["searchState"]["search"]["result"];
        let items_raw = search_data["items"].as_array();
        let total = search_data["totalResultsAvailable"].as_i64();

        let items: Vec<Item> = items_raw
            .map(|arr| arr.iter().filter_map(normalize_search_item).collect())
            .unwrap_or_default();

        let count = items.len().min(limit as usize);
        Ok(SearchResult {
            provider: "yahoo_flea".into(),
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

    pub async fn get_reviews(&self, seller_id: &str, page: i32, limit: i32) -> Result<Value> {
        let url = format!("{BASE}/api/v1/users/{seller_id}/ratings");
        let limit = if limit > 0 { limit } else { 20 };
        let page = if page > 0 { page } else { 1 };

        let resp = self
            .client
            .get(&url)
            .query(&[("page", page.to_string()), ("limit", limit.to_string())])
            .send()
            .await?
            .error_for_status()?;
        let data: Value = resp.json().await?;

        let total = data["totalRating"].as_i64().unwrap_or(0);
        let good = data["totalGoodRating"].as_i64().unwrap_or(0);
        let neutral = data["totalNeutralRating"].as_i64().unwrap_or(0);
        let bad = data["totalBadRating"].as_i64().unwrap_or(0);

        let reviews: Vec<Value> = data["contents"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|r| {
                        json!({
                            "rating": r["score"].as_str().unwrap_or("unknown"),
                            "message": r["comment"].as_str().unwrap_or(""),
                            "reviewer_name": r["userName"].as_str().unwrap_or(""),
                            "reviewer_id": r["userId"].as_str().unwrap_or(""),
                            "date": r["date"].as_str().unwrap_or(""),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let count = reviews.len();
        let has_next = (page as i64) * (limit as i64) < total;

        Ok(json!({
            "provider": "yahoo_flea",
            "ok": true,
            "seller_id": seller_id,
            "ratings_summary": {
                "good": good,
                "neutral": neutral,
                "bad": bad,
                "total": total,
            },
            "reviews": reviews,
            "has_next": has_next,
            "count": count,
        }))
    }

    pub async fn get_item(&self, item_id: &str) -> Result<ItemResult> {
        let url = format!("{BASE}/item/{item_id}");
        let resp = self.client.get(&url).send().await?.error_for_status()?;
        let html = resp.text().await?;

        let ssr = match extract_ssr_json(&html) {
            Some(v) => v,
            None => {
                return Ok(ItemResult {
                    provider: "yahoo_flea".into(),
                    ok: false,
                    item: None,
                    error: Some("no_ssr_data".into()),
                })
            }
        };

        let raw = &ssr["props"]["initialState"]["itemsState"]["items"]["item"];
        if raw.is_null() {
            return Ok(ItemResult {
                provider: "yahoo_flea".into(),
                ok: false,
                item: None,
                error: Some("no_item_data".into()),
            });
        }

        Ok(ItemResult {
            provider: "yahoo_flea".into(),
            ok: true,
            item: Some(normalize_detail_item(raw)),
            error: None,
        })
    }
}

impl Default for YahooFleaClient {
    fn default() -> Self {
        Self::new()
    }
}

fn extract_ssr_json(html: &str) -> Option<Value> {
    let caps = SSR_JSON_RE.captures(html)?;
    serde_json::from_str(&caps[1]).ok()
}

fn normalize_search_item(raw: &Value) -> Option<Item> {
    let id = raw["id"].as_str().unwrap_or_default().to_string();
    let title = raw["title"].as_str().unwrap_or_default().to_string();
    let price = raw["price"].as_i64().map(Price::yen);
    let likes = raw["likeCount"].as_i64();
    let status = raw["itemStatus"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let condition = raw["condition"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let image = raw["thumbnailImageUrl"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let image_count = raw["imageCount"].as_i64();
    let open_time = raw["openTime"]
        .as_str()
        .map(String::from)
        .or_else(|| raw["openTime"].as_i64().map(|v| v.to_string()));
    let end_time = raw["endTime"]
        .as_str()
        .map(String::from)
        .or_else(|| raw["endTime"].as_i64().map(|v| v.to_string()));

    let category = raw["category"]
        .as_object()
        .and_then(|c| c.get("name").and_then(|v| v.as_str()).map(String::from));
    let category_path = raw["category"]["path"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|c| c["name"].as_str().map(String::from))
            .collect()
    });

    let seller = raw["seller"].as_object().and_then(|s| {
        let sid = s.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if sid.is_empty() {
            return None;
        }
        Some(SellerInfo {
            id: sid.to_string(),
            nickname: None,
            rating: s.get("goodRatio").and_then(|v| v.as_f64()),
            num_rating: s.get("numRating").and_then(|v| v.as_i64()),
            total_rating: None,
        })
    });

    let hashtags = raw["hashtag"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty());

    let price_down = raw["isPriceDown"].as_bool().filter(|&b| b);

    Some(Item {
        product_id: id.clone(),
        title,
        url: format!("{BASE}/item/{id}"),
        source: "yahoo_flea".into(),
        price,
        likes,
        status,
        condition,
        image,
        image_count,
        category,
        category_path,
        seller,
        open_time,
        end_time,
        hashtags,
        price_down,
        ..Default::default()
    })
}

fn normalize_detail_item(raw: &Value) -> Item {
    let id = raw["id"].as_str().unwrap_or_default().to_string();
    let title = raw["title"].as_str().unwrap_or_default().to_string();
    let price = raw["price"].as_i64().map(Price::yen);
    let likes = raw["likeCount"].as_i64();
    let views = raw["pvCount"].as_i64();
    let status = raw["status"].as_str().map(String::from);
    let description = raw["description"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);

    let condition = if let Some(obj) = raw["condition"].as_object() {
        obj.get("text")
            .or(obj.get("key"))
            .and_then(|v| v.as_str())
            .map(String::from)
    } else {
        raw["condition"].as_str().map(String::from)
    };

    let (image, images) = if let Some(imgs) = raw["images"].as_array() {
        let first = imgs
            .first()
            .and_then(|im| im["url"].as_str())
            .map(String::from);
        let all: Vec<String> = imgs
            .iter()
            .filter_map(|im| im["url"].as_str().map(String::from))
            .collect();
        (first, if all.is_empty() { None } else { Some(all) })
    } else {
        (None, None)
    };

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

    let seller = raw["seller"].as_object().and_then(|s| {
        let sid = s.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if sid.is_empty() {
            return None;
        }
        let rating_obj = s.get("rating").and_then(|v| v.as_object());
        Some(SellerInfo {
            id: sid.to_string(),
            nickname: s.get("nickname").and_then(|v| v.as_str()).map(String::from),
            rating: rating_obj
                .and_then(|r| r.get("goodRatio"))
                .and_then(|v| v.as_f64()),
            num_rating: None,
            total_rating: rating_obj
                .and_then(|r| r.get("total"))
                .and_then(|v| v.as_i64()),
        })
    });

    let delivery_method = raw["deliveryMethod"]["name"].as_str().map(String::from);
    let delivery_schedule = raw["deliverySchedule"]["text"].as_str().map(String::from);
    let location = raw["location"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);

    let hashtags = raw["hashtags"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty());

    let specs = raw["specs"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|sp| Spec {
                    name: sp["name"].as_str().unwrap_or_default().to_string(),
                    values: sp["specRefs"]
                        .as_array()
                        .map(|refs| {
                            refs.iter()
                                .filter_map(|r| r["name"].as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty());

    let open_time = raw["openDate"]
        .as_str()
        .map(String::from)
        .or_else(|| raw["openDate"].as_i64().map(|v| v.to_string()));

    Item {
        product_id: id.clone(),
        title,
        url: format!("{BASE}/item/{id}"),
        source: "yahoo_flea".into(),
        price,
        likes,
        views,
        status,
        condition,
        description,
        image,
        images,
        category,
        category_path,
        brand,
        seller,
        delivery_method,
        delivery_schedule,
        location,
        hashtags,
        specs,
        open_time,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_params() {
        assert_eq!(sort_params(""), ("ranking", "desc"));
        assert_eq!(sort_params("price_asc"), ("price", "asc"));
        assert_eq!(sort_params("newest"), ("openTime", "desc"));
    }

    #[test]
    fn test_sort_params_all() {
        assert_eq!(sort_params("price_desc"), ("price", "desc"));
        assert_eq!(sort_params("lowest_price"), ("price", "asc"));
        assert_eq!(sort_params("highest_price"), ("price", "desc"));
        assert_eq!(sort_params("NEWEST"), ("openTime", "desc"));
        assert_eq!(sort_params("new"), ("openTime", "desc"));
        assert_eq!(sort_params("relevance"), ("ranking", "desc"));
        assert_eq!(sort_params("unknown"), ("ranking", "desc"));
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search() {
        let client = YahooFleaClient::new();
        let result = client
            .search(SearchParams {
                query: "iPhone".into(),
                limit: 5,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok, "search must succeed");
        assert_eq!(result.provider, "yahoo_flea");
        assert!(
            !result.items.is_empty(),
            "search returned 0 items — API may be broken"
        );
        assert_eq!(result.count, result.items.len());
        for item in &result.items {
            assert!(!item.product_id.is_empty());
            assert!(
                !item.title.is_empty(),
                "title empty for {}",
                item.product_id
            );
            assert!(
                item.url.contains("paypayfleamarket.yahoo.co.jp"),
                "bad url: {}",
                item.url
            );
            if let Some(price) = &item.price {
                assert_eq!(price.currency, "JPY");
                assert!(price.value > 0);
            }
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_multiword() {
        let client = YahooFleaClient::new();
        let result = client
            .search(SearchParams {
                query: "NVIDIA GPU".into(),
                limit: 3,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            result.ok,
            "multi-word search must succeed (not crash with 404)"
        );
        // May return 0 items if no results, but must not error
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_japanese_multiword() {
        let client = YahooFleaClient::new();
        let result = client
            .search(SearchParams {
                query: "ワイヤレス イヤホン".into(),
                limit: 3,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.ok, "Japanese multi-word search must succeed");
        assert!(
            !result.items.is_empty(),
            "Japanese multi-word search returned 0 items"
        );
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_then_get_item() {
        let client = YahooFleaClient::new();
        let search = client
            .search(SearchParams {
                query: "バッグ".into(),
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
        assert_eq!(detail.provider, "yahoo_flea");
        let item = detail.item.expect("item must be present");
        assert!(!item.title.is_empty());
        if let Some(price) = &item.price {
            assert_eq!(price.currency, "JPY");
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_get_reviews() {
        let client = YahooFleaClient::new();
        // Search for an item to get a seller_id
        let search = client
            .search(SearchParams {
                query: "バッグ".into(),
                limit: 5,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(search.ok);
        assert!(!search.items.is_empty(), "need items to find a seller");

        // Find an item with a seller
        let seller_id = search
            .items
            .iter()
            .find_map(|item| item.seller.as_ref().map(|s| s.id.clone()))
            .expect("no item with seller info found");

        let result = client.get_reviews(&seller_id, 1, 20).await.unwrap();
        assert_eq!(result["provider"], "yahoo_flea");
        assert_eq!(result["ok"], true);
        assert_eq!(result["seller_id"], seller_id);

        let summary = &result["ratings_summary"];
        assert!(
            summary["total"].as_i64().unwrap() >= 0,
            "total must be non-negative"
        );
        assert!(summary["good"].as_i64().unwrap() >= 0);
        assert!(summary["neutral"].as_i64().unwrap() >= 0);
        assert!(summary["bad"].as_i64().unwrap() >= 0);

        if let Some(reviews) = result["reviews"].as_array() {
            for review in reviews {
                assert!(
                    !review["rating"].as_str().unwrap_or("").is_empty(),
                    "rating must not be empty"
                );
                assert!(
                    !review["reviewer_id"].as_str().unwrap_or("").is_empty(),
                    "reviewer_id must not be empty"
                );
            }
        }
    }
}
