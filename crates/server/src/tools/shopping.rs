use anyhow::Result;
use serde_json::{json, Value};

use crate::Providers;

fn str_param(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn product_param(args: &Value) -> String {
    let p = str_param(args, "product");
    if p.is_empty() {
        str_param(args, "product_id")
    } else {
        p
    }
}

fn str_or(args: &Value, key: &str, default: &str) -> String {
    let s = str_param(args, key);
    if s.is_empty() {
        default.to_string()
    } else {
        s
    }
}

fn int_param(args: &Value, key: &str, default: i32) -> i32 {
    args.get(key)
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .unwrap_or(default)
}

fn opt_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}

fn bool_param(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn opt_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

fn opt_f64(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(|v| v.as_f64())
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    let s = str_param(args, key);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn str_array(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        // ── AliExpress ──
        tool_def("aliexpress_search_products", "Search AliExpress products through the APK-derived direct MTop search endpoint.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "page": {"type": "integer", "default": 1},
                "limit": {"type": "integer", "default": 10},
                "locale": {"type": "string", "default": "ja_JP"},
                "currency": {"type": "string", "default": "JPY"},
                "ship_to": {"type": "string", "default": "JP"},
                "sort": {"type": "string", "default": ""}
            },
            "required": ["query"]
        })),
        tool_def("aliexpress_get_product", "Fetch normalized AliExpress product details by product id or product URL.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Product ID or URL"},
                "locale": {"type": "string", "default": "ja_JP"},
                "currency": {"type": "string", "default": "JPY"},
                "ship_to": {"type": "string", "default": "JP"}
            },
            "required": ["product"]
        })),
        tool_def("aliexpress_suggest_keywords", "Get AliExpress keyword suggestions through the recommend MTop endpoint (measured working; the APK suggestion API cannot be invoked through the H5 gateway).", json!({
            "type": "object",
            "properties": {
                "keyword": {"type": "string"},
                "locale": {"type": "string", "default": "ja_JP"},
                "limit": {"type": "integer", "default": 10}
            },
            "required": ["keyword"]
        })),
        tool_def("aliexpress_get_reviews", "Fetch AliExpress product reviews through the APK-derived MTop review endpoint.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Product ID or URL"},
                "page": {"type": "integer", "default": 1},
                "limit": {"type": "integer", "default": 20},
                "locale": {"type": "string", "default": "ja_JP"}
            },
            "required": ["product"]
        })),
        tool_def("aliexpress_search_store_products", "Search products in an AliExpress store through the captured Android store MTop endpoint. seller_id is required (measured: omitting it fails with FAIL_SYS_BIZPARAM_MISSED).", json!({
            "type": "object",
            "properties": {
                "store_id": {"type": "string"},
                "seller_id": {"type": "string", "description": "Required"},
                "page": {"type": "integer", "default": 1},
                "limit": {"type": "integer", "default": 20},
                "sort": {"type": "string", "default": "orders_desc"},
                "locale": {"type": "string", "default": "ja_JP"},
                "currency": {"type": "string", "default": "JPY"},
                "ship_to": {"type": "string", "default": "JP"}
            },
            "required": ["store_id", "seller_id"]
        })),

        // ── Goofish (闲鱼 / Xianyu) ──
        tool_def("goofish_search_products", "Search Goofish (Xianyu) products through the APK-derived MTOP search endpoint. Requests are signed by the SecurityGuard runtime bundled with the server (auto-discovered and auto-started); the device identity is generated by the server itself. No configuration is required. The signing harness boots in the background when the server starts; a Goofish call made before the boot finishes waits for it (the boot takes several minutes). Sorted search may be rejected by the server (HTTP 419).", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "page": {"type": "integer", "default": 1},
                "limit": {"type": "integer", "default": 10},
                "sort": {"type": "string", "description": "create | credit | price_asc | price_desc", "default": ""},
                "price_min": {"type": "string", "description": "Minimum price (yuan)"},
                "price_max": {"type": "string", "description": "Maximum price (yuan)"}
            },
            "required": ["query"]
        })),
        tool_def("goofish_get_product", "Fetch normalized Goofish (Xianyu) product details by item id or product URL.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Item ID or URL"}
            },
            "required": ["product"]
        })),
        tool_def("goofish_suggest_keywords", "Get Goofish (Xianyu) keyword suggestions through the APK-derived MTOP suggest endpoint.", json!({
            "type": "object",
            "properties": {
                "keyword": {"type": "string"},
                "limit": {"type": "integer", "default": 10}
            },
            "required": ["keyword"]
        })),
        tool_def("goofish_get_comments", "Fetch public comments for a Goofish (Xianyu) product through the APK-derived MTOP comment endpoint.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Item ID or URL"},
                "page": {"type": "integer", "default": 1}
            },
            "required": ["product"]
        })),
        tool_def("goofish_get_recommendations", "Fetch recommended items related to a Goofish (Xianyu) product through the APK-derived MTOP recommend endpoint.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Item ID or URL"}
            },
            "required": ["product"]
        })),
        tool_def("goofish_home_feed", "Fetch the Goofish (Xianyu) home feed through the APK-derived MTOP home endpoint.", json!({
            "type": "object",
            "properties": {
                "page": {"type": "integer", "default": 1}
            },
            "required": []
        })),

        // ── Mercari ──
        tool_def("mercari_search_products", "Search Mercari Japan products through the public web Connect-RPC JSON endpoint. Login-free: requests are authorized with a locally generated DPoP proof, no account or token needed. Each item carries is_auction and, for auction listings, an auction object (highest_bid is the current bid, not a buy-now price; also initial_price, total_bids, bid_deadline). Set exclude_auction to drop auction listings from the results.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 20},
                "page_token": {"type": "string", "default": ""},
                "sort": {"type": "string", "default": "default"},
                "status": {"type": "string", "default": ""},
                "exclude_keyword": {"type": "string", "default": ""},
                "exclude_auction": {"type": "boolean", "default": false},
                "category_ids": {"type": "string", "default": ""},
                "brand_ids": {"type": "string", "default": ""},
                "size_ids": {"type": "string", "default": ""},
                "seller_ids": {"type": "string", "default": ""},
                "price_min": {"type": "integer"},
                "price_max": {"type": "integer"},
                "item_condition_ids": {"type": "string", "default": ""},
                "shipping_payer_ids": {"type": "string", "default": ""},
                "color_ids": {"type": "string", "default": ""},
                "item_types": {"type": "string", "default": ""}
            },
            "required": ["query"]
        })),
        tool_def("mercari_get_product", "Fetch normalized Mercari item detail by item id or item URL through the APK-derived JSON endpoint. The item carries is_auction and, for auction listings, an auction object (highest_bid, initial_price, total_bids, expected_end_time, state).", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Item ID or URL"}
            },
            "required": ["product"]
        })),
        tool_def("mercari_search_seller_products", "List a seller's Mercari items through the APK-derived items/get_items JSON endpoint. Items carry is_auction and an auction object for auction listings.", json!({
            "type": "object",
            "properties": {
                "seller_id": {"type": "string"},
                "status": {"type": "string", "default": "on_sale"},
                "limit": {"type": "integer", "default": 30}
            },
            "required": ["seller_id"]
        })),
        tool_def("mercari_suggest_keywords", "Fetch Mercari autocomplete keyword/seller suggestions through the APK-derived BFF endpoint.", json!({
            "type": "object",
            "properties": {
                "keyword": {"type": "string"}
            },
            "required": ["keyword"]
        })),
        tool_def("mercari_search_brands", "Search Mercari brand IDs for use with mercari_search_products brand_ids.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 20}
            },
            "required": ["query"]
        })),
        tool_def("mercari_search_categories", "Search Mercari category IDs for use with mercari_search_products category_ids.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 20}
            },
            "required": ["query"]
        })),
        tool_def("mercari_get_reviews", "Get seller reviews/ratings for a Mercari seller. Returns rating summary (good/normal/bad counts, star rating) and individual review messages.", json!({
            "type": "object",
            "properties": {
                "seller_id": {"type": "string", "description": "Seller's numeric user ID (from search results sellerId or item detail seller.id)"},
                "limit": {"type": "integer", "default": 20, "description": "Number of reviews to return (max 100)"},
                "page": {"type": "integer", "default": 1}
            },
            "required": ["seller_id"]
        })),

        // ── Yahoo Flea Market ──
        tool_def("yflea_search_products", "Search Yahoo Flea Market (Yahoo!フリマ / PayPayフリマ) products. Uses anonymous __NEXT_DATA__ SSR from paypayfleamarket.yahoo.co.jp. No login needed.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 40},
                "page": {"type": "integer", "default": 1},
                "sort": {"type": "string", "default": ""},
                "min_price": {"type": "integer"},
                "max_price": {"type": "integer"},
                "category_id": {"type": "string", "default": ""}
            },
            "required": ["query"]
        })),
        tool_def("yflea_get_product", "Fetch Yahoo Flea Market item detail by item ID or URL. Returns full item data. Uses anonymous __NEXT_DATA__ SSR, no login needed.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Item ID or URL"}
            },
            "required": ["product"]
        })),
        tool_def("yflea_get_reviews", "Get seller reviews/ratings for a Yahoo Flea Market seller. Returns rating summary and individual reviews. No login needed.", json!({
            "type": "object",
            "properties": {
                "seller_id": {"type": "string", "description": "Seller ID (from item detail or search results)"},
                "limit": {"type": "integer", "default": 20}
            },
            "required": ["seller_id"]
        })),

        // ── Rakuma ──
        tool_def("rakuma_search_products", "Search Rakuten Rakuma (楽天ラクマ) products through fril.jp. Uses anonymous Rakuten Analytics data. No login needed. 40 items/page.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 40},
                "page": {"type": "integer", "default": 1},
                "sort": {"type": "string", "default": ""},
                "min_price": {"type": "integer"},
                "max_price": {"type": "integer"},
                "category_id": {"type": "string", "default": ""},
                "brand_id": {"type": "string", "default": ""}
            },
            "required": ["query"]
        })),
        tool_def("rakuma_get_product", "Fetch Rakuma item detail by item hash or URL. Returns item data from schema.org JSON-LD Product structured data including seller_hash for use with rakuma_get_reviews. No login needed.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Item hash or URL"}
            },
            "required": ["product"]
        })),
        tool_def("rakuma_get_reviews", "Get seller reviews/ratings for a Rakuma seller. Returns aggregate rating and individual reviews. The seller_hash (32-char hex) is available from rakuma_get_product item detail (seller.seller_hash field).", json!({
            "type": "object",
            "properties": {
                "seller_hash": {"type": "string", "description": "32-char hex seller hash from item detail (seller.seller_hash)"},
                "limit": {"type": "integer", "default": 20, "description": "Number of reviews to return"}
            },
            "required": ["seller_hash"]
        })),

        // ── Yahoo Auctions ──
        tool_def("yauction_search_products", "Search Yahoo Auctions Japan (ヤフオク) products via the official app API (anonymous, appid). Supports open and closed (落札済み) auctions.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 20},
                "page": {"type": "integer", "default": 1},
                "sort": {"type": "string", "default": ""},
                "min_price": {"type": "integer"},
                "max_price": {"type": "integer"},
                "category_id": {"type": "string", "default": ""},
                "status": {"type": "string", "default": "open", "description": "open or closed"},
                "keywords_and": {"type": "string", "default": ""},
                "keywords_not": {"type": "string", "default": ""},
                "seller_id": {"type": "string", "default": ""},
                "buynow_only": {"type": "boolean", "default": false},
                "free_shipping": {"type": "boolean", "default": false},
                "condition": {"type": "string", "default": ""}
            },
            "required": ["query"]
        })),
        tool_def("yauction_get_product", "Fetch Yahoo Auctions Japan item detail by auction ID or item URL via the official app API (anonymous, appid), no login needed.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Auction ID or URL"}
            },
            "required": ["product"]
        })),
        tool_def("yauction_get_reviews", "Get seller reviews/ratings for a Yahoo Auctions Japan seller. Returns rating summary and individual reviews parsed from the seller rating page.", json!({
            "type": "object",
            "properties": {
                "seller_id": {"type": "string", "description": "Seller ID (Yahoo! JAPAN ID)"},
                "limit": {"type": "integer", "default": 25, "description": "Max reviews to return"},
                "page": {"type": "integer", "default": 1, "description": "Page number"}
            },
            "required": ["seller_id"]
        })),
        tool_def("yauction_suggest_keywords", "Get Yahoo Auctions Japan keyword suggestions (with category candidates) via the official app API (anonymous, appid).", json!({
            "type": "object",
            "properties": {
                "keyword": {"type": "string", "description": "Partial keyword to suggest completions for"},
                "limit": {"type": "integer", "default": 10, "description": "Max suggestions to return"}
            },
            "required": ["keyword"]
        })),

        // ── Rakuten Ichiba ──
        tool_def("rakuten_search_products", "Search Rakuten Ichiba products through the Rakuten app's guest BFF (dynamic/search/v1). No login or API key. Result product_id is \"{shop_id}_{item_id}\" and feeds rakuten_get_product / rakuten_get_reviews. sort is one of the app's eight sorts: relevancy, price_low_to_high_all, price_high_to_low_all, newest, review_count_all, review_score_all, price_plus_shippingfee_low_to_high_all, price_plus_shippingfee_high_to_low_all.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "sort": {"type": "string", "default": ""},
                "price_min": {"type": "integer"},
                "price_max": {"type": "integer"},
                "category": {"type": "string", "default": "", "description": "Genre id"},
                "offset": {"type": "integer", "default": 0},
                "limit": {"type": "integer", "default": 20, "description": "Client-side cap on returned items"},
                "filters": {"type": "string", "default": "", "description": "Comma-joined flag filters (free_shipping, out_of_stock, super_deal, subscription, group_distribution, or server-distributed filter values)"},
                "gift": {"type": "string", "default": ""},
                "condition": {"type": "string", "default": "", "description": "new | used"},
                "review_min": {"type": "number", "description": "Review score lower bound (3.0 / 3.5 / 4.0 / 4.5)"},
                "excluded_keyword": {"type": "string", "default": ""},
                "brand_tags": {"type": "string", "default": ""},
                "shop_id": {"type": "string", "default": ""},
                "shop_code": {"type": "string", "default": ""},
                "region": {"type": "integer", "description": "Prefecture id (48 = overseas, pairs with country)"},
                "country": {"type": "string", "default": ""},
                "postal_code": {"type": "string", "default": ""},
                "tags": {"type": "string", "default": ""},
                "tag_groups": {"type": "string", "default": ""},
                "event": {"type": "integer"},
                "ran": {"type": "string", "default": ""},
                "inshop_pure_lexical_search": {"type": "boolean"},
                "image_search_items": {"type": "string", "default": ""},
                "similar_image_search_item": {"type": "string", "default": ""},
                "modules": {"type": "string", "default": "", "description": "Response modules; empty = the app's default set"},
                "ab": {"type": "string", "default": ""}
            },
            "required": ["query"]
        })),
        tool_def("rakuten_get_product", "Fetch Rakuten Ichiba product detail through the app's guest BFF (itemscreen/get/v16). Identify the item by shop_id+item_id (or the search result's product \"{shop_id}_{item_id}\"), or by item_code, shop_id+manage_number, or item_url. The endpoint answers HTTP 207 for partial success; the tool reports ok from the core item data.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "\"{shop_id}_{item_id}\" from search results, or an item URL"},
                "shop_id": {"type": "integer"},
                "item_id": {"type": "integer"},
                "item_code": {"type": "string", "description": "\"shopCode:itemId\" form"},
                "manage_number": {"type": "string"},
                "item_url": {"type": "string"},
                "variant_id": {"type": "string"},
                "postal_code": {"type": "string"},
                "prefecture_id": {"type": "integer"}
            },
            "required": []
        })),
        tool_def("rakuten_get_reviews", "Fetch Rakuten Ichiba product reviews through the app's guest BFF (review/list/v3). sort: -regtime (newest) | -evaluation | -judgment. sex: 0 = male, 1 = female. hits is capped at 30 by the server.", json!({
            "type": "object",
            "properties": {
                "shop_id": {"type": "integer"},
                "item_id": {"type": "integer"},
                "page": {"type": "integer", "default": 1},
                "hits": {"type": "integer", "default": 10},
                "sort": {"type": "string", "default": ""},
                "check_display_condition": {"type": "boolean", "default": true},
                "sex": {"type": "integer"},
                "min_evaluation": {"type": "integer"},
                "max_evaluation": {"type": "integer"},
                "min_age": {"type": "integer"},
                "max_age": {"type": "integer"},
                "image_flag": {"type": "integer", "default": 0, "description": "0 = all reviews, 1 = media-only"}
            },
            "required": ["shop_id", "item_id"]
        })),

        // ── Qoo10 ──
        tool_def("qoo10_search_products", "Search Qoo10 Japan products through the Qoo10 app's mobile API (GetMoveSearchData). No login. sort: 0 ranking, 1 best-selling, 2 newest, 3 price ascending, 4 price descending, 5 review score, 6 review count. Brand / shipping-from / color / attribute codes come from the service's filter info. Note: the app user agent carries a device-derived token that cannot be reproduced off-device; this client sends a format-preserving placeholder token in its place, which the service accepted in live verification. The service requires Japan-side access in practice.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search keyword"},
                "page": {"type": "integer", "default": 1},
                "limit": {"type": "integer", "default": 30},
                "sort": {"type": "integer", "default": 0},
                "search_sub_keyword": {"type": "string", "default": ""},
                "brand": {"type": "array", "items": {"type": "string"}, "description": "Brand codes"},
                "category": {"type": "string", "default": ""},
                "free_shipping": {"type": "string", "default": "", "description": "\"Y\" for free-shipping only"},
                "min_price": {"type": "integer"},
                "max_price": {"type": "integer"},
                "official_shop": {"type": "string", "default": ""},
                "promotion_yn": {"type": "string", "default": ""},
                "shipping_from": {"type": "array", "items": {"type": "string"}},
                "review_score": {"type": "string", "default": "", "description": "Review-score lower-bound code"},
                "color": {"type": "array", "items": {"type": "string"}},
                "attribute": {"type": "array", "items": {"type": "string"}},
                "coupon_label": {"type": "string", "default": ""},
                "keyword_type": {"type": "string", "default": ""},
                "sell_cust_no": {"type": "string", "default": "", "description": "Seller number (in-shop search)"},
                "nosearch_yn": {"type": "string", "default": "N"},
                "page_type": {"type": "string", "default": ""},
                "filter_type": {"type": "string", "default": ""},
                "seller_promotion_no": {"type": "string", "default": ""},
                "only_total_count": {"type": "string", "default": ""},
                "last_section": {"type": "string", "default": ""},
                "related_bg_color": {"type": "string", "default": ""}
            },
            "required": ["query"]
        })),
        tool_def("qoo10_get_product", "Fetch Qoo10 Japan product detail by GD_NO (goods code) or product URL. Qoo10 has no native detail API; this reproduces the app's WebView product page request and extracts the title, price and images from the returned page. Requires Japan-side access in practice (see qoo10_search_products).", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "GD_NO (goods code) or product URL"}
            },
            "required": ["product"]
        })),

        // ── Yahoo! Shopping ──
        tool_def("yshopping_search_products", "Search Yahoo! Shopping Japan products through the Yahoo! Shopping app's native API (searchList/v4). No login; the app's built-in appid is used. order is one of: Recommend, Price, TotalPrice, ReviewCount, ReviewScoreAverage, DiscountRate, CatalogReleaseDate, RegistrationDate, GoodDelivery (asc applies mainly to Price / TotalPrice). Result product_id is \"{storeId}_{itemCode}\" and feeds yshopping_get_product and yshopping_get_reviews.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "limit": {"type": "integer", "default": 48},
                "start_index": {"type": "integer", "default": 1},
                "order": {"type": "string", "default": "Recommend"},
                "asc": {"type": "boolean", "default": false},
                "price_from": {"type": "integer"},
                "price_to": {"type": "integer"},
                "discounted": {"type": "boolean"},
                "discount_from": {"type": "integer"},
                "discount_rate_from": {"type": "integer"},
                "category_ids": {"type": "array", "items": {"type": "string"}},
                "brand_id_list": {"type": "array", "items": {"type": "string"}},
                "seller_id": {"type": "string", "default": ""},
                "seller_rating_from": {"type": "number"},
                "used_status": {"type": "string", "default": ""},
                "jan_code": {"type": "string", "default": ""},
                "include_sold_out": {"type": "boolean"},
                "good_delivery": {"type": "boolean"},
                "asutsuku": {"type": "boolean"},
                "furusato": {"type": "boolean"},
                "has_coupon": {"type": "boolean"},
                "has_premium_price": {"type": "boolean"},
                "is_verified": {"type": "boolean"},
                "delivery_area": {"type": "string", "default": "13", "description": "Prefecture code"},
                "shipping_service_list": {"type": "array", "items": {"type": "string"}},
                "conditional_free_shipping_price_to": {"type": "integer"},
                "payment_list": {"type": "array", "items": {"type": "string"}},
                "kspec_id_list": {"type": "array", "items": {"type": "string"}},
                "sp_codes": {"type": "array", "items": {"type": "string"}},
                "municipality_code": {"type": "string", "default": ""},
                "seller_event_group_id": {"type": "string", "default": ""},
                "campaign_tab_id": {"type": "string", "default": ""}
            },
            "required": ["query"]
        })),
        tool_def("yshopping_get_product", "Fetch Yahoo! Shopping Japan product detail through the app's native API (item-detail/app/v2). product is the search result's product_id (\"{storeId}_{itemCode}\") or a store product URL. Detail images are returned as image_ids (the API hands out ids, not URLs); the main image URL is in image.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "\"{storeId}_{itemCode}\" or store product URL"},
                "prefectures": {"type": "string", "default": ""},
                "sku_id": {"type": "string", "default": ""},
                "donation_id": {"type": "string", "default": ""}
            },
            "required": ["product"]
        })),
        tool_def("yshopping_get_reviews", "Fetch Yahoo! Shopping Japan product reviews. The app's native API has no review-list endpoint; the app itself shows reviews by opening the review page (shopping.yahoo.co.jp/review/item/list) in a WebView, and this tool reads the review list from that page's server-rendered data. product is the search result's product_id (\"{storeId}_{itemCode}\") or a store product URL. Returns the rating summary (total, average, per-star counts) and the first page of reviews (the page carries 20); limit caps how many of them are returned.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "\"{storeId}_{itemCode}\" or store product URL"},
                "limit": {"type": "integer", "default": 20, "description": "Max reviews to return (the page carries 20)"}
            },
            "required": ["product"]
        })),

        // ── Amazon ──
        tool_def("amazon_search_products", "Search Amazon.co.jp products by reproducing the Amazon Shopping app's WebView requests (app-format user agent and the app's pre-navigation cookies; no browser engine). sort (the web s parameter): relevancerank, price-asc-rank, price-desc-rank, review-rank, date-desc-rank. Refinements use Amazon's web facet vocabulary, passed through as the app does: department (i:), category_node (n:), price band (p_36), prime_only (p_85), in_stock_only, min_rating (p_72), brand (p_89), seller_id (me), plus raw rh entries. Note: Amazon serves these pages only to Japan-side access in practice.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search keyword (k)"},
                "page": {"type": "integer", "default": 1},
                "limit": {"type": "integer", "default": 20, "description": "Client-side cap on returned items"},
                "sort": {"type": "string", "default": ""},
                "rank": {"type": "string", "default": "", "description": "The app's rank hand-off parameter"},
                "department": {"type": "string", "default": ""},
                "category_node": {"type": "string", "default": ""},
                "price_min": {"type": "integer"},
                "price_max": {"type": "integer"},
                "prime_only": {"type": "boolean", "default": false},
                "in_stock_only": {"type": "boolean", "default": false},
                "min_rating": {"type": "integer", "description": "Minimum star rating, 1-4"},
                "brand": {"type": "string", "default": ""},
                "seller_id": {"type": "string", "default": ""},
                "rh": {"type": "string", "default": "", "description": "Raw extra rh entries (key:value, comma-joined)"}
            },
            "required": ["query"]
        })),
        tool_def("amazon_get_product", "Fetch Amazon.co.jp product detail by ASIN or product URL, reproducing the app's WebView request (see amazon_search_products). Extracts title, price, main image, rating and review count from the product page. Requires Japan-side access in practice.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "ASIN or product URL"}
            },
            "required": ["product"]
        })),

        // ── Meta ──
        tool_def("shopping_list_sources", "List APK-derived endpoints and runtime data sources currently supported by Shopping_MCP.", json!({
            "type": "object",
            "properties": {}
        })),
    ]
}

fn tool_def(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
    })
}

fn extract_id_from_url(product: &str) -> String {
    let s = product.trim();
    if s.contains('/') {
        s.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(s)
            .to_string()
    } else {
        s.to_string()
    }
}

pub async fn dispatch(tool: &str, args: &Value, p: &Providers) -> Result<Value> {
    match tool {
        // ── AliExpress ──
        "aliexpress_search_products" => {
            let result = p
                .aliexpress
                .search_products(
                    &str_param(args, "query"),
                    int_param(args, "page", 1),
                    int_param(args, "limit", 10),
                    &str_or(args, "locale", "ja_JP"),
                    &str_or(args, "currency", "JPY"),
                    &str_or(args, "ship_to", "JP"),
                    &str_param(args, "sort"),
                )
                .await;
            Ok(result)
        }
        "aliexpress_get_product" => {
            let result = p
                .aliexpress
                .get_product(
                    &product_param(args),
                    &str_or(args, "locale", "ja_JP"),
                    &str_or(args, "currency", "JPY"),
                    &str_or(args, "ship_to", "JP"),
                )
                .await;
            Ok(result)
        }
        "aliexpress_suggest_keywords" => {
            let result = p
                .aliexpress
                .suggest_keywords(
                    &str_param(args, "keyword"),
                    &str_or(args, "locale", "ja_JP"),
                    int_param(args, "limit", 10),
                )
                .await;
            Ok(result)
        }
        "aliexpress_get_reviews" => {
            let result = p
                .aliexpress
                .get_reviews(
                    &product_param(args),
                    int_param(args, "page", 1),
                    int_param(args, "limit", 20),
                    &str_or(args, "locale", "ja_JP"),
                )
                .await;
            Ok(result)
        }
        "aliexpress_search_store_products" => {
            let result = p
                .aliexpress
                .search_store_products(
                    &str_param(args, "store_id"),
                    &str_param(args, "seller_id"),
                    int_param(args, "page", 1),
                    int_param(args, "limit", 20),
                    &str_or(args, "sort", "orders_desc"),
                    &str_or(args, "locale", "ja_JP"),
                    &str_or(args, "currency", "JPY"),
                    &str_or(args, "ship_to", "JP"),
                )
                .await;
            Ok(result)
        }

        // ── Goofish (闲鱼 / Xianyu) ──
        "goofish_search_products" => {
            let sort = str_param(args, "sort");
            let price_min = str_param(args, "price_min");
            let price_max = str_param(args, "price_max");
            let result = p
                .goofish
                .search_products(
                    &str_param(args, "query"),
                    int_param(args, "page", 1) as i64,
                    int_param(args, "limit", 10) as i64,
                    if sort.is_empty() {
                        None
                    } else {
                        Some(sort.as_str())
                    },
                    if price_min.is_empty() {
                        None
                    } else {
                        Some(price_min.as_str())
                    },
                    if price_max.is_empty() {
                        None
                    } else {
                        Some(price_max.as_str())
                    },
                )
                .await;
            Ok(result)
        }
        "goofish_get_product" => {
            let result = p.goofish.get_product(&product_param(args)).await;
            Ok(result)
        }
        "goofish_suggest_keywords" => {
            let result = p
                .goofish
                .suggest_keywords(
                    &str_param(args, "keyword"),
                    int_param(args, "limit", 10) as usize,
                )
                .await;
            Ok(result)
        }
        "goofish_get_comments" => {
            let result = p
                .goofish
                .get_comments(&product_param(args), int_param(args, "page", 1) as i64)
                .await;
            Ok(result)
        }
        "goofish_get_recommendations" => {
            let result = p.goofish.get_recommendations(&product_param(args)).await;
            Ok(result)
        }
        "goofish_home_feed" => {
            let result = p.goofish.home_feed(int_param(args, "page", 1) as i64).await;
            Ok(result)
        }

        // ── Mercari ──
        "mercari_search_products" => {
            let params = shopping_mcp_mercari::SearchParams {
                query: str_param(args, "query"),
                limit: int_param(args, "limit", 20),
                page_token: str_param(args, "page_token"),
                sort: str_or(args, "sort", "default"),
                status: str_param(args, "status"),
                exclude_keyword: str_param(args, "exclude_keyword"),
                category_ids: str_param(args, "category_ids"),
                brand_ids: str_param(args, "brand_ids"),
                size_ids: str_param(args, "size_ids"),
                seller_ids: str_param(args, "seller_ids"),
                price_min: opt_i64(args, "price_min"),
                price_max: opt_i64(args, "price_max"),
                item_condition_ids: str_param(args, "item_condition_ids"),
                shipping_payer_ids: str_param(args, "shipping_payer_ids"),
                color_ids: str_param(args, "color_ids"),
                item_types: str_param(args, "item_types"),
                exclude_auction: bool_param(args, "exclude_auction", false),
            };
            Ok(p.mercari.search_products(params).await?)
        }
        "mercari_get_product" => Ok(p.mercari.get_product(&product_param(args)).await?),
        "mercari_search_seller_products" => Ok(p
            .mercari
            .search_seller_products(
                &str_param(args, "seller_id"),
                &str_or(args, "status", "on_sale"),
                int_param(args, "limit", 30),
            )
            .await?),
        "mercari_suggest_keywords" => Ok(p
            .mercari
            .suggest_keywords(&str_param(args, "keyword"))
            .await?),
        "mercari_search_brands" => Ok(p
            .mercari
            .search_brands(&str_param(args, "query"), int_param(args, "limit", 20))
            .await?),
        "mercari_search_categories" => Ok(p
            .mercari
            .search_categories(&str_param(args, "query"), int_param(args, "limit", 20))
            .await?),
        "mercari_get_reviews" => {
            let seller_id = str_param(args, "seller_id");
            let seller_id = if seller_id.is_empty() {
                // Also accept as integer
                args.get("seller_id")
                    .and_then(|v| v.as_i64())
                    .map(|id| id.to_string())
                    .unwrap_or_default()
            } else {
                seller_id
            };
            Ok(p.mercari
                .get_reviews(
                    &seller_id,
                    int_param(args, "page", 1),
                    int_param(args, "limit", 20),
                )
                .await?)
        }

        // ── Yahoo Flea Market ──
        "yflea_search_products" => {
            let params = shopping_mcp_yahoo_flea::SearchParams {
                query: str_param(args, "query"),
                limit: int_param(args, "limit", 40),
                page: int_param(args, "page", 1),
                sort: str_param(args, "sort"),
                min_price: opt_i64(args, "min_price"),
                max_price: opt_i64(args, "max_price"),
                category_id: str_param(args, "category_id"),
            };
            let result = p.yflea.search(params).await?;
            Ok(serde_json::to_value(result)?)
        }
        "yflea_get_product" => {
            let item_id = extract_id_from_url(&product_param(args));
            let result = p.yflea.get_item(&item_id).await?;
            Ok(serde_json::to_value(result)?)
        }
        "yflea_get_reviews" => {
            let result = p
                .yflea
                .get_reviews(
                    &str_param(args, "seller_id"),
                    1,
                    int_param(args, "limit", 20),
                )
                .await?;
            Ok(result)
        }

        // ── Rakuma ──
        "rakuma_search_products" => {
            let params = shopping_mcp_rakuma::SearchParams {
                query: str_param(args, "query"),
                limit: int_param(args, "limit", 40),
                page: int_param(args, "page", 1),
                sort: str_param(args, "sort"),
                min_price: opt_i64(args, "min_price"),
                max_price: opt_i64(args, "max_price"),
                category_id: str_param(args, "category_id"),
                brand_id: str_param(args, "brand_id"),
            };
            let result = p.rakuma.search(params).await?;
            Ok(serde_json::to_value(result)?)
        }
        "rakuma_get_product" => {
            let item_hash = extract_id_from_url(&product_param(args));
            let result = p.rakuma.get_item(&item_hash).await?;
            Ok(serde_json::to_value(result)?)
        }
        "rakuma_get_reviews" => {
            let result = p
                .rakuma
                .get_reviews(
                    &str_param(args, "seller_hash"),
                    1,
                    int_param(args, "limit", 20),
                )
                .await?;
            Ok(result)
        }

        // ── Yahoo Auctions ──
        "yauction_search_products" => {
            let params = shopping_mcp_yahoo_auctions::SearchParams {
                query: str_param(args, "query"),
                limit: int_param(args, "limit", 20),
                page: int_param(args, "page", 1),
                sort: str_param(args, "sort"),
                min_price: opt_i64(args, "min_price"),
                max_price: opt_i64(args, "max_price"),
                category_id: str_param(args, "category_id"),
                status: str_or(args, "status", "open"),
                keywords_and: str_param(args, "keywords_and"),
                keywords_not: str_param(args, "keywords_not"),
                seller_id: str_param(args, "seller_id"),
                buynow_only: bool_param(args, "buynow_only", false),
                free_shipping: bool_param(args, "free_shipping", false),
                condition: str_param(args, "condition"),
            };
            let result = p.yauction.search(params).await?;
            Ok(serde_json::to_value(result)?)
        }
        "yauction_get_product" => {
            let auction_id = extract_id_from_url(&product_param(args));
            let result = p.yauction.get_item(&auction_id).await?;
            Ok(serde_json::to_value(result)?)
        }
        "yauction_get_reviews" => {
            let seller_id = str_param(args, "seller_id");
            let page = int_param(args, "page", 1);
            let limit = int_param(args, "limit", 25);
            let result = p.yauction.get_reviews(&seller_id, page, limit).await?;
            Ok(serde_json::to_value(result)?)
        }
        "yauction_suggest_keywords" => {
            let keyword = str_param(args, "keyword");
            let keyword = if keyword.is_empty() {
                str_param(args, "query")
            } else {
                keyword
            };
            let result = p
                .yauction
                .suggest_keywords(&keyword, int_param(args, "limit", 10))
                .await?;
            Ok(serde_json::to_value(result)?)
        }

        // ── Rakuten Ichiba ──
        "rakuten_search_products" => {
            let params = shopping_mcp_rakuten::SearchParams {
                query: str_param(args, "query"),
                sort: str_param(args, "sort"),
                price_min: opt_i64(args, "price_min"),
                price_max: opt_i64(args, "price_max"),
                category: str_param(args, "category"),
                offset: int_param(args, "offset", 0),
                filters: str_param(args, "filters"),
                gift: str_param(args, "gift"),
                condition: str_param(args, "condition"),
                review_min: opt_f64(args, "review_min"),
                excluded_keyword: str_param(args, "excluded_keyword"),
                brand_tags: str_param(args, "brand_tags"),
                shop_id: str_param(args, "shop_id"),
                shop_code: str_param(args, "shop_code"),
                region: opt_i64(args, "region"),
                country: str_param(args, "country"),
                postal_code: str_param(args, "postal_code"),
                tags: str_param(args, "tags"),
                tag_groups: str_param(args, "tag_groups"),
                event: opt_i64(args, "event"),
                ran: str_param(args, "ran"),
                inshop_pure_lexical_search: opt_bool(args, "inshop_pure_lexical_search"),
                image_search_items: str_param(args, "image_search_items"),
                similar_image_search_item: str_param(args, "similar_image_search_item"),
                modules: str_param(args, "modules"),
                ab: str_param(args, "ab"),
                limit: int_param(args, "limit", 20),
            };
            let result = p.rakuten.search_products(params).await?;
            Ok(serde_json::to_value(result)?)
        }
        "rakuten_get_product" => {
            let mut pref_ = shopping_mcp_rakuten::ProductRef {
                shop_id: opt_i64(args, "shop_id"),
                item_id: opt_i64(args, "item_id"),
                item_code: opt_str(args, "item_code"),
                manage_number: opt_str(args, "manage_number"),
                item_url: opt_str(args, "item_url"),
                variant_id: opt_str(args, "variant_id"),
                postal_code: opt_str(args, "postal_code"),
                prefecture_id: opt_i64(args, "prefecture_id"),
            };
            // Convenience forms: product = "{shop_id}_{item_id}"
            // (the search result's product_id) or an item URL.
            if pref_.shop_id.is_none() && pref_.item_code.is_none() && pref_.item_url.is_none() {
                let product = product_param(args);
                if let Some((a, b)) = product.split_once('_') {
                    if let (Ok(shop), Ok(item)) = (a.parse::<i64>(), b.parse::<i64>()) {
                        pref_.shop_id = Some(shop);
                        pref_.item_id = Some(item);
                    }
                }
                if pref_.shop_id.is_none() && product.contains("rakuten.co.jp") {
                    pref_.item_url = Some(product);
                }
            }
            let result = p.rakuten.get_product(pref_).await?;
            Ok(serde_json::to_value(result)?)
        }
        "rakuten_get_reviews" => {
            let params = shopping_mcp_rakuten::ReviewParams {
                shop_id: opt_i64(args, "shop_id").unwrap_or(0),
                item_id: opt_i64(args, "item_id").unwrap_or(0),
                page: int_param(args, "page", 1),
                hits: int_param(args, "hits", 10),
                sort: str_param(args, "sort"),
                check_display_condition: bool_param(args, "check_display_condition", true),
                sex: opt_i64(args, "sex"),
                min_evaluation: opt_i64(args, "min_evaluation"),
                max_evaluation: opt_i64(args, "max_evaluation"),
                min_age: opt_i64(args, "min_age"),
                max_age: opt_i64(args, "max_age"),
                image_flag: int_param(args, "image_flag", 0),
            };
            Ok(p.rakuten.get_reviews(params).await?)
        }

        // ── Qoo10 ──
        "qoo10_search_products" => {
            let params = shopping_mcp_qoo10::SearchParams {
                query: str_param(args, "query"),
                page: int_param(args, "page", 1),
                limit: int_param(args, "limit", 30),
                sort: int_param(args, "sort", 0),
                search_sub_keyword: str_param(args, "search_sub_keyword"),
                brand: str_array(args, "brand"),
                category: str_param(args, "category"),
                free_shipping: str_param(args, "free_shipping"),
                min_price: opt_i64(args, "min_price")
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                max_price: opt_i64(args, "max_price")
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                official_shop: str_param(args, "official_shop"),
                promotion_yn: str_param(args, "promotion_yn"),
                shipping_from: str_array(args, "shipping_from"),
                review_score: str_param(args, "review_score"),
                color: str_array(args, "color"),
                attribute: str_array(args, "attribute"),
                coupon_label: str_param(args, "coupon_label"),
                keyword_type: str_param(args, "keyword_type"),
                sell_cust_no: str_param(args, "sell_cust_no"),
                nosearch_yn: str_or(args, "nosearch_yn", "N"),
                page_type: str_param(args, "page_type"),
                filter_type: str_param(args, "filter_type"),
                seller_promotion_no: str_param(args, "seller_promotion_no"),
                only_total_count: str_param(args, "only_total_count"),
                last_section: str_param(args, "last_section"),
                related_bg_color: str_param(args, "related_bg_color"),
            };
            let result = p.qoo10.search_products(params).await?;
            Ok(serde_json::to_value(result)?)
        }
        "qoo10_get_product" => {
            let result = p.qoo10.get_product(&product_param(args)).await?;
            Ok(serde_json::to_value(result)?)
        }

        // ── Yahoo! Shopping ──
        "yshopping_search_products" => {
            let params = shopping_mcp_yahoo_shopping::SearchParams {
                query: str_param(args, "query"),
                limit: int_param(args, "limit", 48),
                start_index: int_param(args, "start_index", 1),
                order: str_or(args, "order", "Recommend"),
                asc: bool_param(args, "asc", false),
                price_from: opt_i64(args, "price_from"),
                price_to: opt_i64(args, "price_to"),
                discounted: opt_bool(args, "discounted"),
                discount_from: opt_i64(args, "discount_from"),
                discount_rate_from: opt_i64(args, "discount_rate_from"),
                category_ids: str_array(args, "category_ids"),
                brand_id_list: str_array(args, "brand_id_list"),
                seller_id: str_param(args, "seller_id"),
                seller_rating_from: opt_f64(args, "seller_rating_from"),
                used_status: str_param(args, "used_status"),
                jan_code: str_param(args, "jan_code"),
                include_sold_out: opt_bool(args, "include_sold_out"),
                good_delivery: opt_bool(args, "good_delivery"),
                asutsuku: opt_bool(args, "asutsuku"),
                furusato: opt_bool(args, "furusato"),
                has_coupon: opt_bool(args, "has_coupon"),
                has_premium_price: opt_bool(args, "has_premium_price"),
                is_verified: opt_bool(args, "is_verified"),
                delivery_area: str_or(args, "delivery_area", "13"),
                shipping_service_list: str_array(args, "shipping_service_list"),
                conditional_free_shipping_price_to: opt_i64(
                    args,
                    "conditional_free_shipping_price_to",
                ),
                payment_list: str_array(args, "payment_list"),
                kspec_id_list: str_array(args, "kspec_id_list"),
                sp_codes: str_array(args, "sp_codes"),
                municipality_code: str_param(args, "municipality_code"),
                seller_event_group_id: str_param(args, "seller_event_group_id"),
                campaign_tab_id: str_param(args, "campaign_tab_id"),
            };
            let result = p.yshopping.search_products(params).await?;
            Ok(serde_json::to_value(result)?)
        }
        "yshopping_get_product" => {
            let params = shopping_mcp_yahoo_shopping::DetailParams {
                item_id: shopping_mcp_yahoo_shopping::extract_item_id(&product_param(args)),
                prefectures: str_param(args, "prefectures"),
                sku_id: str_param(args, "sku_id"),
                donation_id: str_param(args, "donation_id"),
            };
            let result = p.yshopping.get_product(params).await?;
            Ok(serde_json::to_value(result)?)
        }
        "yshopping_get_reviews" => {
            let params = shopping_mcp_yahoo_shopping::ReviewParams {
                item_id: product_param(args),
                limit: int_param(args, "limit", 20),
            };
            let result = p.yshopping.get_reviews(params).await?;
            Ok(result)
        }

        // ── Amazon ──
        "amazon_search_products" => {
            let params = shopping_mcp_amazon::SearchParams {
                query: str_param(args, "query"),
                page: int_param(args, "page", 1),
                limit: int_param(args, "limit", 20),
                sort: str_param(args, "sort"),
                rank: str_param(args, "rank"),
                department: str_param(args, "department"),
                category_node: str_param(args, "category_node"),
                price_min: opt_i64(args, "price_min"),
                price_max: opt_i64(args, "price_max"),
                prime_only: bool_param(args, "prime_only", false),
                in_stock_only: bool_param(args, "in_stock_only", false),
                min_rating: opt_i64(args, "min_rating").map(|v| v as i32),
                brand: str_param(args, "brand"),
                seller_id: str_param(args, "seller_id"),
                rh: str_param(args, "rh"),
            };
            let result = p.amazon.search_products(params).await?;
            Ok(serde_json::to_value(result)?)
        }
        "amazon_get_product" => {
            let result = p.amazon.get_product(&product_param(args)).await?;
            Ok(serde_json::to_value(result)?)
        }

        // ── Meta ──
        "shopping_list_sources" => Ok(json!({
            "server": "ZeroSearch_MCP",
            "providers": [
                {
                    "name": "aliexpress",
                    "display_name": "AliExpress",
                    "auth": "anonymous (MTop H5 cookie signing)",
                    "tools": ["aliexpress_search_products", "aliexpress_get_product", "aliexpress_get_reviews", "aliexpress_suggest_keywords", "aliexpress_search_store_products"]
                },
                {
                    "name": "goofish",
                    "display_name": "Goofish (闲鱼 / Xianyu)",
                    "auth": "anonymous (MTOP + SecurityGuard signer runtime bundled with the server, auto-started; identity auto-generated by the server)",
                    "tools": ["goofish_search_products", "goofish_get_product", "goofish_suggest_keywords", "goofish_get_comments", "goofish_get_recommendations", "goofish_home_feed"]
                },
                {
                    "name": "mercari",
                    "display_name": "Mercari Japan",
                    "auth": "anonymous (DPoP ES256 JWT)",
                    "tools": ["mercari_search_products", "mercari_get_product", "mercari_search_seller_products", "mercari_suggest_keywords", "mercari_search_brands", "mercari_search_categories", "mercari_get_reviews"]
                },
                {
                    "name": "yahoo_auctions",
                    "display_name": "Yahoo Auctions Japan (ヤフオク)",
                    "auth": "anonymous (official app API, appid)",
                    "tools": ["yauction_search_products", "yauction_get_product", "yauction_get_reviews", "yauction_suggest_keywords"]
                },
                {
                    "name": "yahoo_flea",
                    "display_name": "Yahoo Flea Market (Yahoo!フリマ / PayPayフリマ)",
                    "auth": "anonymous",
                    "tools": ["yflea_search_products", "yflea_get_product", "yflea_get_reviews"]
                },
                {
                    "name": "rakuma",
                    "display_name": "Rakuten Rakuma (楽天ラクマ)",
                    "auth": "anonymous",
                    "tools": ["rakuma_search_products", "rakuma_get_product", "rakuma_get_reviews"]
                },
                {
                    "name": "rakuten",
                    "display_name": "Rakuten Ichiba (楽天市場)",
                    "auth": "anonymous (app guest BFF)",
                    "tools": ["rakuten_search_products", "rakuten_get_product", "rakuten_get_reviews"]
                },
                {
                    "name": "qoo10",
                    "display_name": "Qoo10 Japan",
                    "auth": "anonymous (app built-in certification key; Japan-side access required)",
                    "tools": ["qoo10_search_products", "qoo10_get_product"]
                },
                {
                    "name": "yahoo_shopping",
                    "display_name": "Yahoo! Shopping Japan (Yahoo!ショッピング)",
                    "auth": "anonymous (official app API, appid)",
                    "tools": ["yshopping_search_products", "yshopping_get_product", "yshopping_get_reviews"]
                },
                {
                    "name": "amazon",
                    "display_name": "Amazon.co.jp (Amazon Shopping app WebView)",
                    "auth": "anonymous (app WebView reproduction; Japan-side access required)",
                    "tools": ["amazon_search_products", "amazon_get_product"]
                }
            ]
        })),

        _ => anyhow::bail!("Unknown tool: {}", tool),
    }
}
