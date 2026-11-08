//! Offline tests for the unified library API surface: categories,
//! engine registries, the unified item type, and client
//! construction. No network access happens here; live behaviour is
//! covered by the server e2e tests and the harness example.

use zero_search::{
    ArticleOutcome, ArticleProvider, Category, CategoryOutcome, Client, EngineOutcome,
    MediaOutcome, MediaProvider, SearchItem, ShopOutcome, ShopProvider, WebEngine,
};

#[test]
fn categories_cover_nineteen_engines() {
    assert_eq!(Category::Web.id(), "web");
    assert_eq!(Category::Shopping.id(), "shopping");
    assert_eq!(Category::Media.id(), "media");
    assert_eq!(Category::Articles.id(), "articles");
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
            "rakuma",
            "rakuten",
            "qoo10",
            "yahoo_shopping",
            "amazon"
        ]
    );
    assert_eq!(Category::Media.engine_ids(), &["youtube", "bilibili"]);
    assert_eq!(Category::Articles.engine_ids(), &["csdn"]);
    assert_eq!(Category::all().len(), 4);
    let total: usize = Category::all().iter().map(|c| c.engine_ids().len()).sum();
    assert_eq!(total, 19);
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
    assert_eq!(shop.len(), 10);
    let mut sorted = shop.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 10, "shop provider ids must be unique");
    assert_eq!(shop, Category::Shopping.engine_ids());

    let media: Vec<&str> = MediaProvider::all().iter().map(|p| p.id()).collect();
    assert_eq!(media.len(), 2);
    let mut sorted = media.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 2, "media provider ids must be unique");
    assert_eq!(media, Category::Media.engine_ids());

    let articles: Vec<&str> = ArticleProvider::all().iter().map(|p| p.id()).collect();
    assert_eq!(articles.len(), 1);
    assert_eq!(articles, Category::Articles.engine_ids());
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

    let media = CategoryOutcome::Media(vec![MediaOutcome {
        provider: MediaProvider::Bilibili,
        ok: true,
        result: Some(serde_json::json!({"ok": true})),
        error: None,
    }]);
    let value = serde_json::to_value(&media).unwrap();
    assert_eq!(value[0]["provider"], serde_json::json!("bilibili"));

    let articles = CategoryOutcome::Articles(vec![ArticleOutcome {
        provider: ArticleProvider::Csdn,
        ok: true,
        result: Some(serde_json::json!({"ok": true})),
        error: None,
    }]);
    let value = serde_json::to_value(&articles).unwrap();
    assert_eq!(value[0]["provider"], serde_json::json!("csdn"));
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
    let _ = client.rakuten();
    let _ = client.qoo10();
    let _ = client.yahoo_shopping();
    let _ = client.amazon();
    let _ = client.youtube();
    let _ = client.bilibili();
    let _ = client.csdn();
    // And through the providers module under their original names.
    let _ = zero_search::providers::bing_search::PROVIDER;
    let _ = zero_search::providers::naver_search::PROVIDER;
    let _ = zero_search::providers::yahoo_search::PROVIDER;
}
