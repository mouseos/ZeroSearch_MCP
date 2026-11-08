# ZeroSearch MCP

検索エンジン6つとショッピング10サイト、メディア2つと記事プラットフォーム1つを、1つのMCPサーバーに統合したものです。

**設定はMCP登録だけです。APIキーは不要。環境変数も不要。**
サーバーのパスをMCP設定に書けば、そのまま使えます。

Web検索は Baidu、Yahoo! JAPAN、Bing、Naver、Yahoo (US)、Yandex。
ショッピング検索は AliExpress、Goofish（闲鱼）、Mercari、ヤフオク、Yahoo!フリマ、楽天ラクマ、楽天市場、Qoo10、Yahoo!ショッピング、Amazon.co.jp です。
メディアは YouTube、Bilibili。記事プラットフォームは CSDN です。
どのプロバイダも匿名・ログイン不要で動きます。

Rustライブラリとしても使えます。自作ハーネスのデフォルト検索ツールとして組み込むための統一APIを用意しています。

## カテゴリとエンジン

エンジンは4つのカテゴリに分かれます。MCPでは `zero_list_categories` ツールがこの対応を返します。

| カテゴリ | エンジン |
|---|---|
| `web`（Web検索） | `baidu`、`yahoo_jp`、`bing`、`naver`、`yahoo_us`、`yandex` |
| `shopping`（ショッピング検索） | `aliexpress`、`goofish`、`mercari`、`yahoo_auctions`、`yahoo_fleamarket`、`rakuma`、`rakuten`、`qoo10`、`yahoo_shopping`、`amazon` |
| `media`（メディア） | `youtube`、`bilibili` |
| `articles`（記事） | `csdn` |

## ツール一覧（全90件）

ツール名・引数・返却形式は、統合前の各単体サーバーと完全に同一です。

### web

| エンジン | ツール |
|---|---|
| Baidu（5件） | `baidu_search`、`baidu_search_page`、`baidu_parse_search_results`、`baidu_build_search_url`、`baidu_list_sources` |
| Yahoo! JAPAN（8件） | `yahoo_web_search`、`yahoo_image_search`、`yahoo_video_search`、`yahoo_news_search`、`yahoo_realtime_search`、`yahoo_chiebukuro_search`、`yahoo_suggest_keywords`、`yahoo_list_verticals` |
| Bing（7件） | `bing_web_search`、`bing_image_search`、`bing_video_search`、`bing_news_search`、`bing_suggest`、`bing_build_search_url`、`bing_list_verticals` |
| Naver（9件） | `naver_search`、`naver_web_search`、`naver_image_search`、`naver_news_search`、`naver_video_search`、`naver_blog_search`、`naver_suggest`、`naver_build_search_url`、`naver_list_verticals` |
| Yahoo (US)（7件） | `yahoo_us_web_search`、`yahoo_us_video_search`、`yahoo_us_image_search`、`yahoo_us_news_search`、`yahoo_us_suggest`、`yahoo_us_build_search_url`、`yahoo_us_list_verticals` |
| Yandex（6件） | `yandex_web_search`、`yandex_image_search`、`yandex_video_search`、`yandex_suggest`、`yandex_build_search_url`、`yandex_list_verticals` |

### shopping

| プロバイダ | ツール |
|---|---|
| AliExpress（5件） | `aliexpress_search_products`、`aliexpress_get_product`、`aliexpress_suggest_keywords`、`aliexpress_get_reviews`、`aliexpress_search_store_products` |
| Goofish（6件） | `goofish_search_products`、`goofish_get_product`、`goofish_suggest_keywords`、`goofish_get_comments`、`goofish_get_recommendations`、`goofish_home_feed` |
| Mercari（7件） | `mercari_search_products`、`mercari_get_product`、`mercari_search_seller_products`、`mercari_suggest_keywords`、`mercari_search_brands`、`mercari_search_categories`、`mercari_get_reviews` |
| ヤフオク（4件） | `yauction_search_products`、`yauction_get_product`、`yauction_get_reviews`、`yauction_suggest_keywords` |
| Yahoo!フリマ（3件） | `yflea_search_products`、`yflea_get_product`、`yflea_get_reviews` |
| 楽天ラクマ（3件） | `rakuma_search_products`、`rakuma_get_product`、`rakuma_get_reviews` |
| 楽天市場（3件） | `rakuten_search_products`、`rakuten_get_product`、`rakuten_get_reviews` |
| Qoo10（3件） | `qoo10_search_products`、`qoo10_get_product`、`qoo10_suggest_keywords` |
| Yahoo!ショッピング（3件） | `yshopping_search_products`、`yshopping_get_product`、`yshopping_get_reviews` |
| Amazon.co.jp（2件） | `amazon_search_products`、`amazon_get_product` |
| 共通（1件） | `shopping_list_sources` |

### media

| プロバイダ | ツール |
|---|---|
| YouTube（4件） | `youtube_search_videos`、`youtube_get_video`、`youtube_get_related`、`youtube_get_comments` |
| Bilibili（1件） | `bilibili_search_videos` |

### articles

| プロバイダ | ツール |
|---|---|
| CSDN（2件） | `csdn_search_articles`、`csdn_get_article` |

### ZeroSearch共通

| ツール | 内容 |
|---|---|
| `zero_list_categories` | カテゴリ、所属エンジン、対応ツールの一覧を返す |

## プロバイダの補足（v0.2.0で追加した4件）

- **楽天市場**: アプリのゲスト用BFFをそのまま使います。認証は不要です。詳細APIは部分成功で HTTP 207 を返すことがありますが、これは正常な形です。成否は中核データの有無で判定します。
- **Qoo10**: アプリのモバイルAPIを使います。並び替えと絞り込みは、アプリの検索条件の全項目に対応します。商品詳細はネイティブAPIがないため、アプリのWebView表示を再現して取得します。ユーザーエージェントに含まれる端末由来のトークンは端末の外では再現できないため、形式を保った代替値を送ります。この形で実通信の受理を確認済みです。なお、このサービスは日本側からの接続でないと応答しません。
- **Yahoo!ショッピング**: 検索と詳細はアプリのネイティブAPIを使います。appidはアプリ内蔵の定数で、認証は不要です。レビュー一覧はネイティブAPIにないため、アプリ自身がWebViewで開くレビューページ（shopping.yahoo.co.jp/review/item/list）から取得します（`yshopping_get_reviews`）。21件目以降と並び替え（おすすめ順・評価の高い順・いいねの多い順・新着順・ピックアップ）は、ページ自身が続きを読むときに使うAPIを、ページが発行したトークンとともに利用し、`offset` と `sort` で指定できます。星評価と項目別評価での絞り込み（`rating`、`sub_review_id`）も同じAPIで指定できます。サジェストはパラメータが確定していないため、用意していません。
- **Amazon.co.jp**: アプリに検索用のネイティブAPIはありません。アプリのWebView表示を通常のHTTP取得で再現します。使うのはアプリ形式のユーザーエージェントと、表示前にアプリが同期するCookieです。ブラウザエンジンは使いません。このサービスも日本側からの接続でないと応答しません。

## プロバイダの補足（v0.3.0で追加した3件）

- **YouTube**: Androidアプリ自身の取得方式（InnerTube）を再現します。検索は並び替え（関連度・評価・アップロード日・再生数）と絞り込み（アップロード時期・長さ）に対応し、ページ送りは継続トークンでたどります。動画詳細・関連動画・コメント（ページ送り対応）も取得できます。ゲストの識別子（visitorData）はサーバーが払い出すものを保持して使う形で、ログインは不要です。なお、Androidクライアントの詳細応答には公開日といいね数が含まれないため、その2項目は返せません（nullになります）。
- **Bilibili**: AndroidアプリのgRPC検索（SearchAll）を再現します。並び替えコード、地域（tid）・長さの絞り込み形式、公開日時の範囲指定、ページ送り（`next`）に対応します。用意するのは検索だけです。アプリの動画詳細（View）と関連（RelatesFeed）は、端末文脈なしの要求をサーバーが業務エラーで拒否します。その文脈を運ぶメタデータは、サービスの公開エッジが制御バイトを含むヘッダ値を拒否するため通すことができず、詳細と関連はツールとして用意していません。
- **CSDN**: AndroidアプリのゲートウェイAPIを、アプリ内蔵の鍵によるX-Ca署名つきで再現します。記事検索（ページ指定対応）と記事詳細（本文HTMLとテキストまで）がログインなしで取得できます。

## インストール

Releaseから自分のOSのZIPをダウンロードして展開します。

1. `zero-search-mcp-launcher` を起動し、使うエージェントの「Install」を押す
2. または、ターミナルで `zero-search-mcp setup` を実行し、表示されるJSONをMCP設定に貼る

MCP設定はこれだけです。

```json
{
  "mcpServers": {
    "ZeroSearch_MCP": { "command": "/path/to/zero-search-mcp", "args": [] }
  }
}
```

Goofishも例外ではありません。配布物には署名用の Java ランタイムとハーネスが同梱されています。初回の Goofish 利用時に、サーバーが Goofish のアプリパッケージ（7.28.70、約269MB）をアプリストアの CDN から自動でダウンロードし、SHA-256 で検証して、自分のデータディレクトリに署名ランタイムを組み立てます。端末の識別情報もサーバー自身が生成します。利用者が用意するもの、設定するものはありません。初回だけはダウンロードと署名ランタイムの起動のため、呼び出しに数分から数十分かかることがあります。ディスクは合計で約1GB（同梱ランタイム＋アプリパッケージ＋エミュレータの作業領域）を見込んでください。仕組みの詳細は [runtime/goofish](runtime/goofish/README.md) にあります。

Yahoo (US) の画像検索も同じ考え方です。Yahoo の画像ホストは通常の HTTP クライアントに HTTP 500 を返すため、この検索だけは同梱のヘルパープロセス `zero-search-yahoo-us-image-helper`（サーバーと同じフォルダに置かれます）がブラウザ相当のクライアントでページを取得します。設定は要りません。

## ライブラリとして使う

`zero_search` クレートが統一APIです。各プロバイダのクレートもそのまま re-export しているので、素の型やクライアントにも辿れます。

```toml
[dependencies]
zero-search = { git = "https://github.com/mouseos/ZeroSearch_MCP" }
tokio = { version = "1", features = ["full"] }
```

```rust
use zero_search::{Client, WebEngine, ShopProvider, Category};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = Client::new()?; // 設定は読みません

    // エンジンを指定して検索（結果は共通型 SearchItem）
    let items = client.search(WebEngine::Bing, "rust programming").await?;

    // 全Webエンジンへ一括検索（成否はエンジンごとに分かれて返る）
    let outcomes = client.search_all("東京 天気").await;
    for outcome in &outcomes {
        println!("{}: ok={} items={}", outcome.engine.id(), outcome.ok, outcome.items.len());
    }

    // カテゴリ単位の一括検索
    let shopping = client.search_category(Category::Shopping, "ヘッドホン").await;

    // 商品検索（返却は各プロバイダのJSONそのもの）
    let value = client.search_products(ShopProvider::Mercari, "カメラ", 10).await?;

    Ok(())
}
```

### ハーネスへの組み込み

統一APIをそのままツールとして公開する形が想定形です。動く例を [crates/facade/examples/harness_tool.rs](crates/facade/examples/harness_tool.rs) に置いています。

```sh
cargo run -p zero-search --example harness_tool -- "tokyo weather"
cargo run -p zero-search --example harness_tool -- --shop "headphones"
```

## 旧リポジトリからの移行

このリポジトリは、次の7リポジトリを統合したものです。旧リポジトリはアーカイブされています。

- mouseos/Baidu_MCP
- mouseos/Yahoo-Japan-Search-MCP
- mouseos/Bing_MCP
- mouseos/Naver_MCP
- mouseos/Yahoo_US_MCP
- mouseos/Yandex_MCP
- mouseos/Shopping_MCP

移行手順は、MCP設定のサーバーパスを `zero-search-mcp` に変えるだけです。ツール名と引数は変わらないので、既存のプロンプトやワークフローはそのまま動きます。ライブラリのクレート名（`baidu_search`、`bing_search` など）と型も維持しています。

## 開発

```sh
cargo build --workspace
cargo test --workspace -- --skip e2e_     # オフラインテスト
cargo test -p zero-search-mcp-server -- e2e_ --test-threads=1   # live（直列必須）
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## ライセンス

MIT
