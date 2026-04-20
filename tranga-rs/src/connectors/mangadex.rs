use async_trait::async_trait;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::http::{HttpClient, RequestType};
use super::{MangaConnector, NewChapterResult, NewMangaResult};

pub struct MangaDexConnector {
    enabled: bool,
}

impl Default for MangaDexConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl MangaDexConnector {
    pub fn new() -> Self {
        Self { enabled: true }
    }
}

const BASE_URL: &str = "https://api.mangadex.org";

#[derive(Debug, Deserialize)]
struct MangaDexSearchResponse {
    data: Vec<MangaDexManga>,
}

#[derive(Debug, Deserialize)]
struct MangaDexManga {
    id: String,
    attributes: MangaDexAttributes,
    #[serde(default)]
    relationships: Vec<MangaDexRelationship>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaDexAttributes {
    title: HashMap<String, String>,
    description: HashMap<String, String>,
    tags: Vec<MangaDexTag>,
    status: Option<String>,
    year: Option<u32>,
    original_language: Option<String>,
    alt_titles: Vec<HashMap<String, String>>,
    links: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct MangaDexTag {
    attributes: MangaDexTagAttributes,
}

#[derive(Debug, Deserialize)]
struct MangaDexTagAttributes {
    name: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct MangaDexRelationship {
    #[serde(rename = "type")]
    rel_type: String,
    #[allow(dead_code)]
    id: String,
    attributes: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MangaDexFeedResponse {
    data: Vec<MangaDexChapter>,
}

#[derive(Debug, Deserialize)]
struct MangaDexChapter {
    id: String,
    attributes: MangaDexChapterAttributes,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaDexChapterAttributes {
    chapter: Option<String>,
    volume: Option<String>,
    title: Option<String>,
    #[allow(dead_code)]
    translated_language: Option<String>,
    external_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaDexAtHomeResponse {
    base_url: String,
    chapter: MangaDexAtHomeChapter,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaDexAtHomeChapter {
    hash: String,
    data: Vec<String>,
    #[allow(dead_code)]
    data_saver: Vec<String>,
}

use std::collections::HashMap;

#[async_trait]
impl MangaConnector for MangaDexConnector {
    fn name(&self) -> &str { "MangaDex" }
    fn supported_languages(&self) -> &[&str] { &["en", "ja", "ko", "zh"] }
    fn base_uris(&self) -> &[&str] { &["https://mangadex.org", "https://api.mangadex.org"] }
    fn icon_url(&self) -> &str { "https://mangadex.org/favicon.ico" }
    fn enabled(&self) -> bool { self.enabled }
    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }

    async fn search_manga(&self, query: &str, http: &dyn HttpClient) -> Result<Vec<NewMangaResult>, anyhow::Error> {
        let url = format!("{BASE_URL}/manga?title={query}&limit=25&includes[]=author&includes[]=cover_art");
        let resp = http.make_request(&url, RequestType::MangaDexFeed, None, CancellationToken::new()).await?;
        let data: MangaDexSearchResponse = serde_json::from_str(&resp.body)?;

        Ok(data.data.into_iter().map(|m| convert_mangadex_manga(&m)).collect())
    }

    async fn get_manga_from_url(&self, url: &str, http: &dyn HttpClient) -> Result<Option<NewMangaResult>, anyhow::Error> {
        let id = extract_id_from_url(url).ok_or_else(|| anyhow::anyhow!("cannot extract id from url"))?;
        let result = self.get_manga_from_id(&id, http).await?;
        Ok(result)
    }

    async fn get_chapters(
        &self,
        connector_site_id: &str,
        language: Option<&str>,
        http: &dyn HttpClient,
    ) -> Result<Vec<NewChapterResult>, anyhow::Error> {
        let lang = language.unwrap_or("en");
        let mut all_chapters = Vec::new();
        let mut offset = 0i32;
        let limit = 100;

        loop {
            let url = format!(
                "{BASE_URL}/manga/{connector_site_id}/feed?translatedLanguage[]={lang}&limit={limit}&offset={offset}&order[chapter]=asc"
            );
            let resp = http.make_request(&url, RequestType::MangaDexFeed, None, CancellationToken::new()).await?;
            let feed: MangaDexFeedResponse = serde_json::from_str(&resp.body)?;

            let count = feed.data.len();
            for ch in feed.data {
                let ch_num = ch.attributes.chapter.unwrap_or_default();
                if ch_num.is_empty() {
                    continue;
                }
                all_chapters.push(NewChapterResult {
                    chapter_number: crate::utils::normalize_chapter_number(&ch_num),
                    volume_number: ch.attributes.volume.as_deref().and_then(|v| v.parse().ok()),
                    title: ch.attributes.title,
                    connector_site_id: ch.id,
                    website_url: ch.attributes.external_url,
                });
            }

            if count < limit as usize {
                break;
            }
            offset += limit;
        }

        Ok(all_chapters)
    }

    async fn get_chapter_image_urls(
        &self,
        connector_site_id: &str,
        http: &dyn HttpClient,
    ) -> Result<Vec<String>, anyhow::Error> {
        let url = format!("{BASE_URL}/at-home/server/{connector_site_id}");
        let resp = http.make_request(&url, RequestType::Default, None, CancellationToken::new()).await?;
        let at_home: MangaDexAtHomeResponse = serde_json::from_str(&resp.body)?;

        Ok(at_home.chapter.data.iter()
            .map(|p| format!("{}/data/{}/{}", at_home.base_url, at_home.chapter.hash, p))
            .collect())
    }

    fn url_matches_connector(&self, url: &str) -> bool {
        url.contains("mangadex.org")
    }
}

impl MangaDexConnector {
    async fn get_manga_from_id(&self, id: &str, http: &dyn HttpClient) -> Result<Option<NewMangaResult>, anyhow::Error> {
        let url = format!("{BASE_URL}/manga/{id}?includes[]=author&includes[]=cover_art");
        let resp = http.make_request(&url, RequestType::MangaDexFeed, None, CancellationToken::new()).await?;
        let manga: MangaDexManga = serde_json::from_str(&resp.body)?;
        Ok(Some(convert_mangadex_manga(&manga)))
    }
}

fn convert_mangadex_manga(m: &MangaDexManga) -> NewMangaResult {
    let title = m.attributes.title.get("en")
        .or_else(|| m.attributes.title.values().next())
        .cloned()
        .unwrap_or_default();

    let description = m.attributes.description.get("en")
        .or_else(|| m.attributes.description.values().next())
        .cloned()
        .unwrap_or_default();

    let cover_file_name = m.relationships.iter()
        .find(|r| r.rel_type == "cover_art")
        .and_then(|r| r.attributes.as_ref())
        .and_then(|a| a.get("fileName"))
        .and_then(|v| v.as_str())
        .unwrap_or("256x360.jpg");

    let cover_url = format!("https://uploads.mangadex.org/covers/{}/{}", m.id, cover_file_name);

    let tags: Vec<String> = m.attributes.tags.iter()
        .filter_map(|t| t.attributes.name.get("en").cloned())
        .collect();

    let authors: Vec<String> = m.relationships.iter()
        .filter(|r| r.rel_type == "author")
        .filter_map(|r| r.attributes.as_ref())
        .filter_map(|a| a.get("name"))
        .filter_map(|v| v.as_str())
        .map(|s| s.to_string())
        .collect();

    let alt_titles: Vec<(String, String)> = m.attributes.alt_titles.iter()
        .flat_map(|h| h.iter().map(|(k, v)| (k.clone(), v.clone())))
        .collect();

    let links: Vec<(String, String)> = m.attributes.links.iter()
        .flat_map(|l| l.iter().map(|(k, v)| (k.clone(), v.clone())))
        .collect();

    let release_status = match m.attributes.status.as_deref() {
        Some("completed") => "Completed",
        Some("hiatus") => "OnHiatus",
        Some("cancelled") => "Cancelled",
        _ => "Continuing",
    };

    NewMangaResult {
        name: title,
        description,
        cover_url,
        release_status: release_status.to_string(),
        authors,
        tags,
        links,
        alt_titles,
        connector_site_id: m.id.clone(),
        website_url: Some(format!("https://mangadex.org/title/{}", m.id)),
        year: m.attributes.year,
        original_language: m.attributes.original_language.clone(),
    }
}

fn extract_id_from_url(url: &str) -> Option<String> {
    let uuid_re = regex_lazy();
    uuid_re.captures(url).map(|caps| caps[1].to_string())
}

fn regex_lazy() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock_client::MockHttpClient;

    #[test]
    fn extract_id_from_url_works() {
        let url = "https://mangadex.org/title/a1c7d239-3a4b-4e5f-8c9d-0123456789ab";
        let id = extract_id_from_url(url).unwrap();
        assert_eq!(id, "a1c7d239-3a4b-4e5f-8c9d-0123456789ab");
    }

    #[test]
    fn extract_id_from_url_returns_none_for_no_uuid() {
        assert!(extract_id_from_url("https://example.com/no-uuid-here").is_none());
    }

    #[tokio::test]
    async fn search_parses_results() {
        let mock = MockHttpClient::builder()
            .expect_get("mangadex.org/manga", include_str!("fixtures/mangadex_search.json"))
            .build();
        let connector = MangaDexConnector::new();
        let results = connector.search_manga("one piece", &mock).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "One Piece");
        assert_eq!(results[0].release_status, "Continuing");
        assert_eq!(results[0].year, Some(1997));
    }

    #[tokio::test]
    async fn get_chapters_parses_feed() {
        let mock = MockHttpClient::builder()
            .expect_get("mangadex.org", include_str!("fixtures/mangadex_feed.json"))
            .build();
        let connector = MangaDexConnector::new();
        let chapters = connector.get_chapters("test-manga", Some("en"), &mock).await.unwrap();
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].chapter_number, "1");
        assert_eq!(chapters[0].title.as_deref(), Some("Romance Dawn"));
    }

    #[tokio::test]
    async fn get_chapter_image_urls_constructs_urls() {
        let mock = MockHttpClient::builder()
            .expect_get("mangadex.org/at-home", include_str!("fixtures/mangadex_athome.json"))
            .build();
        let connector = MangaDexConnector::new();
        let urls = connector.get_chapter_image_urls("chapter-id", &mock).await.unwrap();
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("/data/"));
    }

    #[test]
    fn convert_mangadex_manga_maps_fields() {
        let json = include_str!("fixtures/mangadex_search.json");
        let search: serde_json::Value = serde_json::from_str(json).unwrap();
        let manga_data = &search["data"][0];
        let manga: MangaDexManga = serde_json::from_value(manga_data.clone()).unwrap();
        let result = convert_mangadex_manga(&manga);
        assert_eq!(result.name, "One Piece");
        assert_eq!(result.authors, vec!["Eiichiro Oda"]);
        assert!(result.tags.contains(&"Action".to_string()));
    }

    #[test]
    fn url_matches_mangadex() {
        let connector = MangaDexConnector::new();
        assert!(connector.url_matches_connector("https://mangadex.org/title/abc"));
        assert!(!connector.url_matches_connector("https://example.com"));
    }
}
