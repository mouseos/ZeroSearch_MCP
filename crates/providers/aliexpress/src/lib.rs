use anyhow::Result;
use regex::Regex;
use reqwest::cookie::Jar;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, LazyLock};
use std::time::{SystemTime, UNIX_EPOCH};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";
const APP_KEY: &str = "12574478";
const PDP_WEB_API: &str = "mtop.aliexpress.pdp.pc.query";
const PDP_WEB_API_VERSION: &str = "1.0";
#[allow(dead_code)]
const ANDROID_PDP_MAIN_API: &str = "mtop.aliexpress.pdp.main.query";
const ANDROID_PDP_BATCH_API: &str = "mtop.aliexpress.pdp.main.batchQueryCache";
const ANDROID_PDP_API_VERSION: &str = "1.0";
#[allow(dead_code)]
const ANDROID_SKU_QUERY_API: &str = "mtop.aliexpress.pdp.sku.query";
#[allow(dead_code)]
const ANDROID_SKU_ADJUST_API: &str = "mtop.aliexpress.pdp.sku.adjust";
#[allow(dead_code)]
const ANDROID_PDP_ADJUST_API: &str = "mtop.aliexpress.pdp.main.adjust";
const SEARCH_API: &str = "mtop.relationrecommend.aliexpressrecommend.recommend";
const SEARCH_API_VERSION: &str = "1.0";
const SEARCH_APP_ID: &str = "999";
const SUGGEST_ACS_HOST: &str = "recom-acs.aliexpress.com";
const SUGGEST_APP_ID: i32 = 18390;
const STORE_PRODUCTS_API: &str = "mtop.aliexpress.store.products.search.wysisyg";
const STORE_API_VERSION: &str = "1.0";
const REVIEW_API: &str = "mtop.aliexpress.review.pc.list";
const REVIEW_API_VERSION: &str = "1.0";
// APK suggestion API (AE-RUST-017): a distinct API from the recommend
// reuse below. Only used when SHOPPING_MCP_AE_SUGGEST_METHOD=apk.
const SUGGEST_APK_API: &str = "mtop.aliexpress.international.search.mobile.suggestion";
const SUGGEST_APK_API_VERSION: &str = "1.0";

// App version bundle for the analysed APK 8.164.1 (AE-RUST-010): the
// version-tied search constants live in exactly one place so a future
// APK bump touches a single block. Sources: docs/analysis/
// aliexpress-8.164.1.md (appVersion build number, app_client_v, ttid
// channel string, sversion/apiversion as required search parameters).
struct AppProfile {
    app_version: &'static str,
    app_client_v: &'static str,
    ttid: &'static str,
    sversion: &'static str,
    apiversion: &'static str,
    // Audit classification "unknown"; sent by the original code and
    // kept for wire-shape parity, not asserted to be required.
    os_version: &'static str,
    device_level: &'static str,
    ab_bucket: &'static str,
}
const APP_PROFILE: AppProfile = AppProfile {
    app_version: "80005893",
    app_client_v: "8.164.1",
    ttid: "play_google-play@Aliexpress_Android_8.164.1",
    sversion: "4.5",
    apiversion: "103",
    os_version: "14",
    device_level: "1003",
    ab_bucket: "unifiedRapidFilter,aeDelivery",
};

// NOTE (AE-RUST-007): there is intentionally NO default pv_feature
// constant. The old value was a captured list of product ids from a
// past session (24 entries, 6 unique); pv_feature is search-session
// state and is built by AliExpressClient from its own results.
// NOTE (AE-RUST-008): there is intentionally NO default device id
// constant. The old value was one captured handset's id; the default
// is now generated per client instance (see generate_device_id).

static PRODUCT_ID_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"/item/(\d+)\.html").unwrap(),
        Regex::new(r"/i/(\d+)\.html").unwrap(),
        Regex::new(r"[?&]productId=(\d+)").unwrap(),
        Regex::new(r"[?&]productIds=(\d+)").unwrap(),
        Regex::new(r"\b(1005\d{9,})\b").unwrap(),
    ]
});

static HTML_TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static NUMERIC_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{8,}$").unwrap());
// NOTE (AE-RUST-023): the old SHIPPING_AMOUNT_RE was dead code and
// has been removed; the Python parsers do not extract shipping
// amounts from free text anywhere, so no replacement was invented.
static NUMBER_TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"-?\d[\d,]*(?:\.\d+)?").unwrap());

static LOCALE_HOSTS: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("ar", "ar.aliexpress.com");
    m.insert("de", "de.aliexpress.com");
    m.insert("en", "www.aliexpress.com");
    m.insert("es", "es.aliexpress.com");
    m.insert("fr", "fr.aliexpress.com");
    m.insert("he", "he.aliexpress.com");
    m.insert("id", "id.aliexpress.com");
    m.insert("it", "it.aliexpress.com");
    m.insert("ja", "ja.aliexpress.com");
    m.insert("ko", "ko.aliexpress.com");
    m.insert("nl", "nl.aliexpress.com");
    m.insert("pl", "pl.aliexpress.com");
    m.insert("pt", "pt.aliexpress.com");
    m.insert("th", "th.aliexpress.com");
    m.insert("tr", "tr.aliexpress.com");
    m.insert("vi", "vi.aliexpress.com");
    m
});

/// Generate a fresh opaque device id for one client instance
/// (AE-RUST-008). The shape mimics the observed format (base64-ish
/// token with a `/` and trailing marker) without copying any
/// captured value.
fn generate_device_id() -> String {
    let a = uuid::Uuid::new_v4().simple().to_string();
    let b = uuid::Uuid::new_v4().simple().to_string();
    format!("{}/v4DA{}", &a[..24], &b[..8])
}

pub struct AliExpressClient {
    client: reqwest::Client,
    jar: Arc<Jar>,
    device_id: String,
    token: std::sync::Mutex<String>,
    // Search-session exposure state (AE-RUST-007): product ids this
    // client's own searches have returned, in exposure order, plus
    // the most recent page's ids. Used to build pv_feature /
    // prePageExposureProducts for subsequent searches.
    exposed_ids: std::sync::Mutex<Vec<String>>,
    last_page_ids: std::sync::Mutex<Vec<String>>,
}

impl AliExpressClient {
    pub fn new() -> Self {
        // AE-RUST-008: default is a freshly generated per-instance id
        // (stable for the life of this client); the captured handset
        // value is never reused. Env still overrides.
        let device_id = env::var("SHOPPING_MCP_AE_DEVICE_ID")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(generate_device_id);
        let jar = Arc::new(Jar::default());
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(20))
            .cookie_provider(Arc::clone(&jar))
            .build()
            .unwrap();
        Self {
            client,
            jar,
            device_id,
            token: std::sync::Mutex::new(String::new()),
            exposed_ids: std::sync::Mutex::new(Vec::new()),
            last_page_ids: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// The `pv_feature` value for the next search, or None when this
    /// session has not exposed anything yet (the parameter is then
    /// omitted entirely, AE-RUST-007).
    fn pv_feature_for_next(&self) -> Option<String> {
        let exposed = self.exposed_ids.lock().unwrap();
        if exposed.is_empty() {
            None
        } else {
            Some(exposed.join(","))
        }
    }

    /// The `prePageExposureProducts` value: only the previous page's
    /// ids (kept distinct from the cumulative pv_feature list).
    fn pre_page_exposure(&self) -> Option<String> {
        let last = self.last_page_ids.lock().unwrap();
        if last.is_empty() {
            None
        } else {
            Some(last.join(","))
        }
    }

    /// Record a returned page's product ids into the session state.
    fn record_search_exposure(&self, page_ids: Vec<String>) {
        let mut last = self.last_page_ids.lock().unwrap();
        *last = page_ids.clone();
        drop(last);
        let mut exposed = self.exposed_ids.lock().unwrap();
        for id in page_ids {
            if !exposed.contains(&id) {
                exposed.push(id);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn search_products(
        &self,
        query: &str,
        page: i32,
        limit: i32,
        locale: &str,
        currency: &str,
        ship_to: &str,
        sort: &str,
    ) -> Value {
        if query.trim().is_empty() {
            return json!({"provider": "aliexpress", "ok": false, "error": {"message": "query must not be empty"}});
        }
        let page = page.max(1);
        let limit = limit.clamp(1, 50);
        // AE-RUST-005: the wire pageSize is the app-sent value 20,
        // kept independent of `limit` exactly as in the Python client
        // (page_size=20 default; limit only caps the returned items).
        // Deriving pageSize from limit was tried and is wrong: the
        // server answers pageSize=3 with request_invalid even though
        // ret is SUCCESS (measured live 2026-10-31 against the Python
        // client: page_size 3 fails, 5 and 20 succeed).
        let page_size = 20;
        let host = host_for_locale(locale);
        self.set_region_cookies(&host, locale, currency, ship_to)
            .await;

        // AE-RUST-007: pv_feature / prePageExposureProducts come from
        // this session's own earlier results; both are omitted on the
        // first search of a session.
        let pv_feature = self.pv_feature_for_next();
        let pre_page = if page > 1 {
            self.pre_page_exposure()
        } else {
            None
        };
        let search_params = build_search_params(
            query,
            page,
            page_size,
            locale,
            currency,
            ship_to,
            &self.device_id,
            sort,
            pv_feature.as_deref(),
            pre_page.as_deref(),
            None,
            None,
            &[],
        );
        let region = search_region_payload(locale, currency, ship_to);
        let mut data_obj = region;
        data_obj.insert("appId".to_string(), json!(SEARCH_APP_ID));
        data_obj.insert(
            "params".to_string(),
            json!(serde_json::to_string(&search_params).unwrap()),
        );
        let data = serde_json::to_string(&data_obj).unwrap();
        let referer = format!("https://{}/", host);

        match self
            .signed_mtop_request_with_retry(
                "acs.aliexpress.com",
                SEARCH_API,
                SEARCH_API_VERSION,
                &data,
                &referer,
            )
            .await
        {
            Ok(call) => {
                let response = &call.response;
                if call.category != "success" {
                    return mtop_failure_json("search", &call);
                }
                let resp_data = response.get("data").cloned().unwrap_or(json!({}));
                let items = normalize_search_items(&resp_data, &format!("https://{}/", host));
                let page_ids: Vec<String> = items
                    .iter()
                    .filter_map(|i| {
                        i.get("product_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();
                self.record_search_exposure(page_ids);
                let count = items.len().min(limit as usize);
                let truncated: Vec<Value> = items.into_iter().take(limit as usize).collect();
                let total = find_search_int(
                    &resp_data,
                    &[
                        "total",
                        "totalCount",
                        "totalNum",
                        "totalResults",
                        "totalItem",
                    ],
                );
                let total_pages = find_search_int(&resp_data, &["totalPage", "totalPages"]);
                let resp_page_size = find_search_int(&resp_data, &["pageSize", "page_size"]);
                let has_next = if let Some(tp) = total_pages {
                    Some((page as i64) < tp)
                } else if let Some(t) = total {
                    let per = resp_page_size.unwrap_or(page_size as i64);
                    if per > 0 {
                        Some((page as i64) * per < t)
                    } else {
                        None
                    }
                } else {
                    None
                };
                json!({
                    "provider": "aliexpress",
                    "ok": true,
                    "category": call.category,
                    "query": query,
                    "page": page,
                    "page_size": page_size,
                    "sort": sort,
                    "locale": locale,
                    "currency": currency,
                    "ship_to": ship_to,
                    "items": truncated,
                    "count": count,
                    "total": total,
                    "pagination": {
                        "page": page,
                        "page_size": resp_page_size.unwrap_or(page_size as i64),
                        "total": total,
                        "total_pages": total_pages,
                        "has_next": has_next,
                    },
                    "ret": response.get("ret"),
                    "trace_id": response.get("traceId"),
                    "http_status": call.http_status,
                    "attempts": call.attempts,
                })
            }
            Err(e) => {
                json!({
                    "provider": "aliexpress",
                    "ok": false,
                    "error": {"message": e.to_string()},
                })
            }
        }
    }

    #[allow(clippy::if_same_then_else)]
    pub async fn get_product(
        &self,
        product: &str,
        locale: &str,
        currency: &str,
        ship_to: &str,
    ) -> Value {
        let product_id = match extract_product_id(product) {
            Some(id) => id,
            None => {
                return json!({
                    "provider": "aliexpress",
                    "ok": false,
                    "error": {"message": format!("Could not find an AliExpress product id in: {}", product)},
                });
            }
        };
        let host = host_for_locale(locale);
        self.set_region_cookies(&host, locale, currency, ship_to)
            .await;
        let source_url = format!("https://{}/item/{}.html", host, product_id);

        // Product detail acceptance (AE-RUST-013/015/016): a route
        // counts as successful only when the protocol succeeded AND
        // the parsed detail matches the requested product id AND it
        // carries meaningful data (title/price/images). The adopted
        // route is recorded verbatim in "source". Blocking responses
        // (403/429/access denial/verification) abort the whole call
        // instead of cascading into more requests.
        let mut route_errors: Vec<String> = Vec::new();

        // Route 1: Android PDP batch
        match self
            .call_android_pdp_batch(&product_id, locale, currency, ship_to, &source_url)
            .await
        {
            Ok(call) => {
                if is_blocking_category(&call.category) {
                    return blocked_json(&product_id, "direct_mtop_android_pdp_batch", &call);
                }
                if call.category != "success" {
                    route_errors.push(format!(
                        "direct_mtop_android_pdp_batch: protocol[{}]",
                        call.category
                    ));
                } else {
                    let data = call.response.get("data").and_then(|d| d.as_object());
                    let entry = data
                        .and_then(|d| d.get(&product_id))
                        .and_then(|v| v.as_object());
                    match entry {
                        Some(entry) if !entry.is_empty() => {
                            let normalized = normalize_android_pdp_result(
                                &product_id,
                                &Value::Object(entry.clone()),
                                &source_url,
                                locale,
                                currency,
                                "direct_mtop_android_pdp_batch",
                            );
                            if detail_accepted(&normalized, &product_id) {
                                let mut out = normalized;
                                out["ok"] = json!(true);
                                out["http_status"] = json!(call.http_status);
                                out["attempts"] = json!(call.attempts);
                                return out;
                            }
                            route_errors.push("direct_mtop_android_pdp_batch: parsed detail failed acceptance (id match/meaningful data)".to_string());
                        }
                        _ => route_errors.push(
                            "direct_mtop_android_pdp_batch: no entry for product id".to_string(),
                        ),
                    }
                }
            }
            Err(e) => route_errors.push(format!("direct_mtop_android_pdp_batch: transport: {}", e)),
        }

        // Route 2: Android PDP main.query
        match self
            .call_android_pdp_main(&product_id, locale, currency, ship_to, &source_url)
            .await
        {
            Ok(call) => {
                if is_blocking_category(&call.category) {
                    return blocked_json(&product_id, "direct_mtop_android_pdp_main", &call);
                }
                if call.category != "success" {
                    route_errors.push(format!(
                        "direct_mtop_android_pdp_main: protocol[{}]",
                        call.category
                    ));
                } else {
                    match extract_android_pdp_entry(&call.response, &product_id) {
                        Some((entry, matched_by_key)) => {
                            // AE-RUST-014: a loose (non-keyed)
                            // candidate is accepted only if its own
                            // structured id fields declare this
                            // product; a candidate declaring a
                            // different product is rejected.
                            let declared = entry_declared_ids(&entry);
                            let id_confirmed = matched_by_key
                                || (!declared.is_empty() && declared.contains(&product_id));
                            if !declared.is_empty() && !declared.contains(&product_id) {
                                route_errors.push("direct_mtop_android_pdp_main: candidate entry does not structurally declare the product id".to_string());
                            } else if !id_confirmed {
                                route_errors.push("direct_mtop_android_pdp_main: candidate entry does not structurally declare the product id".to_string());
                            } else {
                                let normalized = normalize_android_pdp_result(
                                    &product_id,
                                    &entry,
                                    &source_url,
                                    locale,
                                    currency,
                                    "direct_mtop_android_pdp_main",
                                );
                                if detail_accepted(&normalized, &product_id) {
                                    let mut out = normalized;
                                    out["ok"] = json!(true);
                                    out["http_status"] = json!(call.http_status);
                                    out["attempts"] = json!(call.attempts);
                                    return out;
                                }
                                route_errors.push("direct_mtop_android_pdp_main: parsed detail failed acceptance (id match/meaningful data)".to_string());
                            }
                        }
                        None => route_errors
                            .push("direct_mtop_android_pdp_main: no PDP entry found".to_string()),
                    }
                }
            }
            Err(e) => route_errors.push(format!("direct_mtop_android_pdp_main: transport: {}", e)),
        }

        // Route 3: web PDP
        let acs_host = if host.ends_with(".us") {
            "acs.aliexpress.us"
        } else {
            "acs.aliexpress.com"
        };
        let site = site_for_locale(locale);
        let ext_params = json!({
            "site": site,
            "crawler": false,
            "signedIn": false,
            "host": host,
        });
        let data_obj = json!({
            "productId": product_id,
            "_lang": locale,
            "_currency": currency,
            "country": ship_to,
            "province": "",
            "city": "",
            "channel": null,
            "pdp_ext_f": null,
            "pdpNPI": null,
            "sourceType": null,
            "clientType": "pc",
            "ext": serde_json::to_string(&ext_params).unwrap(),
        });
        let data = serde_json::to_string(&data_obj).unwrap();

        match self
            .signed_mtop_request_with_retry(
                acs_host,
                PDP_WEB_API,
                PDP_WEB_API_VERSION,
                &data,
                &source_url,
            )
            .await
        {
            Ok(call) => {
                if is_blocking_category(&call.category) {
                    return blocked_json(&product_id, "direct_mtop_web_pdp", &call);
                }
                if call.category != "success" {
                    let mut out = mtop_failure_json("get_product", &call);
                    out["product_id"] = json!(product_id);
                    out["route_errors"] = json!(route_errors);
                    return out;
                }
                let result = call
                    .response
                    .get("data")
                    .and_then(|d| d.get("result"))
                    .cloned()
                    .unwrap_or(json!({}));
                if result.as_object().is_none_or(|o| o.is_empty()) {
                    route_errors.push("direct_mtop_web_pdp: empty data.result".to_string());
                    return json!({
                        "provider": "aliexpress",
                        "ok": false,
                        "product_id": product_id,
                        "error": {"message": "AliExpress PDP MTop returned no product result"},
                        "route_errors": route_errors,
                    });
                }
                let normalized =
                    normalize_pdp_result(&product_id, &result, &source_url, locale, currency);
                if detail_accepted(&normalized, &product_id) {
                    let mut out = normalized;
                    out["ok"] = json!(true);
                    out["http_status"] = json!(call.http_status);
                    out["attempts"] = json!(call.attempts);
                    return out;
                }
                route_errors.push("direct_mtop_web_pdp: parsed detail failed acceptance (id match/meaningful data)".to_string());
                json!({
                    "provider": "aliexpress",
                    "ok": false,
                    "product_id": product_id,
                    "error": {"message": format!("No PDP route produced meaningful data for product {}", product_id)},
                    "route_errors": route_errors,
                })
            }
            Err(e) => {
                route_errors.push(format!("direct_mtop_web_pdp: transport: {}", e));
                json!({
                    "provider": "aliexpress",
                    "ok": false,
                    "product_id": product_id,
                    "error": {"message": format!("No PDP route produced meaningful data for product {}", product_id)},
                    "route_errors": route_errors,
                })
            }
        }
    }

    pub async fn suggest_keywords(&self, keyword: &str, locale: &str, limit: i32) -> Value {
        if keyword.trim().is_empty() {
            return json!({"provider": "aliexpress", "ok": false, "error": {"message": "keyword must not be empty"}});
        }
        let limit = limit.clamp(1, 30);
        let language = locale.split('_').next().unwrap_or("en").to_lowercase();
        let ship_to = locale.split('_').nth(1).unwrap_or("US").to_uppercase();
        let host = host_for_locale(locale);
        let referer = format!("https://{}/", host);
        // NOTE (AE-RUST-009): no region cookies are forced here — the
        // old code hard-coded JPY for every locale. The recommend
        // request carries the caller's locale-derived country itself.

        // AE-RUST-017/018: the DEFAULT method is the recommend reuse
        // (the search recommend API on recom-acs with appId 18390),
        // which is the measured-working route in the Python SDK. It
        // is NOT the APK suggestion API and is not named as such.
        // Opt-in: SHOPPING_MCP_AE_SUGGEST_METHOD=apk performs exactly
        // one attempt against the APK suggestion API
        // (mtop.aliexpress.international.search.mobile.suggestion,
        // fields keyWord/_lang) and surfaces its failure — measured
        // to be uninvocable via the public H5 gateway
        // (FAIL_SYS_HSF_INVOKE_ERROR); it never falls back.
        let method =
            env::var("SHOPPING_MCP_AE_SUGGEST_METHOD").unwrap_or_else(|_| "recommend".to_string());
        if method == "apk" {
            let data = json!({
                "keyWord": keyword.trim(),
                "_lang": locale,
            });
            let data_str = serde_json::to_string(&data).unwrap();
            return match self
                .signed_mtop_request(
                    "acs.aliexpress.com",
                    SUGGEST_APK_API,
                    SUGGEST_APK_API_VERSION,
                    &data_str,
                    &referer,
                )
                .await
            {
                Ok(call) => {
                    if call.category != "success" {
                        let mut out = mtop_failure_json("suggest", &call);
                        out["keyword"] = json!(keyword);
                        out["locale"] = json!(locale);
                        out["method"] = json!("apk");
                        return out;
                    }
                    let (mut suggestions, shape) = parse_suggestions(&call.response);
                    suggestions.truncate(limit as usize);
                    json!({
                        "provider": "aliexpress",
                        "ok": true,
                        "category": call.category,
                        "keyword": keyword,
                        "locale": locale,
                        "method": "apk",
                        "shape": shape,
                        "suggestions": suggestions,
                        "ret": call.response.get("ret"),
                        "trace_id": call.response.get("traceId"),
                        "http_status": call.http_status,
                        "attempts": call.attempts,
                    })
                }
                Err(e) => json!({
                    "provider": "aliexpress",
                    "ok": false,
                    "error": {"message": e.to_string()},
                    "method": "apk",
                }),
            };
        }

        let params_inner = json!({
            "query": keyword.trim(),
            "language": language,
            "shipToCountry": ship_to,
        });
        let data = json!({
            "appId": SUGGEST_APP_ID,
            "params": serde_json::to_string(&params_inner).unwrap(),
        });
        let data_str = serde_json::to_string(&data).unwrap();

        match self
            .signed_mtop_request_with_retry(
                SUGGEST_ACS_HOST,
                SEARCH_API,
                SEARCH_API_VERSION,
                &data_str,
                &referer,
            )
            .await
        {
            Ok(call) => {
                if call.category != "success" {
                    let mut out = mtop_failure_json("suggest", &call);
                    out["keyword"] = json!(keyword);
                    out["locale"] = json!(locale);
                    out["method"] = json!("recommend");
                    return out;
                }
                let (mut suggestions, shape) = parse_suggestions(&call.response);
                suggestions.truncate(limit as usize);
                json!({
                    "provider": "aliexpress",
                    "ok": true,
                    "category": call.category,
                    "keyword": keyword,
                    "locale": locale,
                    "method": "recommend",
                    "shape": shape,
                    "suggestions": suggestions,
                    "ret": call.response.get("ret"),
                    "trace_id": call.response.get("traceId"),
                    "http_status": call.http_status,
                    "attempts": call.attempts,
                })
            }
            Err(e) => {
                json!({
                    "provider": "aliexpress",
                    "ok": false,
                    "error": {"message": e.to_string()},
                    "method": "recommend",
                })
            }
        }
    }

    pub async fn get_reviews(&self, product: &str, page: i32, limit: i32, locale: &str) -> Value {
        let product_id = match extract_product_id(product) {
            Some(id) => id,
            None => {
                return json!({
                    "provider": "aliexpress",
                    "ok": false,
                    "error": {"message": format!("Could not find an AliExpress product id in: {}", product)},
                });
            }
        };
        let page = if page > 0 { page } else { 1 };
        let limit = limit.clamp(1, 50);
        let host = host_for_locale(locale);
        // AE-RUST-009: region comes from the caller's locale only —
        // the old code forced JPY/JP cookies while taking `country`
        // from the locale, which was inconsistent.
        let ship_to = locale.split('_').nth(1).unwrap_or("US").to_uppercase();
        self.set_region_cookies(&host, locale, "", &ship_to).await;
        let source_url = format!("https://{}/item/{}.html", host, product_id);
        let data_obj = json!({
            "productId": product_id,
            "page": page,
            "pageSize": limit,
            "_lang": locale,
            "filter": "all",
            "sort": "complex_default",
            "country": ship_to,
            "clientType": "web",
        });
        let data = serde_json::to_string(&data_obj).unwrap();
        let acs_host = if host.ends_with(".us") {
            "acs.aliexpress.us"
        } else {
            "acs.aliexpress.com"
        };

        match self
            .signed_mtop_request_with_retry(
                acs_host,
                REVIEW_API,
                REVIEW_API_VERSION,
                &data,
                &source_url,
            )
            .await
        {
            Ok(call) => {
                if call.category != "success" {
                    let mut out = mtop_failure_json("reviews", &call);
                    out["product_id"] = json!(product_id);
                    return out;
                }
                let data = call.response.get("data").cloned().unwrap_or(json!({}));
                let reviews = normalize_reviews(&data);
                // AE-RUST-021: totals accept numeric strings as well
                // as numbers (to_int), and the server-reported total
                // is kept distinct from the page count.
                let total = reviews_total(&data);
                let total_pages =
                    to_int(data.get("totalPage")).or_else(|| to_int(data.get("totalPages")));
                json!({
                    "provider": "aliexpress",
                    "ok": true,
                    "category": call.category,
                    "product_id": product_id,
                    "page": page,
                    "limit": limit,
                    "total": total,
                    "total_pages": total_pages,
                    "reviews": reviews,
                    "count": reviews.len(),
                    "ret": call.response.get("ret"),
                    "trace_id": call.response.get("traceId"),
                    "http_status": call.http_status,
                    "attempts": call.attempts,
                })
            }
            Err(e) => {
                json!({"provider": "aliexpress", "ok": false, "error": {"message": e.to_string()}})
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn search_store_products(
        &self,
        store_id: &str,
        seller_id: &str,
        page: i32,
        limit: i32,
        sort: &str,
        locale: &str,
        currency: &str,
        ship_to: &str,
    ) -> Value {
        if store_id.trim().is_empty() {
            return json!({"provider": "aliexpress", "ok": false, "error": {"message": "store_id must not be empty"}});
        }
        // seller_id is MANDATORY (measured 2026-10-30 in the Python
        // SDK): omitting it returns FAIL_SYS_BIZPARAM_MISSED and
        // sending it empty returns FAIL_SYS_BIZPARAM_TYPE_ERROR, so
        // the call is rejected locally without sending anything.
        if seller_id.trim().is_empty() {
            return json!({"provider": "aliexpress", "ok": false, "error": {"message": "seller_id is required for store search (measured: the API rejects a missing or empty sellerId)", "category": "request_invalid"}});
        }
        let page = page.max(1);
        let limit = limit.clamp(1, 50);
        let host = host_for_locale(locale);
        let referer = format!("https://{}/store/{}", host, store_id);
        self.set_region_cookies(&host, locale, currency, ship_to)
            .await;

        let mut payload = search_region_payload(locale, currency, ship_to);
        payload.insert("country".to_string(), json!(ship_to));
        payload.insert("pageSize".to_string(), json!(limit.to_string()));
        payload.insert("sort".to_string(), json!(sort));
        payload.insert("storeId".to_string(), json!(store_id));
        payload.insert("deviceId".to_string(), json!(&self.device_id));
        payload.insert("platform".to_string(), json!("mobile"));
        payload.insert("site".to_string(), json!("glo"));
        payload.insert("sellerId".to_string(), json!(seller_id));
        payload.insert("cookieId".to_string(), json!(&self.device_id));
        payload.insert("currency".to_string(), json!(currency));
        payload.insert("page".to_string(), json!(page.to_string()));
        let data = serde_json::to_string(&payload).unwrap();

        match self
            .signed_mtop_request_with_retry(
                "acs.aliexpress.com",
                STORE_PRODUCTS_API,
                STORE_API_VERSION,
                &data,
                &referer,
            )
            .await
        {
            Ok(call) => {
                if call.category != "success" {
                    let mut out = mtop_failure_json("store_search", &call);
                    out["store_id"] = json!(store_id);
                    out["seller_id"] = json!(seller_id);
                    return out;
                }
                let response = &call.response;
                let resp_data = response.get("data").cloned().unwrap_or(json!({}));
                let items = normalize_search_items(&resp_data, &referer);
                let count = items.len().min(limit as usize);
                let truncated: Vec<Value> = items.into_iter().take(limit as usize).collect();
                let total = find_search_int(
                    &resp_data,
                    &[
                        "total",
                        "totalCount",
                        "totalNum",
                        "totalResults",
                        "totalItem",
                    ],
                );
                json!({
                    "provider": "aliexpress",
                    "ok": true,
                    "category": call.category,
                    "store_id": store_id,
                    "seller_id": seller_id,
                    "page": page,
                    "sort": sort,
                    "locale": locale,
                    "currency": currency,
                    "ship_to": ship_to,
                    "items": truncated,
                    "count": count,
                    "total": total,
                    "ret": response.get("ret"),
                    "trace_id": response.get("traceId"),
                    "http_status": call.http_status,
                    "attempts": call.attempts,
                })
            }
            Err(e) => {
                json!({
                    "provider": "aliexpress",
                    "ok": false,
                    "error": {"message": e.to_string()},
                })
            }
        }
    }

    async fn call_android_pdp_batch(
        &self,
        product_id: &str,
        locale: &str,
        currency: &str,
        ship_to: &str,
        referer: &str,
    ) -> Result<MtopCall> {
        let mut data_obj = search_region_payload(locale, currency, ship_to);
        data_obj.insert("quantity".to_string(), json!("1"));
        data_obj.insert("productIdList".to_string(), json!(product_id));
        data_obj.insert("channel".to_string(), json!("index"));
        data_obj.insert("isPreload".to_string(), json!("true"));
        data_obj.insert("clientType".to_string(), json!("android"));
        data_obj.insert("pdpNpiList".to_string(), json!("[]"));
        let ext = json!({
            "newPdp": true,
            "galaxyDetail": true,
            "skuOptimize": "true",
            "wideAspectRatioDevice": "false",
            "devicePerformance": "medium",
        });
        data_obj.insert(
            "ext".to_string(),
            json!(serde_json::to_string(&ext).unwrap()),
        );
        let data = serde_json::to_string(&data_obj).unwrap();
        self.signed_mtop_request_with_retry(
            "acs.aliexpress.com",
            ANDROID_PDP_BATCH_API,
            ANDROID_PDP_API_VERSION,
            &data,
            referer,
        )
        .await
    }

    async fn call_android_pdp_main(
        &self,
        product_id: &str,
        locale: &str,
        currency: &str,
        ship_to: &str,
        referer: &str,
    ) -> Result<MtopCall> {
        let mut data_obj = search_region_payload(locale, currency, ship_to);
        data_obj.insert("preselectLogisticsCompany".into(), json!(""));
        data_obj.insert("pdp_ext_f".into(), json!(""));
        data_obj.insert("channel".into(), json!("unknown"));
        data_obj.insert("source".into(), json!(""));
        data_obj.insert("needReal".into(), json!("true"));
        data_obj.insert("promotionId".into(), json!(""));
        data_obj.insert("clientType".into(), json!("android"));
        data_obj.insert("extRequestParams".into(), json!(""));
        data_obj.insert("preselectSkuId".into(), json!(""));
        data_obj.insert("compareFields".into(), json!(""));
        data_obj.insert("pdpNPI".into(), json!(""));
        let ext = json!({
            "cacheType": "local",
            "devicePerformance": "medium",
            "galaxyDetail": "true",
            "longImageExp": true,
            "newPdp": "true",
            "noCache": "true",
            "skuOptimize": "true",
            "wideAspectRatioDevice": "false",
            "workingDayViewCount": 0,
        });
        data_obj.insert("ext".into(), json!(serde_json::to_string(&ext).unwrap()));
        data_obj.insert("productId".into(), json!(product_id));
        let place_order = json!({"channel": "unknown"});
        data_obj.insert(
            "placeOrderParam".into(),
            json!(serde_json::to_string(&place_order).unwrap()),
        );
        // AE-RUST-009: timeZone is a caller/locale value, not a
        // hard-coded "JST"; no value is confirmed for non-JP locales,
        // so it defaults to empty (Python build_product_main_request).
        data_obj.insert("timeZone".into(), json!(""));
        data_obj.insert("originalUrl".into(), json!(referer));
        data_obj.insert("socialShareParamJson".into(), json!("{}"));
        data_obj.insert("openStream".into(), json!("true"));
        data_obj.insert("isTrafficSessionValid".into(), json!("false"));
        data_obj.insert("preselectLogisticsGroup".into(), json!(""));
        data_obj.insert("sourceType".into(), json!(""));
        let data = serde_json::to_string(&data_obj).unwrap();
        self.signed_mtop_request_with_retry(
            "acs.aliexpress.com",
            ANDROID_PDP_MAIN_API,
            ANDROID_PDP_API_VERSION,
            &data,
            referer,
        )
        .await
    }

    async fn set_region_cookies(&self, host: &str, locale: &str, currency: &str, ship_to: &str) {
        let site = site_for_locale(locale);
        let cookie_value = format!(
            "site={}&c_tp={}&region={}&b_locale={}",
            site, currency, ship_to, locale
        );
        let base_url = "https://www.aliexpress.com/"
            .parse::<reqwest::Url>()
            .unwrap();
        self.jar.add_cookie_str(
            &format!(
                "aep_usuc_f={}; Domain=.aliexpress.com; Path=/",
                cookie_value
            ),
            &base_url,
        );
        self.jar.add_cookie_str(
            &format!("intl_locale={}; Domain=.aliexpress.com; Path=/", locale),
            &base_url,
        );
        self.jar.add_cookie_str(
            &format!(
                "xman_us_f=x_locale={}&x_l=1; Domain=.aliexpress.com; Path=/",
                locale
            ),
            &base_url,
        );
        if host.ends_with(".us") {
            let us_url = "https://www.aliexpress.us/"
                .parse::<reqwest::Url>()
                .unwrap();
            self.jar.add_cookie_str(
                &format!("aep_usuc_f={}; Domain=.aliexpress.us; Path=/", cookie_value),
                &us_url,
            );
        }
    }

    /// Execute one signed call applying the retry policy
    /// (AE-RUST-002/011/012): ONLY the token categories
    /// (token_empty / token_expired) are retried — at most 2 extra
    /// attempts, each re-signed against the fresh `_m_h5_tk` the
    /// server issued via Set-Cookie. Verification, 403, 429 and
    /// access-denied responses are never retried. The returned
    /// MtopCall always carries the classification, so a protocol
    /// failure is surfaced with its category instead of being washed
    /// into an indistinguishable Ok.
    async fn signed_mtop_request_with_retry(
        &self,
        acs_host: &str,
        api: &str,
        version: &str,
        data: &str,
        referer: &str,
    ) -> Result<MtopCall> {
        let mut attempts: u32 = 0;
        loop {
            attempts += 1;
            let mut call = self
                .signed_mtop_request(acs_host, api, version, data, referer)
                .await?;
            call.attempts = attempts;
            if call.category == "success" {
                return Ok(call);
            }
            if is_token_retryable(&call.category) && attempts <= MAX_TOKEN_RETRIES {
                let delay = call.retry_after.unwrap_or(0.15 * attempts as f64);
                if delay > 0.0 {
                    tokio::time::sleep(std::time::Duration::from_secs_f64(delay)).await;
                }
                continue;
            }
            return Ok(call);
        }
    }

    /// One signed attempt. Inspects the HTTP status (AE-RUST-004):
    /// the status and Retry-After are recorded, 403/429 are mapped
    /// onto the blocking categories, and a non-JSON (e.g. HTML
    /// verification) body is kept as a classified call rather than a
    /// generic JSON-decode error.
    async fn signed_mtop_request(
        &self,
        acs_host: &str,
        api: &str,
        version: &str,
        data: &str,
        referer: &str,
    ) -> Result<MtopCall> {
        let token = {
            let t = self.token.lock().unwrap();
            t.clone()
        };
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();

        let sign_source = format!("{}&{}&{}&{}", token, timestamp, APP_KEY, data);
        let sign = format!("{:x}", md5_hash(sign_source.as_bytes()));

        // AE-RUST-028: query values are percent-encoded.
        let params = [
            ("jsv", "2.5.1"),
            ("appKey", APP_KEY),
            ("t", &timestamp),
            ("sign", &sign),
            ("api", api),
            ("v", version),
            ("type", "originaljson"),
            ("dataType", "json"),
        ];
        let query_string: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding_encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let url = format!(
            "https://{}/h5/{}/{}/?{}",
            acs_host, api, version, query_string
        );

        let origin = origin_from_referer(referer);
        let body = format!("data={}", urlencoding_encode(data));
        let response = self
            .client
            .post(&url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Origin", &origin)
            .header("Referer", referer)
            .body(body)
            .send()
            .await?;

        let http_status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<f64>().ok());

        // Extract the _m_h5_tk signing token from Set-Cookie for the
        // next request (exact cookie-name match, so _m_h5_tk_enc is
        // never mistaken for it). The _enc value itself is not kept:
        // it was write-only state (AE-RUST-028); the cookie jar holds
        // the actual cookies.
        for cookie_header in response.headers().get_all("set-cookie") {
            if let Ok(cookie_str) = cookie_header.to_str() {
                let first_pair = cookie_str.split(';').next().unwrap_or("");
                if let Some((name, value)) = first_pair.split_once('=') {
                    if name.trim() == "_m_h5_tk" {
                        let token_part = value.trim().split('_').next().unwrap_or("");
                        let mut t = self.token.lock().unwrap();
                        *t = token_part.to_string();
                    }
                }
            }
        }

        let text = response.text().await?;
        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let response_json = if parsed.is_object() {
            parsed
        } else {
            // Non-JSON body (HTML verification page, empty body, ...):
            // keep a snippet for diagnosis; classification below maps
            // the status onto a category instead of failing decode.
            let snippet: String = text.chars().take(200).collect();
            json!({"_http_body_snippet": snippet})
        };
        let mut category = classify_mtop(&response_json).0;
        if http_status == 403 {
            category = "illegal_access".to_string();
        } else if http_status == 429 {
            category = "rate_limit".to_string();
        } else if http_status >= 400 && category == "unknown" {
            category = "failure".to_string();
        }
        let (_, ret, trace_id) = classify_mtop(&response_json);
        Ok(MtopCall {
            response: response_json,
            http_status,
            category,
            ret,
            trace_id,
            attempts: 1,
            retry_after,
        })
    }
}

impl Default for AliExpressClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Maximum token retries: initial attempt + at most 2 retries
/// (Python constants.MAX_TOKEN_RETRIES).
const MAX_TOKEN_RETRIES: u32 = 2;

/// One executed MTop call: the envelope plus its protocol
/// classification (AE-RUST-002/003). `ok` anywhere in this crate is
/// exactly `category == "success"` from the single classify_mtop.
struct MtopCall {
    response: Value,
    http_status: u16,
    category: String,
    ret: Vec<String>,
    trace_id: Option<String>,
    attempts: u32,
    retry_after: Option<f64>,
}

/// Only token categories may be retried (AE-RUST-011): a fresh
/// `_m_h5_tk` cookie can make the next signature valid. Access
/// denial, verification and rate limiting are never retried.
fn is_token_retryable(category: &str) -> bool {
    matches!(category, "token_empty" | "token_expired")
}

/// Categories that abort a multi-route operation immediately
/// (Python errors.BLOCKING_CATEGORIES plus rate_limit).
fn is_blocking_category(category: &str) -> bool {
    matches!(category, "illegal_access" | "rate_limit" | "verification")
}

/// Shared classification (AE-RUST-001/003), a port of the Python
/// parser's classify_mtop (aliexpress_parser/mtop.py) — the single
/// source of truth for `ok` in this crate. Returns
/// (category, ret, trace_id). Data-side `request_invalid`
/// (searchTips tipsType) takes priority over a SUCCESS ret.
fn classify_mtop(response: &Value) -> (String, Vec<String>, Option<String>) {
    let ret: Vec<String> = match response.get("ret") {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    };
    let trace_id = response
        .get("traceId")
        .or_else(|| response.get("trace_id"))
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
    let joined = ret.join(" ").to_uppercase();
    let has = |needles: &[&str]| needles.iter().any(|n| joined.contains(n));
    let tips_type = response
        .get("data")
        .and_then(|d| d.get("mods"))
        .and_then(|m| m.get("searchTips"))
        .and_then(|t| t.get("content"))
        .and_then(|c| c.get("tipsType"))
        .and_then(|v| v.as_str());
    let category = if tips_type == Some("request_invalid") || has(&["REQUEST_INVALID"]) {
        "request_invalid"
    } else if has(&["TOKEN_EMPTY", "FAIL_SYS_TOKEN_EMPTY"]) {
        "token_empty"
    } else if has(&["TOKEN_EXPIRED", "FAIL_SYS_TOKEN_EXPIRED"]) {
        "token_expired"
    } else if has(&[
        "USER_VALIDATE",
        "RGV587",
        "CAPTCHA",
        "VERIFY",
        "SECURITY_CHECK",
        "FAIL_SYS_USER_VALIDATE",
    ]) {
        "verification"
    } else if has(&[
        "RATE_LIMIT",
        "FLOW_LIMIT",
        "TRAFFIC_LIMIT",
        "THROTTLE",
        "FAIL_SYS_RATE_LIMIT",
    ]) {
        "rate_limit"
    } else if has(&["ILLEGAL_ACCESS", "FAIL_SYS_ILLEGAL_ACCESS"]) {
        "illegal_access"
    } else if has(&[
        "SESSION_EXPIRED",
        "SESSION_TIMEOUT",
        "FAIL_SYS_SESSION_EXPIRED",
    ]) {
        "session_expired"
    } else if has(&["SUCCESS"]) {
        "success"
    } else if has(&["FAIL"]) {
        "failure"
    } else {
        "unknown"
    };
    (category.to_string(), ret, trace_id)
}

/// Uniform failure surface for a classified-but-unsuccessful call:
/// callers add their own context fields on top.
fn mtop_failure_json(context: &str, call: &MtopCall) -> Value {
    json!({
        "provider": "aliexpress",
        "ok": false,
        "category": call.category,
        "error": {
            "message": format!("MTop request failed [{}]: {}", call.category, if call.ret.is_empty() { "no ret".to_string() } else { call.ret.join(", ") }),
            "category": call.category,
        },
        "context": context,
        "ret": call.ret,
        "trace_id": call.trace_id,
        "http_status": call.http_status,
        "attempts": call.attempts,
    })
}

/// Failure surface when a blocking category aborts get_product.
fn blocked_json(product_id: &str, source: &str, call: &MtopCall) -> Value {
    let mut out = mtop_failure_json("get_product", call);
    out["product_id"] = json!(product_id);
    out["source"] = json!(source);
    out
}

/// Acceptance test for a normalized product detail (AE-RUST-013/015):
/// the detail must be for the requested product and carry meaningful
/// data (title or price or images).
fn detail_accepted(normalized: &Value, product_id: &str) -> bool {
    normalized.get("product_id").and_then(|v| v.as_str()) == Some(product_id)
        && has_meaningful_data(normalized)
}

fn has_meaningful_data(normalized: &Value) -> bool {
    let title_ok = normalized
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let price_ok = normalized
        .get("price_detail")
        .map(|v| !v.is_null())
        .unwrap_or(false);
    let images_ok = normalized
        .get("images")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    title_ok || price_ok || images_ok
}

pub fn extract_product_id(value: &str) -> Option<String> {
    let value = value.trim();
    for pattern in PRODUCT_ID_PATTERNS.iter() {
        if let Some(caps) = pattern.captures(value) {
            return Some(caps[1].to_string());
        }
    }
    if NUMERIC_ID_RE.is_match(value) {
        return Some(value.to_string());
    }
    None
}

fn host_for_locale(locale: &str) -> String {
    let language = locale.split('_').next().unwrap_or("en").to_lowercase();
    LOCALE_HOSTS
        .get(language.as_str())
        .unwrap_or(&"www.aliexpress.com")
        .to_string()
}

fn site_for_locale(locale: &str) -> String {
    let language = locale.split('_').next().unwrap_or("en").to_lowercase();
    let sites: HashMap<&str, &str> = [
        ("ar", "ara"),
        ("de", "deu"),
        ("en", "glo"),
        ("es", "esp"),
        ("fr", "fra"),
        ("he", "isr"),
        ("id", "idn"),
        ("it", "ita"),
        ("ja", "jpn"),
        ("ko", "kor"),
        ("nl", "nld"),
        ("pl", "pol"),
        ("pt", "bra"),
        ("th", "tha"),
        ("tr", "tur"),
        ("vi", "vnm"),
    ]
    .into_iter()
    .collect();
    sites.get(language.as_str()).unwrap_or(&"glo").to_string()
}

fn origin_from_referer(referer: &str) -> String {
    if let Some(pos) = referer.find("//") {
        if let Some(end) = referer[pos + 2..].find('/') {
            return referer[..pos + 2 + end].to_string();
        }
        return referer.to_string();
    }
    "https://www.aliexpress.com".to_string()
}

/// Region city/state detail values (AE-RUST-009): the old code
/// embedded captured Sapporo/Hokkaido codes for every JP shipment.
/// Those are past-capture values, not specification constants; the
/// Python resolution derives region from locale/currency/ship_to in
/// one place and leaves city/state EMPTY when unknown.
fn android_region_defaults(_ship_to: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("city".to_string(), String::new());
    m.insert("state".to_string(), String::new());
    m.insert("state_name".to_string(), String::new());
    m.insert("city_name".to_string(), String::new());
    m
}

fn looks_like_android_pdp_result(v: &Value) -> bool {
    let obj = match v.as_object() {
        Some(o) => o,
        None => return false,
    };
    if obj.get("global").and_then(|g| g.as_object()).is_some()
        && obj.get("data").and_then(|d| d.as_object()).is_some()
    {
        return true;
    }
    if obj.get("bizContext").and_then(|b| b.as_object()).is_some() {
        return true;
    }
    let keys: String = obj
        .keys()
        .map(|k| k.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    keys.contains("galaxy_")
        && (keys.contains("sku") || keys.contains("desc") || keys.contains("freight"))
}

/// Locate the PDP entry in an Android main response, following the
/// Python client's _extract_android_main_entry (AE-RUST-014).
/// Returns (entry, matched_by_key). Direct-key hits
/// (data[product_id] / data.result[product_id]) match by
/// construction. The loose "looks-like" candidates (result / data)
/// are returned with matched_by_key=false and are accepted by the
/// caller only if the entry's own structured id fields declare the
/// requested product — the old unbounded recursive walk that adopted
/// the first plausible object anywhere is gone.
fn extract_android_pdp_entry(response: &Value, product_id: &str) -> Option<(Value, bool)> {
    let data = response.get("data")?.as_object()?;
    if let Some(direct) = data.get(product_id) {
        if direct.is_object() && direct.as_object().is_some_and(|o| !o.is_empty()) {
            return Some((direct.clone(), true));
        }
    }
    if let Some(result) = data.get("result").and_then(|r| r.as_object()) {
        if let Some(direct) = result.get(product_id) {
            if direct.is_object() && direct.as_object().is_some_and(|o| !o.is_empty()) {
                return Some((direct.clone(), true));
            }
        }
        let result_val = Value::Object(result.clone());
        if looks_like_android_pdp_result(&result_val) {
            return Some((result_val, false));
        }
    }
    let data_val = Value::Object(data.clone());
    if looks_like_android_pdp_result(&data_val) {
        return Some((data_val, false));
    }
    None
}

/// Product ids an entry declares in *structured* id fields only
/// (Python _entry_declared_ids): dedicated id keys with numeric
/// values of at least 8 digits, collected recursively. A mere
/// substring appearance of the id (title, tracking payload, another
/// product's slot) does not count.
fn entry_declared_ids(entry: &Value) -> std::collections::HashSet<String> {
    const ID_KEYS: [&str; 5] = ["productId", "product_id", "itemId", "item_id", "id"];
    fn walk(node: &Value, out: &mut std::collections::HashSet<String>) {
        match node {
            Value::Object(map) => {
                for (k, v) in map {
                    if ID_KEYS.contains(&k.as_str()) {
                        let text = match v {
                            Value::String(s) => Some(s.clone()),
                            Value::Number(n) => Some(n.to_string()),
                            _ => None,
                        };
                        if let Some(t) = text {
                            if t.len() >= 8 && t.chars().all(|c| c.is_ascii_digit()) {
                                out.insert(t);
                            }
                        }
                    }
                    walk(v, out);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    walk(v, out);
                }
            }
            _ => {}
        }
    }
    let mut out = std::collections::HashSet::new();
    walk(entry, &mut out);
    out
}

fn search_region_payload(
    locale: &str,
    currency: &str,
    ship_to: &str,
) -> serde_json::Map<String, Value> {
    let region = android_region_defaults(ship_to);
    let mut m = serde_json::Map::new();
    m.insert("_lang".to_string(), json!(locale));
    m.insert("_systemLang".to_string(), json!(locale));
    m.insert("_currency".to_string(), json!(currency));
    m.insert(
        "_city".to_string(),
        json!(region.get("city").unwrap_or(&String::new())),
    );
    m.insert(
        "_stateName".to_string(),
        json!(region.get("state_name").unwrap_or(&String::new())),
    );
    m.insert(
        "_state".to_string(),
        json!(region.get("state").unwrap_or(&String::new())),
    );
    m.insert("_saasRegion".to_string(), json!("aeg"));
    m.insert("_isDarkMode".to_string(), json!("0"));
    m.insert("locale".to_string(), json!(locale));
    m.insert(
        "_cityName".to_string(),
        json!(region.get("city_name").unwrap_or(&String::new())),
    );
    m
}

#[allow(clippy::too_many_arguments)]
fn build_search_params(
    query: &str,
    page: i32,
    page_size: i32,
    locale: &str,
    currency: &str,
    ship_to: &str,
    device_id: &str,
    sort: &str,
    pv_feature: Option<&str>,
    pre_page_exposure_products: Option<&str>,
    min_price: Option<&str>,
    max_price: Option<&str>,
    switches: &[&str],
) -> Value {
    let language = locale.split('_').next().unwrap_or("en").to_lowercase();
    let region = android_region_defaults(ship_to);
    let mut params = json!({
        "_lang": locale,
        "_city": region.get("city").unwrap_or(&String::new()),
        "_currency": currency,
        "_state": region.get("state").unwrap_or(&String::new()),
        "abBucket": APP_PROFILE.ab_bucket,
        "apiversion": APP_PROFILE.apiversion,
        "appVersion": APP_PROFILE.app_version,
        "app_client_v": APP_PROFILE.app_client_v,
        "cateId": "",
        "cateName": "",
        "clientType": "android",
        "deviceId": device_id,
        "deviceLevel": APP_PROFILE.device_level,
        "guideModule": "history",
        "guidePage": "Page_Search",
        "harnessSearch": "true",
        "isAENSearch": "true",
        "isStream": "true",
        "keywords": query.trim(),
        "lang": language,
        "locale": locale,
        "os_version": APP_PROFILE.os_version,
        "page": page.to_string(),
        "pageSize": page_size.to_string(),
        "q": query.trim(),
        "refine_conf": "0",
        "s1": "MAIN",
        "searchBizScene": "mainSearch",
        "shpt_co": ship_to,
        "supportNJ": "true",
        "supportNewFeedback": "true",
        "sversion": APP_PROFILE.sversion,
        "ttid": APP_PROFILE.ttid,
        // NOTE (AE-RUST-008): the device id is sent again as utd_id,
        // as the original code did. UTDID is a distinct identifier
        // concept; reusing the session device id here preserves the
        // observed wire shape but is an *unverified reuse*, not a
        // claim that the two are the same.
        "utd_id": device_id,
    });
    // AE-RUST-007: pv_feature is session state, never a constant.
    // It is omitted entirely on a first search and built from this
    // session's own exposed ids afterwards.
    if let Some(pv) = pv_feature {
        if !pv.is_empty() {
            params["pv_feature"] = json!(pv);
        }
    }
    if let Some(pre) = pre_page_exposure_products {
        if !pre.is_empty() {
            params["prePageExposureProducts"] = json!(pre);
        }
    }
    let sort_clean = sort.trim();
    if !sort_clean.is_empty() {
        params["sortParam"] = json!(sort_clean);
    }
    // AE-RUST-006: the confirmed filter fields, following the Python
    // builder. selectedSwitches values come from the analysis doc's
    // captured switch list ("filterCode:<name>"); the price fields
    // pr / pageMinPrice follow the captured field names and are sent
    // only when the caller explicitly passes a price (the MCP tool
    // signature carries no price/switch arguments, so the public
    // search path passes none — exactly the Python defaults).
    let switch_list: Vec<String> = switches
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.contains(':') {
                s.to_string()
            } else {
                format!("filterCode:{}", s)
            }
        })
        .collect();
    if !switch_list.is_empty() {
        params["selectedSwitches"] = json!(switch_list.join(","));
    }
    if min_price.is_some() || max_price.is_some() {
        let lo = min_price.unwrap_or("");
        let hi = max_price.unwrap_or("");
        params["pr"] = json!(if hi.is_empty() {
            lo.to_string()
        } else {
            format!("{},{}", lo, hi)
        });
        if let Some(lo_v) = min_price {
            params["pageMinPrice"] = json!(lo_v);
        }
    }
    params
}

fn normalize_search_items(data: &Value, source_url: &str) -> Vec<Value> {
    let mut found = Vec::new();
    let mut seen = std::collections::HashSet::new();

    fn walk(
        value: &Value,
        found: &mut Vec<Value>,
        seen: &mut std::collections::HashSet<String>,
        source_url: &str,
    ) {
        match value {
            Value::Object(map) => {
                maybe_add_item(value, found, seen, source_url);
                for v in map.values() {
                    walk(v, found, seen, source_url);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    walk(v, found, seen, source_url);
                }
            }
            _ => {}
        }
    }

    walk(data, &mut found, &mut seen, source_url);
    found
}

fn maybe_add_item(
    value: &Value,
    found: &mut Vec<Value>,
    seen: &mut std::collections::HashSet<String>,
    _source_url: &str,
) {
    let obj = match value.as_object() {
        Some(o) => o,
        None => return,
    };
    let product_url = first_str(
        obj,
        &[
            "productDetailUrl",
            "productUrl",
            "detailUrl",
            "pcDetailUrl",
            "itemUrl",
            "url",
            "clickUrl",
        ],
    );
    let mut product_id = first_str(obj, &["productId", "product_id", "itemId", "item_id", "id"]);
    if !product_id.is_empty() && !NUMERIC_ID_RE.is_match(&product_id) {
        product_id = String::new();
    }
    if product_id.is_empty() && !product_url.is_empty() {
        product_id = extract_product_id(&product_url).unwrap_or_default();
    }
    if product_id.is_empty() || seen.contains(&product_id) {
        return;
    }

    let title = search_card_title(obj)
        .or_else(|| {
            let t = first_str(
                obj,
                &[
                    "title",
                    "subject",
                    "productTitle",
                    "product_title",
                    "name",
                    "seoTitle",
                ],
            );
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
        .unwrap_or_default();

    let price_value = search_card_price(obj).unwrap_or_else(|| {
        first_present(
            obj,
            &[
                "formattedPrice",
                "formatedPromotionPiecePriceStr",
                "formatedPiecePriceStr",
                "salePrice",
                "salePriceString",
                "price",
                "minPrice",
                "displayPrice",
            ],
        )
    });

    let image = search_card_image(obj)
        .or_else(|| {
            let i = first_str(
                obj,
                &[
                    "imageUrl",
                    "imgUrl",
                    "image",
                    "productImage",
                    "image350Url",
                    "scaleImageUrl",
                    "summImageUrl",
                    "skuImageUrl",
                ],
            );
            if i.is_empty() {
                None
            } else {
                Some(i)
            }
        })
        .unwrap_or_default();

    // Meaningful-card gate (Python parsers/search.py, 2026-10-30
    // measured fix): the recursive walk also encounters bare
    // id-bearing dicts that are not product cards at all — e.g.
    // feedback.params entries holding only {product_type,
    // product_id}. Counting those doubled the reported result count
    // (40 reported vs 20 real cards for pageSize=20). A candidate
    // must carry at least one of title / price / image.
    if title.trim().is_empty() && price_value.is_null() && image.trim().is_empty() {
        return;
    }
    seen.insert(product_id.clone());

    // AE-RUST-019: structured price via the single build_price;
    // "price" keeps the historical display value (formatted string
    // when known, else the amount string).
    let currency_hint = ["currencyCode", "currency"]
        .iter()
        .find_map(|k| obj.get(*k).and_then(|v| v.as_str()))
        .map(|s| s.to_string());
    let price_detail = build_price(&price_value, currency_hint.as_deref());
    let price_display = price_detail
        .as_ref()
        .and_then(|p| {
            p.get("formatted")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    p.get("amount")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
        })
        .map(Value::String)
        .unwrap_or(Value::Null);

    let store = first_str(obj, &["storeName", "shopName", "sellerName"]);
    // AE-RUST-023: search cards carry only a representative skuId
    // (live finding) — surface it, but do not invent variants.
    let sku_id = first_str(obj, &["skuId", "sku_id"]);

    let mut item = json!({
        "product_id": product_id,
        "title": clean_text(&title),
        "price": price_display,
        "price_detail": price_detail,
        "image": normalize_url(&image, "https://www.aliexpress.com/"),
        "url": canonical_product_url(&product_id),
        "store": store,
        "source": "direct_mtop_search",
    });
    if !sku_id.is_empty() {
        item["sku_id"] = json!(sku_id);
    }
    if let Some(r) = to_float(obj.get("starRating").or_else(|| obj.get("rating"))) {
        item["rating"] = json!(r);
    }
    if let Some(rc) = to_int(obj.get("reviewCount").or_else(|| obj.get("totalValidNum"))) {
        item["review_count"] = json!(rc);
    }
    if let Some(sc) = to_int(obj.get("tradeCount").or_else(|| obj.get("soldCount"))) {
        item["sold_count"] = json!(sc);
    }
    if price_detail_is_none(&item) {
        item["warnings"] = json!([{"field": "price", "reason": "missing", "message": "no parseable price on search card"}]);
    }
    found.push(item);
}

fn price_detail_is_none(item: &Value) -> bool {
    item.get("price_detail")
        .map(|v| v.is_null())
        .unwrap_or(true)
}

fn normalize_pdp_result(
    product_id: &str,
    result: &Value,
    source_url: &str,
    locale: &str,
    currency: &str,
) -> Value {
    let global_data = result
        .get("GLOBAL_DATA")
        .and_then(|v| v.get("globalData"))
        .cloned()
        .unwrap_or(json!({}));
    let title = result
        .get("PRODUCT_TITLE")
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .or_else(|| global_data.get("subject").and_then(|v| v.as_str()))
        .unwrap_or("");

    let shop = result.get("SHOP_CARD_PC").cloned().unwrap_or(json!({}));
    let rating = result.get("PC_RATING").cloned().unwrap_or(json!({}));
    let shipping = result.get("SHIPPING").cloned().unwrap_or(json!({}));
    let desc = result.get("DESC").cloned().unwrap_or(json!({}));
    let images = normalize_images_from_result(result, &global_data);

    let ship_biz = shipping
        .get("deliveryLayoutInfo")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("bizData"))
        .cloned()
        .unwrap_or(json!({}));

    let sku_variants = extract_web_sku_variants(result);

    // AE-RUST-016: the web route must surface a price when the
    // payload has one (PRICE / PRODUCT_PRICE / PRICE_CARD_PC
    // blocks) instead of ok-with-empty.
    let price_source = result
        .get("PRICE")
        .cloned()
        .filter(|v| !v.is_null())
        .or_else(|| {
            result
                .get("PRODUCT_PRICE")
                .cloned()
                .filter(|v| !v.is_null())
        })
        .or_else(|| {
            result
                .get("PRICE_CARD_PC")
                .cloned()
                .filter(|v| !v.is_null())
        });
    let price_detail = price_source
        .as_ref()
        .and_then(|p| build_price(p, Some(currency)));

    // The response-declared id wins over the requested one.
    let declared_id = global_data
        .get("itemId")
        .or(global_data.get("productId"))
        .map(|v| value_to_string(Some(v)))
        .filter(|s| !s.is_empty());
    let out_product_id = declared_id.unwrap_or_else(|| product_id.to_string());

    // AE-RUST-015: missing essential fields produce warnings instead
    // of a silent empty ok.
    let mut warnings: Vec<String> = Vec::new();
    if title.trim().is_empty() {
        warnings.push("missing product title".to_string());
    }
    if price_detail.is_none() {
        warnings.push("missing product price".to_string());
    }
    if images.is_empty() {
        warnings.push("missing product images".to_string());
    }
    if out_product_id != product_id {
        warnings
            .push("product id declared by the response differs from the requested id".to_string());
    }

    let mut out = json!({
        "provider": "aliexpress",
        "product_id": out_product_id,
        "url": canonical_product_url(product_id),
        "source_url": source_url,
        "locale": locale,
        "currency": currency,
        "source": "direct_mtop_web_pdp",
        "title": clean_text(title),
        "price_detail": price_detail,
        "warnings": warnings,
        "rating": {
            "rating": rating.get("rating"),
            "review_count": rating.get("totalValidNum"),
            "sales_text": rating.get("otherText"),
        },
        "store": {
            "name": shop.get("storeName"),
            "seller_id": shop.get("sellerInfo").and_then(|s| s.get("adminSeq")).or_else(|| global_data.get("sellerId")),
        },
        "images": images,
        "shipping": {
            "ship_from": ship_biz.get("shipFrom"),
            "ship_to": ship_biz.get("shipToCode"),
            "amount": ship_biz.get("displayAmount").or(ship_biz.get("amountDisplay")),
        },
        "description": {
            "pc_url": desc.get("pcDescUrl"),
            "mobile_url": desc.get("msiteDescUrl"),
            "json_url": desc.get("nativeDescUrl"),
        },
    });
    if !sku_variants.is_empty() {
        out["sku_variants"] = json!(sku_variants);
    }
    out
}

fn parse_sku_properties(props: &Value) -> Vec<Value> {
    let arr = match props.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    let mut variants = Vec::new();
    for prop in arr {
        let prop_name = prop
            .get("skuPropertyName")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let prop_id = json_value_to_string(prop.get("skuPropertyId"));
        // AE-RUST-020: hasSkuImage accepts bool / "true" / "false" /
        // 0 / 1 through the one to_bool conversion.
        let has_image = to_bool(prop.get("hasSkuImage"), Some(false)).unwrap_or(false);
        if let Some(vals) = prop.get("skuPropertyValues").and_then(|v| v.as_array()) {
            let values: Vec<Value> = vals.iter().map(|v| {
                let name = v.get("definitionName")
                    .or(v.get("displayName"))
                    .or(v.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                let mut entry = json!({
                    "name": clean_text(name),
                    "display_name": v.get("displayName").and_then(|n| n.as_str()).map(clean_text),
                    "value_id": json_value_to_string(v.get("propertyValueId")),
                });
                if has_image {
                    if let Some(img) = v.get("imagePath").and_then(|x| x.as_str()) {
                        entry["image"] = json!(normalize_url(img, "https://www.aliexpress.com/"));
                    }
                }
                entry
            }).collect();
            variants.push(json!({
                "property_id": prop_id,
                "property_name": prop_name,
                "values": values,
            }));
        }
    }
    variants
}

fn extract_web_sku_variants(result: &Value) -> Vec<Value> {
    for key in ["SKU_CARD_PC", "SKU_CARD", "SKU"] {
        if let Some(block) = result.get(key) {
            if let Some(props) = block.get("skuProperties") {
                let v = parse_sku_properties(props);
                if !v.is_empty() {
                    return v;
                }
            }
        }
    }
    vec![]
}

fn extract_sku_variants(result: &Value) -> Vec<Value> {
    let data = result.get("data").unwrap_or(result);
    let data_obj = match data.as_object() {
        Some(o) => o,
        None => return vec![],
    };
    for (key, block) in data_obj {
        if !key.contains("sku") {
            continue;
        }
        if let Some(fields) = block.get("fields") {
            if let Some(props) = fields.get("skuProperties") {
                let v = parse_sku_properties(props);
                if !v.is_empty() {
                    return v;
                }
            }
        }
    }
    vec![]
}

fn normalize_android_pdp_result(
    product_id: &str,
    result: &Value,
    source_url: &str,
    locale: &str,
    currency: &str,
    source: &str,
) -> Value {
    let mut warnings: Vec<String> = Vec::new();
    let global = result.get("global").cloned().unwrap_or(json!({}));
    let biz = global
        .get("bizContext")
        .or(result.get("bizContext"))
        .cloned()
        .unwrap_or(json!({}));
    let product_info = biz.get("productInfo").cloned().unwrap_or(json!({}));
    let item_info = biz.get("itemInfo").cloned().unwrap_or(json!({}));

    // The response-declared id wins over the requested one (Python
    // parse rule); a mismatch becomes an id_mismatch warning and the
    // caller rejects the detail via detail_accepted.
    let declared_id = [
        product_info.get("productId"),
        product_info.get("itemId"),
        item_info.get("itemId"),
        item_info.get("id"),
        biz.get("productId"),
    ]
    .into_iter()
    .flatten()
    .map(|v| value_to_string(Some(v)))
    .find(|s| !s.is_empty());
    let (out_product_id, id_warning) = match &declared_id {
        Some(d) if d != product_id => (
            d.clone(),
            Some("product id declared by the response differs from the requested id"),
        ),
        Some(_) => (product_id.to_string(), None),
        None => (
            product_id.to_string(),
            Some("response carries no verifiable product id"),
        ),
    };
    if let Some(w) = id_warning {
        warnings.push(w.to_string());
    }

    let title = product_info
        .get("subject")
        .or(item_info.get("subject"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // AE-RUST-019: prices go through the one price-block helper;
    // amountCent is never silently overwritten by `value`.
    let price_info = biz.get("priceInfo").cloned().unwrap_or(json!({}));
    let sale_min = price_info.get("saleMinPrice").cloned().unwrap_or(json!({}));
    let sale_max = price_info.get("saleMaxPrice").cloned().unwrap_or(json!({}));
    let (price_detail, mut price_warnings) = price_block_price(&sale_min, Some(currency));
    warnings.append(&mut price_warnings);
    let (_, mut max_warnings) = price_block_price(&sale_max, Some(currency));
    warnings.append(&mut max_warnings);

    let seller = biz.get("sellerInfo").cloned().unwrap_or(json!({}));

    let mut images = Vec::new();
    for info in [&product_info, &item_info] {
        if let Some(arr) = info.get("mainImages").and_then(|v| v.as_array()) {
            for img in arr {
                // Main images may be plain strings or {imageUrl} dicts.
                let raw = match img {
                    Value::String(s) => Some(s.clone()),
                    _ => img
                        .get("imageUrl")
                        .or(img.get("imgPath"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                };
                if let Some(url) = raw {
                    let normalized = normalize_url(&url, "https://www.aliexpress.com/");
                    if !normalized.is_empty() && !images.contains(&normalized) {
                        images.push(normalized);
                    }
                }
            }
        }
        if let Some(url) = info.get("defaultImageUrl").and_then(|v| v.as_str()) {
            let normalized = normalize_url(url, "https://www.aliexpress.com/");
            if !normalized.is_empty() && !images.contains(&normalized) {
                images.push(normalized);
            }
        }
    }

    let sku_variants = extract_sku_variants(result);
    let (specs, desc_url) = extract_specs_and_desc(result);

    if title.trim().is_empty() {
        warnings.push("missing product title".to_string());
    }
    if price_detail.is_none() {
        warnings.push("missing product price".to_string());
    }

    let mut out = json!({
        "provider": "aliexpress",
        "product_id": out_product_id,
        "url": canonical_product_url(product_id),
        "source_url": source_url,
        "locale": locale,
        "currency": currency,
        "title": clean_text(title),
        "price": {
            "sale_min": sale_min.get("amountCent"),
            "sale_min_value": sale_min.get("value"),
            "sale_max": sale_max.get("amountCent"),
            "sale_max_value": sale_max.get("value"),
            "currency": sale_min.get("currencyCode").or(sale_max.get("currencyCode")),
        },
        "price_detail": price_detail,
        "store": {
            "name": seller.get("storeName"),
            "seller_id": seller.get("sellerId").or(seller.get("adminSeq")),
            "store_id": seller.get("storeId"),
        },
        "images": images,
        "source": source,
        "warnings": warnings,
    });
    if !sku_variants.is_empty() {
        out["sku_variants"] = json!(sku_variants);
    }
    if !specs.is_empty() {
        out["specifications"] = json!(specs);
    }
    if let Some(url) = desc_url {
        out["description_url"] = json!(url);
    }
    out
}

fn normalize_reviews(data: &Value) -> Vec<Value> {
    let list = data
        .get("dataList")
        .or(data.get("evaViewList"))
        .and_then(|v| v.as_array());
    let list = match list {
        Some(l) => l,
        None => return vec![],
    };
    list.iter()
        .map(|item| {
            let feedback = item
                .get("feedback")
                .or(item.get("buyerFeedback"))
                .or(item.get("buyerProductFeedBack"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let translated = item
                .get("translatedFeedback")
                .or(item.get("buyerTranslationFeedback"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());
            let sku_info = item
                .get("skuDisplayInfo")
                .or(item.get("skuInfo"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());
            let country = item
                .get("buyerCountry")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let buyer = item
                .get("buyerDisplayName")
                .or(item.get("buyerName"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let date = item
                .get("evalDate")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());
            // AE-RUST-021: buyerEval accepts a number or a numeric
            // string through the one to_float conversion.
            let star = to_float(item.get("buyerEval"));
            // Images may be plain strings or {url|imageUrl} objects.
            let images: Vec<String> = item
                .get("images")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| match v {
                            Value::String(s) => {
                                Some(normalize_url(s, "https://www.aliexpress.com/"))
                            }
                            other => other
                                .get("url")
                                .or(other.get("imageUrl"))
                                .and_then(|u| u.as_str())
                                .map(|u| normalize_url(u, "https://www.aliexpress.com/")),
                        })
                        .filter(|u| !u.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let mut review = json!({
                "feedback": clean_text(feedback),
                "buyer_country": country,
                "buyer_name": buyer,
            });
            if let Some(t) = translated {
                review["translated_feedback"] = json!(t);
            }
            if let Some(s) = sku_info {
                review["sku"] = json!(s);
            }
            if let Some(d) = date {
                review["date"] = json!(d);
            }
            if let Some(s) = star {
                review["star"] = json!(s);
            }
            if !images.is_empty() {
                review["images"] = json!(images);
            }
            review
        })
        .collect()
}

fn extract_specs_and_desc(result: &Value) -> (Vec<Value>, Option<String>) {
    let data = result.get("data").unwrap_or(result);
    let data_obj = match data.as_object() {
        Some(o) => o,
        None => return (vec![], None),
    };
    let mut specs = Vec::new();
    let mut desc_url = None;
    for (key, block) in data_obj {
        if !key.contains("item_desc") && !key.contains("galaxy_desc") {
            continue;
        }
        let fields = match block.get("fields").and_then(|v| v.as_object()) {
            Some(f) => f,
            None => continue,
        };
        for prop_key in ["allAppGroups", "outerProps"] {
            if let Some(arr) = fields.get(prop_key).and_then(|v| v.as_array()) {
                for item in arr {
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let value = item.get("value").and_then(|v| v.as_str()).unwrap_or("");
                    if !name.is_empty() && !value.is_empty() {
                        let cleaned_name = clean_text(name);
                        let cleaned_value = clean_text(value);
                        if !specs
                            .iter()
                            .any(|s: &Value| s["name"].as_str() == Some(&cleaned_name))
                        {
                            specs.push(json!({"name": cleaned_name, "value": cleaned_value}));
                        }
                    }
                }
            }
        }
        if let Some(url) = fields
            .get("async")
            .and_then(|v| v.get("params"))
            .and_then(|v| v.get("nativeDescUrl"))
            .and_then(|v| v.as_str())
        {
            if !url.is_empty() {
                desc_url = Some(url.to_string());
            }
        }
        if !specs.is_empty() {
            break;
        }
    }
    (specs, desc_url)
}

fn normalize_images_from_result(result: &Value, global_data: &Value) -> Vec<String> {
    let image_block = result.get("HEADER_IMAGE_PC").cloned().unwrap_or(json!({}));
    let candidates = image_block
        .get("imgList")
        .or(image_block.get("imagePathList"))
        .or(image_block.get("currentSkuImages"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_else(|| {
            if let Some(img) = global_data.get("image").and_then(|v| v.as_str()) {
                vec![json!(img)]
            } else {
                vec![]
            }
        });
    let mut images = Vec::new();
    for url_val in &candidates {
        if let Some(url) = url_val.as_str() {
            let normalized = normalize_url(url, "https://www.aliexpress.com/");
            if !normalized.is_empty() && !images.contains(&normalized) {
                images.push(normalized);
            }
        }
    }
    images
}

/// Extract suggestions from either response shape (AE-RUST-017),
/// following the Python parser's suggestions.py. Returns
/// (keywords, shape_tag) so callers can record which shape was seen:
/// "recommend_mods_asQueryList" for the measured recommend shape,
/// "apk_containers" for the APK API shape, "generic_walk" when only
/// the generic fallback found anything, "" when nothing was found.
fn parse_suggestions(response: &Value) -> (Vec<String>, String) {
    let data = response.get("data").cloned().unwrap_or(Value::Null);
    // Recommend shape: mods.asQueryList.content[].keywords
    let mut keywords: Vec<String> = Vec::new();
    if let Some(arr) = data
        .get("mods")
        .and_then(|m| m.get("asQueryList"))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.as_array())
    {
        for item in arr {
            if let Some(kw) = item.get("keywords").and_then(|v| v.as_str()) {
                let cleaned = clean_text(kw);
                if !cleaned.is_empty() && !keywords.contains(&cleaned) {
                    keywords.push(cleaned);
                }
            }
        }
    }
    if !keywords.is_empty() {
        return (keywords, "recommend_mods_asQueryList".to_string());
    }
    // APK shape: known containers of strings / keyword objects.
    const CONTAINERS: [&str; 6] = [
        "suggestions",
        "suggestionList",
        "list",
        "dataList",
        "result",
        "items",
    ];
    const KW_KEYS: [&str; 6] = ["keyWord", "keyword", "query", "text", "word", "catName"];
    let push_kw = |text: &str, out: &mut Vec<String>| {
        let cleaned = clean_text(text);
        if !cleaned.is_empty() && !out.contains(&cleaned) {
            out.push(cleaned);
        }
    };
    if let Some(obj) = data.as_object() {
        for key in CONTAINERS {
            if let Some(container) = obj.get(key) {
                match container {
                    Value::Array(arr) => {
                        for entry in arr {
                            match entry {
                                Value::String(s) => push_kw(s, &mut keywords),
                                Value::Object(e) => {
                                    for k in KW_KEYS {
                                        if let Some(v) = e.get(k).and_then(|x| x.as_str()) {
                                            push_kw(v, &mut keywords);
                                            break;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Value::Object(single) => {
                        for k in KW_KEYS {
                            if let Some(v) = single.get(k).and_then(|x| x.as_str()) {
                                push_kw(v, &mut keywords);
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if !keywords.is_empty() {
        return (keywords, "apk_containers".to_string());
    }
    let walked = normalize_suggestions(&data);
    if !walked.is_empty() {
        return (walked, "generic_walk".to_string());
    }
    (Vec::new(), String::new())
}

/// Generic keyword-object walk (fallback of parse_suggestions;
/// AE-RUST-017 wires this previously dead helper in).
fn normalize_suggestions(data: &Value) -> Vec<String> {
    let mut suggestions = Vec::new();

    fn walk(value: &Value, suggestions: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for key in &["keyWord", "keyword", "query", "text", "word", "catName"] {
                    if let Some(v) = map.get(*key).and_then(|v| v.as_str()) {
                        let cleaned = clean_text(v);
                        if !cleaned.is_empty()
                            && !suggestions.contains(&cleaned)
                            && cleaned.len() <= 120
                        {
                            suggestions.push(cleaned);
                        }
                    }
                }
                for v in map.values() {
                    walk(v, suggestions);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    walk(v, suggestions);
                }
            }
            _ => {}
        }
    }

    walk(data, &mut suggestions);
    suggestions
}

/// Search problem check built on the single classification
/// (AE-RUST-001/003): a search is a problem unless classify_mtop
/// says success — which already encodes the Python rule that a
/// data-side request_invalid (searchTips tipsType) takes priority
/// over a SUCCESS ret string. Kept as a small named predicate for
/// tests and future call sites; the public methods classify through
/// MtopCall directly.
#[allow(dead_code)]
fn has_search_problem(data: &Value, ret: &Option<Value>) -> bool {
    let envelope = json!({"data": data, "ret": ret.clone().unwrap_or(json!([]))});
    classify_mtop(&envelope).0 != "success"
}

/// `ok` is exactly classification == success (AE-RUST-003).
#[allow(dead_code)]
fn mtop_success(response: &Value) -> bool {
    classify_mtop(response).0 == "success"
}

fn search_card_title(obj: &serde_json::Map<String, Value>) -> Option<String> {
    match obj.get("title") {
        Some(Value::Object(title_obj)) => {
            let t = first_str(title_obj, &["displayTitle", "seoTitle", "title"]);
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn search_card_image(obj: &serde_json::Map<String, Value>) -> Option<String> {
    match obj.get("image") {
        Some(Value::Object(img_obj)) => {
            let i = first_str(img_obj, &["imgUrl", "imageUrl", "url"]);
            if i.is_empty() {
                None
            } else {
                Some(i)
            }
        }
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn search_card_price(obj: &serde_json::Map<String, Value>) -> Option<Value> {
    if let Some(prices) = obj.get("prices").and_then(|v| v.as_object()) {
        if let Some(sale_price) = prices.get("salePrice").and_then(|v| v.as_object()) {
            for key in &[
                "formattedPrice",
                "formattedActivityPrice",
                "cent",
                "minPrice",
            ] {
                if let Some(v) = sale_price.get(*key) {
                    if !is_empty_val(v) {
                        // Keep the dict context so the currency
                        // survives into build_price (Python
                        // _card_price_value).
                        return Some(Value::Object(sale_price.clone()));
                    }
                }
            }
        }
        for key in &[
            "formattedPrice",
            "salePriceString",
            "displayPrice",
            "minPrice",
        ] {
            if let Some(v) = prices.get(*key) {
                if !is_empty_val(v) {
                    return Some(v.clone());
                }
            }
        }
    }
    if let Some(trace) = obj.get("trace").and_then(|v| v.as_object()) {
        if let Some(ut_log) = trace.get("utLogMap").and_then(|v| v.as_object()) {
            if let Some(v) = ut_log.get("formatted_price") {
                if !is_empty_val(v) {
                    return Some(v.clone());
                }
            }
            if let Some(v) = ut_log.get("salePriceAmount") {
                if !is_empty_val(v) {
                    // Cent-denominated (Python investigation
                    // 2026-10-30): wrap as a cent dict so build_price
                    // applies the /100 conversion instead of reading
                    // the raw number as major units.
                    return Some(json!({"cent": v}));
                }
            }
        }
    }
    None
}

fn first_str(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    for key in keys {
        if let Some(v) = obj.get(*key) {
            match v {
                Value::String(s) if !s.trim().is_empty() => return s.clone(),
                Value::Number(n) => return n.to_string(),
                _ => {}
            }
        }
    }
    String::new()
}

fn first_present(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Value {
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if !is_empty_val(v) {
                return v.clone();
            }
        }
    }
    Value::Null
}

fn is_empty_val(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

fn json_value_to_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Common conversions (AE-RUST-019/020/021/024): ports of the Python
// parser's convert.py. Every failed conversion returns None (never a
// guessed value); callers attach warnings.
// ---------------------------------------------------------------------------

/// Full-width digits/punctuation to ASCII (Python _FULLWIDTH map).
fn fullwidth_to_ascii(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '０' => '0',
            '１' => '1',
            '２' => '2',
            '３' => '3',
            '４' => '4',
            '５' => '5',
            '６' => '6',
            '７' => '7',
            '８' => '8',
            '９' => '9',
            '．' => '.',
            '，' => ',',
            other => other,
        })
        .collect()
}

/// Normalize a decimal token to a canonical string: strip grouping
/// commas, leading zeros and trailing fractional zeros.
fn normalize_decimal_token(token: &str) -> Option<String> {
    let t = token.replace(',', "");
    if t.is_empty() {
        return None;
    }
    let (neg, rest) = if let Some(r) = t.strip_prefix('-') {
        (true, r)
    } else {
        (false, t.as_str())
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, f),
        None => (rest, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let int_norm = {
        let stripped = int_part.trim_start_matches('0');
        if stripped.is_empty() {
            "0"
        } else {
            stripped
        }
    };
    let frac_norm = frac_part.trim_end_matches('0');
    let mut out = String::new();
    if neg && !(int_norm == "0" && frac_norm.is_empty()) {
        out.push('-');
    }
    out.push_str(int_norm);
    if !frac_norm.is_empty() {
        out.push('.');
        out.push_str(frac_norm);
    }
    Some(out)
}

/// Decimal-string conversion (Python to_decimal): accepts numbers
/// and numeric/formatted strings (commas, currency symbols/text,
/// full-width digits); the first numeric token of a string is used.
fn to_decimal_str(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Some(i.to_string())
            } else if let Some(u) = n.as_u64() {
                Some(u.to_string())
            } else if let Some(f) = n.as_f64() {
                // via the shortest repr, like Python Decimal(str(f))
                normalize_decimal_token(&f.to_string())
            } else {
                None
            }
        }
        Some(Value::String(s)) => {
            let text = fullwidth_to_ascii(s.trim());
            if text.is_empty() {
                return None;
            }
            if let Some(norm) = normalize_decimal_token(&text) {
                return Some(norm);
            }
            let m = NUMBER_TOKEN_RE.find(&text)?;
            normalize_decimal_token(m.as_str())
        }
        _ => None,
    }
}

/// Accept ints, integral floats and numeric strings (Python to_int).
fn to_int(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).ok()
            } else if let Some(f) = n.as_f64() {
                if f == f.trunc() && f.abs() < 9.007_199_254_740_992e15 {
                    Some(f as i64)
                } else {
                    None
                }
            } else {
                None
            }
        }
        Some(Value::String(_)) => {
            let dec = to_decimal_str(value)?;
            if dec.contains('.') {
                None
            } else {
                dec.parse::<i64>().ok()
            }
        }
        _ => None,
    }
}

fn to_float(value: Option<&Value>) -> Option<f64> {
    to_decimal_str(value).and_then(|d| d.parse::<f64>().ok())
}

/// Accept real bools, string bools and 0/1 (Python to_bool,
/// AE-RUST-020).
fn to_bool(value: Option<&Value>, default: Option<bool>) -> Option<bool> {
    match value {
        Some(Value::Bool(b)) => Some(*b),
        Some(Value::Number(n)) => {
            let f = n.as_f64()?;
            if f == 1.0 {
                Some(true)
            } else if f == 0.0 {
                Some(false)
            } else {
                default
            }
        }
        Some(Value::String(s)) => {
            let low = s.trim().to_lowercase();
            match low.as_str() {
                "true" | "1" | "yes" | "y" | "on" => Some(true),
                "false" | "0" | "no" | "n" | "off" | "" => Some(false),
                _ => default,
            }
        }
        _ => default,
    }
}

/// Value-to-string accepting numbers and numeric strings
/// (AE-RUST-021); bools and containers yield "".
fn value_to_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Divide a cent amount by 100 as a decimal string.
fn cent_to_amount_str(cent: i64) -> String {
    let neg = cent < 0;
    let abs = cent.unsigned_abs();
    let int_part = abs / 100;
    let frac = abs % 100;
    let raw = if frac == 0 {
        format!("{}", int_part)
    } else {
        format!("{}.{:02}", int_part, frac)
    };
    let norm = normalize_decimal_token(&raw).unwrap_or(raw);
    if neg && norm != "0" {
        format!("-{}", norm)
    } else {
        norm
    }
}

fn price_from_cent(cent: &Value, currency: Option<&str>, raw: Value) -> Option<Value> {
    let cent_int = to_int(Some(cent))?;
    Some(json!({
        "amount": cent_to_amount_str(cent_int),
        "currency": currency,
        "formatted": Value::Null,
        "amount_cent": cent_int,
        "notes": [],
        "raw": raw,
    }))
}

/// Build the normalized price object (AE-RUST-019), a port of the
/// Python build_price: {amount (decimal string), currency, formatted,
/// amount_cent, notes, raw}. Cent-denominated fields take precedence
/// but are cross-checked against the formatted amount (0.1.1 fix):
/// when the formatted amount equals the raw cent value, the cent
/// field actually carries major units and the amount is corrected
/// with a note; when they simply disagree, the discrepancy is noted
/// and the cent-derived amount is kept.
fn build_price(value: &Value, currency_hint: Option<&str>) -> Option<Value> {
    if is_empty_val(value) {
        return None;
    }
    if let Some(obj) = value.as_object() {
        let currency = ["currencyCode", "currency", "currency_code"]
            .iter()
            .find_map(|k| obj.get(*k).and_then(|v| v.as_str()))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| currency_hint.map(|s| s.to_string()));
        let formatted = [
            "formattedPrice",
            "formattedActivityPrice",
            "formatted_price",
            "displayPrice",
            "salePriceString",
            "formatedPromotionPiecePriceStr",
            "formatedPiecePriceStr",
        ]
        .iter()
        .find_map(|k| obj.get(*k).and_then(|v| v.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
        for key in ["amountCent", "cent"] {
            if let Some(cent_val) = obj.get(key) {
                if !cent_val.is_null() {
                    if let Some(mut price) =
                        price_from_cent(cent_val, currency.as_deref(), value.clone())
                    {
                        price["formatted"] = json!(formatted);
                        if let Some(fmt) = &formatted {
                            if let (Some(fmt_amount), Some(cent_int)) = (
                                to_decimal_str(Some(&Value::String(fmt.clone()))),
                                to_int(Some(cent_val)),
                            ) {
                                let cent_amount =
                                    price["amount"].as_str().unwrap_or("").to_string();
                                if fmt_amount == cent_int.to_string() && cent_amount != fmt_amount {
                                    price["amount"] = json!(fmt_amount);
                                    price["amount_cent"] = Value::Null;
                                    price["notes"] = json!(["cent field interpreted as major units (formatted amount cross-check)"]);
                                } else if fmt_amount != cent_amount {
                                    price["notes"] = json!(["formatted amount disagrees with cent-derived amount; cent value kept, see raw"]);
                                }
                            }
                        }
                        return Some(price);
                    }
                }
            }
        }
        for key in [
            "value",
            "amount",
            "minPrice",
            "salePriceAmount",
            "salePrice",
            "price",
        ] {
            if let Some(v) = obj.get(key) {
                if !v.is_null() {
                    if let Some(amount) = to_decimal_str(Some(v)) {
                        return Some(json!({
                            "amount": amount,
                            "currency": currency,
                            "formatted": formatted,
                            "amount_cent": Value::Null,
                            "notes": [],
                            "raw": value,
                        }));
                    }
                }
            }
        }
        if let Some(fmt) = &formatted {
            if let Some(amount) = to_decimal_str(Some(&Value::String(fmt.clone()))) {
                return Some(json!({
                    "amount": amount,
                    "currency": currency,
                    "formatted": formatted,
                    "amount_cent": Value::Null,
                    "notes": [],
                    "raw": value,
                }));
            }
        }
        return None;
    }
    // Scalar: number / numeric or formatted string.
    let amount = to_decimal_str(Some(value))?;
    let formatted = match value {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        _ => None,
    };
    Some(json!({
        "amount": amount,
        "currency": currency_hint,
        "formatted": formatted,
        "amount_cent": Value::Null,
        "notes": [],
        "raw": value,
    }))
}

/// Build a price from an Android PDP price block (Python _price_block,
/// 0.1.1 fix): amountCent and value are NOT assumed to be the same
/// quantity in the same unit. The cent-derived amount is kept; an
/// agreeing value is confirmation only; a disagreeing value produces
/// a warning and a note instead of a silent overwrite.
fn price_block_price(block: &Value, currency_hint: Option<&str>) -> (Option<Value>, Vec<String>) {
    let mut warnings: Vec<String> = Vec::new();
    let obj = match block.as_object() {
        Some(o) if !o.is_empty() => o,
        _ => return (None, warnings),
    };
    let currency = obj
        .get("currencyCode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| currency_hint.map(|s| s.to_string()));
    if let Some(cent_val) = obj.get("amountCent") {
        if !cent_val.is_null() {
            if let Some(mut price) = price_from_cent(cent_val, currency.as_deref(), block.clone()) {
                if let Some(value_field) = obj.get("value") {
                    if !value_field.is_null() {
                        if let Some(value_price) = build_price(value_field, None) {
                            let value_amount = value_price.get("amount").and_then(|v| v.as_str());
                            let cent_amount = price.get("amount").and_then(|v| v.as_str());
                            if value_amount.is_some() && value_amount != cent_amount {
                                warnings.push("price block value disagrees with amountCent-derived amount; cent-derived amount kept, raw preserved".to_string());
                                price["notes"] =
                                    json!(["value field disagrees with amountCent; see raw"]);
                            }
                        }
                    }
                }
                return (Some(price), warnings);
            }
        }
    }
    if let Some(value_field) = obj.get("value") {
        if !value_field.is_null() {
            if let Some(mut price) = build_price(value_field, currency.as_deref()) {
                price["raw"] = block.clone();
                return (Some(price), warnings);
            }
        }
    }
    (None, warnings)
}

/// First integer found for `keys` at the top of `data` or up to 3
/// levels of nested objects (Python _find_int); accepts numeric
/// strings via to_int.
fn find_search_int(data: &Value, keys: &[&str]) -> Option<i64> {
    fn go(node: &Value, keys: &[&str], depth: usize) -> Option<i64> {
        if depth > 3 {
            return None;
        }
        let obj = node.as_object()?;
        for k in keys {
            if let Some(v) = to_int(obj.get(*k)) {
                return Some(v);
            }
        }
        for v in obj.values() {
            if v.is_object() {
                if let Some(r) = go(v, keys, depth + 1) {
                    return Some(r);
                }
            }
        }
        None
    }
    go(data, keys, 0)
}

/// Review total: server-reported total kept distinct from the page
/// count; the first of totalItem/totalNum/totalCount/total that
/// parses (numbers or numeric strings, AE-RUST-021).
fn reviews_total(data: &Value) -> Option<i64> {
    for key in ["totalItem", "totalNum", "totalCount", "total"] {
        if let Some(v) = to_int(data.get(key)) {
            return Some(v);
        }
    }
    None
}

/// Decode HTML entities (AE-RUST-024): named entities for the common
/// set plus decimal/hex numeric references. Unknown entities are
/// left as-is.
fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        let mut entity = String::new();
        let mut terminated = false;
        for _ in 0..32 {
            match chars.next() {
                Some(';') => {
                    terminated = true;
                    break;
                }
                Some(ch) if ch.is_ascii_alphanumeric() || ch == '#' => entity.push(ch),
                Some(ch) => {
                    // Not an entity: emit what we consumed plus ch.
                    out.push('&');
                    out.push_str(&entity);
                    out.push(ch);
                    entity.clear();
                    break;
                }
                None => break,
            }
        }
        if !terminated {
            if !entity.is_empty() {
                out.push('&');
                out.push_str(&entity);
            }
            continue;
        }
        let decoded: Option<String> = match entity.as_str() {
            "amp" => Some("&".to_string()),
            "lt" => Some("<".to_string()),
            "gt" => Some(">".to_string()),
            "quot" => Some("\"".to_string()),
            "apos" => Some("'".to_string()),
            "nbsp" => Some(" ".to_string()),
            _ => {
                if let Some(num) = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                {
                    u32::from_str_radix(num, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .map(|c| c.to_string())
                } else if let Some(num) = entity.strip_prefix('#') {
                    num.parse::<u32>()
                        .ok()
                        .and_then(char::from_u32)
                        .map(|c| c.to_string())
                } else {
                    None
                }
            }
        };
        match decoded {
            Some(d) => out.push_str(&d),
            None => {
                out.push('&');
                out.push_str(&entity);
                out.push(';');
            }
        }
    }
    out
}

fn clean_text(value: &str) -> String {
    // AE-RUST-024: strip tags, decode entities, collapse whitespace
    // (Python clean_text order).
    let no_tags = HTML_TAG_RE.replace_all(value, " ");
    let decoded = decode_html_entities(&no_tags);
    WHITESPACE_RE.replace_all(&decoded, " ").trim().to_string()
}

fn normalize_url(url: &str, base: &str) -> String {
    // AE-RUST-024: relative URLs are resolved against base (Python
    // normalize_url via urljoin); empty only when unresolvable.
    let url = url.trim().replace("\\u0026", "&").replace("&amp;", "&");
    if url.is_empty() {
        return String::new();
    }
    if url.starts_with("//") {
        return format!("https:{}", url);
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return url;
    }
    if base.is_empty() {
        return String::new();
    }
    resolve_relative_url(&url, base)
}

/// Minimal RFC-3986 relative resolution against an absolute base.
fn resolve_relative_url(rel: &str, base: &str) -> String {
    let base = base.trim();
    let scheme_end = match base.find("://") {
        Some(p) => p,
        None => return String::new(),
    };
    let scheme = &base[..scheme_end];
    let rest = &base[scheme_end + 3..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..host_end];
    if host.is_empty() {
        return String::new();
    }
    if let Some(stripped) = rel.strip_prefix('/') {
        return format!("{}://{}/{}", scheme, host, stripped);
    }
    // Directory of the base path.
    let base_path = &rest[host_end..];
    let dir = match base_path.rfind('/') {
        Some(idx) => &base_path[..=idx],
        None => "/",
    };
    let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    format!("{}://{}/{}/", scheme, host, segments.join("/"))
        .trim_end_matches('/')
        .to_string()
        + if rel.ends_with('/') { "/" } else { "" }
}

fn canonical_product_url(product_id: &str) -> String {
    format!("https://www.aliexpress.com/item/{}.html", product_id)
}

fn md5_hash(data: &[u8]) -> u128 {
    // Simple MD5 implementation using the md-5 algorithm
    // We compute MD5 manually since we don't want to add another dependency
    md5_compute(data)
}

fn urlencoding_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

fn md5_compute(data: &[u8]) -> u128 {
    // MD5 constants
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let orig_len_bits = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&orig_len_bits.to_le_bytes());

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for (i, word) in chunk.chunks(4).enumerate() {
            m[i] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        }

        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let bytes: [u8; 16] = [
        a0 as u8,
        (a0 >> 8) as u8,
        (a0 >> 16) as u8,
        (a0 >> 24) as u8,
        b0 as u8,
        (b0 >> 8) as u8,
        (b0 >> 16) as u8,
        (b0 >> 24) as u8,
        c0 as u8,
        (c0 >> 8) as u8,
        (c0 >> 16) as u8,
        (c0 >> 24) as u8,
        d0 as u8,
        (d0 >> 8) as u8,
        (d0 >> 16) as u8,
        (d0 >> 24) as u8,
    ];
    u128::from_be_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_product_id_url() {
        assert_eq!(
            extract_product_id("https://www.aliexpress.com/item/1005012345678901.html"),
            Some("1005012345678901".to_string())
        );
    }

    #[test]
    fn test_extract_product_id_short_url() {
        assert_eq!(
            extract_product_id("https://www.aliexpress.com/i/1005012345678901.html"),
            Some("1005012345678901".to_string())
        );
    }

    #[test]
    fn test_extract_product_id_param() {
        assert_eq!(
            extract_product_id("?productId=1005012345678901"),
            Some("1005012345678901".to_string())
        );
    }

    #[test]
    fn test_extract_product_id_bare() {
        assert_eq!(
            extract_product_id("1005012345678901"),
            Some("1005012345678901".to_string())
        );
    }

    #[test]
    fn test_extract_product_id_none() {
        assert_eq!(extract_product_id("hello world"), None);
    }

    #[test]
    fn test_md5() {
        let hash = md5_compute(b"");
        assert_eq!(format!("{:032x}", hash), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn test_md5_hello() {
        let hash = md5_compute(b"hello");
        assert_eq!(format!("{:032x}", hash), "5d41402abc4b2a76b9719d911017c592");
    }

    #[test]
    fn test_host_for_locale() {
        assert_eq!(host_for_locale("ja_JP"), "ja.aliexpress.com");
        assert_eq!(host_for_locale("en_US"), "www.aliexpress.com");
    }

    #[test]
    fn test_site_for_locale() {
        assert_eq!(site_for_locale("ja_JP"), "jpn");
        assert_eq!(site_for_locale("en_US"), "glo");
    }

    #[test]
    fn test_normalize_url() {
        assert_eq!(
            normalize_url(
                "//img.aliexpress.com/test.jpg",
                "https://www.aliexpress.com/"
            ),
            "https://img.aliexpress.com/test.jpg"
        );
        assert_eq!(normalize_url("", "https://www.aliexpress.com/"), "");
    }

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding_encode("hello world"), "hello%20world");
        assert_eq!(urlencoding_encode("a=b&c=d"), "a%3Db%26c%3Dd");
    }

    #[test]
    fn test_md5_sign_source() {
        let sign_source = "abc123&1700000000000&12574478&{\"q\":\"test\"}";
        let hash = md5_compute(sign_source.as_bytes());
        let hex = format!("{:032x}", hash);
        assert_eq!(hex.len(), 32);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_extract_product_id_with_query_params() {
        assert_eq!(
            extract_product_id("https://www.aliexpress.com/item/1005012345678901.html?spm=abc"),
            Some("1005012345678901".to_string())
        );
        assert_eq!(
            extract_product_id("https://www.aliexpress.com/item/1005012345678901.html#anchor"),
            Some("1005012345678901".to_string())
        );
    }

    #[test]
    fn test_extract_product_id_product_ids_param() {
        assert_eq!(
            extract_product_id("?productIds=1005099999999999"),
            Some("1005099999999999".to_string())
        );
    }

    #[test]
    fn test_extract_product_id_short_numeric_rejected() {
        assert_eq!(extract_product_id("12345"), None);
        assert_eq!(extract_product_id("1005"), None);
    }

    #[test]
    fn test_host_for_all_locales() {
        assert_eq!(host_for_locale("ja_JP"), "ja.aliexpress.com");
        assert_eq!(host_for_locale("en_US"), "www.aliexpress.com");
        assert_eq!(host_for_locale("ko_KR"), "ko.aliexpress.com");
        assert_eq!(host_for_locale("fr_FR"), "fr.aliexpress.com");
        assert_eq!(host_for_locale("xx_XX"), "www.aliexpress.com");
    }

    #[test]
    fn test_site_for_all_locales() {
        assert_eq!(site_for_locale("ja_JP"), "jpn");
        assert_eq!(site_for_locale("en_US"), "glo");
        assert_eq!(site_for_locale("ko_KR"), "kor");
        assert_eq!(site_for_locale("xx_XX"), "glo");
    }

    #[test]
    fn test_origin_from_referer() {
        assert_eq!(
            origin_from_referer("https://ja.aliexpress.com/item/123.html"),
            "https://ja.aliexpress.com"
        );
        assert_eq!(
            origin_from_referer("https://www.aliexpress.com/"),
            "https://www.aliexpress.com"
        );
        assert_eq!(
            origin_from_referer("not-a-url"),
            "https://www.aliexpress.com"
        );
    }

    #[test]
    fn test_normalize_url_variants() {
        assert_eq!(
            normalize_url("//img.aliexpress.com/test.jpg", ""),
            "https://img.aliexpress.com/test.jpg"
        );
        assert_eq!(
            normalize_url("https://img.aliexpress.com/test.jpg", ""),
            "https://img.aliexpress.com/test.jpg"
        );
        assert_eq!(
            normalize_url("http://img.aliexpress.com/test.jpg", ""),
            "http://img.aliexpress.com/test.jpg"
        );
        assert_eq!(normalize_url("", ""), "");
        assert_eq!(normalize_url("relative/path.jpg", ""), "");
        assert_eq!(
            normalize_url("//img.com/a\\u0026b", ""),
            "https://img.com/a&b"
        );
    }

    #[test]
    fn test_urlencoding_full() {
        assert_eq!(urlencoding_encode("hello world"), "hello%20world");
        assert_eq!(urlencoding_encode("a=b&c=d"), "a%3Db%26c%3Dd");
        assert_eq!(urlencoding_encode("abc123"), "abc123");
        assert_eq!(urlencoding_encode("日本語"), "%E6%97%A5%E6%9C%AC%E8%AA%9E");
        assert_eq!(urlencoding_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn test_clean_text_html() {
        assert_eq!(clean_text("<b>Bold</b> text"), "Bold text");
        assert_eq!(clean_text("  spaces   "), "spaces");
        assert_eq!(clean_text("no tags"), "no tags");
        assert_eq!(clean_text("<a href='x'>link</a>"), "link");
    }

    #[test]
    fn test_search_region_payload() {
        let payload = search_region_payload("ja_JP", "JPY", "JP");
        assert_eq!(payload.get("_lang").unwrap(), "ja_JP");
        assert_eq!(payload.get("_currency").unwrap(), "JPY");
        assert_eq!(payload.get("locale").unwrap(), "ja_JP");
        assert!(payload.contains_key("_city"));
        assert!(payload.contains_key("_state"));
        assert!(payload.contains_key("_stateName"));
        assert!(payload.contains_key("_cityName"));
    }

    #[test]
    fn test_android_region_defaults_jp() {
        // AE-RUST-009: the captured Sapporo/Hokkaido codes are gone;
        // unknown city/state stay empty for every ship_to.
        let region = android_region_defaults("JP");
        assert!(region["city"].is_empty());
        assert!(region["state"].is_empty());
        assert!(region["state_name"].is_empty());
        assert!(region["city_name"].is_empty());
    }

    #[test]
    fn test_android_region_defaults_us() {
        let region = android_region_defaults("US");
        assert!(region["city"].is_empty());
        assert!(region["state"].is_empty());
    }

    #[test]
    fn test_build_search_params() {
        let params = build_search_params(
            "test query",
            2,
            20,
            "ja_JP",
            "JPY",
            "JP",
            "device123",
            "price_asc",
            None,
            None,
            None,
            None,
            &[],
        );
        assert_eq!(params["keywords"], "test query");
        assert_eq!(params["q"], "test query");
        assert_eq!(params["page"], "2");
        assert_eq!(params["pageSize"], "20");
        assert_eq!(params["clientType"], "android");
        assert_eq!(params["shpt_co"], "JP");
        assert_eq!(params["sortParam"], "price_asc");
        assert_eq!(params["deviceId"], "device123");
        // AE-RUST-007: pv_feature omitted on a first request.
        assert!(params.get("pv_feature").is_none());
        assert!(params.get("prePageExposureProducts").is_none());
        // AE-RUST-009: no captured city/state codes.
        assert_eq!(params["_city"], "");
        assert_eq!(params["_state"], "");
    }

    #[test]
    fn test_build_search_params_session_pv_feature() {
        // AE-RUST-007: subsequent searches carry session-built values.
        let params = build_search_params(
            "test",
            2,
            20,
            "ja_JP",
            "JPY",
            "JP",
            "dev",
            "",
            Some("pv_from_own_session"),
            Some("1005000000000001"),
            None,
            None,
            &[],
        );
        assert_eq!(params["pv_feature"], "pv_from_own_session");
        assert_eq!(params["prePageExposureProducts"], "1005000000000001");
    }

    #[test]
    fn test_build_search_params_no_sort() {
        let params = build_search_params(
            "test",
            1,
            10,
            "en_US",
            "USD",
            "US",
            "dev",
            "",
            None,
            None,
            None,
            None,
            &[],
        );
        assert!(params.get("sortParam").is_none());
    }

    #[test]
    fn test_normalize_search_items_empty() {
        let data = json!({});
        let items = normalize_search_items(&data, "https://www.aliexpress.com/");
        assert!(items.is_empty());
    }

    #[test]
    fn test_normalize_search_items_dedup() {
        let data = json!({
            "items": [
                {"productId": "1005099999999999", "title": "Item A", "imageUrl": "//img.com/a.jpg"},
                {"productId": "1005099999999999", "title": "Item A dup", "imageUrl": "//img.com/b.jpg"},
                {"productId": "1005088888888888", "title": "Item B", "imageUrl": "//img.com/c.jpg"},
            ]
        });
        let items = normalize_search_items(&data, "https://www.aliexpress.com/");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["product_id"], "1005099999999999");
        assert_eq!(items[1]["product_id"], "1005088888888888");
    }

    #[test]
    fn test_normalize_search_item_fields() {
        let data = json!({
            "items": [{
                "productId": "1005099999999999",
                "title": "Test Product",
                "imageUrl": "//img.com/test.jpg",
                "formattedPrice": "¥ 1,234",
                "storeName": "Test Store"
            }]
        });
        let items = normalize_search_items(&data, "https://www.aliexpress.com/");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item["product_id"], "1005099999999999");
        assert_eq!(item["title"], "Test Product");
        assert_eq!(item["price"], "¥ 1,234");
        assert_eq!(item["image"], "https://img.com/test.jpg");
        assert_eq!(item["store"], "Test Store");
        assert_eq!(
            item["url"],
            "https://www.aliexpress.com/item/1005099999999999.html"
        );
        assert_eq!(item["source"], "direct_mtop_search");
    }

    #[test]
    fn test_has_search_problem() {
        let ok_data = json!({"mods": {}});
        let success_ret = Some(json!(["SUCCESS::调用成功"]));
        assert!(!has_search_problem(&ok_data, &success_ret));

        // Data-side request_invalid wins even over ret SUCCESS
        // (AE-RUST-003, Python mtop.py behavior).
        let bad_data = json!({
            "mods": {"searchTips": {"content": {"tipsType": "request_invalid"}}}
        });
        assert!(has_search_problem(&bad_data, &success_ret));
        assert!(has_search_problem(&bad_data, &None));
        // No ret at all is not success.
        assert!(has_search_problem(&ok_data, &None));
    }

    // ---- Ported offline suites (AE-RUST-001..029 resolutions) ----

    #[test]
    fn test_classify_mtop_matrix() {
        let cases: Vec<(Value, &str)> = vec![
            (json!({"ret": ["SUCCESS::ok"]}), "success"),
            (json!({"ret": ["FAIL_SYS_TOKEN_EMPTY::x"]}), "token_empty"),
            (
                json!({"ret": ["FAIL_SYS_TOKEN_EXPIRED::x"]}),
                "token_expired",
            ),
            (
                json!({"ret": ["FAIL_SYS_USER_VALIDATE::x"]}),
                "verification",
            ),
            (json!({"ret": ["RGV587_ERROR::x"]}), "verification"),
            (json!({"ret": ["FAIL_SYS_RATE_LIMIT::x"]}), "rate_limit"),
            (
                json!({"ret": ["FAIL_SYS_ILLEGAL_ACCESS::x"]}),
                "illegal_access",
            ),
            (
                json!({"ret": ["FAIL_SYS_SESSION_EXPIRED::x"]}),
                "session_expired",
            ),
            (json!({"ret": ["FAIL_SYS_SOMETHING::x"]}), "failure"),
            (json!({}), "unknown"),
            // Data-side request_invalid beats ret SUCCESS (003).
            (
                json!({"ret": ["SUCCESS::ok"], "data": {"mods": {"searchTips": {"content": {"tipsType": "request_invalid"}}}}}),
                "request_invalid",
            ),
        ];
        for (resp, expected) in cases {
            assert_eq!(classify_mtop(&resp).0, expected, "resp={}", resp);
        }
    }

    #[test]
    fn test_retry_decision_per_category() {
        // Only token categories retry (011/012).
        assert!(is_token_retryable("token_empty"));
        assert!(is_token_retryable("token_expired"));
        for c in [
            "success",
            "illegal_access",
            "verification",
            "rate_limit",
            "request_invalid",
            "session_expired",
            "failure",
            "unknown",
        ] {
            assert!(!is_token_retryable(c), "{}", c);
        }
        // Blocking categories abort multi-route calls.
        assert!(is_blocking_category("illegal_access"));
        assert!(is_blocking_category("verification"));
        assert!(is_blocking_category("rate_limit"));
        assert!(!is_blocking_category("failure"));
    }

    #[test]
    fn test_device_id_generated_not_captured() {
        // AE-RUST-008: generated per instance, stable per instance,
        // never the removed captured constant.
        let a = AliExpressClient::new();
        let b = AliExpressClient::new();
        assert_eq!(a.device_id, a.device_id);
        assert_ne!(a.device_id, b.device_id);
        assert!(a.device_id.contains("/v4DA"));
        assert_eq!(a.device_id.len(), 24 + 5 + 8);
    }

    #[test]
    fn test_price_cent_formatted_cross_check() {
        // Python 0.1.1 case: cent field actually carries major units.
        let p = build_price(
            &json!({"cent": 175, "currencyCode": "JPY", "formattedPrice": "¥175"}),
            None,
        )
        .unwrap();
        assert_eq!(p["amount"], "175");
        assert!(p["amount_cent"].is_null());
        assert!(p["notes"][0].as_str().unwrap().contains("major units"));
        // Genuine cent case.
        let p = build_price(
            &json!({"amountCent": 109, "currencyCode": "USD", "formattedPrice": "US $1.09"}),
            None,
        )
        .unwrap();
        assert_eq!(p["amount"], "1.09");
        assert_eq!(p["amount_cent"], 109);
        assert_eq!(p["notes"].as_array().unwrap().len(), 0);
        // trace salePriceAmount is cent-denominated (109 -> 1.09, not 109).
        let p = build_price(&json!({"cent": 109}), Some("USD")).unwrap();
        assert_eq!(p["amount"], "1.09");
        // Formatted string parsing: symbols, commas, full-width digits.
        let p = build_price(&json!("US $1,234.50"), None).unwrap();
        assert_eq!(p["amount"], "1234.5");
    }

    #[test]
    fn test_price_block_value_conflict_keeps_cent() {
        // Python 0.1.1 _price_block case: value disagrees with
        // amountCent; cent-derived amount wins, warning recorded.
        let (price, warnings) = price_block_price(
            &json!({"amountCent": 1099, "value": "55.00", "currencyCode": "USD"}),
            None,
        );
        let price = price.unwrap();
        assert_eq!(price["amount"], "10.99");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_clean_text_entities_and_relative_url() {
        assert_eq!(clean_text("Fish &amp; Chips &#8212; x"), "Fish & Chips — x");
        assert_eq!(clean_text("<b>Bold</b> &lt;tag&gt;"), "Bold <tag>");
        // AE-RUST-024: relative URLs resolve against the base.
        assert_eq!(
            normalize_url("item/123.html", "https://www.aliexpress.com/store/"),
            "https://www.aliexpress.com/store/item/123.html"
        );
        assert_eq!(
            normalize_url("/item/123.html", "https://www.aliexpress.com/store/"),
            "https://www.aliexpress.com/item/123.html"
        );
        assert_eq!(normalize_url("rel/path", ""), "");
    }

    #[test]
    fn test_reviews_numeric_string_total() {
        // AE-RUST-021: totals accept numeric strings.
        assert_eq!(reviews_total(&json!({"totalNum": "1234"})), Some(1234));
        assert_eq!(reviews_total(&json!({"totalItem": 42})), Some(42));
        assert_eq!(reviews_total(&json!({})), None);
        // buyerEval star accepts number or string.
        let reviews = normalize_reviews(&json!({"dataList": [
            {"buyerEval": "5", "feedback": "good", "images": [{"url": "//img.com/r.jpg"}]},
            {"buyerEval": 4, "feedback": "ok"}
        ]}));
        assert_eq!(reviews[0]["star"], 5.0);
        assert_eq!(reviews[1]["star"], 4.0);
        assert_eq!(reviews[0]["images"][0], "https://img.com/r.jpg");
    }

    #[tokio::test]
    async fn test_store_seller_id_required() {
        // Seller id is mandatory (measured FAIL_SYS_BIZPARAM_MISSED):
        // empty seller fails locally without any request.
        let client = AliExpressClient::new();
        let result = client
            .search_store_products("12345", "", 1, 10, "", "ja_JP", "JPY", "JP")
            .await;
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"]["category"], "request_invalid");
    }

    #[test]
    fn test_suggest_dual_shape_extraction() {
        // Recommend shape.
        let resp = json!({"data": {"mods": {"asQueryList": {"content": [
            {"keywords": "wireless earbuds"}, {"keywords": "wireless earbuds case"}
        ]}}}});
        let (kws, shape) = parse_suggestions(&resp);
        assert_eq!(shape, "recommend_mods_asQueryList");
        assert_eq!(kws.len(), 2);
        // APK shape.
        let resp = json!({"data": {"suggestions": [{"keyWord": "phone case"}, "charger"]}});
        let (kws, shape) = parse_suggestions(&resp);
        assert_eq!(shape, "apk_containers");
        assert_eq!(kws, vec!["phone case".to_string(), "charger".to_string()]);
    }

    #[test]
    fn test_meaningful_data_gate() {
        // AE-RUST-013/015: a bare entry (id only) is not a product.
        assert!(!has_meaningful_data(
            &json!({"product_id": "1005000000000001", "title": "", "price_detail": null, "images": []})
        ));
        assert!(has_meaningful_data(&json!({"title": "Something"})));
        // Bare-id search candidates are not counted as items
        // (feedback.params style dicts).
        let data = json!({"data": {"feedback": {"params": {"product_id": "1005000000000001", "product_type": "x"}}}});
        let items = normalize_search_items(&data, "https://www.aliexpress.com/");
        assert!(items.is_empty());
        // Declared-id extraction only trusts structured id fields.
        let entry =
            json!({"subject": "see 1005000000000002 in text", "productId": "1005000000000001"});
        let ids = entry_declared_ids(&entry);
        assert!(ids.contains("1005000000000001"));
        assert!(!ids.contains("1005000000000002"));
    }

    #[test]
    fn test_session_exposure_state() {
        // AE-RUST-007: pv_feature is built from the session's own
        // exposed ids, cumulative; prePage is the last page only.
        let client = AliExpressClient::new();
        assert!(client.pv_feature_for_next().is_none());
        assert!(client.pre_page_exposure().is_none());
        client.record_search_exposure(vec![
            "1005000000000001".to_string(),
            "1005000000000002".to_string(),
        ]);
        let pv = client.pv_feature_for_next().unwrap();
        assert!(pv.contains("1005000000000001"));
        assert_eq!(
            client.pre_page_exposure().unwrap(),
            "1005000000000001,1005000000000002"
        );
        client.record_search_exposure(vec!["1005000000000003".to_string()]);
        let pv = client.pv_feature_for_next().unwrap();
        assert!(pv.contains("1005000000000001"));
        assert!(pv.contains("1005000000000003"));
        assert_eq!(client.pre_page_exposure().unwrap(), "1005000000000003");
    }

    #[test]
    fn test_mtop_success() {
        assert!(mtop_success(&json!({"ret": ["SUCCESS::调用成功"]})));
        assert!(!mtop_success(
            &json!({"ret": ["FAIL_SYS_ILLEGAL_ACCESS::非法请求"]})
        ));
        assert!(!mtop_success(&json!({})));
    }

    #[tokio::test]
    #[ignore = "live network test (AE-RUST-029)"]
    async fn test_live_search() {
        let client = AliExpressClient::new();
        let result = client
            .search_products("bluetooth earphone", 1, 5, "ja_JP", "JPY", "JP", "")
            .await;
        assert_eq!(result["provider"], "aliexpress");
        assert_eq!(result["ok"], true, "search failed: {}", result);
        assert_eq!(result["query"], "bluetooth earphone");
        let items = result["items"].as_array().expect("items must be array");
        assert!(
            !items.is_empty(),
            "search returned 0 items — API may be broken"
        );
        let count = result["count"].as_i64().unwrap();
        assert_eq!(count as usize, items.len(), "count must match items.len()");
        for item in items {
            let pid = item["product_id"].as_str().unwrap();
            assert!(pid.len() >= 10, "product_id too short: {}", pid);
            let url = item["url"].as_str().unwrap();
            assert!(
                url.starts_with("https://") && url.contains("aliexpress.com/item/"),
                "bad url: {}",
                url
            );
            assert!(url.contains(pid), "url must contain product_id");
            assert_eq!(item["source"], "direct_mtop_search");
        }
    }

    #[tokio::test]
    #[ignore = "live network test (AE-RUST-029)"]
    async fn test_live_search_multiword() {
        let client = AliExpressClient::new();
        let result = client
            .search_products("USB C cable fast charging", 1, 3, "ja_JP", "JPY", "JP", "")
            .await;
        assert_eq!(result["ok"], true, "multi-word search failed: {}", result);
        let items = result["items"].as_array().unwrap();
        assert!(!items.is_empty(), "multi-word search returned 0 items");
    }

    #[tokio::test]
    #[ignore = "live network test (AE-RUST-029)"]
    async fn test_live_search_japanese() {
        let client = AliExpressClient::new();
        let result = client
            .search_products("ワイヤレスイヤホン", 1, 3, "ja_JP", "JPY", "JP", "")
            .await;
        assert_eq!(result["ok"], true, "Japanese search failed: {}", result);
        let items = result["items"].as_array().unwrap();
        assert!(!items.is_empty(), "Japanese search returned 0 items");
    }

    #[tokio::test]
    #[ignore = "live network test (AE-RUST-029)"]
    async fn test_live_search_then_get_product() {
        let client = AliExpressClient::new();
        let search = client
            .search_products("USB cable", 1, 3, "en_US", "USD", "US", "")
            .await;
        assert_eq!(search["ok"], true, "search failed: {}", search);
        let items = search["items"].as_array().unwrap();
        assert!(!items.is_empty(), "search returned 0 items");
        let pid = items[0]["product_id"].as_str().unwrap();
        let detail = client.get_product(pid, "en_US", "USD", "US").await;
        assert_eq!(detail["provider"], "aliexpress");
        assert_eq!(
            detail["ok"], true,
            "get_product FAILED for {}: {}",
            pid, detail
        );
        assert_eq!(detail["product_id"].as_str().unwrap(), pid);
        let url = detail["url"].as_str().unwrap();
        assert!(url.contains(pid), "detail url must contain product_id");
    }

    #[tokio::test]
    #[ignore = "live network test (AE-RUST-029)"]
    async fn test_live_search_empty_query() {
        let client = AliExpressClient::new();
        let result = client
            .search_products("", 1, 5, "ja_JP", "JPY", "JP", "")
            .await;
        assert_eq!(result["ok"], false);
        assert!(result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("empty"));
    }

    #[tokio::test]
    #[ignore = "live network test (AE-RUST-029)"]
    async fn test_live_get_product_invalid() {
        let client = AliExpressClient::new();
        let result = client
            .get_product("not-a-product", "ja_JP", "JPY", "JP")
            .await;
        assert_eq!(result["ok"], false);
        assert!(!result["error"]["message"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore = "live network test (AE-RUST-029)"]
    async fn test_live_suggest() {
        let client = AliExpressClient::new();
        let result = client.suggest_keywords("bluetooth", "ja_JP", 5).await;
        assert_eq!(result["provider"], "aliexpress");
        assert_eq!(result["ok"], true, "suggest failed: {}", result);
        let suggestions = result["suggestions"]
            .as_array()
            .expect("suggestions must be array");
        assert!(!suggestions.is_empty(), "suggest returned 0 suggestions");
        for s in suggestions {
            assert!(
                s.is_string() || s.is_object(),
                "suggestion must be string or object: {:?}",
                s
            );
        }
    }

    #[tokio::test]
    #[ignore = "live network test (AE-RUST-029)"]
    async fn test_live_get_reviews() {
        let client = AliExpressClient::new();
        let search = client
            .search_products("bluetooth earbuds", 1, 1, "en_US", "USD", "US", "")
            .await;
        assert_eq!(search["ok"], true, "search failed: {}", search);
        let pid = search["items"][0]["product_id"].as_str().unwrap();
        let result = client.get_reviews(pid, 1, 5, "en_US").await;
        assert_eq!(result["provider"], "aliexpress");
        assert_eq!(
            result["ok"], true,
            "get_reviews FAILED for {}: {}",
            pid, result
        );
        let reviews = result["reviews"].as_array().expect("reviews must be array");
        if !reviews.is_empty() {
            let first = &reviews[0];
            assert!(
                first.get("feedback").is_some(),
                "review must have feedback field"
            );
            assert!(
                first.get("buyer_country").is_some(),
                "review must have buyer_country field"
            );
        }
    }
}
