use std::sync::Arc;

use crate::domain::chapter::{Chapter, ChapterFilter, ChapterRepository};
use crate::domain::error::DomainError;
use crate::domain::manga::ConnectorMapping;

pub type AppResult<T> = Result<T, DomainError>;

pub struct ChapterService {
    repo: Arc<dyn ChapterRepository>,
}

impl ChapterService {
    pub fn new(repo: Arc<dyn ChapterRepository>) -> Self {
        Self { repo }
    }

    pub async fn get(&self, id: &str) -> AppResult<Chapter> {
        self.repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::not_found("Chapter", id))
    }

    pub async fn list_by_manga(
        &self,
        manga_id: &str,
        filter: &ChapterFilter,
    ) -> AppResult<Vec<Chapter>> {
        self.repo.list_by_manga(manga_id, filter).await
    }

    pub async fn delete(&self, id: &str) -> AppResult<bool> {
        self.repo.delete(id).await
    }

    pub async fn recheck(&self, chapter_id: &str) -> AppResult<i64> {
        self.repo.recheck_chapter(chapter_id).await
    }

    pub async fn get_mapping(
        &self,
        chapter_id: &str,
        connector: &str,
    ) -> AppResult<ConnectorMapping> {
        self.repo
            .get_connector(chapter_id, connector)
            .await?
            .ok_or_else(|| {
                DomainError::not_found("ConnectorMapping", format!("{chapter_id}/{connector}"))
            })
    }

    pub async fn delete_mapping(&self, mapping_id: &str) -> AppResult<bool> {
        self.repo.delete_connector_mapping(mapping_id).await
    }
}
