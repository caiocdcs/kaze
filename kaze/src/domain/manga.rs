use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::error::DomainError;

pub type DomainResult<T> = Result<T, DomainError>;

// ── Value Objects ──

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseStatus {
    Continuing,
    Completed,
    OnHiatus,
    Cancelled,
}

impl ReleaseStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Continuing => "Continuing",
            Self::Completed => "Completed",
            Self::OnHiatus => "OnHiatus",
            Self::Cancelled => "Cancelled",
        }
    }
}

impl From<&str> for ReleaseStatus {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "completed" | "complete" => Self::Completed,
            "hiatus" | "onhiatus" => Self::OnHiatus,
            "cancelled" | "canceled" => Self::Cancelled,
            _ => Self::Continuing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorMapping {
    pub id: String,
    pub connector_name: String,
    pub site_id: String,
    pub website_url: Option<String>,
    pub use_for_download: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AltTitle {
    pub language: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub provider: String,
    pub url: String,
}

// ── Entity ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manga {
    pub id: String,
    pub name: String,
    pub description: String,
    pub cover_url: String,
    pub release_status: ReleaseStatus,
    pub year: Option<u32>,
    pub original_language: Option<String>,

    // Relations
    pub connectors: Vec<ConnectorMapping>,
    pub authors: Vec<Author>,
    pub tags: Vec<String>,
    pub links: Vec<Link>,
    pub alt_titles: Vec<AltTitle>,

    // Library assignment
    pub library_id: Option<String>,
    pub ignore_chapters_before: f64,
}

impl Manga {
    pub fn is_downloading(&self) -> bool {
        self.connectors.iter().any(|c| c.use_for_download)
    }

    pub fn connectors_for_download(&self) -> impl Iterator<Item = &ConnectorMapping> {
        self.connectors.iter().filter(|c| c.use_for_download)
    }
}

// ── Repository Trait ──

#[async_trait]
pub trait MangaRepository: Send + Sync {
    async fn list(&self) -> DomainResult<Vec<Manga>>;
    async fn list_downloading(&self) -> DomainResult<Vec<Manga>>;
    async fn find_by_id(&self, id: &str) -> DomainResult<Option<Manga>>;
    async fn find_by_name(&self, name: &str) -> DomainResult<Option<String>>;
    async fn list_names(&self) -> DomainResult<Vec<(String, String)>>;
    async fn save(&self, manga: &Manga) -> DomainResult<()>;
    async fn delete(&self, id: &str) -> DomainResult<bool>;
    async fn set_library(&self, manga_id: &str, library_id: Option<&str>) -> DomainResult<()>;
    async fn set_ignore_before(&self, manga_id: &str, threshold: f64) -> DomainResult<()>;
    async fn set_connector_download(
        &self,
        manga_id: &str,
        connector: &str,
        enabled: bool,
    ) -> DomainResult<bool>;

    // Connector mappings for a manga
    async fn add_connector(
        &self,
        manga_id: &str,
        connector: &str,
        site_id: &str,
        website_url: Option<&str>,
    ) -> DomainResult<ConnectorMapping>;
    async fn get_connector(
        &self,
        manga_id: &str,
        connector: &str,
    ) -> DomainResult<Option<ConnectorMapping>>;

    // Tags + Authors
    async fn upsert_author(&self, name: &str) -> DomainResult<String>;
    async fn link_author(&self, manga_id: &str, author_id: &str) -> DomainResult<()>;
    async fn upsert_tag(&self, name: &str) -> DomainResult<()>;
    async fn link_tag(&self, manga_id: &str, tag: &str) -> DomainResult<()>;
    async fn upsert_alt_title(&self, manga_id: &str, lang: &str, title: &str) -> DomainResult<()>;
    async fn upsert_link(&self, manga_id: &str, provider: &str, url: &str) -> DomainResult<()>;
    async fn merge(&self, source_id: &str, target_id: &str) -> DomainResult<()>;

    // Lookups
    async fn by_tag(&self, tag: &str) -> DomainResult<Vec<Manga>>;
    async fn by_author(&self, author_id: &str) -> DomainResult<Vec<Manga>>;

    // Bulk ops
    async fn recheck_chapters_for_manga(&self, manga_id: Option<&str>) -> DomainResult<i64>;
}
