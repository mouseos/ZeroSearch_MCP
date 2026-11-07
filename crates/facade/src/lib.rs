//! `zero_search` — the ZeroSearch library.
//!
//! One zero-config entry point to twelve search providers: six web
//! engines (Baidu, Yahoo! JAPAN, Bing, Naver, Yahoo US, Yandex) and
//! six shopping providers (AliExpress, Goofish, Mercari, Yahoo!
//! Auctions, Yahoo! Flea Market, Rakuma). No API key, no environment
//! variables, no configuration files — construct a [`Client`] (or
//! call the free functions) and search.
//!
//! The crate works on two levels:
//!
//! - The **unified API** in this module: [`Client::search`] /
//!   [`search`] for one web engine, [`Client::search_all`] /
//!   [`search_all`] for every web engine at once (per-engine
//!   success/failure kept separate), [`Client::search_category`] for
//!   a whole [`Category`], and [`Client::search_products`] /
//!   [`search_products`] for shopping. Web results are normalised
//!   into [`SearchItem`].
//! - The **raw providers**, re-exported under [`providers`] (and at
//!   the crate root under their original crate names), so every
//!   provider's own client, request types and result types stay
//!   reachable exactly as in the standalone crates.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Raw provider re-exports (original package / lib names preserved)
// ---------------------------------------------------------------------------

pub use baidu_search;
pub use bing_search;
pub use naver_search;
pub use yahoo_search;
pub use yahoo_us_search;
pub use yandex_search;

/// All twelve provider crates under one module, by role.
pub mod providers {
    pub use baidu_search;
    pub use bing_search;
    pub use naver_search;
    pub use yahoo_search;
    pub use yahoo_us_search;
    pub use yandex_search;

    pub use shopping_mcp_aliexpress as aliexpress;
    pub use shopping_mcp_goofish as goofish;
    pub use shopping_mcp_mercari as mercari;
    pub use shopping_mcp_rakuma as rakuma;
    pub use shopping_mcp_yahoo_auctions as yahoo_auctions;
    pub use shopping_mcp_yahoo_flea as yahoo_flea;
}

// ---------------------------------------------------------------------------
// Categories and engines
// ---------------------------------------------------------------------------

/// The two engine categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// General web search engines.
    Web,
    /// Shopping / marketplace search providers.
    Shopping,
}

impl Category {
    /// Stable category id (`"web"` / `"shopping"`).
    pub fn id(&self) -> &'static str {
        match self {
            Category::Web => "web",
            Category::Shopping => "shopping",
        }
    }

    /// Ids of the engines that belong to this category.
    pub fn engine_ids(&self) -> &'static [&'static str] {
        match self {
            Category::Web => &["baidu", "yahoo_jp", "bing", "naver", "yahoo_us", "yandex"],
            Category::Shopping => &[
                "aliexpress",
                "goofish",
                "mercari",
                "yahoo_auctions",
                "yahoo_fleamarket",
                "rakuma",
            ],
        }
    }

    /// Both categories.
    pub fn all() -> &'static [Category] {
        &[Category::Web, Category::Shopping]
    }
}

/// A web search engine (category [`Category::Web`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebEngine {
    Baidu,
    YahooJapan,
    Bing,
    Naver,
    YahooUs,
    Yandex,
}

impl WebEngine {
    /// Stable engine id, as used in [`SearchItem::engine`] and the
    /// MCP `zero_list_categories` tool.
    pub fn id(&self) -> &'static str {
        match self {
            WebEngine::Baidu => "baidu",
            WebEngine::YahooJapan => "yahoo_jp",
            WebEngine::Bing => "bing",
            WebEngine::Naver => "naver",
            WebEngine::YahooUs => "yahoo_us",
            WebEngine::Yandex => "yandex",
        }
    }

    /// Every web engine, in canonical order.
    pub fn all() -> &'static [WebEngine] {
        &[
            WebEngine::Baidu,
            WebEngine::YahooJapan,
            WebEngine::Bing,
            WebEngine::Naver,
            WebEngine::YahooUs,
            WebEngine::Yandex,
        ]
    }
}

/// A shopping search provider (category [`Category::Shopping`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShopProvider {
    AliExpress,
    Goofish,
    Mercari,
    YahooAuctions,
    YahooFleaMarket,
    Rakuma,
}

impl ShopProvider {
    /// Stable provider id.
    pub fn id(&self) -> &'static str {
        match self {
            ShopProvider::AliExpress => "aliexpress",
            ShopProvider::Goofish => "goofish",
            ShopProvider::Mercari => "mercari",
            ShopProvider::YahooAuctions => "yahoo_auctions",
            ShopProvider::YahooFleaMarket => "yahoo_fleamarket",
            ShopProvider::Rakuma => "rakuma",
        }
    }

    /// Every shopping provider, in canonical order.
    pub fn all() -> &'static [ShopProvider] {
        &[
            ShopProvider::AliExpress,
            ShopProvider::Goofish,
            ShopProvider::Mercari,
            ShopProvider::YahooAuctions,
            ShopProvider::YahooFleaMarket,
            ShopProvider::Rakuma,
        ]
    }
}

// ---------------------------------------------------------------------------
// Unified result types
// ---------------------------------------------------------------------------

/// One web search result, normalised across engines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchItem {
    /// The engine that produced this result.
    pub engine: WebEngine,
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// The outcome of one engine inside a multi-engine search. Failures
/// are kept per engine: one engine failing never fails the others.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineOutcome {
    pub engine: WebEngine,
    pub ok: bool,
    #[serde(default)]
    pub items: Vec<SearchItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The outcome of one provider inside a multi-provider shopping
/// search. `result` is the provider's own JSON payload (the exact
/// value the corresponding MCP tool returns).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShopOutcome {
    pub provider: ShopProvider,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The outcome of a category-wide search ([`Client::search_category`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CategoryOutcome {
    Web(Vec<EngineOutcome>),
    Shopping(Vec<ShopOutcome>),
}

// ---------------------------------------------------------------------------
// The unified client
// ---------------------------------------------------------------------------

/// A zero-config client holding one raw client per provider.
///
/// Construction performs no network access. All methods are async
/// and safe to call concurrently; multi-engine searches run their
/// engines sequentially (several providers rate-limit bursts).
pub struct Client {
    baidu: baidu_search::BaiduClient<baidu_search::ReqwestFetcher>,
    yahoo: yahoo_search::YahooSearchClient,
    bing: bing_search::BingSearchClient,
    naver: naver_search::NaverSearchClient,
    yahoo_us: yahoo_us_search::YahooUsSearchClient,
    yandex: yandex_search::YandexSearchClient,
    aliexpress: shopping_mcp_aliexpress::AliExpressClient,
    goofish: shopping_mcp_goofish::GoofishClient,
    mercari: shopping_mcp_mercari::MercariClient,
    rakuma: shopping_mcp_rakuma::RakumaClient,
    yauction: shopping_mcp_yahoo_auctions::YahooAuctionsClient,
    yflea: shopping_mcp_yahoo_flea::YahooFleaClient,
}

impl Client {
    /// Build a client. No configuration is read from anywhere; the
    /// only fallible step is the Bing client's internal setup.
    pub fn new() -> Result<Self> {
        Ok(Self {
            baidu: baidu_search::BaiduClient::new_live(),
            yahoo: yahoo_search::YahooSearchClient::new(),
            bing: bing_search::BingSearchClient::new()?,
            naver: naver_search::NaverSearchClient::new(),
            yahoo_us: yahoo_us_search::YahooUsSearchClient::new(),
            yandex: yandex_search::YandexSearchClient::new(),
            aliexpress: shopping_mcp_aliexpress::AliExpressClient::new(),
            goofish: shopping_mcp_goofish::GoofishClient::new(),
            mercari: shopping_mcp_mercari::MercariClient::new(),
            rakuma: shopping_mcp_rakuma::RakumaClient::new(),
            yauction: shopping_mcp_yahoo_auctions::YahooAuctionsClient::new(),
            yflea: shopping_mcp_yahoo_flea::YahooFleaClient::new(),
        })
    }

    /// Access the raw provider clients (escape hatch to each
    /// provider's full API surface).
    pub fn baidu(&self) -> &baidu_search::BaiduClient<baidu_search::ReqwestFetcher> {
        &self.baidu
    }
    pub fn yahoo(&self) -> &yahoo_search::YahooSearchClient {
        &self.yahoo
    }
    pub fn bing(&self) -> &bing_search::BingSearchClient {
        &self.bing
    }
    pub fn naver(&self) -> &naver_search::NaverSearchClient {
        &self.naver
    }
    pub fn yahoo_us(&self) -> &yahoo_us_search::YahooUsSearchClient {
        &self.yahoo_us
    }
    pub fn yandex(&self) -> &yandex_search::YandexSearchClient {
        &self.yandex
    }
    pub fn aliexpress(&self) -> &shopping_mcp_aliexpress::AliExpressClient {
        &self.aliexpress
    }
    pub fn goofish(&self) -> &shopping_mcp_goofish::GoofishClient {
        &self.goofish
    }
    pub fn mercari(&self) -> &shopping_mcp_mercari::MercariClient {
        &self.mercari
    }
    pub fn rakuma(&self) -> &shopping_mcp_rakuma::RakumaClient {
        &self.rakuma
    }
    pub fn yahoo_auctions(&self) -> &shopping_mcp_yahoo_auctions::YahooAuctionsClient {
        &self.yauction
    }
    pub fn yahoo_flea(&self) -> &shopping_mcp_yahoo_flea::YahooFleaClient {
        &self.yflea
    }

    /// Search one web engine (first page, provider-default size).
    pub async fn search(&self, engine: WebEngine, query: &str) -> Result<Vec<SearchItem>> {
        self.search_paged(engine, query, 1, 10).await
    }

    /// Search one web engine with paging.
    ///
    /// `page` is 1-based. Baidu's client aggregates from the first
    /// page and has no offset API, so for Baidu this returns the
    /// first aggregated page regardless of `page`/`limit`.
    pub async fn search_paged(
        &self,
        engine: WebEngine,
        query: &str,
        page: i64,
        limit: i64,
    ) -> Result<Vec<SearchItem>> {
        let page = page.max(1);
        let limit = limit.max(1);
        match engine {
            WebEngine::Baidu => {
                let agg = self.baidu.search(query, 1).await?;
                Ok(agg
                    .results
                    .iter()
                    .filter_map(|r| {
                        let title = r.get("title").and_then(Value::as_str)?.to_string();
                        let url = r.get("url").and_then(Value::as_str)?.to_string();
                        Some(SearchItem {
                            engine,
                            title,
                            url,
                            snippet: r.get("snippet").and_then(Value::as_str).map(str::to_string),
                        })
                    })
                    .collect())
            }
            WebEngine::YahooJapan => {
                let res = self.yahoo.web_search(query, page, limit, "", "").await?;
                Ok(res
                    .items
                    .into_iter()
                    .map(|it| SearchItem {
                        engine,
                        title: it.title,
                        url: it.url,
                        snippet: it.description,
                    })
                    .collect())
            }
            WebEngine::Bing => {
                let res = self.bing.web_search(query, page, limit, "", "").await?;
                Ok(res
                    .items
                    .into_iter()
                    .map(|it| SearchItem {
                        engine,
                        title: it.title,
                        url: it.url,
                        snippet: it.snippet,
                    })
                    .collect())
            }
            WebEngine::Naver => {
                let res = self.naver.web_search(query, page, limit).await?;
                Ok(res
                    .items
                    .into_iter()
                    .map(|it| SearchItem {
                        engine,
                        title: it.title,
                        url: it.url,
                        snippet: it.snippet,
                    })
                    .collect())
            }
            WebEngine::YahooUs => {
                let res = self.yahoo_us.web_search(query, page, limit).await?;
                Ok(res
                    .items
                    .into_iter()
                    .map(|it| SearchItem {
                        engine,
                        title: it.title,
                        url: it.url,
                        snippet: it.description,
                    })
                    .collect())
            }
            WebEngine::Yandex => {
                let res = self.yandex.web_search(query, page, limit).await?;
                Ok(res
                    .items
                    .into_iter()
                    .map(|it| SearchItem {
                        engine,
                        title: it.title,
                        url: it.url,
                        snippet: it.description,
                    })
                    .collect())
            }
        }
    }

    /// Search every web engine with the same query. Engines run
    /// sequentially; each engine's success or failure is reported
    /// separately in its [`EngineOutcome`].
    pub async fn search_all(&self, query: &str) -> Vec<EngineOutcome> {
        let mut out = Vec::with_capacity(WebEngine::all().len());
        for &engine in WebEngine::all() {
            out.push(self.search_outcome(engine, query).await);
        }
        out
    }

    async fn search_outcome(&self, engine: WebEngine, query: &str) -> EngineOutcome {
        match self.search(engine, query).await {
            Ok(items) => EngineOutcome {
                engine,
                ok: true,
                items,
                error: None,
            },
            Err(e) => EngineOutcome {
                engine,
                ok: false,
                items: Vec::new(),
                error: Some(format!("{e:#}")),
            },
        }
    }

    /// Search one shopping provider. The returned JSON is the
    /// provider's own payload — what the matching MCP tool
    /// (`<provider>_search_products`) returns with default options,
    /// except that `limit` is passed through as given. Defaults
    /// mirror the MCP tools (e.g. AliExpress `ja_JP`/`JPY`/
    /// ship-to `JP`).
    pub async fn search_products(
        &self,
        provider: ShopProvider,
        query: &str,
        limit: i64,
    ) -> Result<Value> {
        let limit = limit.max(1);
        // The shopping providers take i32 limits (as do their MCP
        // tools); the facade API uses i64, so clamp on conversion.
        let limit_i32 = i32::try_from(limit).unwrap_or(i32::MAX);
        match provider {
            ShopProvider::AliExpress => Ok(self
                .aliexpress
                .search_products(query, 1, limit_i32, "ja_JP", "JPY", "JP", "")
                .await),
            ShopProvider::Goofish => Ok(self
                .goofish
                .search_products(query, 1, limit, None, None, None)
                .await),
            ShopProvider::Mercari => {
                let params = shopping_mcp_mercari::SearchParams {
                    query: query.to_string(),
                    limit: limit_i32,
                    page_token: String::new(),
                    sort: "default".to_string(),
                    status: String::new(),
                    exclude_keyword: String::new(),
                    category_ids: String::new(),
                    brand_ids: String::new(),
                    size_ids: String::new(),
                    seller_ids: String::new(),
                    price_min: None,
                    price_max: None,
                    item_condition_ids: String::new(),
                    shipping_payer_ids: String::new(),
                    color_ids: String::new(),
                    item_types: String::new(),
                    exclude_auction: false,
                };
                Ok(self.mercari.search_products(params).await?)
            }
            ShopProvider::YahooAuctions => {
                let params = shopping_mcp_yahoo_auctions::SearchParams {
                    query: query.to_string(),
                    limit: limit_i32,
                    page: 1,
                    sort: String::new(),
                    min_price: None,
                    max_price: None,
                    category_id: String::new(),
                    status: "open".to_string(),
                    keywords_and: String::new(),
                    keywords_not: String::new(),
                    seller_id: String::new(),
                    buynow_only: false,
                    free_shipping: false,
                    condition: String::new(),
                };
                let result = self.yauction.search(params).await?;
                Ok(serde_json::to_value(result)?)
            }
            ShopProvider::YahooFleaMarket => {
                let params = shopping_mcp_yahoo_flea::SearchParams {
                    query: query.to_string(),
                    limit: limit_i32,
                    page: 1,
                    sort: String::new(),
                    min_price: None,
                    max_price: None,
                    category_id: String::new(),
                };
                let result = self.yflea.search(params).await?;
                Ok(serde_json::to_value(result)?)
            }
            ShopProvider::Rakuma => {
                let params = shopping_mcp_rakuma::SearchParams {
                    query: query.to_string(),
                    limit: limit_i32,
                    page: 1,
                    sort: String::new(),
                    min_price: None,
                    max_price: None,
                    category_id: String::new(),
                    brand_id: String::new(),
                };
                let result = self.rakuma.search(params).await?;
                Ok(serde_json::to_value(result)?)
            }
        }
    }

    /// Search every shopping provider with the same query. Each
    /// provider's success or failure is reported separately.
    /// Providers run sequentially and each performs a live request,
    /// so callers should treat this as six searches, not one.
    pub async fn search_products_all(&self, query: &str, limit: i64) -> Vec<ShopOutcome> {
        let mut out = Vec::with_capacity(ShopProvider::all().len());
        for &provider in ShopProvider::all() {
            let outcome = match self.search_products(provider, query, limit).await {
                Ok(result) => ShopOutcome {
                    provider,
                    ok: result.get("ok").and_then(Value::as_bool).unwrap_or(true),
                    result: Some(result),
                    error: None,
                },
                Err(e) => ShopOutcome {
                    provider,
                    ok: false,
                    result: None,
                    error: Some(format!("{e:#}")),
                },
            };
            out.push(outcome);
        }
        out
    }

    /// Search a whole category with one query: every web engine for
    /// [`Category::Web`], every shopping provider for
    /// [`Category::Shopping`] (with the shopping default limit of
    /// 10 per provider).
    pub async fn search_category(&self, category: Category, query: &str) -> CategoryOutcome {
        match category {
            Category::Web => CategoryOutcome::Web(self.search_all(query).await),
            Category::Shopping => {
                CategoryOutcome::Shopping(self.search_products_all(query, 10).await)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Free functions (build a Client per call — the zero-config shortcut)
// ---------------------------------------------------------------------------

/// Search one web engine with a throwaway client. Harnesses that
/// search repeatedly should keep a [`Client`] instead.
pub async fn search(engine: WebEngine, query: &str) -> Result<Vec<SearchItem>> {
    Client::new()?.search(engine, query).await
}

/// Search every web engine with a throwaway client.
pub async fn search_all(query: &str) -> Result<Vec<EngineOutcome>> {
    Ok(Client::new()?.search_all(query).await)
}

/// Search one shopping provider with a throwaway client.
pub async fn search_products(provider: ShopProvider, query: &str, limit: i64) -> Result<Value> {
    Client::new()?.search_products(provider, query, limit).await
}
