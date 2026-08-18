use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::domain::chapter::ChapterRepository;
use crate::domain::library::LibraryRepository;
use crate::domain::manga::MangaRepository;
use crate::http::HttpClient;
use crate::id::EntityId;
use crate::infra::connectors::SourceRegistry;
use crate::infra::http::HttpDownloader;

/// Pool of download workers. Dropping the pool cancels all workers.
pub struct WorkerPool {
    handles: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    /// Start `count` download workers.
    pub fn start(
        count: usize,
        chapter_repo: Arc<dyn ChapterRepository>,
        manga_repo: Arc<dyn MangaRepository>,
        library_repo: Arc<dyn LibraryRepository>,
        sources: Arc<SourceRegistry>,
        http: Arc<HttpDownloader>,
        config: Arc<Config>,
    ) -> Self {
        let handles: Vec<_> = (0..count)
            .map(|id| {
                tokio::spawn(download_loop(
                    id,
                    chapter_repo.clone(),
                    manga_repo.clone(),
                    library_repo.clone(),
                    sources.clone(),
                    http.clone(),
                    config.clone(),
                ))
            })
            .collect();
        Self { handles }
    }

    /// Number of running workers.
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// Wait for all workers to finish (they run until shutdown signal).
    pub async fn shutdown(self) {
        for h in self.handles {
            let _ = h.await;
        }
    }
}

async fn download_loop(
    id: usize,
    chapter_repo: Arc<dyn ChapterRepository>,
    manga_repo: Arc<dyn MangaRepository>,
    library_repo: Arc<dyn LibraryRepository>,
    sources: Arc<SourceRegistry>,
    http: Arc<HttpDownloader>,
    config: Arc<Config>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));
    loop {
        interval.tick().await;
        let chapter = match chapter_repo.claim_next_pending().await {
            Ok(Some(ch)) => ch,
            Ok(None) => continue,
            Err(e) => {
                error!("Worker {id}: claim error: {e}");
                continue;
            }
        };

        info!(
            "Worker {id}: downloading chapter {} ({})",
            chapter.id, chapter.chapter_number
        );

        match download_single(
            &chapter,
            &*manga_repo,
            &*library_repo,
            &*sources,
            &*http,
            &config,
        )
        .await
        {
            Ok(file_name) => {
                if let Err(e) = chapter_repo.mark_completed(&chapter.id, &file_name).await {
                    error!("Worker {id}: failed to mark completed: {e}");
                } else {
                    info!(
                        "Worker {id}: completed chapter {} -> {file_name}",
                        chapter.id
                    );
                }
            }
            Err(e) => {
                warn!("Worker {id}: chapter {} failed: {e}", chapter.id);
                let _ = chapter_repo.mark_failed(&chapter.id).await;
            }
        }
    }
}

async fn download_single(
    chapter: &crate::domain::chapter::Chapter,
    manga_repo: &dyn MangaRepository,
    library_repo: &dyn LibraryRepository,
    sources: &SourceRegistry,
    http: &HttpDownloader,
    config: &Config,
) -> Result<String, anyhow::Error> {
    use anyhow::Context;

    // Get manga
    let manga = manga_repo
        .find_by_id(&chapter.manga_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("manga not found: {}", chapter.manga_id))?;

    // Get library path
    let library_id = manga
        .library_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("manga {} has no library", manga.id))?;
    let library = library_repo
        .find_by_id(library_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("library not found: {library_id}"))?;

    // Find the connector mapping with download enabled
    let mapping = manga
        .connectors_for_download()
        .next()
        .ok_or_else(|| anyhow::anyhow!("manga {} has no download-enabled connector", manga.id))?;

    // Get source
    let source = sources
        .get(&mapping.connector_name)
        .ok_or_else(|| anyhow::anyhow!("source not found: {}", mapping.connector_name))?;

    // Get chapter-level connector site ID from chapter's connectors
    let chapter_mapping = chapter
        .connectors
        .iter()
        .find(|c| c.connector_name == mapping.connector_name)
        .ok_or_else(|| anyhow::anyhow!("chapter has no connector for {}", mapping.connector_name))?;

    // Get page images from source
    let image_urls = source
        .page_images(&chapter_mapping.site_id)
        .await
        .map_err(|e| anyhow::anyhow!("page_images failed: {e}"))?;

    if image_urls.is_empty() {
        return Err(anyhow::anyhow!("no image URLs for chapter {}", chapter.id));
    }

    // Build output path
    let manga_dir = std::path::Path::new(&library.base_path)
        .join(crate::utils::clean_name_for_windows(&manga.name));
    tokio::fs::create_dir_all(&manga_dir).await?;

    // Generate filename from naming scheme
    let settings = config.settings.read().await;
    let naming_ctx = crate::naming::ChapterNamingContext {
        manga_name: manga.name.clone(),
        volume_number: chapter.volume,
        chapter_number: chapter.chapter_number.clone(),
        chapter_title: chapter.title.clone(),
        author_name: manga.authors.first().map(|a| a.name.clone()),
        year: manga.year,
        always_include_volume: config.always_include_volume_in_filename,
    };
    let file_name =
        crate::naming::resolve_naming_scheme(&settings.chapter_naming_scheme, &naming_ctx);
    let archive_path = manga_dir.join(&file_name);
    drop(settings);

    // Generate ComicInfo.xml
    let comic_info = if config.create_comicinfo_xml {
        Some(crate::comicinfo::generate_comic_info_xml(
            &crate::comicinfo::ComicInfoData {
                chapter_number: chapter.chapter_number.clone(),
                title: chapter.title.clone(),
                tags: manga.tags.clone(),
                volume_number: chapter.volume,
                authors: manga.authors.iter().map(|a| a.name.clone()).collect(),
                original_language: manga.original_language.clone(),
                description: if manga.description.is_empty() {
                    String::new()
                } else {
                    manga.description.clone()
                },
            },
        ))
    } else {
        None
    };

    // Create CBZ
    {
        let file = tokio::fs::File::create(&archive_path).await?;
        let mut zip = async_zip::tokio::write::ZipFileWriter::with_tokio(file);

        if let Some(ref xml) = comic_info {
            let entry = async_zip::ZipEntryBuilder::new(
                "ComicInfo.xml".into(),
                async_zip::Compression::Deflate,
            );
            zip.write_entry_whole(entry, xml.as_bytes()).await?;
        }

        let compression = config.settings.read().await.image_compression;
        let black_white = config.settings.read().await.black_white_images;
        let referrer = source.image_referrer();

        for (i, url) in image_urls.iter().enumerate() {
            let bytes = http
                .download_image(url, referrer, tokio_util::sync::CancellationToken::new())
                .await
                .with_context(|| format!("failed to download: {url}"))?
                .ok_or_else(|| anyhow::anyhow!("no data from: {url}"))?;

            let processed = crate::image::process_image(&bytes, black_white, compression);

            let entry = async_zip::ZipEntryBuilder::new(
                format!("{i}.jpg").into(),
                async_zip::Compression::Deflate,
            );
            zip.write_entry_whole(entry, &processed).await?;
        }

        zip.close().await?;
    }

    info!("Created archive: {}", archive_path.display());
    Ok(file_name)
}

// ── Periodic tasks ──

pub fn start_periodic(
    chapter_repo: Arc<dyn ChapterRepository>,
    manga_repo: Arc<dyn MangaRepository>,
    sources: Arc<SourceRegistry>,
    _http: Arc<HttpDownloader>,
    config: Arc<Config>,
) {
    // Check for new chapters every 3 hours
    let chapter_repo_c = chapter_repo.clone();
    let manga_repo_c = manga_repo.clone();
    let sources_c = sources.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3 * 3600));
        loop {
            interval.tick().await;
            info!("Periodic: checking for new chapters");
            if let Err(e) =
                check_new_chapters(&*manga_repo_c, &*chapter_repo_c, &*sources_c, &config).await
            {
                error!("Periodic: check new chapters failed: {e}");
            }
        }
    });
}

async fn check_new_chapters(
    manga_repo: &dyn MangaRepository,
    chapter_repo: &dyn ChapterRepository,
    sources: &SourceRegistry,
    config: &Config,
) -> Result<(), anyhow::Error> {
    let downloading = manga_repo.list_downloading().await?;
    let _language = config.settings.read().await.download_language.clone();

    for manga in &downloading {
        for mapping in manga.connectors_for_download() {
            let source = match sources.get(&mapping.connector_name) {
                Some(s) => s,
                None => continue,
            };

            let candidates = match source.chapters(&mapping.site_id).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to get chapters for {}: {e}", manga.name);
                    continue;
                }
            };

            let existing = chapter_repo
                .list_by_manga(&manga.id, &Default::default())
                .await?;
            let existing_nums: std::collections::HashSet<String> =
                existing.iter().map(|c| c.chapter_number.clone()).collect();

            for cand in candidates {
                if !existing_nums.contains(&cand.chapter_number) {
                    let ch_id = crate::id::ChapterId::new().to_string();
                    let ch = crate::domain::chapter::Chapter {
                        id: ch_id,
                        manga_id: manga.id.clone(),
                        volume: cand.volume_number,
                        chapter_number: cand.chapter_number.clone(),
                        title: cand.title,
                        connectors: vec![],
                        status: crate::domain::chapter::ChapterStatus::Pending,
                        file_name: None,
                    };
                    chapter_repo.save(&ch).await?;
                    chapter_repo
                        .add_connector(
                            &ch.id,
                            &mapping.connector_name,
                            &cand.source_id,
                            cand.website_url.as_deref(),
                        )
                        .await?;
                    info!("New chapter: {} Ch.{}", manga.name, cand.chapter_number);
                }
            }
        }
    }

    Ok(())
}
