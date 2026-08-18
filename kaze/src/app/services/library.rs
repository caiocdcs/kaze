use std::sync::Arc;

use crate::domain::error::DomainError;
use crate::domain::library::{FileLibrary, LibraryRepository};
use crate::id::EntityId;

pub type AppResult<T> = Result<T, DomainError>;

pub struct LibraryService {
    repo: Arc<dyn LibraryRepository>,
}

impl LibraryService {
    pub fn new(repo: Arc<dyn LibraryRepository>) -> Self {
        Self { repo }
    }

    pub async fn list(&self) -> AppResult<Vec<FileLibrary>> {
        self.repo.list().await
    }

    pub async fn get(&self, id: &str) -> AppResult<FileLibrary> {
        self.repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::not_found("FileLibrary", id))
    }

    pub async fn create(&self, base_path: &str, name: &str) -> AppResult<FileLibrary> {
        let lib = FileLibrary {
            id: crate::id::FileLibraryId::new().to_string(),
            base_path: base_path.to_string(),
            library_name: name.to_string(),
        };
        self.repo.save(&lib).await?;
        Ok(lib)
    }

    pub async fn update(&self, id: &str, base_path: &str, name: &str) -> AppResult<bool> {
        self.get(id).await?;
        self.repo.update(id, base_path, name).await
    }

    pub async fn delete(&self, id: &str) -> AppResult<bool> {
        self.get(id).await?;
        self.repo.delete(id).await
    }
}
