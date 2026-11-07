//! Offline tests for the unified library API surface: categories,
//! engine registries, the unified item type, and client
//! construction. No network access happens here; live behaviour is
//! covered by the server e2e tests and the harness example.

use zero_search::{
    Category, CategoryOutcome, Client, EngineOutcome, SearchItem, ShopOutcome, ShopProvider,
    WebEngine,
};

#[test]
fn categories_cover_twelve_engines() {
    assert_eq!(Category::Web.id(), "web");
    assert_eq!(Category::Shopping.id(), "shopping");
    assert_eq!(
        Category::Web.engine_ids(),
        &["baidu", "yahoo_jp", "bing", "naver", "yahoo_us", "yandex"]
    );
    assert_eq!(
        Category::Shopping.engine_ids(),
        &[
            "aliexpress",
            "goofish",
            "mercari",
            "yahoo_auctions",
            "yahoo_fleamarket",
            "rakuma"
        ]
    );
    let total: usize = Category::all().iter().map(|c| c.engine_ids().len()).sum();
    assert_eq!(total, 12);
}

#[test]
fn engine_registries_are_complete_and_unique() {
    let web: Vec<&str> = WebEngine::all().iter().map(|e| e.id()).collect();
    assert_eq!(web.len(), 6);
    let mut sorted = web.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 6, "web engine ids must be unique");
    assert_eq!(web, Category::Web.engine_ids());

    let shop: Vec<&str> = ShopProvider::all().iter().map(|p| p.id()).collect();
    assert_eq!(shop.len(), 6);
    let mut sorted = shop.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 6, "shop provider ids must be unique");
    assert_eq!(shop, Category::Shopping.engine_ids());
}

#[test]
fn search_item_roundtrips_through_json() {
    let item = SearchItem {
        engine: WebEngine::Bing,
        title: "Example".to_string(),
        url: "https://example.com/".to_string(),
        snippet: Some("snippet".to_string()),
    };
    let text = serde_json::to_string(&item).unwrap();
    let back: SearchItem = serde_json::from_str(&text).unwrap();
    assert_eq!(item, back);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["engine"], serde_json::json!("bing"));
}

#[test]
fn outcome_types_serialise() {
    let web = CategoryOutcome::Web(vec![EngineOutcome {
        engine: WebEngine::Naver,
        ok: false,
        items: vec![],
        error: Some("boom".to_string()),
    }]);
    let value = serde_json::to_value(&web).unwrap();
    assert_eq!(value[0]["engine"], serde_json::json!("naver"));
    assert_eq!(value[0]["ok"], serde_json::json!(false));

    let shop = CategoryOutcome::Shopping(vec![ShopOutcome {
        provider: ShopProvider::Mercari,
        ok: true,
        result: Some(serde_json::json!({"ok": true})),
        error: None,
    }]);
    let value = serde_json::to_value(&shop).unwrap();
    assert_eq!(value[0]["provider"], serde_json::json!("mercari"));
}

#[test]
fn client_constructs_without_configuration() {
    // Construction must not read config files, environment
    // variables or the network — it just builds the raw clients.
    let client = Client::new().expect("zero-config client construction");
    // The raw providers stay reachable through the facade.
    let _ = client.baidu();
    let _ = client.yahoo();
    let _ = client.bing();
    let _ = client.naver();
    let _ = client.yahoo_us();
    let _ = client.yandex();
    let _ = client.aliexpress();
    let _ = client.goofish();
    let _ = client.mercari();
    let _ = client.rakuma();
    let _ = client.yahoo_auctions();
    let _ = client.yahoo_flea();
    // And through the providers module under their original names.
    let _ = zero_search::providers::bing_search::PROVIDER;
    let _ = zero_search::providers::naver_search::PROVIDER;
    let _ = zero_search::providers::yahoo_search::PROVIDER;
}
