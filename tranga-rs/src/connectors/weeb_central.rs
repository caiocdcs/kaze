use async_trait::async_trait;
use scraper::Element;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::http::{HttpClient, RequestType};
use super::{BrowserManager, MangaConnector, NewChapterResult, NewMangaResult};

pub struct WeebCentralConnector {
    enabled: bool,
    browser: Option<Arc<BrowserManager>>,
}
impl Default for WeebCentralConnector {
    fn default() -> Self {
        Self::new(None)
    }
}
impl WeebCentralConnector {
    pub fn new(browser: Option<Arc<BrowserManager>>) -> Self {
        Self { enabled: true, browser }
    }
}

const BASE_URL: &str = "https://weebcentral.com";

#[async_trait]
impl MangaConnector for WeebCentralConnector {
    fn name(&self) -> &str { "WeebCentral" }
    fn supported_languages(&self) -> &[&str] { &["en"] }
    fn base_uris(&self) -> &[&str] { &["https://weebcentral.com"] }
    fn icon_url(&self) -> &str { "https://weebcentral.com/favicon.ico" }
    fn enabled(&self) -> bool { self.enabled }
    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }

    async fn search_manga(&self, query: &str, http: &dyn HttpClient) -> Result<Vec<NewMangaResult>, anyhow::Error> {
        let url = format!("{BASE_URL}/search/data?text={query}");
        let resp = http.make_request(&url, RequestType::Default, None, CancellationToken::new()).await?;
        let html = resp.body;
        let document = scraper::Html::parse_document(&html);
        let sel = scraper::Selector::parse("article").map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut results = Vec::new();
        for element in document.select(&sel) {
            let link_sel = scraper::Selector::parse("a").ok();
            let name = link_sel.as_ref().and_then(|s| element.select(s).next())
                .and_then(|e| e.text().next())
                .unwrap_or("")
                .trim()
                .to_string();
            if name.is_empty() { continue; }

            let href = link_sel.as_ref().and_then(|s| element.select(s).next())
                .and_then(|e| e.value().attr("href"))
                .unwrap_or("")
                .to_string();
            let site_id = href.trim_start_matches('/').to_string();

            let img_sel = scraper::Selector::parse("img").ok();
            let cover_url = img_sel.as_ref().and_then(|s| element.select(s).next())
                .and_then(|e| e.value().attr("src"))
                .unwrap_or("")
                .to_string();
            let cover_url = make_absolute_url(&cover_url, "https://temp.compsci88.com");

            results.push(NewMangaResult {
                name, description: String::new(), cover_url,
                release_status: "Continuing".into(),
                authors: vec![], tags: vec![], links: vec![], alt_titles: vec![],
                connector_site_id: site_id,
                website_url: Some(format!("{BASE_URL}/{href}")),
                year: None, original_language: Some("en".into()),
            });
        }
        Ok(results)
    }

    async fn get_manga_from_url(&self, url: &str, http: &dyn HttpClient) -> Result<Option<NewMangaResult>, anyhow::Error> {
        let caps = regex::Regex::new(r"https?://(?:www\.)?weebcentral\.com/(?:series|chapters)/([^/]+)")
            .unwrap()
            .captures(url);
        let site_id = match caps {
            Some(c) => c.get(1).unwrap().as_str().to_string(),
            None => return Ok(None),
        };

        let page_url = format!("{BASE_URL}/series/{site_id}");
        let resp = http.make_request(&page_url, RequestType::Default, None, CancellationToken::new()).await?;
        if !(200..300).contains(&resp.status) {
            return Ok(None);
        }

        Ok(Some(parse_weebcentral_manga(&resp.body, &site_id, &page_url)?))
    }

    async fn get_chapters(&self, connector_site_id: &str, _language: Option<&str>, _http: &dyn HttpClient) -> Result<Vec<NewChapterResult>, anyhow::Error> {
        let url = format!("{BASE_URL}/series/{connector_site_id}");
        let browser = self.browser.as_ref().ok_or_else(|| anyhow::anyhow!("chromium not available"))?;
        let html = browser.fetch_html(&url).await?;
        let document = scraper::Html::parse_document(&html);

        let chapter_sel = scraper::Selector::parse("a[href*='/chapters/']")
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut chapters = Vec::new();
        for element in document.select(&chapter_sel) {
            let href = element.value().attr("href").unwrap_or("").to_string();
            if !href.contains("/chapters/") {
                continue;
            }
            let text = element.text().collect::<Vec<_>>().join("").trim().to_string();

            let ch_num = text.replace("Chapter ", "").replace("ch ", "").trim().to_string();
            let ch_num = crate::utils::normalize_chapter_number(&ch_num);

            if ch_num.is_empty() { continue; }

            let connector_id = href.trim_start_matches('/').to_string();

            chapters.push(NewChapterResult {
                chapter_number: ch_num,
                volume_number: None,
                title: if text.is_empty() { None } else { Some(text) },
                connector_site_id: connector_id,
                website_url: Some(format!("{BASE_URL}{href}")),
            });
        }

        Ok(chapters)
    }

    async fn get_chapter_image_urls(&self, connector_site_id: &str, _http: &dyn HttpClient) -> Result<Vec<String>, anyhow::Error> {
        let url = format!("{BASE_URL}/{connector_site_id}");
        let browser = self.browser.as_ref().ok_or_else(|| anyhow::anyhow!("chromium not available"))?;
        let html = browser.fetch_html(&url).await?;
        Ok(extract_weebcentral_page_images(&html))
    }

    fn url_matches_connector(&self, url: &str) -> bool {
        url.contains("weebcentral.com")
    }
}

fn extract_weebcentral_page_images(html: &str) -> Vec<String> {
    let document = scraper::Html::parse_document(html);
    let img_sel = match scraper::Selector::parse("img") {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let valid_exts = [".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif"];
    let mut urls = Vec::new();

    for img in document.select(&img_sel) {
        let alt = img.value().attr("alt").unwrap_or("");
        if !alt.starts_with("Page") {
            continue;
        }

        let src = img.value().attr("data-src")
            .or_else(|| img.value().attr("src"))
            .unwrap_or("");

        if src.is_empty() { continue; }

        let abs = make_absolute_url(src, BASE_URL);
        let lower = abs.to_lowercase();
        if valid_exts.iter().any(|ext| lower.contains(ext)) || abs.contains("?") {
            urls.push(abs);
        }
    }

    urls
}

fn make_absolute_url(url: &str, base: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with("//") {
        format!("https:{}", url)
    } else if url.starts_with('/') {
        format!("{}{}", base, url)
    } else {
        url.to_string()
    }
}

fn regex_lazy() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"https?://(?:www\.)?weebcentral\.com/(?:series|chapters)/([^/]+)").unwrap())
}

fn parse_weebcentral_manga(html: &str, site_id: &str, url: &str) -> Result<NewMangaResult, anyhow::Error> {
    let document = scraper::Html::parse_document(html);

    let title = scraper::Selector::parse("title")
        .ok()
        .and_then(|s| document.select(&s).next())
        .and_then(|e| e.text().next())
        .map(|t| t.split('|').next().unwrap_or(t).trim().to_string())
        .unwrap_or_else(|| site_id.to_string());

    let cover_url = scraper::Selector::parse("img[alt*='cover' i], img[alt*='Cover']")
        .ok()
        .and_then(|s| document.select(&s).next())
        .and_then(|e| e.value().attr("src"))
        .unwrap_or("")
        .to_string();
    let cover_url = make_absolute_url(&cover_url, "https://temp.compsci88.com");

    let description = scraper::Selector::parse("strong")
        .ok()
        .and_then(|s| {
            document.select(&s).find(|e| e.text().collect::<String>().starts_with("Description"))
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|e| e.text().next().map(|t| t.trim().to_string()))
        .unwrap_or_default();

    let tags = extract_sibling_texts_by_label(&document, "Tag", "span");
    let release_status = extract_status_by_label(&document, "Status");
    let authors = extract_sibling_texts_by_label(&document, "Author", "span");

    let year = scraper::Selector::parse("strong")
        .ok()
        .and_then(|s| {
            document.select(&s).find(|e| e.text().collect::<String>().starts_with("Released"))
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|e| e.text().next())
        .and_then(|t| t.split_whitespace().last()?.parse::<u32>().ok());

    Ok(NewMangaResult {
        name: title,
        description,
        cover_url,
        release_status,
        authors,
        tags,
        links: vec![],
        alt_titles: vec![],
        connector_site_id: site_id.to_string(),
        website_url: Some(url.to_string()),
        year,
        original_language: Some("en".into()),
    })
}

fn extract_sibling_texts_by_label(document: &scraper::Html, label: &str, child_tag: &str) -> Vec<String> {
    scraper::Selector::parse("strong")
        .ok()
        .and_then(|s| {
            document.select(&s).find(|e| e.text().collect::<String>().starts_with(label))
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|parent| {
            let child_sel = scraper::Selector::parse(child_tag).ok()?;
            Some(parent.select(&child_sel).map(|e| e.text().collect::<String>()).collect::<Vec<_>>())
        })
        .map(|v| v.into_iter().filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

fn extract_status_by_label(document: &scraper::Html, label: &str) -> String {
    scraper::Selector::parse("strong")
        .ok()
        .and_then(|s| {
            document.select(&s).find(|e| e.text().collect::<String>().starts_with(label))
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|e| e.text().next().map(|t| t.trim().to_lowercase()))
        .map(|t| match t.as_str() {
            "ongoing" => "Continuing",
            "hiatus" => "OnHiatus",
            "completed" => "Completed",
            "canceled" | "cancelled" => "Cancelled",
            _ => "Continuing",
        })
        .unwrap_or("Continuing")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock_client::MockHttpClient;

    #[tokio::test]
    async fn search_parses_articles() {
        let mock = MockHttpClient::builder()
            .expect_get("weebcentral.com/search", include_str!("fixtures/weebcentral_search.html"))
            .build();
        let connector = WeebCentralConnector::new(None);
        let results = connector.search_manga("solo", &mock).await.unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn url_matches_weebcentral() {
        let connector = WeebCentralConnector::new(None);
        assert!(connector.url_matches_connector("https://weebcentral.com/manga/solo-leveling"));
        assert!(!connector.url_matches_connector("https://example.com"));
    }

    #[tokio::test]
    async fn get_chapters_fails_without_browser() {
        let mock = MockHttpClient::builder().build();
        let connector = WeebCentralConnector::new(None);
        let result = connector.get_chapters("solo-leveling", Some("en"), &mock).await;
        assert!(result.is_err());
    }
}
