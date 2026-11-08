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

fn opt_int(args: &Value, key: &str) -> Option<i32> {
    args.get(key).and_then(|v| v.as_i64()).map(|v| v as i32)
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

fn int_array(args: &Value, key: &str) -> Vec<i64> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default()
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
                "sort": {"type": "string", "default": ""},
                "min_price": {"type": "string", "description": "Minimum price filter (wire pr/pageMinPrice); omit for none"},
                "max_price": {"type": "string", "description": "Maximum price filter (wire pr); omit for none"},
                "switches": {"type": "array", "items": {"type": "string"}, "description": "Filter switches; bare names or filterCode:<name> values"}
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
                "locale": {"type": "string", "default": "ja_JP"},
                "filter": {"type": "string", "default": "all", "description": "Review filter (API filter parameter, passed through verbatim)"},
                "sort": {"type": "string", "default": "complex_default", "description": "Review sort (API sort parameter, passed through verbatim)"}
            },
            "required": ["product"]
        })),
        tool_def("aliexpress_query_sku", "Query one SKU of an AliExpress product (mtop.aliexpress.pdp.sku.query): price/stock for the selected SKU. Live-verified anonymously. The response data shape is not fully documented, so the raw data is returned alongside any extracted sku_combinations.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Product ID or URL"},
                "sku_id": {"type": "string"},
                "quantity": {"type": "integer", "default": 1},
                "pdp_npi": {"type": "string", "default": ""},
                "pdp_ext_f": {"type": "string", "default": ""},
                "locale": {"type": "string", "default": "ja_JP"},
                "currency": {"type": "string", "default": "JPY"},
                "ship_to": {"type": "string", "default": "JP"}
            },
            "required": ["product", "sku_id"]
        })),
        tool_def("aliexpress_adjust_sku", "Adjust the selected SKU of an AliExpress product (mtop.aliexpress.pdp.sku.adjust). Request shape is implemented from the app-derived builder, but this endpoint has NOT been live-verified yet (results carry live_verified: false).", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Product ID or URL"},
                "sku_id": {"type": "string"},
                "quantity": {"type": "integer", "default": 1},
                "pdp_npi": {"type": "string", "default": ""},
                "pdp_ext_f": {"type": "string", "default": ""},
                "locale": {"type": "string", "default": "ja_JP"},
                "currency": {"type": "string", "default": "JPY"},
                "ship_to": {"type": "string", "default": "JP"}
            },
            "required": ["product", "sku_id"]
        })),
        tool_def("aliexpress_adjust_pdp", "Adjust the PDP main block for a selected SKU (mtop.aliexpress.pdp.main.adjust). Request shape is implemented from the app-derived builder, but this endpoint has NOT been live-verified yet (results carry live_verified: false).", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Product ID or URL"},
                "sku_id": {"type": "string"},
                "quantity": {"type": "integer", "default": 1},
                "pdp_npi": {"type": "string", "default": ""},
                "pdp_ext_f": {"type": "string", "default": ""},
                "locale": {"type": "string", "default": "ja_JP"},
                "currency": {"type": "string", "default": "JPY"},
                "ship_to": {"type": "string", "default": "JP"}
            },
            "required": ["product", "sku_id"]
        })),
        tool_def("aliexpress_get_store_groups", "Fetch an AliExpress store's product groups (mtop.ae.shop.search.product.groupType). Request shape is implemented from the app-derived builder; live acceptance is not yet confirmed (results carry live_verified: false).", json!({
            "type": "object",
            "properties": {
                "store_id": {"type": "string"},
                "group_type": {"type": "string", "default": "", "description": "e.g. shop_wireless_compare_items | shop_wireless_bundle_items; empty = default"},
                "seller_id": {"type": "string", "default": ""},
                "locale": {"type": "string", "default": "ja_JP"},
                "currency": {"type": "string", "default": "JPY"},
                "ship_to": {"type": "string", "default": "JP"}
            },
            "required": ["store_id"]
        })),
        tool_def("aliexpress_get_store_module", "Fetch an AliExpress store page module (ModuleAsyncService). Request shape is implemented from the app-derived builder; live acceptance is not yet confirmed (results carry live_verified: false).", json!({
            "type": "object",
            "properties": {
                "store_id": {"type": "string"},
                "component_key": {"type": "string", "default": "", "description": "empty = promotePruductList (captured spelling)"},
                "seller_id": {"type": "string", "default": ""},
                "locale": {"type": "string", "default": "ja_JP"},
                "currency": {"type": "string", "default": "JPY"},
                "ship_to": {"type": "string", "default": "JP"}
            },
            "required": ["store_id"]
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
        tool_def("goofish_search_products", "Search Goofish (Xianyu) products through the APK-derived MTOP search endpoint. Requests are signed by the SecurityGuard runtime bundled with the server; the device identity is generated by the server itself. No configuration is required: on first use the server downloads the Goofish app package (~269 MB, SHA-256 verified) and assembles the runtime in its data directory. The signing harness boots in the background when the server starts; a Goofish call made before the boot finishes waits for it (the boot takes several minutes). Sorted or filtered search may be rejected by the server (HTTP 419; cause undetermined). The optional filter arguments (prop_selections = CPV property selections as {property_id: [value_id, ...]}, price_range in \"min,max\" form, province/city/area, category ids) switch the call to the full filter form.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "page": {"type": "integer", "default": 1},
                "limit": {"type": "integer", "default": 10},
                "sort": {"type": "string", "description": "create | credit | price_asc | price_desc", "default": ""},
                "price_min": {"type": "string", "description": "Minimum price (yuan)"},
                "price_max": {"type": "string", "description": "Maximum price (yuan)"},
                "prop_selections": {"type": "object", "description": "CPV selections: {property_id: [value_id, ...]}"},
                "price_range": {"type": "string", "description": "Panel price band in \"min,max\" form"},
                "province": {"type": "string", "description": "Region filter: province"},
                "city": {"type": "string", "description": "Region filter: city"},
                "area": {"type": "string", "description": "Region filter: area"},
                "category_id": {"type": "string"},
                "front_cat_id": {"type": "string"},
                "leaf_id": {"type": "string"}
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
        tool_def("goofish_get_comments", "Fetch public comments for a Goofish (Xianyu) product through the APK-derived MTOP comment endpoint. The app's comment.list has no page or cursor mechanism — rows_per_page is the only control; the response's next_page field is informational only and cannot be used to fetch further comments.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Item ID or URL"},
                "rows_per_page": {"type": "integer", "default": 10, "description": "Number of comments to request (the app's rowsPerPage)"}
            },
            "required": ["product"]
        })),
        tool_def("goofish_get_recommendations", "Fetch recommended items related to a Goofish (Xianyu) product through the APK-derived MTOP recommend endpoint. Continuation is caller-carried: pass the previous result's next_page_state back as page_state to fetch the next page; every result carries the advanced next_page_state and has_more.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Item ID or URL"},
                "page_state": {"type": "object", "description": "Continuation state {page_num, page_size, current_size, history_item_ids} from the previous result's next_page_state"}
            },
            "required": ["product"]
        })),
        tool_def("goofish_home_feed", "Fetch the Goofish (Xianyu) home feed through the APK-derived MTOP home endpoint. Continuation is caller-carried: pass the previous result's next_page_state back as page_state (it takes precedence over page); results fetched through the state flow carry the advanced next_page_state.", json!({
            "type": "object",
            "properties": {
                "page": {"type": "integer", "default": 1},
                "page_state": {"type": "object", "description": "Continuation state {page_number, last_response_count, client_recycle_params} from the previous result's next_page_state"}
            },
            "required": []
        })),
        tool_def("goofish_get_similar_items", "Fetch items similar to a Goofish (Xianyu) product (search.spusimilar.itemlist). conditions is the caller-supplied conditions object (in the app it is built from the search response's server-issued spuId; the detail response carries no spu identifier). Continuation: pass page_no from the previous result's next_page_state. NOT live-verified: the one measured attempt was rejected by the server-side app-version gate FAIL_BIZ_SERVER_NEED_UPGRADE_MSG2, so a live call may return that business error instead of data.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Item ID or URL"},
                "conditions": {"type": "object", "description": "Caller-supplied conditions object"},
                "page_no": {"type": "integer", "default": 1},
                "page_size": {"type": "integer", "default": 10}
            },
            "required": ["product"]
        })),
        tool_def("goofish_get_category_guide", "Fetch a Goofish (Xianyu) category guide tree through the APK-derived MTOP category endpoint. name optionally selects a guide by name.", json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Optional guide/category name"}
            },
            "required": []
        })),
        tool_def("goofish_get_rate_list", "Fetch the seller evaluations (rate list) for one Goofish (Xianyu) item through the APK-derived MTOP rate endpoint. rated_uid is the rated seller's user id.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "Item ID or URL"},
                "rated_uid": {"type": "string", "description": "The rated user's id"},
                "page": {"type": "integer", "default": 1}
            },
            "required": ["product", "rated_uid"]
        })),
        tool_def("goofish_get_user_headinfo", "Fetch a Goofish (Xianyu) user's header info through the APK-derived MTOP headinfo endpoint. NOT live-verified: the request shape is fixed by the app's typed classes, but no in-app caller was found, so reachability is unconfirmed.", json!({
            "type": "object",
            "properties": {
                "user_id": {"type": "string"},
                "nick": {"type": "string"},
                "my_user_id": {"type": "integer"}
            },
            "required": []
        })),
        tool_def("goofish_get_seller_items", "Fetch Goofish (Xianyu) publish.items. NOTE: the endpoint has no parameter identifying another user — its confirmed use is selecting one's own items; this is NOT another seller's public listing.", json!({
            "type": "object",
            "properties": {
                "page": {"type": "integer", "default": 1},
                "page_size": {"type": "integer", "default": 20}
            },
            "required": []
        })),
        tool_def("goofish_get_city_feed", "Fetch the Goofish (Xianyu) same-city feed (local.home / local.home.feeds). data_source_id and version are caller-supplied (they are delivered to the app by its page configuration). feeds_api=true selects the feeds variant, which is NOT live-verified.", json!({
            "type": "object",
            "properties": {
                "data_source_id": {"type": "string"},
                "version": {"type": "string"},
                "feeds_api": {"type": "boolean", "default": false},
                "page_number": {"type": "string", "default": "1"},
                "last_response_count": {"type": "string", "default": "0"},
                "tab_name": {"type": "string"},
                "keywords": {"type": "string"},
                "latitude": {"type": "number"},
                "longitude": {"type": "number"}
            },
            "required": ["data_source_id", "version"]
        })),
        tool_def("goofish_get_city_nearby", "Fetch Goofish (Xianyu) nearby corner info (local.near.by.corner.info). NOT live-verified. Coordinates are caller-supplied strings, never invented.", json!({
            "type": "object",
            "properties": {
                "city": {"type": "string"},
                "latitude": {"type": "string"},
                "longitude": {"type": "string"},
                "select_city": {"type": "string"}
            },
            "required": []
        })),
        tool_def("goofish_get_fishpond_list", "List Goofish (Xianyu) fishponds (鱼塘). NOT live-verified (the gateway registration record notes this API may not be registered).", json!({
            "type": "object",
            "properties": {
                "page": {"type": "integer", "default": 1},
                "limit": {"type": "integer", "default": 20}
            },
            "required": []
        })),
        tool_def("goofish_get_fishpond_info", "Fetch one Goofish (Xianyu) fishpond's detail (fishpool.id). NOT live-verified.", json!({
            "type": "object",
            "properties": {
                "pool_id": {"type": "integer"}
            },
            "required": ["pool_id"]
        })),
        tool_def("goofish_get_fishpond_comments", "Fetch a Goofish (Xianyu) fishpond's public posts/comments (fishpool.comment.list). NOT live-verified. Coordinates are caller-supplied and omitted when absent.", json!({
            "type": "object",
            "properties": {
                "pool_id": {"type": "integer"},
                "page": {"type": "integer", "default": 1},
                "latitude": {"type": "number"},
                "longitude": {"type": "number"}
            },
            "required": ["pool_id"]
        })),
        tool_def("goofish_get_video_play", "Fetch Goofish (Xianyu) video play info (video.play) for a video id. NOT live-verified.", json!({
            "type": "object",
            "properties": {
                "video_id": {"type": "integer"},
                "item_id": {"type": "integer", "default": 0}
            },
            "required": ["video_id"]
        })),
        tool_def("goofish_get_video_play_from_feed_card", "Fetch Goofish (Xianyu) video play info from a normalized feed card (a card object as returned by the feed/search parsers, carrying video_id/has_video). Cards without a confirmed video, or with a mismatched item_id, are refused before sending. NOT live-verified.", json!({
            "type": "object",
            "properties": {
                "card": {"type": "object", "description": "Normalized feed card object"},
                "item_id": {"type": "integer"}
            },
            "required": ["card"]
        })),
        tool_def("goofish_get_fun_comments", "Fetch Goofish (Xianyu) fun/explore comments (fun.general.comment.list). Cursor paging: pass the previous response's last_cursor as begin_cursor. NOT live-verified.", json!({
            "type": "object",
            "properties": {
                "target_id": {"type": "string"},
                "target_type": {"type": "string"},
                "namespace": {"type": "string"},
                "source_id": {"type": "string"},
                "begin_cursor": {"type": "string"},
                "page_size": {"type": "integer", "default": 10},
                "rows_per_page": {"type": "integer", "default": 10},
                "top_comment_id": {"type": "string"}
            },
            "required": ["target_id", "target_type", "namespace", "source_id"]
        })),

        // ── Mercari ──
        tool_def("mercari_search_products", "Search Mercari Japan products through the public web Connect-RPC JSON endpoint. Login-free: requests are authorized with a locally generated DPoP proof, no account or token needed. Each item carries is_auction and, for auction listings, an auction object (highest_bid is the current bid, not a buy-now price; also initial_price, total_bids, bid_deadline). Set exclude_auction to drop auction listings from the results.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 20},
                "page_token": {"type": "string", "default": ""},
                "sort": {"type": "string", "default": "default", "description": "Sort name; in addition to the usual values, price_without_time_filter / price_asc_without_time_filter / price_desc_without_time_filter (and their sort_/lowest_/highest_ spellings) select SORT_PRICE_WITHOUT_TIME_FILTER"},
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
                "item_types": {"type": "string", "default": ""},
                "has_coupon": {"type": "boolean", "default": false},
                "attributes": {"type": "array", "items": {"type": "object"}, "default": [], "description": "Raw Mercari attribute filter objects, passed through verbatim"},
                "sku_ids": {"type": "array", "items": {"type": "integer"}, "default": []},
                "shop_ids": {"type": "array", "items": {"type": "integer"}, "default": []},
                "exclude_shipping_method_ids": {"type": "array", "items": {"type": "integer"}, "default": []},
                "shipping_from_area": {"type": "array", "items": {"type": "integer"}, "default": []},
                "shipping_method": {"type": "array", "items": {"type": "integer"}, "default": []}
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
        tool_def("mercari_get_reviews", "Get seller reviews/ratings for a Mercari seller. Returns rating summary (good/normal/bad counts, star rating) and individual review messages. Note: the endpoint ignores pagination (verified live — page 2 returns page 1), so page > 1 is rejected with an explicit error rather than silently repeating the first page; the real continuation mechanism is undetermined.", json!({
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
                "sort": {"type": "string", "default": "", "description": "default | price_asc | price_desc | newest (aliases: lowest_price, highest_price, new, relevance, ranking)"},
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
                "limit": {"type": "integer", "default": 20},
                "page": {"type": "integer", "default": 1}
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
                "sort": {"type": "string", "default": "", "description": "default | price_asc | price_desc | newest | likes (aliases: lowest_price, highest_price, new, popular, relevance)"},
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
        tool_def("rakuma_get_reviews", "Get seller reviews/ratings for a Rakuma seller. Returns aggregate rating and individual reviews. The seller_hash (32-char hex) is available from rakuma_get_product item detail (seller.seller_hash field). Reviews are sliced client-side from the shop page's full review list, so page/limit page through that list.", json!({
            "type": "object",
            "properties": {
                "seller_hash": {"type": "string", "description": "32-char hex seller hash from item detail (seller.seller_hash)"},
                "limit": {"type": "integer", "default": 20, "description": "Number of reviews to return"},
                "page": {"type": "integer", "default": 1}
            },
            "required": ["seller_hash"]
        })),

        // ── Yahoo Auctions ──
        tool_def("yauction_search_products", "Search Yahoo Auctions Japan (ヤフオク) products via the official app API (anonymous, appid). Supports open and closed (落札済み) auctions. sort vocabulary: price_asc | price_desc | bids | bids_asc | end_time | end_time_desc | newest | total_price_asc | total_price_desc | watch_count | featured | start_price_asc | start_price_desc; literal API values (e.g. +TOTAL_PRICE) are also accepted for the matching status. seller_type is a comma-joined list of AUC_STORE / AUCTION / FLEAMARKET; prefecture_code takes comma-joined Prefecture values (e.g. TOKYO,OSAKA). conditions (raw ItemCondition ids, comma-joined) takes precedence over condition, and submit_type (ALL / AUCTION / FIXED) over buynow_only.", json!({
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
                "condition": {"type": "string", "default": ""},
                "brand_ids": {"type": "string", "default": "", "description": "Comma-joined brand ids"},
                "catalog_ids": {"type": "string", "default": "", "description": "Comma-joined catalog ids"},
                "min_buy_now_price": {"type": "integer"},
                "max_buy_now_price": {"type": "integer"},
                "seller_type": {"type": "string", "default": "", "description": "Comma-joined: AUC_STORE / AUCTION / FLEAMARKET"},
                "prefecture_code": {"type": "string", "default": "", "description": "Comma-joined Prefecture values, e.g. TOKYO,OSAKA"},
                "destination_prefecture_code": {"type": "string", "default": ""},
                "is_featured": {"type": "boolean"},
                "is_appraisal": {"type": "boolean"},
                "is_finish_soon": {"type": "boolean"},
                "is_convenience_store": {"type": "boolean"},
                "is_new_arrival": {"type": "boolean"},
                "except_suspected_fake": {"type": "boolean"},
                "can_privacy_delivery": {"type": "boolean"},
                "is_adult_ok": {"type": "boolean"},
                "auction_ids": {"type": "string", "default": "", "description": "Comma-joined auction ids"},
                "except_auction_ids": {"type": "string", "default": ""},
                "except_auc_user_ids": {"type": "string", "default": ""},
                "except_catalog_ids": {"type": "string", "default": ""},
                "shp_spec": {"type": "string", "default": "", "description": "Spec tokens, space-joined, passed through verbatim"},
                "spec_size_type": {"type": "string", "default": "", "description": "e.g. MENS_SHOES"},
                "spec_size_ids": {"type": "string", "default": "", "description": "Comma-joined numeric ids"},
                "conditions": {"type": "string", "default": "", "description": "Raw ItemCondition ids, comma-joined"},
                "submit_type": {"type": "string", "default": "", "description": "ALL / AUCTION / FIXED"},
                "priority": {"type": "string", "default": ""},
                "image_url": {"type": "string", "default": "", "description": "Image-search image URL"},
                "search_type": {"type": "string", "default": ""},
                "time_from_first_start": {"type": "integer", "description": "Unix seconds"},
                "rewrite_query_type": {"type": "string", "default": ""},
                "campaign_type": {"type": "string", "default": ""},
                "campaign_start_time": {"type": "integer", "description": "Unix seconds"},
                "campaign_end_time": {"type": "integer", "description": "Unix seconds"},
                "module": {"type": "string", "default": "", "description": "Comma-joined module names"},
                "src": {"type": "string", "default": ""}
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
        tool_def("yauction_get_statistics", "Get closed-auction price statistics for a Yahoo Auctions Japan catalog id (search/items/statistics, hm1 variant): totals, the last closed item, and per-day count/avg/max/min. conditions takes ItemCondition ids (a single value is recommended until the multi-value joining form is live-confirmed).", json!({
            "type": "object",
            "properties": {
                "catalog_id": {"type": "string", "description": "Catalog id (from item detail)"},
                "conditions": {"type": "string", "default": "", "description": "ItemCondition ids, comma-joined"}
            },
            "required": ["catalog_id"]
        })),
        tool_def("yauction_get_statistics_closed", "Get closed-auction price statistics (yne variant, itemState=closed fixed) with the additional shp_spec / seller_type filters. Same result shape as yauction_get_statistics.", json!({
            "type": "object",
            "properties": {
                "catalog_id": {"type": "string", "description": "Catalog id (from item detail)"},
                "conditions": {"type": "string", "default": "", "description": "ItemCondition ids, comma-joined"},
                "shp_spec": {"type": "string", "default": ""},
                "seller_type": {"type": "string", "default": ""}
            },
            "required": ["catalog_id"]
        })),
        tool_def("yauction_get_seller_profile", "Get a Yahoo Auctions Japan seller's profile (user/v2 profile): display name, icon, store flags, eKYC/speed-shipping flags and the ratings summary.", json!({
            "type": "object",
            "properties": {
                "seller_id": {"type": "string", "description": "Seller ID (Yahoo! JAPAN ID)"}
            },
            "required": ["seller_id"]
        })),
        tool_def("yauction_get_store_profile", "Get a Yahoo Auctions Japan store seller's store profile (user/v1 store-profile): announcement text and banners. Non-store sellers may legitimately error here.", json!({
            "type": "object",
            "properties": {
                "seller_id": {"type": "string", "description": "Seller ID (Yahoo! JAPAN ID)"}
            },
            "required": ["seller_id"]
        })),
        tool_def("yauction_get_reviews", "Get seller reviews/ratings for a Yahoo Auctions Japan seller. Returns individual reviews parsed from the seller rating page plus a ratings_summary (enriched from the seller profile API when available).", json!({
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
                "limit": {"type": "integer", "default": 10, "description": "Max suggestions to return"},
                "category_limit": {"type": "integer", "description": "Max category candidates (default 5)"},
                "brand_limit": {"type": "integer", "description": "Max brand candidates (default 5)"}
            },
            "required": ["keyword"]
        })),

        // ── Rakuten Ichiba ──
        tool_def("rakuten_search_products", "Search Rakuten Ichiba products through the Rakuten app's guest BFF (dynamic/search/v1). No login or API key. Result product_id is \"{shop_id}_{item_id}\" and feeds rakuten_get_product / rakuten_get_reviews. sort is one of the app's sorts: the standard form (relevancy, price_low_to_high_all, price_high_to_low_all, newest, review_count_all, review_score_all, price_plus_shippingfee_low_to_high_all, price_plus_shippingfee_high_to_low_all) or the highRelevancy form without the _all suffix (price_low_to_high, price_high_to_low, review_count, review_score, and the two shipping-inclusive forms); the value passes through to the BFF.", json!({
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

        tool_def("rakuten_suggest_keywords", "Get Rakuten Ichiba keyword suggestions through the app's guest BFF (search/suggest): keyword completions and genre candidates for a partial keyword. No login.", json!({
            "type": "object",
            "properties": {
                "keyword": {"type": "string", "description": "Partial keyword to complete"},
                "limit": {"type": "integer", "default": 10, "description": "Client-side cap on returned suggestions"}
            },
            "required": ["keyword"]
        })),
        tool_def("rakuten_search_products_aggregated", "Search Rakuten Ichiba in the app's product-aggregated form (product/search/v1): results are grouped by product rather than by shop listing. No login.", json!({
            "type": "object",
            "properties": {
                "keyword": {"type": "string", "description": "Search keyword"},
                "page": {"type": "integer", "default": 1},
                "hits": {"type": "integer", "default": 30},
                "genre_id": {"type": "integer"},
                "min_price": {"type": "integer"},
                "max_price": {"type": "integer"},
                "product_code": {"type": "string"},
                "product_id": {"type": "string", "description": "Product id hash from a previous aggregated search"},
                "keyword_search_type": {"type": "integer", "default": 0},
                "genre_information_flag": {"type": "integer", "default": 0}
            },
            "required": ["keyword"]
        })),
        tool_def("rakuten_get_genre_tree", "Fetch a Rakuten Ichiba genre subtree (genre/search/v1): the genre itself plus, per the include flags, its children, siblings, tag groups and tags. No login.", json!({
            "type": "object",
            "properties": {
                "genre_id": {"type": "integer"},
                "include_children": {"type": "boolean", "default": true},
                "include_brother": {"type": "boolean", "default": false},
                "include_tag_group": {"type": "boolean", "default": false},
                "include_tag": {"type": "boolean", "default": false}
            },
            "required": ["genre_id"]
        })),
        tool_def("rakuten_get_variant_details", "Fetch per-variant details for a Rakuten Ichiba item (variantdetails/get/v5): price, inventory and delivery captions for one variant. The state parameters (variant_id, is_available_for_sale, prices, inventory, is_subscription_eligible, delivery_caption_variant) are carried over from rakuten_get_product's response (its variants entries) — pass them from there. No login.", json!({
            "type": "object",
            "properties": {
                "shop_id": {"type": "integer"},
                "item_id": {"type": "integer"},
                "variant_id": {"type": "string"},
                "is_available_for_sale": {"type": "boolean", "default": true},
                "postal_code": {"type": "string"},
                "prefecture_id": {"type": "integer", "default": 0},
                "article_number": {"type": "string"},
                "genre_id": {"type": "integer"},
                "standard_price": {"type": "integer"},
                "tax_included_price": {"type": "integer"},
                "inventory": {"type": "integer"},
                "is_subscription_eligible": {"type": "boolean", "default": false},
                "delivery_caption_variant": {"type": "object", "description": "One delivery-caption variant object from the detail response"}
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
        tool_def("qoo10_get_search_filters", "Get the Qoo10 Japan search filter information for a keyword (GetMoveSearchFilterInfo — the data the app's filter popup is built from): the brand, colour, price-band, category, shipping-from, review-score and attribute codes that qoo10_search_products accepts as filter values in this keyword's context. Optionally pass the category code or already-selected brand codes to scope the context. No login. Requires Japan-side access in practice (see qoo10_search_products).", json!({
            "type": "object",
            "properties": {
                "keyword": {"type": "string", "description": "Search keyword the filters apply to"},
                "category": {"type": "string", "default": "", "description": "Category code context"},
                "brand": {"type": "array", "items": {"type": "string"}, "description": "Already-selected brand codes"}
            },
            "required": ["keyword"]
        })),
        tool_def("qoo10_suggest_keywords", "Get Qoo10 Japan keyword suggestions through the Qoo10 app's mobile API (GetMoveAutoKeywordSearch). No login. Requires Japan-side access in practice (see qoo10_search_products).", json!({
            "type": "object",
            "properties": {
                "keyword": {"type": "string", "description": "Partial keyword to complete"},
                "limit": {"type": "integer", "default": 10}
            },
            "required": ["keyword"]
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
        tool_def("yshopping_get_reviews", "Fetch Yahoo! Shopping Japan product reviews. The app's native API has no review-list endpoint; the app itself shows reviews by opening the review page (shopping.yahoo.co.jp/review/item/list) in a WebView, and this tool reads reviews from that page's server-rendered data and, for anything beyond the first 20, a non-default sort, or a filter, from the continuation endpoint the page's own SPA uses. product is the search result's product_id (\"{storeId}_{itemCode}\") or a store product URL. offset is the zero-based index of the first review to return (page through results with offset + count, or the returned next_offset). sort is one of the page's own sort orders: lengthDesc (default, おすすめ順), ratingDesc (評価の高い順), referenceCountDesc (いいねの多い順), postedTimeDesc (新着順), pickUp. rating and sub_review_id apply the page's own review filters (both live-verified). Returns the rating summary (total, average, per-star counts), the reviews, next_offset, and subReviewSummaries — the item's aspect summaries whose id values are what sub_review_id takes.", json!({
            "type": "object",
            "properties": {
                "product": {"type": "string", "description": "\"{storeId}_{itemCode}\" or store product URL"},
                "limit": {"type": "integer", "default": 20, "description": "Max reviews to return"},
                "offset": {"type": "integer", "default": 0, "description": "Zero-based index of the first review to return"},
                "sort": {"type": "string", "default": "lengthDesc", "description": "lengthDesc | ratingDesc | referenceCountDesc | postedTimeDesc | pickUp"},
                "rating": {"type": "integer", "description": "Only reviews with this star rating (the review page's rating filter)"},
                "sub_review_id": {"type": "integer", "description": "Only reviews that rated this aspect; an id from the item's aspect summaries (subReviewSummaries)"}
            },
            "required": ["product"]
        })),

        tool_def("yshopping_suggest_keywords", "Get Yahoo! Shopping Japan keyword suggestions through the suggest endpoint the app uses (suggest-shop.yahooapis.jp, appid built in). Returns keyword completions plus, with detailed (default), category candidates and direct store links. No login.", json!({
            "type": "object",
            "properties": {
                "keyword": {"type": "string", "description": "Partial keyword to complete"},
                "limit": {"type": "integer", "default": 20, "description": "Max suggestions (the app's fixed 20)"},
                "detailed": {"type": "boolean", "default": true, "description": "Also return category and store suggestions"}
            },
            "required": ["keyword"]
        })),

        // ── Amazon ──
        tool_def("amazon_search_products", "Search Amazon.co.jp products by reproducing the Amazon Shopping app's WebView requests (app-format user agent and the app's pre-navigation cookies; no browser engine). sort (the web s parameter): relevancerank, price-asc-rank, price-desc-rank, review-rank, date-desc-rank. Refinements use Amazon's web facet vocabulary, passed through as the app does: department (i:), category_node (n:), price band (p_36), prime_only (p_85), in_stock_only, min_rating (p_72), brand (p_89), seller_id (me), plus raw rh entries. Note: Amazon serves these pages only to Japan-side access in practice.", json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search keyword (k)"},
                "asin": {"type": "string", "default": "", "description": "ASIN to search for directly (field-keywords=ASIN form); takes the place of the keyword search"},
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
                "min_rating": {"type": "integer", "minimum": 1, "maximum": 4, "description": "Minimum star rating, 1-4"},
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

        tool_def("amazon_suggest_keywords", "Get Amazon.co.jp keyword suggestions through the completion endpoint the Amazon Shopping app uses. No login. Requires Japan-side access in practice (see amazon_search_products). Note: Amazon product reviews are not exposed — the review pages sit behind a sign-in wall (verified), so no review tool exists.", json!({
            "type": "object",
            "properties": {
                "keyword": {"type": "string", "description": "Partial keyword to complete"},
                "limit": {"type": "integer", "default": 10}
            },
            "required": ["keyword"]
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
            let options = shopping_mcp_aliexpress::SearchOptions {
                min_price: opt_str(args, "min_price"),
                max_price: opt_str(args, "max_price"),
                switches: str_array(args, "switches"),
            };
            let result = p
                .aliexpress
                .search_products_with_options(
                    &str_param(args, "query"),
                    int_param(args, "page", 1),
                    int_param(args, "limit", 10),
                    &str_or(args, "locale", "ja_JP"),
                    &str_or(args, "currency", "JPY"),
                    &str_or(args, "ship_to", "JP"),
                    &str_param(args, "sort"),
                    &options,
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
            let options = shopping_mcp_aliexpress::ReviewOptions::new(
                str_or(args, "filter", "all"),
                str_or(args, "sort", "complex_default"),
            );
            let result = p
                .aliexpress
                .get_reviews_with_options(
                    &product_param(args),
                    int_param(args, "page", 1),
                    int_param(args, "limit", 20),
                    &str_or(args, "locale", "ja_JP"),
                    &options,
                )
                .await;
            Ok(result)
        }
        "aliexpress_query_sku" | "aliexpress_adjust_sku" | "aliexpress_adjust_pdp" => {
            let options = shopping_mcp_aliexpress::SkuSelectionOptions {
                quantity: int_param(args, "quantity", 1),
                pdp_npi: str_param(args, "pdp_npi"),
                pdp_ext_f: str_param(args, "pdp_ext_f"),
            };
            let product = product_param(args);
            let sku_id = str_param(args, "sku_id");
            let locale = str_or(args, "locale", "ja_JP");
            let currency = str_or(args, "currency", "JPY");
            let ship_to = str_or(args, "ship_to", "JP");
            let result = match tool {
                "aliexpress_query_sku" => {
                    p.aliexpress
                        .query_sku(&product, &sku_id, &locale, &currency, &ship_to, &options)
                        .await
                }
                "aliexpress_adjust_sku" => {
                    p.aliexpress
                        .adjust_sku(&product, &sku_id, &locale, &currency, &ship_to, &options)
                        .await
                }
                _ => {
                    p.aliexpress
                        .adjust_pdp(&product, &sku_id, &locale, &currency, &ship_to, &options)
                        .await
                }
            };
            Ok(result)
        }
        "aliexpress_get_store_groups" => {
            let result = p
                .aliexpress
                .get_store_groups(
                    &str_param(args, "store_id"),
                    &str_param(args, "group_type"),
                    &str_param(args, "seller_id"),
                    &str_or(args, "locale", "ja_JP"),
                    &str_or(args, "currency", "JPY"),
                    &str_or(args, "ship_to", "JP"),
                )
                .await;
            Ok(result)
        }
        "aliexpress_get_store_module" => {
            let result = p
                .aliexpress
                .get_store_module(
                    &str_param(args, "store_id"),
                    &str_param(args, "component_key"),
                    &str_param(args, "seller_id"),
                    &str_or(args, "locale", "ja_JP"),
                    &str_or(args, "currency", "JPY"),
                    &str_or(args, "ship_to", "JP"),
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
            let query = str_param(args, "query");
            let page = int_param(args, "page", 1) as i64;
            let limit = int_param(args, "limit", 10) as i64;
            let sort = opt_str(args, "sort");
            let price_min = opt_str(args, "price_min");
            let price_max = opt_str(args, "price_max");
            let prop_selections: Vec<(String, Vec<String>)> = args
                .get("prop_selections")
                .and_then(|v| v.as_object())
                .map(|m| {
                    m.iter()
                        .map(|(pid, vids)| {
                            let vids = vids
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|v| {
                                            v.as_str()
                                                .map(str::to_string)
                                                .or_else(|| v.as_i64().map(|n| n.to_string()))
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            (pid.clone(), vids)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let price_range = opt_str(args, "price_range");
            let province = opt_str(args, "province");
            let city = opt_str(args, "city");
            let area = opt_str(args, "area");
            let category_id = opt_str(args, "category_id");
            let front_cat_id = opt_str(args, "front_cat_id");
            let leaf_id = opt_str(args, "leaf_id");
            let has_filter_args = !prop_selections.is_empty()
                || price_range.is_some()
                || province.is_some()
                || city.is_some()
                || area.is_some()
                || category_id.is_some()
                || front_cat_id.is_some()
                || leaf_id.is_some();
            let result = if has_filter_args {
                let mut filters = shopping_mcp_goofish::SearchFilters {
                    prop_selections,
                    price_range_value: price_range,
                    start_price: price_min.clone(),
                    end_price: price_max.clone(),
                    province,
                    city,
                    area,
                    category_id,
                    front_cat_id,
                    leaf_id,
                    ..Default::default()
                };
                let sort_pair = match sort.as_deref() {
                    Some("create") | Some("newest") => Some(("create", "desc")),
                    Some("create_asc") => Some(("create", "asc")),
                    Some("credit") => Some(("credit", "desc")),
                    Some("price_asc") => Some(("price", "asc")),
                    Some("price_desc") | Some("price") => Some(("price", "desc")),
                    _ => None,
                };
                if let Some((field, value)) = sort_pair {
                    filters.sort_field = Some(field.to_string());
                    filters.sort_value = Some(value.to_string());
                }
                p.goofish
                    .search_products_with_filters(&query, page, limit, filters)
                    .await
            } else {
                p.goofish
                    .search_products(
                        &query,
                        page,
                        limit,
                        sort.as_deref(),
                        price_min.as_deref(),
                        price_max.as_deref(),
                    )
                    .await
            };
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
                .get_comments(
                    &product_param(args),
                    int_param(args, "rows_per_page", 10) as i64,
                )
                .await;
            Ok(result)
        }
        "goofish_get_recommendations" => {
            let mut state = match args.get("page_state") {
                Some(ps) => shopping_mcp_goofish::RecommendPagination {
                    page_num: ps.get("page_num").and_then(|v| v.as_i64()).unwrap_or(0),
                    page_size: ps.get("page_size").and_then(|v| v.as_i64()).unwrap_or(20),
                    current_size: ps.get("current_size").and_then(|v| v.as_i64()).unwrap_or(0),
                    history_item_ids: ps
                        .get("history_item_ids")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                },
                None => shopping_mcp_goofish::RecommendPagination::default(),
            };
            let mut result = p
                .goofish
                .get_recommendations_with_state(&product_param(args), &state)
                .await;
            if result.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                let data = result.get("data").cloned().unwrap_or(Value::Null);
                let has_more = state.advance(&data);
                if let Value::Object(m) = &mut result {
                    m.insert(
                        "next_page_state".to_string(),
                        json!({
                            "page_num": state.page_num,
                            "page_size": state.page_size,
                            "current_size": state.current_size,
                            "history_item_ids": state.history_item_ids,
                        }),
                    );
                    m.insert("has_more".to_string(), json!(has_more));
                }
            }
            Ok(result)
        }
        "goofish_home_feed" => match args.get("page_state") {
            Some(ps) => {
                let mut state = shopping_mcp_goofish::HomePagination {
                    page_number: ps.get("page_number").and_then(|v| v.as_i64()).unwrap_or(1),
                    last_response_count: ps
                        .get("last_response_count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    client_recycle_params: ps
                        .get("client_recycle_params")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                };
                let mut result = p.goofish.home_feed_with_state(&state).await;
                if result.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                    let data = result.get("data").cloned().unwrap_or(Value::Null);
                    state.advance(&data);
                    if let Value::Object(m) = &mut result {
                        m.insert(
                            "next_page_state".to_string(),
                            json!({
                                "page_number": state.page_number,
                                "last_response_count": state.last_response_count,
                                "client_recycle_params": state.client_recycle_params,
                            }),
                        );
                    }
                }
                Ok(result)
            }
            None => {
                let result = p.goofish.home_feed(int_param(args, "page", 1) as i64).await;
                Ok(result)
            }
        },
        "goofish_get_similar_items" => {
            let conditions = args.get("conditions").and_then(|v| v.as_object()).cloned();
            let mut state = shopping_mcp_goofish::PageNoPagination {
                page_no: int_param(args, "page_no", 1) as i64,
                page_size: int_param(args, "page_size", 10) as i64,
            };
            let mut result = p
                .goofish
                .get_similar_items(&product_param(args), conditions, Some(&state))
                .await;
            if result.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                let data = result.get("data").cloned().unwrap_or(Value::Null);
                let has_more = state.advance(&data);
                if let Value::Object(m) = &mut result {
                    m.insert(
                        "next_page_state".to_string(),
                        json!({
                            "page_no": state.page_no,
                            "page_size": state.page_size,
                        }),
                    );
                    m.insert("has_more".to_string(), json!(has_more));
                }
            }
            Ok(result)
        }
        "goofish_get_category_guide" => {
            let name = opt_str(args, "name");
            Ok(p.goofish.get_category_guide(name.as_deref()).await)
        }
        "goofish_get_rate_list" => Ok(p
            .goofish
            .get_rate_list(
                &product_param(args),
                &str_param(args, "rated_uid"),
                int_param(args, "page", 1) as i64,
            )
            .await),
        "goofish_get_user_headinfo" => {
            let user_id = opt_str(args, "user_id");
            let nick = opt_str(args, "nick");
            Ok(p.goofish
                .get_user_headinfo(
                    user_id.as_deref(),
                    nick.as_deref(),
                    opt_i64(args, "my_user_id"),
                )
                .await)
        }
        "goofish_get_seller_items" => Ok(p
            .goofish
            .get_seller_items(
                int_param(args, "page", 1) as i64,
                int_param(args, "page_size", 20) as i64,
            )
            .await),
        "goofish_get_city_feed" => {
            let opts = shopping_mcp_goofish::CityFeedOptions {
                version: str_param(args, "version"),
                data_source_id: str_param(args, "data_source_id"),
                tab_name: opt_str(args, "tab_name"),
                page_number: str_or(args, "page_number", "1"),
                last_response_count: str_or(args, "last_response_count", "0"),
                keywords: opt_str(args, "keywords"),
                latitude: opt_f64(args, "latitude"),
                longitude: opt_f64(args, "longitude"),
                feeds_api: bool_param(args, "feeds_api", false),
                extra_fields: None,
            };
            Ok(p.goofish.get_city_feed(&opts).await)
        }
        "goofish_get_city_nearby" => {
            let city = opt_str(args, "city");
            let latitude = opt_str(args, "latitude");
            let longitude = opt_str(args, "longitude");
            let select_city = opt_str(args, "select_city");
            Ok(p.goofish
                .get_city_nearby(
                    city.as_deref(),
                    latitude.as_deref(),
                    longitude.as_deref(),
                    select_city.as_deref(),
                )
                .await)
        }
        "goofish_get_fishpond_list" => Ok(p
            .goofish
            .get_fishpond_list(
                int_param(args, "page", 1) as i64,
                int_param(args, "limit", 20) as i64,
            )
            .await),
        "goofish_get_fishpond_info" => Ok(p
            .goofish
            .get_fishpond_info(opt_i64(args, "pool_id").unwrap_or(0))
            .await),
        "goofish_get_fishpond_comments" => Ok(p
            .goofish
            .get_fishpond_comments(
                opt_i64(args, "pool_id").unwrap_or(0),
                int_param(args, "page", 1) as i64,
                opt_f64(args, "latitude"),
                opt_f64(args, "longitude"),
            )
            .await),
        "goofish_get_video_play" => Ok(p
            .goofish
            .get_video_play(
                opt_i64(args, "video_id").unwrap_or(0),
                opt_i64(args, "item_id").unwrap_or(0),
            )
            .await),
        "goofish_get_video_play_from_feed_card" => {
            let card = args.get("card").cloned().unwrap_or(Value::Null);
            Ok(p.goofish
                .get_video_play_from_feed_card(&card, opt_i64(args, "item_id"))
                .await)
        }
        "goofish_get_fun_comments" => Ok(p
            .goofish
            .get_fun_comments(
                &str_param(args, "target_id"),
                &str_param(args, "target_type"),
                &str_param(args, "namespace"),
                &str_param(args, "source_id"),
                opt_str(args, "begin_cursor").as_deref(),
                int_param(args, "page_size", 10) as i64,
                int_param(args, "rows_per_page", 10) as i64,
                opt_str(args, "top_comment_id").as_deref(),
            )
            .await),

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
                has_coupon: bool_param(args, "has_coupon", false),
                attributes: args
                    .get("attributes")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default(),
                sku_ids: int_array(args, "sku_ids"),
                shop_ids: int_array(args, "shop_ids"),
                exclude_shipping_method_ids: int_array(args, "exclude_shipping_method_ids"),
                shipping_from_area: int_array(args, "shipping_from_area"),
                shipping_method: int_array(args, "shipping_method"),
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
                    int_param(args, "page", 1),
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
                    int_param(args, "page", 1),
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
                brand_ids: str_param(args, "brand_ids"),
                catalog_ids: str_param(args, "catalog_ids"),
                min_buy_now_price: opt_i64(args, "min_buy_now_price"),
                max_buy_now_price: opt_i64(args, "max_buy_now_price"),
                seller_type: str_param(args, "seller_type"),
                prefecture_code: str_param(args, "prefecture_code"),
                destination_prefecture_code: str_param(args, "destination_prefecture_code"),
                is_featured: opt_bool(args, "is_featured"),
                is_appraisal: opt_bool(args, "is_appraisal"),
                is_finish_soon: opt_bool(args, "is_finish_soon"),
                is_convenience_store: opt_bool(args, "is_convenience_store"),
                is_new_arrival: opt_bool(args, "is_new_arrival"),
                except_suspected_fake: opt_bool(args, "except_suspected_fake"),
                can_privacy_delivery: opt_bool(args, "can_privacy_delivery"),
                is_adult_ok: opt_bool(args, "is_adult_ok"),
                auction_ids: str_param(args, "auction_ids"),
                except_auction_ids: str_param(args, "except_auction_ids"),
                except_auc_user_ids: str_param(args, "except_auc_user_ids"),
                except_catalog_ids: str_param(args, "except_catalog_ids"),
                shp_spec: str_param(args, "shp_spec"),
                spec_size_type: str_param(args, "spec_size_type"),
                spec_size_ids: str_param(args, "spec_size_ids"),
                conditions: str_param(args, "conditions"),
                submit_type: str_param(args, "submit_type"),
                priority: str_param(args, "priority"),
                image_url: str_param(args, "image_url"),
                search_type: str_param(args, "search_type"),
                time_from_first_start: opt_i64(args, "time_from_first_start"),
                rewrite_query_type: str_param(args, "rewrite_query_type"),
                campaign_type: str_param(args, "campaign_type"),
                campaign_start_time: opt_i64(args, "campaign_start_time"),
                campaign_end_time: opt_i64(args, "campaign_end_time"),
                module: str_param(args, "module"),
                src: str_param(args, "src"),
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
                .suggest_keywords_with_limits(
                    &keyword,
                    int_param(args, "limit", 10),
                    opt_int(args, "category_limit"),
                    opt_int(args, "brand_limit"),
                )
                .await?;
            Ok(serde_json::to_value(result)?)
        }
        "yauction_get_statistics" => {
            let params = shopping_mcp_yahoo_auctions::StatisticsParams {
                catalog_id: str_param(args, "catalog_id"),
                conditions: str_param(args, "conditions")
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                ..Default::default()
            };
            let result = p.yauction.get_statistics(params).await?;
            Ok(serde_json::to_value(result)?)
        }
        "yauction_get_statistics_closed" => {
            let params = shopping_mcp_yahoo_auctions::StatisticsParams {
                catalog_id: str_param(args, "catalog_id"),
                conditions: str_param(args, "conditions")
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                shp_spec: str_param(args, "shp_spec"),
                seller_type: str_param(args, "seller_type"),
            };
            let result = p.yauction.get_statistics_closed(params).await?;
            Ok(serde_json::to_value(result)?)
        }
        "yauction_get_seller_profile" => {
            let result = p
                .yauction
                .get_seller_profile(&str_param(args, "seller_id"))
                .await?;
            Ok(serde_json::to_value(result)?)
        }
        "yauction_get_store_profile" => {
            let result = p
                .yauction
                .get_store_profile(&str_param(args, "seller_id"))
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
        "rakuten_suggest_keywords" => {
            let mut result = p
                .rakuten
                .suggest_keywords(&str_param(args, "keyword"))
                .await?;
            let limit = int_param(args, "limit", 10);
            if limit > 0 {
                if let Some(arr) = result.get_mut("suggestions").and_then(|v| v.as_array_mut()) {
                    arr.truncate(limit as usize);
                    let n = arr.len();
                    if let Some(obj) = result.as_object_mut() {
                        obj.insert("count".to_string(), json!(n));
                    }
                }
            }
            Ok(result)
        }
        "rakuten_search_products_aggregated" => {
            let params = shopping_mcp_rakuten::ProductSearchParams {
                keyword: str_param(args, "keyword"),
                page: int_param(args, "page", 1),
                hits: int_param(args, "hits", 30),
                genre_id: opt_i64(args, "genre_id"),
                min_price: opt_i64(args, "min_price"),
                max_price: opt_i64(args, "max_price"),
                product_code: opt_str(args, "product_code"),
                product_id: opt_str(args, "product_id"),
                keyword_search_type: int_param(args, "keyword_search_type", 0),
                genre_information_flag: int_param(args, "genre_information_flag", 0),
            };
            Ok(p.rakuten.search_products_aggregated(params).await?)
        }
        "rakuten_get_genre_tree" => {
            let params = shopping_mcp_rakuten::GenreSearchParams {
                genre_id: opt_i64(args, "genre_id").unwrap_or(0),
                include_children: bool_param(args, "include_children", true),
                include_brother: bool_param(args, "include_brother", false),
                include_tag_group: bool_param(args, "include_tag_group", false),
                include_tag: bool_param(args, "include_tag", false),
            };
            Ok(p.rakuten.get_genre_tree(params).await?)
        }
        "rakuten_get_variant_details" => {
            let params = shopping_mcp_rakuten::VariantDetailsParams {
                shop_id: opt_i64(args, "shop_id").unwrap_or(0),
                item_id: opt_i64(args, "item_id").unwrap_or(0),
                variant_id: opt_str(args, "variant_id"),
                is_available_for_sale: bool_param(args, "is_available_for_sale", true),
                postal_code: opt_str(args, "postal_code"),
                prefecture_id: opt_i64(args, "prefecture_id").unwrap_or(0),
                article_number: opt_str(args, "article_number"),
                genre_id: opt_i64(args, "genre_id"),
                standard_price: opt_i64(args, "standard_price"),
                tax_included_price: opt_i64(args, "tax_included_price"),
                inventory: opt_i64(args, "inventory"),
                is_subscription_eligible: bool_param(args, "is_subscription_eligible", false),
                delivery_caption_variant: args.get("delivery_caption_variant").cloned(),
                ..Default::default()
            };
            Ok(p.rakuten.get_variant_details(params).await?)
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
        "qoo10_suggest_keywords" => Ok(p
            .qoo10
            .suggest_keywords(&str_param(args, "keyword"), int_param(args, "limit", 10))
            .await?),
        "qoo10_get_search_filters" => Ok(p
            .qoo10
            .get_search_filters(
                &str_param(args, "keyword"),
                &str_param(args, "category"),
                &str_array(args, "brand"),
            )
            .await?),

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
                offset: int_param(args, "offset", 0),
                sort: str_param(args, "sort"),
                rating: opt_i64(args, "rating").map(|v| v as i32),
                sub_review_id: opt_i64(args, "sub_review_id"),
            };
            let result = p.yshopping.get_reviews(params).await?;
            Ok(result)
        }
        "yshopping_suggest_keywords" => Ok(p
            .yshopping
            .suggest_keywords(
                &str_param(args, "keyword"),
                int_param(args, "limit", 20),
                bool_param(args, "detailed", true),
            )
            .await?),

        // ── Amazon ──
        "amazon_search_products" => {
            let params = shopping_mcp_amazon::SearchParams {
                query: str_param(args, "query"),
                asin: str_param(args, "asin"),
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
        "amazon_suggest_keywords" => {
            let result = p
                .amazon
                .suggest_keywords(&str_param(args, "keyword"), int_param(args, "limit", 10))
                .await?;
            Ok(result)
        }

        // ── Meta ──
        "shopping_list_sources" => Ok(json!({
            "server": "ZeroSearch_MCP",
            "providers": [
                {
                    "name": "aliexpress",
                    "display_name": "AliExpress",
                    "auth": "anonymous (MTop H5 cookie signing)",
                    "tools": ["aliexpress_search_products", "aliexpress_get_product", "aliexpress_get_reviews", "aliexpress_suggest_keywords", "aliexpress_search_store_products", "aliexpress_query_sku", "aliexpress_adjust_sku", "aliexpress_adjust_pdp", "aliexpress_get_store_groups", "aliexpress_get_store_module"]
                },
                {
                    "name": "goofish",
                    "display_name": "Goofish (闲鱼 / Xianyu)",
                    "auth": "anonymous (MTOP + SecurityGuard signer runtime bundled with the server, auto-started; identity auto-generated by the server)",
                    "tools": ["goofish_search_products", "goofish_get_product", "goofish_suggest_keywords", "goofish_get_comments", "goofish_get_recommendations", "goofish_home_feed", "goofish_get_similar_items", "goofish_get_category_guide", "goofish_get_rate_list", "goofish_get_user_headinfo", "goofish_get_seller_items", "goofish_get_city_feed", "goofish_get_city_nearby", "goofish_get_fishpond_list", "goofish_get_fishpond_info", "goofish_get_fishpond_comments", "goofish_get_video_play", "goofish_get_video_play_from_feed_card", "goofish_get_fun_comments"]
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
                    "tools": ["yauction_search_products", "yauction_get_product", "yauction_get_reviews", "yauction_suggest_keywords", "yauction_get_statistics", "yauction_get_statistics_closed", "yauction_get_seller_profile", "yauction_get_store_profile"]
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
                    "tools": ["rakuten_search_products", "rakuten_get_product", "rakuten_get_reviews", "rakuten_suggest_keywords", "rakuten_search_products_aggregated", "rakuten_get_genre_tree", "rakuten_get_variant_details"]
                },
                {
                    "name": "qoo10",
                    "display_name": "Qoo10 Japan",
                    "auth": "anonymous (app built-in certification key; Japan-side access required)",
                    "tools": ["qoo10_search_products", "qoo10_get_product", "qoo10_suggest_keywords", "qoo10_get_search_filters"]
                },
                {
                    "name": "yahoo_shopping",
                    "display_name": "Yahoo! Shopping Japan (Yahoo!ショッピング)",
                    "auth": "anonymous (official app API, appid)",
                    "tools": ["yshopping_search_products", "yshopping_get_product", "yshopping_get_reviews", "yshopping_suggest_keywords"]
                },
                {
                    "name": "amazon",
                    "display_name": "Amazon.co.jp (Amazon Shopping app WebView)",
                    "auth": "anonymous (app WebView reproduction; Japan-side access required)",
                    "tools": ["amazon_search_products", "amazon_get_product", "amazon_suggest_keywords"]
                }
            ]
        })),

        _ => anyhow::bail!("Unknown tool: {}", tool),
    }
}
