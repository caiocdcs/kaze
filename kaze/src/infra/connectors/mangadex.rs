use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::domain::error::DomainError;
use crate::domain::manga_source::{ChapterCandidate, MangaCandidate, MangaSource};
use crate::http::{HttpClient, RequestType};

pub struct MangaDexSource {
    http: Arc<dyn HttpClient>,
}

impl MangaDexSource {
    pub fn new(http: Arc<dyn HttpClient>) -> Self {
        Self { http }
    }
}

const BASE_URL: &str = "https://api.mangadex.org";

#[async_trait]
impl MangaSource for MangaDexSource {
    fn name(&self) -> &str {
        "MangaDex"
    }

    fn matches_url(&self, url: &str) -> bool {
        url.contains("mangadex.org")
    }

    async fn search(&self, query: &str) -> Result<Vec<MangaCandidate>, DomainError> {
        let url = format!(
            "{BASE_URL}/manga?title={query}&limit=25&includes[]=author&includes[]=cover_art"
        );
        let resp = self
            .http
            .make_request(
                &url,
                RequestType::MangaDexFeed,
                None,
                CancellationToken::new(),
            )
            .await
            .map_err(|e| DomainError::source(e.to_string()))?;

        let search: MangaDexSearchResponse = serde_json::from_str(&resp.body)
            .map_err(|e| DomainError::invalid(format!("MangaDex JSON: {e}")))?;

        Ok(search.data.into_iter().map(|m| convert_manga(&m)).collect())
    }

    async fn from_url(&self, url: &str) -> Result<Option<MangaCandidate>, DomainError> {
        let id = match extract_id_from_url(url) {
            Some(id) => id,
            None => return Ok(None),
        };

        let api_url = format!("{BASE_URL}/manga/{id}?includes[]=author&includes[]=cover_art");
        let resp = self
            .http
            .make_request(
                &api_url,
                RequestType::MangaDexFeed,
                None,
                CancellationToken::new(),
            )
            .await
            .map_err(|e| DomainError::source(e.to_string()))?;

        let entity: MangaDexEntityResponse = serde_json::from_str(&resp.body)
            .map_err(|e| DomainError::invalid(format!("MangaDex JSON: {e}")))?;

        Ok(Some(convert_manga(&entity.data)))
    }

    async fn chapters(&self, source_id: &str) -> Result<Vec<ChapterCandidate>, DomainError> {
        let url = format!(
            "{BASE_URL}/manga/{source_id}/feed?limit=500&translatedLanguage[]=en&order[chapter]=asc&includes[]=scanlation_group"
        );
        let resp = self
            .http
            .make_request(
                &url,
                RequestType::MangaDexFeed,
                None,
                CancellationToken::new(),
            )
            .await
            .map_err(|e| DomainError::source(e.to_string()))?;

        let feed: MangaDexFeedResponse = serde_json::from_str(&resp.body)
            .map_err(|e| DomainError::invalid(format!("MangaDex JSON: {e}")))?;

        Ok(feed
            .data
            .into_iter()
            .map(|ch| {
                let id = ch.id.clone();
                ChapterCandidate {
                    chapter_number: ch.attributes.chapter.unwrap_or_default(),
                    volume_number: ch.attributes.volume.and_then(|v| v.parse().ok()),
                    title: ch.attributes.title,
                    source_id: id.clone(),
                    website_url: ch
                        .attributes
                        .external_url
                        .or(Some(format!("https://mangadex.org/chapter/{id}"))),
                }
            })
            .collect())
    }

    async fn page_images(&self, chapter_source_id: &str) -> Result<Vec<String>, DomainError> {
        let url = format!("{BASE_URL}/at-home/server/{chapter_source_id}");
        let resp = self
            .http
            .make_request(
                &url,
                RequestType::MangaDexFeed,
                None,
                CancellationToken::new(),
            )
            .await
            .map_err(|e| DomainError::source(e.to_string()))?;

        let athome: MangaDexAtHomeResponse = serde_json::from_str(&resp.body)
            .map_err(|e| DomainError::invalid(format!("MangaDex JSON: {e}")))?;

        Ok(athome
            .chapter
            .data
            .into_iter()
            .map(|f| format!("{}/data/{}/{}", athome.base_url, athome.chapter.hash, f))
            .collect())
    }

    fn cover_url_for(&self, source_id: &str) -> Option<String> {
        Some(format!("https://mangadex.org/covers/{source_id}/cover.jpg"))
    }

    fn image_referrer(&self) -> Option<&str> {
        Some("https://mangadex.org")
    }
}

// ── Helper: extract UUID from MangaDex URL ──

fn extract_id_from_url(url: &str) -> Option<String> {
    let re = regex_lazy();
    re.captures(url).map(|caps| caps[1].to_string())
}

fn regex_lazy() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})")
            .unwrap()
    })
}

// ── Domain conversion ──

fn convert_manga(m: &MangaDexManga) -> MangaCandidate {
    let title = m
        .attributes
        .title
        .get("en")
        .or_else(|| m.attributes.title.values().next())
        .cloned()
        .unwrap_or_default();

    let description = m
        .attributes
        .description
        .get("en")
        .or_else(|| m.attributes.description.values().next())
        .map(|d| crate::utils::sanitize_for_json(d))
        .unwrap_or_default();

    let (cover_url, authors, tags, release_status, year) = extract_relations(m);

    MangaCandidate {
        name: title,
        description,
        cover_url,
        source_id: m.id.clone(),
        website_url: Some(format!("https://mangadex.org/title/{}", m.id)),
        release_status,
        authors,
        tags,
        year,
        original_language: m.attributes.original_language.clone(),
    }
}

fn extract_relations(
    m: &MangaDexManga,
) -> (
    String,
    Vec<String>,
    Vec<String>,
    Option<String>,
    Option<u32>,
) {
    let mut cover_url = String::new();
    let mut authors = Vec::new();
    let mut tags = Vec::new();
    let mut year: Option<u32> = None;

    for rel in &m.relationships {
        match rel.rel_type.as_str() {
            "cover_art" => {
                if let Some(ref attrs) = rel.attributes {
                    if let Some(file_name) = attrs.get("fileName").and_then(|v| v.as_str()) {
                        cover_url = format!("https://mangadex.org/covers/{}/{}", m.id, file_name);
                    }
                }
            }
            "author" => {
                if let Some(ref attrs) = rel.attributes {
                    if let Some(name) = attrs.get("name").and_then(|v| v.as_str()) {
                        authors.push(name.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(ref attr_tags) = m.attributes.tags {
        for tag_group in attr_tags {
            if let Some(tag_name) = tag_group.attributes.name.get("en") {
                tags.push(tag_name.clone());
            }
        }
    }

    if let Some(ref year_val) = m.attributes.year {
        year = Some(*year_val);
    }

    let release_status = m
        .attributes
        .status
        .as_deref()
        .map(|s| match s {
            "ongoing" => Some("Continuing".into()),
            "completed" => Some("Completed".into()),
            "hiatus" => Some("OnHiatus".into()),
            "cancelled" => Some("Cancelled".into()),
            _ => Some(s.to_string()),
        })
        .unwrap_or(None);

    (cover_url, authors, tags, release_status, year)
}

// ── MangaDex API types ──

#[derive(Debug, Deserialize)]
struct MangaDexSearchResponse {
    data: Vec<MangaDexManga>,
}

#[derive(Debug, Deserialize)]
struct MangaDexManga {
    id: String,
    attributes: MangaDexMangaAttributes,
    relationships: Vec<MangaDexRelationship>,
}

#[derive(Debug, Deserialize)]
struct MangaDexMangaAttributes {
    title: HashMap<String, String>,
    description: HashMap<String, String>,
    #[serde(default)]
    tags: Option<Vec<MangaDexTag>>,
    status: Option<String>,
    year: Option<u32>,
    #[serde(rename = "originalLanguage")]
    original_language: Option<String>,
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
    attributes: Option<HashMap<String, serde_json::Value>>,
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
    external_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MangaDexEntityResponse {
    data: MangaDexManga,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaDexAtHomeResponse {
    base_url: String,
    chapter: MangaDexAtHomeChapter,
}

#[derive(Debug, Deserialize)]
struct MangaDexAtHomeChapter {
    hash: String,
    data: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::http::mock::MockHttpClient;

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
            .expect_get(
                "mangadex.org/manga",
                include_str!("fixtures/mangadex_search_solo.json"),
            )
            .build();
        let source = MangaDexSource::new(Arc::new(mock));
        let results = source.search("solo").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Solo Leveling");
        assert_eq!(results[0].authors, vec!["Chugong"]);
        assert_eq!(results[0].tags, vec!["Action", "Fantasy"]);
        assert_eq!(results[0].year, Some(2018));
    }

    #[tokio::test]
    async fn get_chapters_parses_feed() {
        let mock = MockHttpClient::builder()
            .expect_get(
                "mangadex.org",
                include_str!("../../connectors/fixtures/mangadex_feed.json"),
            )
            .build();
        let source = MangaDexSource::new(Arc::new(mock));
        let chapters = source.chapters("abc").await.unwrap();
        assert!(!chapters.is_empty());
    }

    #[tokio::test]
    async fn get_chapter_image_urls_constructs_urls() {
        let mock = MockHttpClient::builder()
            .expect_get(
                "mangadex.org/at-home",
                include_str!("../../connectors/fixtures/mangadex_athome.json"),
            )
            .build();
        let source = MangaDexSource::new(Arc::new(mock));
        let urls = source.page_images("abc").await.unwrap();
        assert!(!urls.is_empty());
        assert!(urls[0].contains("https://"));
    }

    #[test]
    fn url_matches_mangadex() {
        let source = MangaDexSource::new(Arc::new(MockHttpClient::builder().build()));
        assert!(source.matches_url("https://mangadex.org/title/abc"));
        assert!(!source.matches_url("https://example.com"));
    }
}
