mod dpop;
mod proto;

use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use dpop::DpopSigner;

const API_BASE: &str = "https://api.mercari.jp";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

const SEARCH_ENDPOINT: &str = "/v2/entities:search";
const ITEM_GET_ENDPOINT: &str = "/items/get";
const ITEMS_GET_ENDPOINT: &str = "/items/get_items";
const AUTOCOMPLETE_ENDPOINT: &str = "/services/bff/search/v1/autocomplete:search";
const BRANDS_DATASET_ENDPOINT: &str = "/master/v2/datasets/item_brands";
const CATEGORIES_DATASET_ENDPOINT: &str = "/master/v2/datasets/item_categories";
const REVIEWS_ENDPOINT: &str = "/reviews/history";

static SORT_ENUMS: &[&str] = &[
    "SORT_DEFAULT",
    "SORT_CREATED_TIME",
    "SORT_NUM_LIKES",
    "SORT_SCORE",
    "SORT_PRICE",
    "SORT_SIMILARITY",
    "SORT_PRICE_WITHOUT_TIME_FILTER",
];
static ORDER_ENUMS: &[&str] = &["ORDER_DESC", "ORDER_ASC"];
static STATUS_ENUMS: &[&str] = &[
    "STATUS_UNSPECIFIED",
    "STATUS_ON_SALE",
    "STATUS_TRADING",
    "STATUS_SOLD_OUT",
];
#[allow(dead_code)]
static ITEM_TYPE_ENUMS: &[&str] = &["ITEM_TYPE_MERCARI", "ITEM_TYPE_BEYOND"];

fn enum_at<'a>(table: &'a [&'a str], idx: usize) -> &'a str {
    table.get(idx).unwrap_or(&table[0])
}

fn search_sort(s: &str) -> (usize, usize) {
    match s.to_lowercase().as_str() {
        "" | "default" | "recommended" => (0, 0),
        "score" | "relevance" => (3, 0),
        "newest" | "created_desc" => (1, 0),
        "oldest" | "created_asc" => (1, 1),
        "liked" | "likes" | "num_likes" => (2, 0),
        "lowest_price" | "price_asc" => (4, 1),
        "highest_price" | "price_desc" => (4, 0),
        "similarity" => (5, 0),
        _ => (0, 0),
    }
}

fn search_status_value(s: &str) -> Option<usize> {
    match s.to_lowercase().as_str() {
        "" | "default" | "all" => None,
        "on_sale" | "onsale" | "selling" => Some(1),
        "trading" => Some(2),
        "sold_out" | "sold" => Some(3),
        _ => None,
    }
}

fn item_status_label(s: &str) -> &'static str {
    match s {
        "ITEM_STATUS_ON_SALE" => "on_sale",
        "ITEM_STATUS_TRADING" => "trading",
        "ITEM_STATUS_SOLD_OUT" => "sold_out",
        "ITEM_STATUS_STOP" => "stop",
        "ITEM_STATUS_CANCEL" => "cancel",
        "ITEM_STATUS_ADMIN_CANCEL" => "admin_cancel",
        _ => "unspecified",
    }
}

fn item_type_label(s: &str) -> &'static str {
    match s {
        "ITEM_TYPE_MERCARI" => "mercari",
        "ITEM_TYPE_BEYOND" => "beyond",
        _ => "",
    }
}

const MERCARI_AFID: &str = "6694470352";

fn item_url(id: &str) -> String {
    if id.is_empty() {
        String::new()
    } else {
        format!("https://jp.mercari.com/item/{id}?afid={MERCARI_AFID}")
    }
}

fn to_int(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn split_values(v: &str) -> Vec<String> {
    v.split(&[',', ';', ' '][..])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn int_list(v: &str) -> Vec<i64> {
    split_values(v)
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub query: String,
    pub limit: i32,
    pub page_token: String,
    pub sort: String,
    pub status: String,
    pub exclude_keyword: String,
    pub category_ids: String,
    pub brand_ids: String,
    pub size_ids: String,
    pub seller_ids: String,
    pub price_min: Option<i64>,
    pub price_max: Option<i64>,
    pub item_condition_ids: String,
    pub shipping_payer_ids: String,
    pub color_ids: String,
    pub item_types: String,
    pub exclude_auction: bool,
}

pub struct MercariClient {
    client: Client,
    dpop: Arc<Mutex<DpopSigner>>,
}

impl MercariClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent(USER_AGENT)
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .expect("failed to build HTTP client"),
            dpop: Arc::new(Mutex::new(DpopSigner::new())),
        }
    }

    pub async fn search_products(&self, params: SearchParams) -> Result<Value> {
        let limit = params.limit.clamp(1, 120);
        let session_id = uuid::Uuid::new_v4().to_string();
        let (sort_idx, order_idx) = search_sort(&params.sort);

        let mut status_list = vec![];
        if let Some(sv) = search_status_value(&params.status) {
            status_list.push(enum_at(STATUS_ENUMS, sv));
        }

        let mut item_type_list = vec![];
        for t in split_values(&params.item_types) {
            match t.to_lowercase().as_str() {
                "mercari" | "item" => item_type_list.push("ITEM_TYPE_MERCARI"),
                "beyond" | "shops" | "shop" => item_type_list.push("ITEM_TYPE_BEYOND"),
                _ => {}
            }
        }

        let condition = json!({
            "keyword": params.query,
            "excludeKeyword": params.exclude_keyword,
            "sort": enum_at(SORT_ENUMS, sort_idx),
            "order": enum_at(ORDER_ENUMS, order_idx),
            "status": status_list,
            "sizeId": split_values(&params.size_ids),
            "categoryId": int_list(&params.category_ids),
            "brandId": int_list(&params.brand_ids),
            "sellerId": split_values(&params.seller_ids),
            "priceMin": params.price_min.unwrap_or(0),
            "priceMax": params.price_max.unwrap_or(0),
            "itemConditionId": int_list(&params.item_condition_ids),
            "shippingPayerId": int_list(&params.shipping_payer_ids),
            "colorId": int_list(&params.color_ids),
            "hasCoupon": false,
            "attributes": [],
            "itemTypes": item_type_list,
            "skuIds": [],
            "shopIds": [],
            "excludeShippingMethodIds": [],
            "shippingFromArea": [],
            "shippingMethod": [],
        });

        let request = json!({
            "userId": "",
            "config": {"responseToggles": []},
            "pageSize": limit,
            "pageToken": params.page_token,
            "searchSessionId": session_id,
            "source": "BaseSerp",
            "searchCondition": condition,
            "serviceFrom": "suruga",
            "withItemBrand": true,
            "withItemSizes": true,
            "withItemPromotions": true,
            "withAuction": true,
            "useDynamicAttribute": true,
        });

        let body = self.post_json(SEARCH_ENDPOINT, &request).await?;
        let items = normalize_search_response(&body);
        let items = filter_exclude_auction(items, params.exclude_auction);

        Ok(json!({
            "provider": "mercari",
            "ok": true,
            "query": params.query,
            "limit": limit,
            "sort": params.sort,
            "exclude_auction": params.exclude_auction,
            "search_session_id": session_id,
            "items": items,
            "count": items.len(),
            "next_page_token": body["meta"]["nextPageToken"].as_str().unwrap_or(""),
            "num_found": to_int(&body["meta"]["numFound"]),
        }))
    }

    pub async fn get_product(&self, product: &str) -> Result<Value> {
        let item_id = extract_item_id(product);
        if item_id.is_empty() {
            anyhow::bail!("Could not find a Mercari item id in: {product}");
        }

        let url = format!("{API_BASE}{ITEM_GET_ENDPOINT}");
        let params = [
            ("id", item_id.as_str()),
            ("include_item_attributes", "true"),
            ("include_product_page_component", "true"),
            ("include_non_ui_item_attributes", "true"),
            ("include_auction", "true"),
        ];

        let dpop_proof = {
            let full_url = reqwest::Url::parse_with_params(&url, &params)?;
            let signer = self.dpop.lock().await;
            signer.proof("GET", full_url.as_ref())
        };

        let resp = self
            .client
            .get(&url)
            .query(&params)
            .header("DPoP", dpop_proof)
            .header("X-Platform", "web")
            .header("Accept-Language", "ja-JP")
            .header("Accept", "application/json")
            .send()
            .await?
            .error_for_status()?;

        let envelope: Value = resp.json().await?;
        let data = if envelope["data"].is_object() {
            &envelope["data"]
        } else {
            &envelope
        };
        let item = normalize_item_detail(data);

        Ok(json!({
            "provider": "mercari",
            "ok": true,
            "item": item,
        }))
    }

    pub async fn search_seller_products(
        &self,
        seller_id: &str,
        status: &str,
        limit: i32,
    ) -> Result<Value> {
        let url = format!("{API_BASE}{ITEMS_GET_ENDPOINT}");
        let limit = limit.clamp(1, 100);
        let params = [
            ("seller_id", seller_id.to_string()),
            ("status", status.to_string()),
            ("limit", limit.to_string()),
            ("with_auction", "true".to_string()),
            ("with_total_item_count", "true".to_string()),
        ];

        let dpop_proof = {
            let full_url = reqwest::Url::parse_with_params(&url, &params)?;
            let signer = self.dpop.lock().await;
            signer.proof("GET", full_url.as_ref())
        };

        let resp = self
            .client
            .get(&url)
            .query(&params)
            .header("DPoP", dpop_proof)
            .header("X-Platform", "web")
            .header("Accept-Language", "ja-JP")
            .header("Accept", "application/json")
            .send()
            .await?
            .error_for_status()?;

        let data: Value = resp.json().await?;
        let items: Vec<Value> = data["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        if !v.is_object() {
                            return None;
                        }
                        Some(normalize_item_summary(v))
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(json!({
            "provider": "mercari",
            "ok": true,
            "seller_id": seller_id,
            "status": status,
            "items": items,
            "count": items.len(),
            "meta": data.get("meta"),
        }))
    }

    pub async fn suggest_keywords(&self, keyword: &str) -> Result<Value> {
        let request = json!({
            "keyword": keyword,
            "requestId": uuid::Uuid::new_v4().to_string(),
            "searchSessionId": uuid::Uuid::new_v4().to_string(),
        });
        let body = self.post_json(AUTOCOMPLETE_ENDPOINT, &request).await?;
        let suggestions: Vec<Value> = body["suggestions"]
            .as_array()
            .map(|arr| arr.iter().map(normalize_autocomplete).collect())
            .unwrap_or_default();

        Ok(json!({
            "provider": "mercari",
            "ok": true,
            "keyword": keyword,
            "suggestions": suggestions,
        }))
    }

    pub async fn search_brands(&self, query: &str, limit: i32) -> Result<Value> {
        let data = self.get_json(BRANDS_DATASET_ENDPOINT).await?;
        let rows = data["itemBrands"].as_array();
        let limit = limit.clamp(1, 100) as usize;
        let needle = query.to_lowercase();

        let brands: Vec<Value> = rows
            .map(|arr| {
                arr.iter()
                    .filter(|row| {
                        if needle.is_empty() {
                            return true;
                        }
                        let haystack = format!(
                            "{} {} {} {}",
                            row["name"].as_str().unwrap_or(""),
                            row["subname"].as_str().unwrap_or(""),
                            row["initial"].as_str().unwrap_or(""),
                            row["jaPronunciation"].as_str().unwrap_or(""),
                        )
                        .to_lowercase();
                        haystack.contains(&needle)
                    })
                    .take(limit)
                    .map(|row| {
                        json!({
                            "id": to_int(&row["id"]),
                            "name": row["name"],
                            "official_name": row["subname"],
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(json!({
            "provider": "mercari",
            "ok": true,
            "query": query,
            "brands": brands,
        }))
    }

    pub async fn search_categories(&self, query: &str, limit: i32) -> Result<Value> {
        let data = self.get_json(CATEGORIES_DATASET_ENDPOINT).await?;
        let rows = data["itemCategories"].as_array();
        let limit = limit.clamp(1, 100) as usize;
        let needle = query.to_lowercase();

        let categories: Vec<Value> = rows
            .map(|arr| {
                arr.iter()
                    .filter(|row| {
                        if needle.is_empty() {
                            return true;
                        }
                        let name = row["name"].as_str().unwrap_or("").to_lowercase();
                        name.contains(&needle)
                    })
                    .take(limit)
                    .map(|row| {
                        json!({
                            "id": to_int(&row["id"]),
                            "name": row["name"],
                            "is_leaf": !row["hasChild"].as_bool().unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(json!({
            "provider": "mercari",
            "ok": true,
            "query": query,
            "categories": categories,
        }))
    }

    pub async fn get_reviews(&self, seller_id: &str, _page: i32, limit: i32) -> Result<Value> {
        let limit = limit.clamp(1, 100);
        let url = format!("{API_BASE}{REVIEWS_ENDPOINT}");
        let params = [
            ("user_id", seller_id.to_string()),
            ("fame", "good,normal,bad".to_string()),
            ("limit", limit.to_string()),
        ];

        let dpop_proof = {
            let full_url = reqwest::Url::parse_with_params(&url, &params)?;
            let signer = self.dpop.lock().await;
            signer.proof("GET", full_url.as_ref())
        };

        let resp = self
            .client
            .get(&url)
            .query(&params)
            .header("DPoP", dpop_proof)
            .header("X-Platform", "web")
            .header("Accept-Language", "ja-JP")
            .header("Accept", "application/json")
            .send()
            .await?
            .error_for_status()?;

        let body: Value = resp.json().await?;
        let meta = &body["meta"];

        let reviews: Vec<Value> = body["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| {
                        if !r.is_object() {
                            return None;
                        }
                        Some(json!({
                            "rating": r["fame"].as_str().unwrap_or("unknown"),
                            "message": r["message"].as_str().unwrap_or(""),
                            "reviewer_name": r["user"]["name"].as_str().unwrap_or(""),
                            "reviewer_id": r["user"]["id"].as_i64()
                                .map(|id| id.to_string())
                                .or_else(|| r["user"]["id"].as_str().map(|s| s.to_string()))
                                .unwrap_or_default(),
                            "role": r["subject"].as_str().unwrap_or(""),
                            "date": to_int(&r["created"]),
                        }))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let ratings = &meta["ratings"];
        let good = ratings["good"].as_i64().unwrap_or(0);
        let normal = ratings["normal"].as_i64().unwrap_or(0);
        let bad = ratings["bad"].as_i64().unwrap_or(0);

        Ok(json!({
            "provider": "mercari",
            "ok": true,
            "seller_id": seller_id,
            "ratings_summary": {
                "good": good,
                "normal": normal,
                "bad": bad,
                "total": meta["num_ratings"].as_i64().unwrap_or(good + normal + bad),
                "star_rating": meta["star_rating_score"].as_i64().unwrap_or(0),
            },
            "reviews": reviews,
            "has_next": meta["has_next"].as_bool().unwrap_or(false),
            "count": reviews.len(),
        }))
    }

    async fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        let url = format!("{API_BASE}{path}");
        let dpop_proof = {
            let signer = self.dpop.lock().await;
            signer.proof("POST", &url)
        };

        let resp = self
            .client
            .post(&url)
            .header("DPoP", dpop_proof)
            .header("X-Platform", "web")
            .header("Accept-Language", "ja-JP")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await?
            .error_for_status()?;

        Ok(resp.json().await?)
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let url = format!("{API_BASE}{path}");
        let dpop_proof = {
            let signer = self.dpop.lock().await;
            signer.proof("GET", &url)
        };

        let resp = self
            .client
            .get(&url)
            .header("DPoP", dpop_proof)
            .header("X-Platform", "web")
            .header("Accept-Language", "ja-JP")
            .header("Accept", "application/json")
            .send()
            .await?
            .error_for_status()?;

        Ok(resp.json().await?)
    }
}

impl Default for MercariClient {
    fn default() -> Self {
        Self::new()
    }
}

pub fn extract_item_id(value: &str) -> String {
    let text = value.trim();
    // m + digits (classic format)
    let re = Regex::new(r"\bm\d{6,}\b").unwrap();
    if let Some(m) = re.find(text) {
        return m.as_str().to_string();
    }
    // bare digits → prepend m
    let digits_re = Regex::new(r"^\d{6,}$").unwrap();
    if digits_re.is_match(text) {
        return format!("m{text}");
    }
    // URL with /item/{hash} path
    if text.contains("mercari.") && text.contains("/item/") {
        if let Some(seg) = text.rsplit("/item/").next() {
            let id = seg.split(&['?', '#', '/'][..]).next().unwrap_or("");
            if id.len() >= 6 {
                return id.to_string();
            }
        }
    }
    // alphanumeric hash ID (new format, 20+ chars)
    let hash_re = Regex::new(r"^[A-Za-z0-9]{20,}$").unwrap();
    if hash_re.is_match(text) {
        return text.to_string();
    }
    String::new()
}

/// Normalize auction data from either upstream shape into one object.
///
/// Search results carry `auction` (camelCase, string values, only populated
/// when the search request sets `withAuction: true`); item detail and seller
/// item lists carry `auction_info` (snake_case). Detail uses
/// `total_bids` + `expected_end_time` (epoch seconds), seller lists use
/// `total_bid` + `bid_deadline` (ISO 8601) — both deadline fields describe
/// the same instant. Returns `None` for non-auction items.
fn normalize_auction(raw: &Value) -> Option<Value> {
    let obj = raw.as_object()?;
    let get = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| obj.get(*k))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let id = get(&["id"]).as_str().unwrap_or_default().to_string();
    Some(json!({
        "id": if id.is_empty() { Value::Null } else { json!(id) },
        "highest_bid": to_int(&get(&["highestBid", "highest_bid"])),
        "initial_price": to_int(&get(&["initialPrice", "initial_price"])),
        "total_bids": to_int(&get(&["totalBid", "total_bids", "total_bid"])),
        "bid_deadline": get(&["bidDeadline", "bid_deadline"]),
        "expected_end_time": to_int(&get(&["expected_end_time"])),
        "start_time": to_int(&get(&["start_time"])),
        "state": get(&["state"]),
        "auction_type": get(&["auction_type"]),
    }))
}

fn filter_exclude_auction(items: Vec<Value>, exclude_auction: bool) -> Vec<Value> {
    if !exclude_auction {
        return items;
    }
    items
        .into_iter()
        .filter(|item| item["is_auction"].as_bool() != Some(true))
        .collect()
}

fn normalize_search_response(data: &Value) -> Vec<Value> {
    data["items"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    if !item.is_object() {
                        return None;
                    }
                    Some(normalize_search_item(item))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_search_item(item: &Value) -> Value {
    let id = item["id"].as_str().unwrap_or_default();
    let status = item["status"].as_str().unwrap_or_default();
    let item_type = item["itemType"].as_str().unwrap_or_default();
    let photos: Vec<&str> = item["photos"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let thumbnails: Vec<&str> = item["thumbnails"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let image = photos
        .first()
        .or(thumbnails.first())
        .copied()
        .unwrap_or_default();
    let auction = normalize_auction(&item["auction"]);

    let mut out = json!({
        "product_id": id,
        "item_id": id,
        "url": item_url(id),
        "title": item.get("title").or(item.get("name")),
        "name": item["name"],
        "price": to_int(&item["price"]),
        "is_auction": auction.is_some(),
        "auction": auction,
        "status": item_status_label(status),
        "seller_id": item["sellerId"],
        "created": to_int(&item["created"]),
        "updated": to_int(&item["updated"]),
        "thumbnails": thumbnails,
        "photos": photos,
        "image": image,
        "item_type": item_type_label(item_type),
        "item_condition_id": to_int(&item["itemConditionId"]),
        "shipping_payer_id": to_int(&item["shippingPayerId"]),
        "category_id": to_int(&item["categoryId"]),
        "source": "mercari",
    });

    if let Some(brand) = item.get("itemBrand").filter(|v| v.is_object()) {
        out["brand"] = json!({
            "id": to_int(&brand["id"]),
            "name": brand["name"],
            "sub_name": brand["subName"],
        });
    }

    out
}

fn normalize_item_detail(data: &Value) -> Value {
    let id = data["id"].as_str().unwrap_or_default();
    let photos: Vec<&str> = data["photos"]
        .as_array()
        .or(data["photo_urls"].as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let thumbnails: Vec<&str> = data["thumbnails"]
        .as_array()
        .or(data["thumbnail_urls"].as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let image = photos
        .first()
        .or(thumbnails.first())
        .copied()
        .unwrap_or_default();
    let auction = normalize_auction(&data["auction_info"]);

    json!({
        "product_id": id,
        "item_id": id,
        "url": item_url(id),
        "title": data["name"],
        "description": data["description"],
        "price": data["price"],
        "is_auction": auction.is_some(),
        "auction": auction,
        "status": data["status"],
        "category": data.get("item_category").or(data.get("category")),
        "condition": data.get("item_condition").or(data.get("condition")),
        "brand": data.get("item_brand").or(data.get("brand")),
        "photos": photos,
        "thumbnails": thumbnails,
        "image": image,
        "like_count": data.get("num_likes").or(data.get("like_count")),
        "comment_count": data.get("num_comments").or(data.get("comment_count")),
        "seller": data["seller"],
        "shipping": {
            "from_area": data["shipping_from_area"],
            "method": data["shipping_method"],
            "payer": data["shipping_payer"],
            "duration": data["shipping_duration"],
        },
        "created": data["created"],
        "updated": data["updated"],
        "source": "mercari",
    })
}

fn normalize_item_summary(data: &Value) -> Value {
    let id = data["id"]
        .as_str()
        .or(data["item_id"].as_str())
        .unwrap_or_default();
    let photos: Vec<&str> = data["photo_urls"]
        .as_array()
        .or(data["thumbnail_urls"].as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let auction = normalize_auction(&data["auction_info"]);

    json!({
        "product_id": id,
        "item_id": id,
        "url": item_url(id),
        "title": data.get("name").or(data.get("title")),
        "price": data["price"],
        "is_auction": auction.is_some(),
        "auction": auction,
        "status": data["status"],
        "image": photos.first().copied().unwrap_or(""),
        "seller_id": data["seller_id"],
    })
}

fn normalize_autocomplete(s: &Value) -> Value {
    if let Some(kw) = s.get("keyword").filter(|v| v.is_object()) {
        return json!({
            "type": "keyword",
            "title": kw["title"],
            "subtitle": kw["subtitle"],
        });
    }
    if let Some(seller) = s.get("seller").filter(|v| v.is_object()) {
        return json!({
            "type": "seller",
            "user_id": seller["id"],
            "display_name": seller["name"],
        });
    }
    json!({"type": "unknown"})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_item_id() {
        assert_eq!(extract_item_id("m1234567890"), "m1234567890");
        assert_eq!(extract_item_id("1234567890"), "m1234567890");
        assert_eq!(
            extract_item_id("https://jp.mercari.com/item/m1234567890"),
            "m1234567890"
        );
        // new hash format
        assert_eq!(
            extract_item_id("2JTNgESdSinuH7PX7iJk2w"),
            "2JTNgESdSinuH7PX7iJk2w"
        );
        assert_eq!(
            extract_item_id("https://jp.mercari.com/item/2JTNgESdSinuH7PX7iJk2w"),
            "2JTNgESdSinuH7PX7iJk2w"
        );
        assert_eq!(extract_item_id("short"), "");
    }

    #[test]
    fn test_search_sort() {
        assert_eq!(search_sort(""), (0, 0));
        assert_eq!(search_sort("newest"), (1, 0));
        assert_eq!(search_sort("price_asc"), (4, 1));
        assert_eq!(search_sort("price_desc"), (4, 0));
    }

    #[test]
    fn test_extract_item_id_url_variants() {
        assert_eq!(
            extract_item_id("https://jp.mercari.com/item/m1234567890?x=1"),
            "m1234567890"
        );
        assert_eq!(
            extract_item_id("https://mercari.com/jp/items/m9876543210/"),
            "m9876543210"
        );
    }

    #[test]
    fn test_extract_item_id_edge_cases() {
        assert_eq!(extract_item_id(""), "");
        assert_eq!(extract_item_id("abc"), "");
        assert_eq!(extract_item_id("m123"), "");
    }

    #[test]
    fn test_normalize_search_item_auction() {
        // Shape measured live 2026-11-05 (m62825700406): search `auction`
        // is populated only when the request sets withAuction: true.
        let item = json!({
            "id": "m62825700406",
            "name": "Apple iPhone 14 Pro 256GB",
            "price": "30200",
            "status": "ITEM_STATUS_ON_SALE",
            "itemType": "ITEM_TYPE_MERCARI",
            "auction": {
                "id": "",
                "bidDeadline": "2026-11-05T11:11:00Z",
                "highestBid": "30200",
                "initialPrice": "10000",
                "totalBid": "29"
            }
        });
        let out = normalize_search_item(&item);
        assert_eq!(out["is_auction"], true);
        assert_eq!(out["auction"]["highest_bid"], 30200);
        assert_eq!(out["auction"]["initial_price"], 10000);
        assert_eq!(out["auction"]["total_bids"], 29);
        assert_eq!(out["auction"]["bid_deadline"], "2026-11-05T11:11:00Z");
        assert!(
            out["auction"]["id"].is_null(),
            "empty search auction id must become null"
        );
    }

    #[test]
    fn test_normalize_search_item_not_auction() {
        let item = json!({
            "id": "m45533881258",
            "name": "通常出品",
            "price": "40000",
            "status": "ITEM_STATUS_ON_SALE",
            "itemType": "ITEM_TYPE_MERCARI",
            "auction": null
        });
        let out = normalize_search_item(&item);
        assert_eq!(out["is_auction"], false);
        assert!(out["auction"].is_null());
    }

    #[test]
    fn test_normalize_item_detail_auction() {
        // Detail shape measured live 2026-11-05 (m62825700406).
        let data = json!({
            "id": "m62825700406",
            "name": "Apple iPhone 14 Pro 256GB",
            "price": 30200,
            "status": "on_sale",
            "auction_info": {
                "id": "110329327",
                "start_time": 1793784237_i64,
                "expected_end_time": 1793877060_i64,
                "total_bids": 29,
                "initial_price": 10000,
                "highest_bid": 30200,
                "state": "STATE_ONGOING",
                "auction_type": "AUCTION_TYPE_NORMAL"
            }
        });
        let out = normalize_item_detail(&data);
        assert_eq!(out["is_auction"], true);
        assert_eq!(out["auction"]["id"], "110329327");
        assert_eq!(out["auction"]["highest_bid"], 30200);
        assert_eq!(out["auction"]["total_bids"], 29);
        assert_eq!(out["auction"]["expected_end_time"], 1793877060_i64);
        assert_eq!(out["auction"]["state"], "STATE_ONGOING");
    }

    #[test]
    fn test_normalize_item_detail_not_auction() {
        let data =
            json!({"id": "m45533881258", "name": "通常出品", "price": 40000, "status": "on_sale"});
        let out = normalize_item_detail(&data);
        assert_eq!(out["is_auction"], false);
        assert!(out["auction"].is_null());
    }

    #[test]
    fn test_normalize_item_summary_auction_seller_shape() {
        // Seller-list shape measured live 2026-11-05: total_bid (singular)
        // and bid_deadline (ISO), unlike the detail shape.
        let data = json!({
            "id": "m62825700406",
            "name": "Apple iPhone 14 Pro 256GB",
            "price": 30200,
            "status": "on_sale",
            "auction_info": {
                "id": "110329327",
                "highest_bid": 30200,
                "initial_price": 10000,
                "total_bid": 29,
                "bid_deadline": "2026-11-05T11:11:00Z"
            }
        });
        let out = normalize_item_summary(&data);
        assert_eq!(out["is_auction"], true);
        assert_eq!(out["auction"]["total_bids"], 29);
        assert_eq!(out["auction"]["bid_deadline"], "2026-11-05T11:11:00Z");
    }

    #[test]
    fn test_filter_exclude_auction() {
        let items = vec![
            json!({"product_id": "a", "is_auction": true}),
            json!({"product_id": "b", "is_auction": false}),
            json!({"product_id": "c", "is_auction": true}),
        ];
        let kept = filter_exclude_auction(items.clone(), true);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0]["product_id"], "b");
        assert_eq!(filter_exclude_auction(items, false).len(), 3);
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search() {
        let client = MercariClient::new();
        let result = client
            .search_products(SearchParams {
                query: "iPhone".into(),
                limit: 5,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result["provider"], "mercari");
        assert_eq!(result["ok"], true, "search failed: {}", result);
        let items = result["items"].as_array().unwrap();
        assert!(
            !items.is_empty(),
            "search returned 0 items — API may be broken"
        );
        let count = result["count"].as_i64().unwrap();
        assert_eq!(count as usize, items.len());
        for item in items {
            let pid = item["product_id"].as_str().unwrap();
            assert!(!pid.is_empty());
            let url = item["url"].as_str().unwrap();
            assert!(url.contains("mercari.com"), "bad url: {}", url);
            assert!(url.contains("afid="), "missing afid: {}", url);
            assert!(
                item["price"].as_i64().is_some() || item["price"].is_object(),
                "price missing: {:?}",
                item["price"]
            );
            assert_eq!(item["source"], "mercari");
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_multiword() {
        let client = MercariClient::new();
        let result = client
            .search_products(SearchParams {
                query: "ワイヤレス イヤホン Bluetooth".into(),
                limit: 3,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result["ok"], true, "multi-word search failed: {}", result);
        let items = result["items"].as_array().unwrap();
        assert!(!items.is_empty(), "multi-word search returned 0 items");
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_sort_price() {
        let client = MercariClient::new();
        let result = client
            .search_products(SearchParams {
                query: "カメラ".into(),
                limit: 5,
                sort: "price_asc".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result["ok"], true, "sorted search failed: {}", result);
        let items = result["items"].as_array().unwrap();
        assert!(!items.is_empty(), "sorted search returned 0 items");
        let prices: Vec<i64> = items.iter().filter_map(|i| i["price"].as_i64()).collect();
        if prices.len() >= 2 {
            for w in prices.windows(2) {
                assert!(w[0] <= w[1], "prices not ascending: {} > {}", w[0], w[1]);
            }
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_auction_fields() {
        let client = MercariClient::new();
        let result = client
            .search_products(SearchParams {
                query: "iPhone 14 Pro".into(),
                limit: 120,
                sort: "newest".into(),
                status: "on_sale".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result["ok"], true, "search failed: {}", result);
        let items = result["items"].as_array().unwrap();
        assert!(!items.is_empty(), "search returned 0 items");
        for item in items {
            assert!(
                item["is_auction"].is_boolean(),
                "is_auction missing: {:?}",
                item
            );
            if item["is_auction"] == true {
                assert!(
                    item["auction"]["highest_bid"].is_i64(),
                    "auction highest_bid missing: {:?}",
                    item
                );
                assert!(
                    item["auction"]["total_bids"].is_i64(),
                    "auction total_bids missing: {:?}",
                    item
                );
            } else {
                assert!(
                    item["auction"].is_null(),
                    "non-auction item must have null auction: {:?}",
                    item
                );
            }
        }
        let auction_item = items.iter().find(|i| i["is_auction"] == true)
            .expect("no auction item found in 120 newest iPhone 14 Pro results — auction population may have regressed");
        // Cross-check: the same item must also be flagged in detail.
        let pid = auction_item["product_id"].as_str().unwrap();
        let detail = client.get_product(pid).await.unwrap();
        assert_eq!(detail["ok"], true, "get_product failed: {}", detail);
        assert_eq!(
            detail["item"]["is_auction"], true,
            "detail disagrees with search for auction item {}: {}",
            pid, detail["item"]
        );
        assert!(detail["item"]["auction"]["total_bids"].is_i64());
        assert!(detail["item"]["auction"]["expected_end_time"].is_i64());
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_exclude_auction() {
        let client = MercariClient::new();
        let result = client
            .search_products(SearchParams {
                query: "iPhone 14 Pro".into(),
                limit: 40,
                sort: "newest".into(),
                status: "on_sale".into(),
                exclude_auction: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result["ok"], true, "search failed: {}", result);
        assert_eq!(result["exclude_auction"], true);
        let items = result["items"].as_array().unwrap();
        assert!(!items.is_empty(), "exclude_auction search returned 0 items");
        for item in items {
            assert_eq!(
                item["is_auction"], false,
                "auction item leaked through exclude_auction: {:?}",
                item
            );
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_get_product() {
        let client = MercariClient::new();
        let search = client
            .search_products(SearchParams {
                query: "バッグ".into(),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(search["ok"], true);
        let items = search["items"].as_array().unwrap();
        assert!(!items.is_empty(), "need items to test get_product");
        // Find an m-format ID
        let m_item = items.iter().find(|i| {
            i["product_id"]
                .as_str()
                .is_some_and(|id| id.starts_with('m'))
        });
        assert!(
            m_item.is_some(),
            "no m-format product_id found in {} items — Mercari may have changed ID format",
            items.len()
        );
        let pid = m_item.unwrap()["product_id"].as_str().unwrap();
        let detail = client.get_product(pid).await.unwrap();
        assert_eq!(detail["provider"], "mercari");
        assert_eq!(
            detail["ok"], true,
            "get_product FAILED for {}: {}",
            pid, detail
        );
        let item = &detail["item"];
        assert!(item.is_object(), "item must be object");
        assert!(!item["product_id"].as_str().unwrap_or("").is_empty());
        assert!(item["url"].as_str().unwrap().contains("mercari.com"));
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_suggest_keywords() {
        let client = MercariClient::new();
        let result = client.suggest_keywords("iPhone").await.unwrap();
        assert_eq!(result["provider"], "mercari");
        assert_eq!(result["ok"], true, "suggest failed: {}", result);
        let suggestions = result["suggestions"]
            .as_array()
            .expect("suggestions must be array");
        assert!(!suggestions.is_empty(), "suggest returned 0 suggestions");
        for s in suggestions {
            assert!(s.is_object(), "suggestion must be object: {:?}", s);
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_search_empty_query() {
        let client = MercariClient::new();
        let result = client
            .search_products(SearchParams {
                query: "".into(),
                limit: 5,
                ..Default::default()
            })
            .await;
        // Mercari returns 400 for empty query — this must not panic
        match result {
            Ok(v) => assert_eq!(v["provider"], "mercari"),
            Err(e) => assert!(
                e.to_string().contains("400") || e.to_string().contains("Bad Request"),
                "unexpected error: {}",
                e
            ),
        }
    }

    #[tokio::test]
    #[ignore = "live test: run with --ignored"]
    async fn test_live_get_reviews() {
        let client = MercariClient::new();
        // Search to find a seller_id
        let search = client
            .search_products(SearchParams {
                query: "バッグ".into(),
                limit: 5,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(search["ok"], true);
        let items = search["items"].as_array().unwrap();
        assert!(!items.is_empty(), "need items to find a seller_id");

        let seller_id = items
            .iter()
            .find_map(|i| {
                let v = &i["seller_id"];
                if let Some(s) = v.as_str() {
                    if !s.is_empty() && s != "0" {
                        return Some(s.to_string());
                    }
                }
                if let Some(n) = v.as_i64() {
                    if n > 0 {
                        return Some(n.to_string());
                    }
                }
                None
            })
            .expect("no seller_id found in search results");

        let result = client.get_reviews(&seller_id, 1, 5).await.unwrap();
        assert_eq!(result["provider"], "mercari");
        assert_eq!(result["ok"], true, "get_reviews failed: {}", result);
        assert!(
            result["ratings_summary"].is_object(),
            "ratings_summary must be object"
        );
        assert!(result["reviews"].is_array(), "reviews must be array");
        let reviews = result["reviews"].as_array().unwrap();
        assert!(result["count"].as_i64().unwrap() as usize == reviews.len());
        for r in reviews {
            assert!(r["rating"].is_string(), "rating must be string: {:?}", r);
            assert!(r["message"].is_string(), "message must be string: {:?}", r);
        }
    }
}
