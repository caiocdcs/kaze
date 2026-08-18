use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::error::DomainError;

pub type DomainResult<T> = Result<T, DomainError>;

// ── Candidate types (returned by source, not persisted) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MangaCandidate {
    pub name: String,
    pub description: String,
    pub cover_url: String,
    pub source_id: String,
    pub website_url: Option<String>,
    pub release_status: Option<String>,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
    pub year: Option<u32>,
    pub original_language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterCandidate {
    pub chapter_number: String,
    pub volume_number: Option<i32>,
    pub title: Option<String>,
    pub source_id: String,
    pub website_url: Option<String>,
}

// ── Connector Trait ──

#[async_trait]
pub trait MangaSource: Send + Sync {
    fn name(&self) -> &str;

    /// Search for manga by name. Returns candidates to be reviewed and added.
    async fn search(&self, query: &str) -> DomainResult<Vec<MangaCandidate>>;

    /// Get detailed info for a manga from its URL (used for URL import).
    async fn from_url(&self, url: &str) -> DomainResult<Option<MangaCandidate>>;

    /// Get chapter list for a manga, given its source-side ID.
    async fn chapters(&self, source_id: &str) -> DomainResult<Vec<ChapterCandidate>>;

    /// Get page image URLs for a specific chapter.
    async fn page_images(&self, chapter_source_id: &str) -> DomainResult<Vec<String>>;

    /// Check if a URL belongs to this source.
    fn matches_url(&self, url: &str) -> bool;

    /// Optional: cover URL for a specific manga on this source.
    fn cover_url_for(&self, _source_id: &str) -> Option<String> {
        None
    }

    /// Referrer to send when downloading images from this source.
    fn image_referrer(&self) -> Option<&str> {
        None
    }
}
