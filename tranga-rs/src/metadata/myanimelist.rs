use async_trait::async_trait;
use crate::api::dto::MetadataSearchResultDto;
use crate::http::{HttpClient, RequestType};
use crate::metadata::{MangaMetadataUpdate, MetadataFetcher};
use tokio_util::sync::CancellationToken;

pub struct MyAnimeListFetcher;

impl Default for MyAnimeListFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl MyAnimeListFetcher {
    pub fn new() -> Self {
        Self
    }
}

const JIKAN_BASE: &str = "https://api.jikan.moe/v4";

#[derive(Debug, serde::Deserialize)]
struct JikanSearchResponse {
    data: Vec<JikanMangaBrief>,
}

#[derive(Debug, serde::Deserialize)]
struct JikanMangaBrief {
    mal_id: i64,
    title: Option<String>,
    url: Option<String>,
    synopsis: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct JikanFullResponse {
    data: JikanMangaFull,
}

#[derive(Debug, serde::Deserialize)]
struct JikanMangaFull {
    #[allow(dead_code)]
    mal_id: i64,
    title: Option<String>,
    synopsis: Option<String>,
    titles: Vec<JikanTitle>,
    authors: Vec<JikanAuthor>,
}

#[derive(Debug, serde::Deserialize)]
struct JikanTitle {
    #[serde(rename = "type")]
    title_type: Option<String>,
    title: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct JikanAuthor {
    name: Option<String>,
}

#[async_trait]
impl MetadataFetcher for MyAnimeListFetcher {
    fn name(&self) -> &str { "MyAnimeList" }

    async fn search_by_manga_name(&self, name: &str, http: &dyn HttpClient) -> Result<Vec<MetadataSearchResultDto>, anyhow::Error> {
        let url = format!("{JIKAN_BASE}/manga?q={}&limit=10", urlencoding::encode(name));
        let resp = http.make_request(&url, RequestType::Default, None, CancellationToken::new()).await?;

        if resp.status == 429 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            return Ok(vec![]);
        }

        let data: JikanSearchResponse = serde_json::from_str(&resp.body)?;
        Ok(data.data.iter().filter_map(|m| {
            Some(MetadataSearchResultDto {
                identifier: m.mal_id.to_string(),
                title: m.title.clone()?,
                url: m.url.clone()?,
                description: m.synopsis.clone().unwrap_or_default(),
            })
        }).collect())
    }

    async fn update_metadata(&self, identifier: &str, http: &dyn HttpClient) -> Result<Option<MangaMetadataUpdate>, anyhow::Error> {
        let url = format!("{JIKAN_BASE}/manga/{identifier}/full");
        let resp = http.make_request(&url, RequestType::Default, None, CancellationToken::new()).await?;

        if resp.status == 429 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            return Ok(None);
        }

        let data: JikanFullResponse = serde_json::from_str(&resp.body)?;
        let manga = data.data;

        let alt_titles: Vec<(String, String)> = manga.titles.iter()
            .filter_map(|t| Some((t.title_type.clone()?, t.title.clone()?)))
            .collect();

        let authors: Vec<String> = manga.authors.iter()
            .filter_map(|a| a.name.clone())
            .collect();

        Ok(Some(MangaMetadataUpdate {
            name: manga.title.unwrap_or_default(),
            description: manga.synopsis.unwrap_or_default(),
            alt_titles,
            authors,
        }))
    }
}
