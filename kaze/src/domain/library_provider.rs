use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::error::DomainError;

pub type DomainResult<T> = Result<T, DomainError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookInfo {
    pub id: String,
    pub file_name: String,
    pub chapter_number: String,
}

/// A library provider (Komga, Kavita) that receives downloaded chapters.
#[async_trait]
pub trait LibraryProvider: Send + Sync {
    fn name(&self) -> &str;

    /// Test connection to the library server.
    async fn test_connection(&self) -> DomainResult<bool>;

    /// Trigger a library scan so new chapters appear.
    async fn scan_libraries(&self) -> DomainResult<()>;

    /// Find a series by name, returns the best match.
    async fn find_series(&self, name: &str) -> DomainResult<Option<SeriesInfo>>;

    /// List all books in a series.
    async fn books_in_series(&self, series_id: &str) -> DomainResult<Vec<BookInfo>>;

    /// Find which chapters already exist in the library for a given manga name.
    /// Returns (chapter_number, file_name) pairs that already exist.
    async fn existing_chapters(
        &self,
        manga_name: &str,
        chapters: &[(String, String)],
    ) -> DomainResult<Vec<(String, String)>>;
}
