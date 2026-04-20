use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::http::HttpClient;
use super::{MangaConnector, NewChapterResult, NewMangaResult, ConnectorRegistry};

pub struct GlobalConnector {
    enabled: bool,
    registry: Arc<RwLock<ConnectorRegistry>>,
}

impl GlobalConnector {
    pub fn new(registry: Arc<RwLock<ConnectorRegistry>>) -> Self {
        Self { enabled: true, registry }
    }
}

#[async_trait]
impl MangaConnector for GlobalConnector {
    fn name(&self) -> &str { "Global" }
    fn supported_languages(&self) -> &[&str] { &[] }
    fn base_uris(&self) -> &[&str] { &[] }
    fn icon_url(&self) -> &str { "" }
    fn enabled(&self) -> bool { self.enabled }
    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }

    async fn search_manga(&self, query: &str, http: &dyn HttpClient) -> Result<Vec<NewMangaResult>, anyhow::Error> {
        let reg = self.registry.read().await;
        let mut results = Vec::new();
        for connector in reg.enabled() {
            if connector.name() == "Global" { continue; }
            if let Ok(manga) = connector.search_manga(query, http).await {
                results.extend(manga);
            }
        }
        Ok(results)
    }

    async fn get_manga_from_url(&self, url: &str, http: &dyn HttpClient) -> Result<Option<NewMangaResult>, anyhow::Error> {
        let reg = self.registry.read().await;
        for connector in reg.enabled() {
            if connector.name() == "Global" { continue; }
            if connector.url_matches_connector(url)
                && let Ok(Some(result)) = connector.get_manga_from_url(url, http).await
            {
                return Ok(Some(result));
            }
        }
        Ok(None)
    }

    async fn get_chapters(&self, _connector_site_id: &str, _language: Option<&str>, _http: &dyn HttpClient) -> Result<Vec<NewChapterResult>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_chapter_image_urls(&self, _connector_site_id: &str, _http: &dyn HttpClient) -> Result<Vec<String>, anyhow::Error> {
        Ok(vec![])
    }

    fn url_matches_connector(&self, _url: &str) -> bool { true }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock_client::MockHttpClient;

    #[test]
    fn url_matches_always() {
        let reg = Arc::new(RwLock::new(ConnectorRegistry::new(None)));
        let connector = GlobalConnector::new(reg);
        assert!(connector.url_matches_connector("https://anything.com"));
    }

    #[tokio::test]
    async fn search_delegates_to_enabled_connectors() {
        let reg = Arc::new(RwLock::new(ConnectorRegistry::new(None)));
        let connector = GlobalConnector::new(reg.clone());
        let mock = MockHttpClient::builder().build();
        let results = connector.search_manga("test", &mock).await.unwrap();
        assert!(results.is_empty(), "no real connectors in default registry");
    }
}
