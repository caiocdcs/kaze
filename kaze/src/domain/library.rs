use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::error::DomainError;

pub type DomainResult<T> = Result<T, DomainError>;

// ── Entity ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileLibrary {
    pub id: String,
    pub base_path: String,
    pub library_name: String,
}

// ── Repository Trait ──

#[async_trait]
pub trait LibraryRepository: Send + Sync {
    async fn list(&self) -> DomainResult<Vec<FileLibrary>>;
    async fn find_by_id(&self, id: &str) -> DomainResult<Option<FileLibrary>>;
    async fn find_default(&self) -> DomainResult<Option<FileLibrary>>;
    async fn save(&self, library: &FileLibrary) -> DomainResult<()>;
    async fn update(&self, id: &str, base_path: &str, name: &str) -> DomainResult<bool>;
    async fn delete(&self, id: &str) -> DomainResult<bool>;
}
