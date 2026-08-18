use std::sync::Arc;

use crate::domain::chapter::{ChapterFilter, ChapterRepository};
use crate::domain::error::DomainError;
use crate::domain::library::LibraryRepository;
use crate::domain::manga::{ConnectorMapping, Manga, MangaRepository, ReleaseStatus};
use crate::domain::manga_source::{MangaCandidate, MangaSource};
use crate::id::*;
use crate::infra::connectors::SourceRegistry;

pub type AppResult<T> = Result<T, DomainError>;

pub struct MangaService {
    manga_repo: Arc<dyn MangaRepository>,
    chapter_repo: Arc<dyn ChapterRepository>,
    library_repo: Arc<dyn LibraryRepository>,
    sources: Arc<SourceRegistry>,
}

impl MangaService {
    pub fn new(
        manga_repo: Arc<dyn MangaRepository>,
        chapter_repo: Arc<dyn ChapterRepository>,
        library_repo: Arc<dyn LibraryRepository>,
        sources: Arc<SourceRegistry>,
    ) -> Self {
        Self {
            manga_repo,
            chapter_repo,
            library_repo,
            sources,
        }
    }

    // ── Queries ──

    pub async fn list(
        &self,
        downloading: Option<bool>,
        tag: Option<String>,
    ) -> AppResult<Vec<Manga>> {
        match (downloading, tag) {
            (Some(true), _) => self.manga_repo.list_downloading().await,
            (_, Some(t)) => self.manga_repo.by_tag(&t).await,
            _ => self.manga_repo.list().await,
        }
    }

    pub async fn get(&self, id: &str) -> AppResult<Manga> {
        self.manga_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::not_found("Manga", id))
    }

    pub async fn delete(&self, id: &str) -> AppResult<bool> {
        self.manga_repo.delete(id).await
    }

    pub async fn similar(&self, manga_id: &str) -> AppResult<Vec<String>> {
        let manga = self.get(manga_id).await?;
        let all = self.manga_repo.list_names().await?;
        let threshold = 0.8;
        Ok(all
            .into_iter()
            .filter(|(key, _)| key != manga_id)
            .filter(|(_, name)| strsim::normalized_levenshtein(&manga.name, name) > threshold)
            .map(|(key, _)| key)
            .collect())
    }

    // ── Search & Add ──

    pub async fn search_and_add(&self, connector_name: &str, query: &str) -> AppResult<Vec<Manga>> {
        let source = self
            .sources
            .get(connector_name)
            .ok_or_else(|| DomainError::not_found("Source", connector_name))?;

        let candidates = source.search(query).await?;
        let mut results = Vec::new();
        for cand in candidates {
            results.push(self.add_candidate(&cand, &source).await?);
        }
        Ok(results)
    }

    pub async fn search_by_url(&self, url: &str) -> AppResult<Manga> {
        let source = self
            .sources
            .find_by_url(url)
            .ok_or_else(|| DomainError::invalid("No source matches this URL"))?;

        let candidate = source
            .from_url(url)
            .await?
            .ok_or_else(|| DomainError::invalid("Could not fetch manga from URL"))?;

        self.add_candidate(&candidate, &source).await
    }

    pub async fn search_on_source(
        &self,
        manga_id: &str,
        source_name: &str,
    ) -> AppResult<Vec<MangaCandidate>> {
        let manga = self.get(manga_id).await?;
        let source = self
            .sources
            .get(source_name)
            .ok_or_else(|| DomainError::not_found("Source", source_name))?;
        Ok(source.search(&manga.name).await?)
    }

    // ── Mutations ──

    pub async fn merge(&self, source_id: &str, target_id: &str) -> AppResult<()> {
        if source_id == target_id {
            return Err(DomainError::invalid("Cannot merge manga into itself"));
        }
        self.manga_repo
            .find_by_id(source_id)
            .await?
            .ok_or_else(|| DomainError::not_found("Manga", source_id))?;
        self.manga_repo
            .find_by_id(target_id)
            .await?
            .ok_or_else(|| DomainError::not_found("Manga", target_id))?;
        self.manga_repo.merge(source_id, target_id).await
    }

    pub async fn recheck(&self, manga_id: &str) -> AppResult<i64> {
        self.manga_repo
            .recheck_chapters_for_manga(Some(manga_id))
            .await
    }

    pub async fn set_ignore_before(&self, manga_id: &str, threshold: f64) -> AppResult<()> {
        self.manga_repo.set_ignore_before(manga_id, threshold).await
    }

    pub async fn set_download(
        &self,
        manga_id: &str,
        connector_name: &str,
        enabled: bool,
    ) -> AppResult<bool> {
        self.manga_repo
            .set_connector_download(manga_id, connector_name, enabled)
            .await
    }

    pub async fn move_to_library(&self, manga_id: &str, library_id: &str) -> AppResult<()> {
        let manga = self.get(manga_id).await?;
        self.library_repo
            .find_by_id(library_id)
            .await?
            .ok_or_else(|| DomainError::not_found("FileLibrary", library_id))?;

        // Move files if library changed
        if let Some(ref old_id) = manga.library_id {
            if old_id != library_id {
                if let Ok(Some(old_lib)) = self.library_repo.find_by_id(old_id).await {
                    let old_path = std::path::Path::new(&old_lib.base_path).join(&manga.name);
                    if let Ok(new_lib) = self.library_repo.find_by_id(library_id).await {
                        if let Some(new_lib) = new_lib {
                            let new_path =
                                std::path::Path::new(&new_lib.base_path).join(&manga.name);
                            if old_path.exists() && old_path != new_path {
                                let _ = tokio::fs::create_dir_all(new_path.parent().unwrap()).await;
                                let _ = tokio::fs::rename(&old_path, &new_path).await;
                            }
                        }
                    }
                }
            }
        }

        self.manga_repo
            .set_library(manga_id, Some(library_id))
            .await
    }

    pub async fn add_to_library(&self, manga_id: &str, library_id: &str) -> AppResult<()> {
        let manga = self.get(manga_id).await?;
        self.library_repo
            .find_by_id(library_id)
            .await?
            .ok_or_else(|| DomainError::not_found("FileLibrary", library_id))?;

        if manga.library_id.as_deref() != Some(library_id) {
            self.manga_repo
                .set_library(manga_id, Some(library_id))
                .await?;
        }
        Ok(())
    }

    pub async fn get_connector_mapping(
        &self,
        manga_id: &str,
        connector: &str,
    ) -> AppResult<ConnectorMapping> {
        self.manga_repo
            .get_connector(manga_id, connector)
            .await?
            .ok_or_else(|| {
                DomainError::not_found("ConnectorMapping", format!("{manga_id}/{connector}"))
            })
    }

    // ── Chapter queries (delegated) ──

    pub async fn list_chapters(
        &self,
        manga_id: &str,
        filter: &ChapterFilter,
    ) -> AppResult<Vec<crate::domain::chapter::Chapter>> {
        self.chapter_repo.list_by_manga(manga_id, filter).await
    }

    pub async fn latest_chapter(
        &self,
        manga_id: &str,
    ) -> AppResult<crate::domain::chapter::Chapter> {
        self.chapter_repo
            .latest_by_manga(manga_id)
            .await?
            .ok_or_else(|| DomainError::not_found("Chapter", manga_id))
    }

    pub async fn latest_downloaded(
        &self,
        manga_id: &str,
    ) -> AppResult<crate::domain::chapter::Chapter> {
        self.chapter_repo
            .latest_downloaded(manga_id)
            .await?
            .ok_or_else(|| DomainError::not_found("Chapter", manga_id))
    }

    // ── Internals ──

    async fn add_candidate(
        &self,
        cand: &MangaCandidate,
        source: &Arc<dyn MangaSource>,
    ) -> AppResult<Manga> {
        // Check if already tracked
        if let Some(existing_id) = self.manga_repo.find_by_name(&cand.name).await? {
            let existing = self.manga_repo.find_by_id(&existing_id).await?.unwrap();
            // Add connector mapping if not already present
            let has_connector = existing.connectors.iter().any(|c| {
                c.connector_name.eq_ignore_ascii_case(source.name()) && c.site_id == cand.source_id
            });
            if !has_connector {
                self.manga_repo
                    .add_connector(
                        &existing_id,
                        source.name(),
                        &cand.source_id,
                        cand.website_url.as_deref(),
                    )
                    .await?;
            }
            return Ok(existing);
        }

        // New manga
        let manga_id = MangaId::new().to_string();

        let manga = Manga {
            id: manga_id.clone(),
            name: cand.name.clone(),
            description: cand.description.clone(),
            cover_url: cand.cover_url.clone(),
            release_status: cand
                .release_status
                .as_deref()
                .map(ReleaseStatus::from)
                .unwrap_or(ReleaseStatus::Continuing),
            year: cand.year,
            original_language: cand.original_language.clone(),
            connectors: vec![],
            authors: vec![],
            tags: vec![],
            links: vec![],
            alt_titles: vec![],
            library_id: None,
            ignore_chapters_before: 0.0,
        };

        self.manga_repo.save(&manga).await?;

        // Authors
        for author_name in &cand.authors {
            let author_key = self.manga_repo.upsert_author(author_name).await?;
            self.manga_repo.link_author(&manga_id, &author_key).await?;
        }

        // Tags
        for tag in &cand.tags {
            self.manga_repo.upsert_tag(tag).await?;
            self.manga_repo.link_tag(&manga_id, tag).await?;
        }

        // Connector mapping
        self.manga_repo
            .add_connector(
                &manga_id,
                source.name(),
                &cand.source_id,
                cand.website_url.as_deref(),
            )
            .await?;

        // Assign to default library if one exists
        if let Ok(Some(lib)) = self.library_repo.find_default().await {
            self.manga_repo
                .set_library(&manga_id, Some(&lib.id))
                .await?;
        }

        self.manga_repo
            .find_by_id(&manga_id)
            .await?
            .ok_or_else(|| DomainError::not_found("Manga", &manga_id))
    }
}
