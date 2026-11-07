//! Naver web-results view (`ssc=tab.m_ur.all` on the mobile host).
//!
//! Results are the anchors carrying `data-nlog-area="*.w.link"`; the
//! title is the anchor text (Naver wraps matched terms in `<mark>`, which
//! text extraction drops), the rank is in the anchor's
//! `data-nlog-params` JSON, and the snippet is the body paragraph inside
//! the same result card. Card boundaries are found structurally (the
//! smallest ancestor holding exactly one `.w.link`), because the CSS
//! classes around them are hashed per build.

use anyhow::Result;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use crate::common::*;
use crate::urlbuild::build_page_url;
use crate::{NaverSearchClient, SearchResult};

static LINK_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(r#"a[data-nlog-area$=".w.link"]"#).unwrap());
static P_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("p").unwrap());

/// One web result from the web-results view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebItem {
    pub rank: i64,
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// Parse a saved Naver web-results page. `limit` caps the returned items.
pub fn parse_web_results(html: &str, limit: usize) -> Result<Vec<WebItem>> {
    let doc = Html::parse_document(html);
    let mut items = Vec::new();
    for link in doc.select(&LINK_SEL) {
        if items.len() >= limit {
            break;
        }
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        if href.is_empty() || href.starts_with('#') {
            continue;
        }
        let title = element_text(link);
        if title.is_empty() {
            continue;
        }
        let rank = link
            .value()
            .attr("data-nlog-params")
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .and_then(|v| match v.get("cr_rank_legacy") {
                Some(serde_json::Value::String(s)) => s.parse::<i64>().ok(),
                Some(serde_json::Value::Number(n)) => n.as_i64(),
                _ => None,
            })
            .unwrap_or(items.len() as i64 + 1);
        let card = card_for(link, &LINK_SEL, 10);
        let snippet = card.select(&P_SEL).map(|p| element_text(p)).find(|t| {
            t.chars().count() >= 20 && !t.contains("네이버가 AI") && !t.contains("Keep에 저장")
        });
        items.push(WebItem {
            rank,
            title,
            url: resolve_url(href, "https://m.search.naver.com"),
            snippet,
        });
    }
    if items.is_empty() {
        return Err(layout_error("web"));
    }
    Ok(items)
}

impl NaverSearchClient {
    /// Search the web-results view. The tab serves one top-selection
    /// batch: its own paging links exist, but over plain HTTP every
    /// page parameter returns that same batch (measured — page 1 and
    /// page 2 responses are identical), and the page embeds no
    /// more-API chain like the other tabs. Only page 1 is therefore
    /// served; deeper pages are an explicit `unsupported_page` error
    /// rather than a silent repeat of page 1. `limit` is clamped
    /// to 20.
    pub async fn web_search(
        &self,
        query: &str,
        page: i64,
        limit: i64,
    ) -> Result<SearchResult<WebItem>> {
        let page = page.max(1);
        if page > 1 {
            return Err(anyhow::anyhow!(
                "unsupported_page: the web tab serves the same top-selection batch for every page over plain HTTP; page {page} is not available"
            ));
        }
        let limit = clamp_limit(limit, 20);
        let url = build_page_url(query, "web", 1)?;
        let html = self.get_html(&url, SEARCH_REFERER).await?;
        let items = parse_web_results(&html, limit)?;
        Ok(SearchResult::new("web", query, page, items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_page() {
        let html = r#"<html><body><div class="card">
            <p>이것은 스니펫으로 쓰일 만큼 충분히 긴 본문 텍스트입니다. 추가 설명 문장입니다.</p>
            <a href="https://example.com/" data-nlog-area="urB_boR.w.link"
               data-nlog-params="{&quot;cr_rank_legacy&quot;:&quot;3&quot;}"><span><mark>테스트</mark>잇</span></a>
            </div></body></html>"#;
        let items = parse_web_results(html, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "테스트잇");
        assert_eq!(items[0].url, "https://example.com/");
        assert_eq!(items[0].rank, 3);
        assert!(items[0].snippet.as_ref().unwrap().contains("스니펫"));
    }

    #[test]
    fn parse_empty_shell_is_a_layout_error() {
        assert!(parse_web_results("<html><body><p>shell</p></body></html>", 10).is_err());
    }
}
