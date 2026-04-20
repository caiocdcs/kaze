use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::api::dto::ConnectorInfo;
use crate::http::HttpClient;

pub mod mangadex;
pub mod asura_comic;
pub mod weeb_central;
pub mod mangaworld;
pub mod global;
pub mod chromium;

pub use mangadex::MangaDexConnector;
pub use asura_comic::AsuraComicConnector;
pub use weeb_central::WeebCentralConnector;
pub use mangaworld::MangaworldConnector;
pub use global::GlobalConnector;
pub use chromium::BrowserManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMangaResult {
    pub name: String,
    pub description: String,
    pub cover_url: String,
    pub release_status: String,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
    pub links: Vec<(String, String)>,
    pub alt_titles: Vec<(String, String)>,
    pub connector_site_id: String,
    pub website_url: Option<String>,
    pub year: Option<u32>,
    pub original_language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewChapterResult {
    pub chapter_number: String,
    pub volume_number: Option<i32>,
    pub title: Option<String>,
    pub connector_site_id: String,
    pub website_url: Option<String>,
}

#[async_trait]
pub trait MangaConnector: Send + Sync {
    fn name(&self) -> &str;
    fn supported_languages(&self) -> &[&str];
    fn base_uris(&self) -> &[&str];
    fn icon_url(&self) -> &str;
    fn enabled(&self) -> bool;
    fn set_enabled(&mut self, enabled: bool);

    async fn search_manga(
        &self,
        query: &str,
        http: &dyn HttpClient,
    ) -> Result<Vec<NewMangaResult>, anyhow::Error>;

    async fn get_manga_from_url(
        &self,
        url: &str,
        http: &dyn HttpClient,
    ) -> Result<Option<NewMangaResult>, anyhow::Error>;

    async fn get_chapters(
        &self,
        connector_site_id: &str,
        language: Option<&str>,
        http: &dyn HttpClient,
    ) -> Result<Vec<NewChapterResult>, anyhow::Error>;

    async fn get_chapter_image_urls(
        &self,
        connector_site_id: &str,
        http: &dyn HttpClient,
    ) -> Result<Vec<String>, anyhow::Error>;

    fn url_matches_connector(&self, url: &str) -> bool;

    fn get_cover_url_for_manga(&self, _connector_site_id: &str, _manga_name: &str) -> Option<String> {
        None
    }

    fn image_referrer(&self) -> Option<&str> {
        self.base_uris().first().copied()
    }

    fn to_info(&self) -> ConnectorInfo {
        ConnectorInfo {
            name: self.name().to_string(),
            supported_languages: self.supported_languages().iter().map(|s| s.to_string()).collect(),
            icon_url: self.icon_url().to_string(),
            base_uris: self.base_uris().iter().map(|s| s.to_string()).collect(),
            enabled: self.enabled(),
        }
    }
}

/// Scrape all `(text, href)` pairs from elements matching `selector`.
pub fn scrape_links(html: &str, selector: &str) -> Vec<(String, String)> {
    let document = scraper::Html::parse_document(html);
    let sel = match scraper::Selector::parse(selector) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    document
        .select(&sel)
        .map(|el| {
            let text = el.text().collect::<Vec<_>>().join("").trim().to_string();
            let href = el.value().attr("href").unwrap_or("").to_string();
            (text, href)
        })
        .filter(|(t, _)| !t.is_empty())
        .collect()
}

/// Scrape text content from elements matching `selector`.
pub fn scrape_texts(html: &str, selector: &str) -> Vec<String> {
    let document = scraper::Html::parse_document(html);
    let sel = match scraper::Selector::parse(selector) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    document
        .select(&sel)
        .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Scrape an attribute from elements matching `selector`.
pub fn scrape_attrs(html: &str, selector: &str, attr: &str) -> Vec<String> {
    let document = scraper::Html::parse_document(html);
    let sel = match scraper::Selector::parse(selector) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    document
        .select(&sel)
        .filter_map(|el| el.value().attr(attr).map(|v| v.to_string()))
        .collect()
}

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ConnectorRegistry {
    connectors: HashMap<String, Box<dyn MangaConnector>>,
}

impl Default for ConnectorRegistry {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ConnectorRegistry {
    pub fn new(browser: Option<Arc<BrowserManager>>) -> Self {
        let mut reg = Self {
            connectors: HashMap::new(),
        };
        reg.register(Box::new(MangaDexConnector::new()));
        reg.register(Box::new(AsuraComicConnector::new(browser.clone())));
        reg.register(Box::new(WeebCentralConnector::new(browser.clone())));
        reg.register(Box::new(MangaworldConnector::new(browser.clone())));
        reg
    }

    pub fn register(&mut self, connector: Box<dyn MangaConnector>) {
        let name = connector.name().to_lowercase();
        self.connectors.insert(name, connector);
    }

    pub fn get(&self, name: &str) -> Option<&dyn MangaConnector> {
        self.connectors.get(&name.to_lowercase()).map(|b| b.as_ref())
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut dyn MangaConnector> {
        let key = name.to_lowercase();
        match self.connectors.get_mut(&key) {
            Some(b) => Some(b.as_mut()),
            None => None,
        }
    }

    pub fn all(&self) -> Vec<&dyn MangaConnector> {
        self.connectors.values().map(|b| b.as_ref()).collect()
    }

    pub fn enabled(&self) -> Vec<&dyn MangaConnector> {
        self.all().into_iter().filter(|c| c.enabled()).collect()
    }

    pub fn all_info(&self) -> Vec<ConnectorInfo> {
        self.all().iter().map(|c| c.to_info()).collect()
    }

    pub fn enabled_info(&self) -> Vec<ConnectorInfo> {
        self.enabled().iter().map(|c| c.to_info()).collect()
    }
}

pub async fn add_global_connector(registry: &Arc<RwLock<ConnectorRegistry>>) {
    let global = GlobalConnector::new(registry.clone());
    registry.write().await.register(Box::new(global));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrape_links_extracts_href_and_text() {
        let html = r#"<html><body><a href="/chapter/1">Ch.1</a><a href="/chapter/2">Ch.2</a></body></html>"#;
        let links = scrape_links(html, "a");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], ("Ch.1".into(), "/chapter/1".into()));
        assert_eq!(links[1], ("Ch.2".into(), "/chapter/2".into()));
    }

    #[test]
    fn scrape_texts_extracts_text() {
        let html = r#"<html><body><h1>Title</h1><h1>Other</h1></body></html>"#;
        let texts = scrape_texts(html, "h1");
        assert_eq!(texts, vec!["Title", "Other"]);
    }

    #[test]
    fn scrape_attrs_extracts_attribute() {
        let html = r#"<html><body><img src="a.jpg"/><img src="b.jpg"/></body></html>"#;
        let srcs = scrape_attrs(html, "img", "src");
        assert_eq!(srcs, vec!["a.jpg", "b.jpg"]);
    }

    #[test]
    fn scrape_returns_empty_for_bad_selector() {
        assert!(scrape_links("<html></html>", "!!!invalid").is_empty());
        assert!(scrape_texts("<html></html>", "!!!invalid").is_empty());
        assert!(scrape_attrs("<html></html>", "!!!invalid", "src").is_empty());
    }
}
