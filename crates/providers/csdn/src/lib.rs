//! CSDN provider reproducing the Android app's own retrieval routes
//! (net.csdn.csdnplus, analyzed build 6.5.7).
//!
//! Search and article detail both go through the app gateway
//! `https://app-gw.csdn.net` with the app's X-Ca HMAC signature
//! (see `sign.rs`):
//!
//! - Search: `POST /silkroad-api/api/v2/assemble/list/pub/channel/app_search_result_v6`
//!   with the `SearchVipRequestEntity` JSON body the app builds for
//!   its "all" tab (`SearchAll` results).
//! - Detail: `GET /blog/phoenix/app/v1/get-article-basic-info?articleId=..&skin=`
//!   (`BlogPhoenixService`), returning the article including its
//!   `contentHtml` body.
//!
//! Both work as a guest: the app sends `user_name: ""` and no login
//! headers while logged out, and so does this client.

pub mod sign;

use anyhow::{anyhow, Context};
use serde_json::{json, Value};

pub const PROVIDER: &str = "csdn";

const SEARCH_PATH: &str = "/silkroad-api/api/v2/assemble/list/pub/channel/app_search_result_v6";
const SEARCH_CHANNEL: &str = "app_search_result_v6";
const DETAIL_PATH: &str = "/blog/phoenix/app/v1/get-article-basic-info";

pub struct CsdnClient {
    http: reqwest::Client,
    /// The app's persisted device identifier (`X-Device-ID`). A fresh
    /// install generates its own; this client generates one UUID per
    /// instance as the stateless equivalent.
    device_id: String,
}

impl Default for CsdnClient {
    fn default() -> Self {
        Self::new()
    }
}

impl CsdnClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!("CSDNApp/{}(Android)", sign::APP_VERSION))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            http,
            device_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    fn apply_headers(
        &self,
        builder: reqwest::RequestBuilder,
        headers: Vec<(String, String)>,
    ) -> reqwest::RequestBuilder {
        let mut builder = builder;
        for (k, v) in headers {
            builder = builder.header(k, v);
        }
        builder
    }

    /// Raw search call against the app gateway; returns the decoded
    /// `ResponseResult` JSON (`{code, message, data, ...}`).
    pub async fn search_raw(
        &self,
        query: &str,
        page: u32,
        limit: u32,
        sort: Option<&str>,
    ) -> anyhow::Result<Value> {
        let page = page.max(1);
        let limit = limit.clamp(1, 50);
        let mut ext = json!({
            "keywords": query,
            "page": page,
            "pageSize": limit,
            "type": "all",
            "block": "all",
            "use_highlight": true,
            "insert_card": "1",
            "completion_down": 1,
            "search_version": "v4",
            "version": 1,
            "user_name": "",
            "imei": "",
            "oaid": ""
        });
        if let Some(sort) = sort {
            if !sort.is_empty() {
                ext["second_rank_name_sort"] = json!(sort);
            }
        }
        let body = json!({
            "channel": SEARCH_CHANNEL,
            "size": limit,
            "ext": ext
        });
        let headers = sign::build_headers("POST", SEARCH_PATH, &[], &self.device_id);
        let url = format!("{}{}", sign::GATEWAY_HOST, SEARCH_PATH);
        let resp = self
            .apply_headers(self.http.post(&url), headers)
            .json(&body)
            .send()
            .await
            .context("csdn search request failed")?;
        let status = resp.status();
        let text = resp.text().await.context("csdn search body read failed")?;
        if !status.is_success() {
            return Err(anyhow!(
                "csdn search returned HTTP {status}: {}",
                trim(&text)
            ));
        }
        serde_json::from_str(&text).context("csdn search response was not JSON")
    }

    /// Raw article-detail call; returns the decoded `ResponseResult`.
    pub async fn article_raw(&self, article_id: &str) -> anyhow::Result<Value> {
        let params = vec![
            ("articleId".to_string(), article_id.to_string()),
            ("skin".to_string(), String::new()),
        ];
        let headers = sign::build_headers("GET", DETAIL_PATH, &params, &self.device_id);
        let url = format!("{}{}", sign::GATEWAY_HOST, DETAIL_PATH);
        let resp = self
            .apply_headers(self.http.get(&url), headers)
            .query(&[("articleId", article_id), ("skin", "")])
            .send()
            .await
            .context("csdn article request failed")?;
        let status = resp.status();
        let text = resp.text().await.context("csdn article body read failed")?;
        if !status.is_success() {
            return Err(anyhow!(
                "csdn article returned HTTP {status}: {}",
                trim(&text)
            ));
        }
        serde_json::from_str(&text).context("csdn article response was not JSON")
    }
}

fn trim(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() > 300 {
        t.chars().take(300).collect()
    } else {
        t.to_string()
    }
}

/// The app treats response codes 200 and 2000 as success
/// (`ResponseResult.CODE_V2_SUCCESS` / `CODE_SUCCESS`).
fn check_code(v: &Value) -> anyhow::Result<()> {
    match v.get("code") {
        Some(c) => {
            let code = c.as_i64().or_else(|| c.as_str()?.parse().ok());
            match code {
                Some(200) | Some(2000) => Ok(()),
                _ => {
                    let msg = v
                        .get("message")
                        .or_else(|| v.get("msg"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    Err(anyhow!("csdn api code {c}: {msg}"))
                }
            }
        }
        None => Ok(()),
    }
}

fn as_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn as_i64(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Map one `SearchAll` entry to the provider item shape.
pub fn parse_search_item(item: &Value) -> Value {
    let title = as_string(item.get("title"));
    let author = {
        let u = as_string(item.get("user_name"));
        if u.is_empty() {
            as_string(item.get("author"))
        } else {
            u
        }
    };
    let digest = {
        let d = as_string(item.get("digest"));
        if d.is_empty() {
            as_string(item.get("description"))
        } else {
            d
        }
    };
    let url = {
        let u = as_string(item.get("url"));
        if u.is_empty() {
            as_string(item.get("dataUrl"))
        } else {
            u
        }
    };
    let view_count = as_i64(item.get("viewcount"))
        .or_else(|| as_i64(item.get("view_num")))
        .or_else(|| as_i64(item.get("view")));
    let item_type = {
        let t = as_string(item.get("so_type"));
        if t.is_empty() {
            as_string(item.get("type"))
        } else {
            t
        }
    };
    json!({
        "id": as_string(item.get("id")),
        "title": title,
        "author": author,
        "digest": digest,
        "url": url,
        "published_at": as_string(item.get("created_at")),
        "view_count": view_count,
        "type": item_type
    })
}

/// Map a search `ResponseResult` to the standard search envelope.
pub fn parse_search_response(v: &Value, query: &str, page: u32, limit: u32) -> Value {
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    let total = data
        .as_array()
        .and_then(|a| a.first())
        .and_then(|first| as_i64(first.get("total")));
    let arr: Vec<Value> = match data {
        Value::Array(a) => a.iter().map(parse_search_item).collect(),
        _ => Vec::new(),
    };
    json!({
        "provider": PROVIDER,
        "ok": true,
        "query": query,
        "page": page,
        "limit": limit,
        "count": arr.len(),
        "total_available": total,
        "items": arr
    })
}

/// Very small HTML-to-text used for the article body convenience
/// field: block elements become newlines, tags are dropped, the
/// common named entities are decoded.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for c in html.chars() {
        if in_tag {
            if c == '>' {
                in_tag = false;
                let t = tag.trim().to_lowercase();
                if t == "br"
                    || t == "br/"
                    || t.starts_with("/p")
                    || t.starts_with("/div")
                    || t.starts_with("/li")
                    || t.starts_with("/h")
                    || t.starts_with("/tr")
                    || t.starts_with("/pre")
                    || t.starts_with("/blockquote")
                {
                    out.push('\n');
                }
                tag.clear();
            } else {
                tag.push(c);
            }
            continue;
        }
        if c == '<' {
            in_tag = true;
            continue;
        }
        out.push(c);
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    // Collapse 3+ newlines to 2 and trim trailing spaces per line.
    let mut cleaned = String::with_capacity(decoded.len());
    let mut newlines = 0;
    for c in decoded.chars() {
        if c == '\n' {
            newlines += 1;
            if newlines <= 2 {
                cleaned.push(c);
            }
        } else {
            newlines = 0;
            cleaned.push(c);
        }
    }
    cleaned.trim().to_string()
}

/// Map a detail `ResponseResult` to the standard item envelope.
pub fn parse_article_response(v: &Value, article_id: &str) -> Value {
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    let article = data.get("article").cloned().unwrap_or(Value::Null);
    let author = data.get("author").cloned().unwrap_or(Value::Null);
    let content_html = as_string(article.get("contentHtml"));
    let tags: Vec<Value> = article
        .get("tags")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|t| json!(as_string(Some(t)))).collect())
        .unwrap_or_default();
    let item_id = {
        let id = as_string(article.get("articleId"));
        if id.is_empty() {
            article_id.to_string()
        } else {
            id
        }
    };
    let item_author = {
        let u = as_string(article.get("username"));
        if u.is_empty() {
            as_string(author.get("username"))
        } else {
            u
        }
    };
    json!({
        "provider": PROVIDER,
        "ok": true,
        "item": {
            "id": item_id,
            "title": as_string(article.get("title")),
            "author": item_author,
            "author_name": as_string(author.get("nickname")),
            "url": as_string(article.get("url")),
            "description": as_string(article.get("description")),
            "content_html": content_html,
            "content_text": html_to_text(&content_html),
            "tags": tags,
            "view_count": as_i64(article.get("viewCount")),
            "comment_count": as_i64(article.get("commentCount")),
            "digg_count": as_i64(article.get("diggCount")),
            "collect_count": as_i64(article.get("collectCount"))
        }
    })
}

fn error_value(kind: &str, message: &str) -> Value {
    json!({
        "provider": PROVIDER,
        "ok": false,
        "error": { "kind": kind, "message": message }
    })
}

/// Tool-shaped search: `csdn_search_articles`.
pub async fn search_articles(
    client: &CsdnClient,
    query: &str,
    page: u32,
    limit: u32,
    sort: Option<&str>,
) -> Value {
    match client.search_raw(query, page, limit, sort).await {
        Ok(v) => match check_code(&v) {
            Ok(()) => parse_search_response(&v, query, page.max(1), limit.clamp(1, 50)),
            Err(e) => error_value("api_error", &e.to_string()),
        },
        Err(e) => error_value("request_error", &e.to_string()),
    }
}

/// Extract the numeric article id from a blog URL of the form
/// `https://blog.csdn.net/{user}/article/details/{id}`, or return
/// the input unchanged when it already looks like an id.
pub fn extract_article_id(input: &str) -> String {
    let t = input.trim();
    if let Some(idx) = t.find("/article/details/") {
        let rest = &t[idx + "/article/details/".len()..];
        let id: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !id.is_empty() {
            return id;
        }
    }
    t.to_string()
}

/// Tool-shaped detail: `csdn_get_article`.
pub async fn get_article(client: &CsdnClient, article_id_or_url: &str) -> Value {
    let id = extract_article_id(article_id_or_url);
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return error_value(
            "invalid_argument",
            "article_id must be a numeric id or a blog.csdn.net article URL",
        );
    }
    match client.article_raw(&id).await {
        Ok(v) => match check_code(&v) {
            Ok(()) => parse_article_response(&v, &id),
            Err(e) => error_value("api_error", &e.to_string()),
        },
        Err(e) => error_value("request_error", &e.to_string()),
    }
}
