use async_trait::async_trait;
use crate::api::dto::MetadataSearchResultDto;
use crate::http::HttpClient;

#[async_trait]
pub trait MetadataFetcher: Send + Sync {
    fn name(&self) -> &str;
    async fn search_by_manga_name(&self, name: &str, http: &dyn HttpClient) -> Result<Vec<MetadataSearchResultDto>, anyhow::Error>;
    async fn update_metadata(&self, identifier: &str, http: &dyn HttpClient) -> Result<Option<MangaMetadataUpdate>, anyhow::Error>;
}

#[derive(Debug, Clone)]
pub struct MangaMetadataUpdate {
    pub name: String,
    pub description: String,
    pub alt_titles: Vec<(String, String)>,
    pub authors: Vec<String>,
}

pub mod myanimelist;
pub use myanimelist::MyAnimeListFetcher;
