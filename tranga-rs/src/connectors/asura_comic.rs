use async_trait::async_trait;
use scraper::Element;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::http::{HttpClient, RequestType};
use super::{BrowserManager, MangaConnector, NewChapterResult, NewMangaResult};

pub struct AsuraComicConnector {
    enabled: bool,
    browser: Option<Arc<BrowserManager>>,
}

impl Default for AsuraComicConnector {
    fn default() -> Self {
        Self::new(None)
    }
}

impl AsuraComicConnector {
    pub fn new(browser: Option<Arc<BrowserManager>>) -> Self {
        Self { enabled: true, browser }
    }
}

const BASE_URL: &str = "https://asuracomic.net";

#[async_trait]
impl MangaConnector for AsuraComicConnector {
    fn name(&self) -> &str { "AsuraComic" }
    fn supported_languages(&self) -> &[&str] { &["en"] }
    fn base_uris(&self) -> &[&str] { &["https://asuracomic.net"] }
    fn icon_url(&self) -> &str { "https://asuracomic.net/favicon.ico" }
    fn enabled(&self) -> bool { self.enabled }
    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }

    async fn search_manga(&self, query: &str, http: &dyn HttpClient) -> Result<Vec<NewMangaResult>, anyhow::Error> {
        let url = format!("{BASE_URL}/series?name={query}");
        let resp = http.make_request(&url, RequestType::Default, None, CancellationToken::new()).await?;
        let html = resp.body;
        let document = scraper::Html::parse_document(&html);
        let selector = scraper::Selector::parse("div.bs-item").map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut results = Vec::new();
        for element in document.select(&selector) {
            let title_sel = scraper::Selector::parse("div.bs-item a").ok();
            let name = title_sel.as_ref().and_then(|s| element.select(s).next())
                .and_then(|e| e.text().next())
                .unwrap_or("")
                .trim()
                .to_string();

            if name.is_empty() { continue; }

            let href = title_sel.as_ref().and_then(|s| element.select(s).next())
                .and_then(|e| e.value().attr("href"))
                .unwrap_or("")
                .to_string();

            let site_id = href.trim_start_matches('/').trim_end_matches('/').to_string();

            let cover_sel = scraper::Selector::parse("img").ok();
            let cover_url = cover_sel.as_ref().and_then(|s| element.select(s).next())
                .and_then(|e| e.value().attr("src"))
                .unwrap_or("")
                .to_string();

            results.push(NewMangaResult {
                name,
                description: String::new(),
                cover_url,
                release_status: "Continuing".into(),
                authors: vec![],
                tags: vec![],
                links: vec![],
                alt_titles: vec![],
                connector_site_id: site_id,
                website_url: Some(format!("{BASE_URL}/{href}")),
                year: None,
                original_language: Some("en".into()),
            });
        }
        Ok(results)
    }

    async fn get_manga_from_url(&self, url: &str, http: &dyn HttpClient) -> Result<Option<NewMangaResult>, anyhow::Error> {
        if !self.url_matches_connector(url) {
            return Ok(None);
        }

        let re = regex_lazy();
        let site_id = match re.captures(url) {
            Some(c) => c.get(1).unwrap().as_str().to_string(),
            None => url.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string(),
        };

        let page_url = format!("{BASE_URL}/series/{site_id}");
        let resp = http.make_request(&page_url, RequestType::Default, None, CancellationToken::new()).await?;
        if !(200..300).contains(&resp.status) {
            return Ok(None);
        }

        Ok(Some(parse_asura_manga(&resp.body, &site_id, &page_url)?))
    }

    async fn get_chapters(&self, connector_site_id: &str, _language: Option<&str>, _http: &dyn HttpClient) -> Result<Vec<NewChapterResult>, anyhow::Error> {
        let url = format!("{BASE_URL}/{connector_site_id}");
        let browser = self.browser.as_ref().ok_or_else(|| anyhow::anyhow!("chromium not available"))?;
        let html = browser.fetch_html(&url).await?;
        let document = scraper::Html::parse_document(&html);

        let early_access = extract_early_access_chapters(&document);

        let chapter_sel = scraper::Selector::parse("a[href*='/chapter/']")
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut chapters = Vec::new();
        for element in document.select(&chapter_sel) {
            let href = element.value().attr("href").unwrap_or("").to_string();
            let title = element.text().collect::<Vec<_>>().join("").trim().to_string();

            let ch_num = href.rsplit('/').next().unwrap_or("").to_string();
            let ch_num = crate::utils::normalize_chapter_number(&ch_num);

            if ch_num.is_empty() { continue; }
            if early_access.contains(&ch_num) { continue; }

            chapters.push(NewChapterResult {
                chapter_number: ch_num,
                volume_number: None,
                title: if title.is_empty() { None } else { Some(title) },
                connector_site_id: href.trim_start_matches('/').to_string(),
                website_url: Some(format!("{BASE_URL}{href}")),
            });
        }

        chapters.reverse();
        Ok(chapters)
    }

    async fn get_chapter_image_urls(&self, connector_site_id: &str, _http: &dyn HttpClient) -> Result<Vec<String>, anyhow::Error> {
        let url = format!("{BASE_URL}/{connector_site_id}");
        let browser = self.browser.as_ref().ok_or_else(|| anyhow::anyhow!("chromium not available"))?;
        let html = browser.fetch_html(&url).await?;
        Ok(extract_asura_page_images(&html))
    }

    fn url_matches_connector(&self, url: &str) -> bool {
        url.contains("asuracomic.net")
    }
}

fn extract_asura_page_images(html: &str) -> Vec<String> {
    let document = scraper::Html::parse_document(html);
    let img_sel = match scraper::Selector::parse("img") {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let valid_exts = [".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif"];
    let mut urls = Vec::new();

    for img in document.select(&img_sel) {
        let alt = img.value().attr("alt").unwrap_or("").to_lowercase();
        let class = img.value().attr("class").unwrap_or("");
        if !alt.contains("chapter page") && !alt.contains("chapter") && !class.contains("page-break") {
            continue;
        }

        let src = img.value().attr("src")
            .or_else(|| img.value().attr("data-src"))
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

fn extract_early_access_chapters(document: &scraper::Html) -> std::collections::HashSet<String> {
    let script_sel = match scraper::Selector::parse("script") {
        Ok(s) => s,
        Err(_) => return std::collections::HashSet::new(),
    };

    let script_text = document.select(&script_sel)
        .find(|e| e.text().any(|t| t.contains("is_early_access")))
        .and_then(|e| e.text().next())
        .unwrap_or("");

    let unescaped = script_text.replace("\\", "");

    let re = regex_lazy_chapters_json();
    let Some(caps) = re.captures(&unescaped) else {
        return std::collections::HashSet::new();
    };

    let chapters_json = caps.get(1).unwrap().as_str();
    let chapters_json = format!("[{}]", chapters_json.trim().trim_end_matches(','));

    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&chapters_json) else {
        return std::collections::HashSet::new();
    };

    let Some(arr) = parsed.as_array() else {
        return std::collections::HashSet::new();
    };

    let mut result = std::collections::HashSet::new();
    for chapter in arr {
        let name = match chapter.get("name") {
            Some(n) if n.is_number() => n.as_f64().unwrap_or(0.0).to_string(),
            Some(n) => n.as_str().unwrap_or("").to_string(),
            None => continue,
        };
        if name.is_empty() { continue; }

        let is_early = chapter.get("is_early_access")
            .and_then(|v| match v {
                serde_json::Value::Bool(b) => Some(*b),
                serde_json::Value::String(s) => Some(s.eq_ignore_ascii_case("true")),
                _ => None,
            })
            .unwrap_or(false);

        if is_early {
            result.insert(name);
        }
    }

    result
}

fn regex_lazy_chapters_json() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#""chapters":\s*\[(.*?)\],\s*"comic""#).unwrap())
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
    RE.get_or_init(|| regex::Regex::new(r"https?://(?:www\.)?asuracomic\.net/series/([^/]+)").unwrap())
}

fn parse_asura_manga(html: &str, site_id: &str, url: &str) -> Result<NewMangaResult, anyhow::Error> {
    let document = scraper::Html::parse_document(html);

    let title = scraper::Selector::parse("title")
        .ok()
        .and_then(|s| document.select(&s).next())
        .and_then(|e| e.text().next())
        .map(|t| t.split(" - Asura Scans").next().unwrap_or(t).trim().to_string())
        .unwrap_or_else(|| site_id.to_string());

    let cover_url = scraper::Selector::parse("img[alt='poster']")
        .ok()
        .and_then(|s| document.select(&s).next())
        .and_then(|e| e.value().attr("src"))
        .unwrap_or("")
        .to_string();
    let cover_url = make_absolute_url(&cover_url, BASE_URL);

    let description = scraper::Selector::parse("h3")
        .ok()
        .and_then(|s| {
            document.select(&s).find(|e| e.text().collect::<String>().starts_with("Synopsis"))
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|e| e.text().next().map(|t| t.trim().to_string()))
        .unwrap_or_default();

    let tags = extract_asura_sibling_texts(&document, "Genres", "button");

    let release_status = scraper::Selector::parse("h3")
        .ok()
        .and_then(|s| {
            document.select(&s).find(|e| e.text().collect::<String>() == "Status")
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|e| e.text().next().map(|t| t.trim().to_lowercase()))
        .map(|t| match t.as_str() {
            "ongoing" | "season end" => "Continuing",
            "hiatus" => "OnHiatus",
            "completed" => "Completed",
            "dropped" => "Cancelled",
            _ => "Continuing",
        })
        .unwrap_or("Continuing")
        .to_string();

    let authors = extract_asura_sibling_texts(&document, "Author", "h3");

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
        year: None,
        original_language: Some("en".into()),
    })
}

fn extract_asura_sibling_texts(document: &scraper::Html, label: &str, child_tag: &str) -> Vec<String> {
    scraper::Selector::parse("h3")
        .ok()
        .and_then(|s| {
            document.select(&s).find(|e| e.text().collect::<String>() == label)
        })
        .and_then(|e| e.next_sibling_element())
        .and_then(|parent| {
            let child_sel = scraper::Selector::parse(child_tag).ok()?;
            Some(parent.select(&child_sel).map(|e| e.text().collect::<String>()).collect::<Vec<_>>())
        })
        .map(|v| v.into_iter().filter(|s| !s.is_empty() && s != "_").collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock_client::MockHttpClient;

    #[tokio::test]
    async fn search_parses_html() {
        let mock = MockHttpClient::builder()
            .expect_get("asuracomic.net/series", include_str!("fixtures/asura_search.html"))
            .build();
        let connector = AsuraComicConnector::new(None);
        let results = connector.search_manga("solo", &mock).await.unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn url_matches_asura() {
        let connector = AsuraComicConnector::new(None);
        assert!(connector.url_matches_connector("https://asuracomic.net/series/solo-leveling"));
        assert!(!connector.url_matches_connector("https://example.com"));
    }

    #[tokio::test]
    async fn get_chapters_fails_without_browser() {
        let mock = MockHttpClient::builder().build();
        let connector = AsuraComicConnector::new(None);
        let result = connector.get_chapters("solo-leveling", Some("en"), &mock).await;
        assert!(result.is_err());
    }
}
