use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::error::DomainError;
use crate::domain::manga::ConnectorMapping;

pub type DomainResult<T> = Result<T, DomainError>;

// ── Value Objects ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChapterStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
}

// ── Entity ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: String,
    pub manga_id: String,
    pub volume: Option<i32>,
    pub chapter_number: String,
    pub title: Option<String>,
    pub connectors: Vec<ConnectorMapping>,
    pub status: ChapterStatus,
    pub file_name: Option<String>,
}

impl Chapter {
    pub fn is_downloaded(&self) -> bool {
        self.status == ChapterStatus::Completed
    }
}

// ── Repository Trait ──

#[async_trait]
pub trait ChapterRepository: Send + Sync {
    async fn find_by_id(&self, id: &str) -> DomainResult<Option<Chapter>>;
    async fn list_by_manga(
        &self,
        manga_id: &str,
        filter: &ChapterFilter,
    ) -> DomainResult<Vec<Chapter>>;
    async fn count_by_manga(&self, manga_id: &str, filter: &ChapterFilter) -> DomainResult<i64>;
    async fn latest_by_manga(&self, manga_id: &str) -> DomainResult<Option<Chapter>>;
    async fn latest_downloaded(&self, manga_id: &str) -> DomainResult<Option<Chapter>>;
    async fn save(&self, chapter: &Chapter) -> DomainResult<()>;
    async fn delete(&self, id: &str) -> DomainResult<bool>;

    /// Claim the next pending chapter atomically (returns None if nothing pending).
    /// Sets status to Downloading so no other worker claims it.
    async fn claim_next_pending(&self) -> DomainResult<Option<Chapter>>;

    /// Mark a chapter as completed after successful download.
    async fn mark_completed(&self, id: &str, file_name: &str) -> DomainResult<()>;

    /// Mark a chapter as failed (for retry later).
    async fn mark_failed(&self, id: &str) -> DomainResult<()>;

    /// Reset failed chapters back to pending (for retry queue).
    async fn reset_failed(&self, older_than_secs: u64) -> DomainResult<u64>;

    // Connector mappings
    async fn add_connector(
        &self,
        chapter_id: &str,
        connector: &str,
        site_id: &str,
        website_url: Option<&str>,
    ) -> DomainResult<ConnectorMapping>;
    async fn get_connector(
        &self,
        chapter_id: &str,
        connector: &str,
    ) -> DomainResult<Option<ConnectorMapping>>;
    async fn delete_connector_mapping(&self, mapping_id: &str) -> DomainResult<bool>;

    /// Re-detect chapters: deletes stale connector mappings for re-fetch.
    async fn recheck_chapter(&self, chapter_id: &str) -> DomainResult<i64>;

    // Bulk
    async fn pending_count(&self) -> DomainResult<i64>;
    async fn get_missing_downloads(&self) -> DomainResult<Vec<ConnectorMapping>>;
}

#[derive(Debug, Clone, Default)]
pub struct ChapterFilter {
    pub downloaded: Option<bool>,
    pub name: Option<String>,
    pub volume: Option<i32>,
    pub chapter_number: Option<String>,
}
