use crate::app_state::AppState;
use crate::db::chapter_repo::ChapterRepo;
use crate::db::manga_repo::MangaRepo;
use crate::db::action_repo::ActionRepo;
use crate::db::notification_repo::NotificationRepo;
use crate::id::{EntityId, WorkerId, ChapterId, NotificationId, ActionId, ConnectorMappingId};
use crate::image::cover_cache::CoverCache;
use crate::naming::ChapterNamingContext;
use crate::comicinfo::{self, ComicInfoData};
use crate::utils::clean_name_for_windows;
use crate::workers::{WorkerEntry, WorkerPayload, WorkerState};
use crate::library_connectors::LibraryConnector;

use tracing::{info, warn};
use anyhow::{anyhow, Context};

pub struct DownloadChapterWorker;

impl DownloadChapterWorker {
    pub fn create(connector_mapping_key: String) -> WorkerEntry {
        WorkerEntry {
            id: WorkerId::new(),
            worker_type: "DownloadChapter".into(),
            state: WorkerState::Created,
            depends_on: vec![],
            is_periodic: false,
            interval_secs: None,
            last_execution: None,
            payload: Some(WorkerPayload::DownloadChapter { connector_mapping_key }),
        }
    }
}

pub struct DownloadCoverWorker;

impl DownloadCoverWorker {
    pub fn create(connector_mapping_key: String) -> WorkerEntry {
        WorkerEntry {
            id: WorkerId::new(),
            worker_type: "DownloadCover".into(),
            state: WorkerState::Created,
            depends_on: vec![],
            is_periodic: false,
            interval_secs: None,
            last_execution: None,
            payload: Some(WorkerPayload::DownloadCover { connector_mapping_key }),
        }
    }
}

pub struct RetrieveChaptersWorker;

impl RetrieveChaptersWorker {
    pub fn create(connector_mapping_key: String, language: String) -> WorkerEntry {
        WorkerEntry {
            id: WorkerId::new(),
            worker_type: "RetrieveChapters".into(),
            state: WorkerState::Created,
            depends_on: vec![],
            is_periodic: false,
            interval_secs: None,
            last_execution: None,
            payload: Some(WorkerPayload::RetrieveChapters {
                connector_mapping_key,
                language,
            }),
        }
    }
}

pub struct RefreshLibrariesWorker;

impl RefreshLibrariesWorker {
    pub fn create() -> WorkerEntry {
        WorkerEntry {
            id: WorkerId::new(),
            worker_type: "RefreshLibraries".into(),
            state: WorkerState::Created,
            depends_on: vec![],
            is_periodic: false,
            interval_secs: None,
            last_execution: None,
            payload: Some(WorkerPayload::RefreshLibraries),
        }
    }
}

pub struct MoveFileWorker;

impl MoveFileWorker {
    pub fn create(from: String, to: String) -> WorkerEntry {
        WorkerEntry {
            id: WorkerId::new(),
            worker_type: "MoveFile".into(),
            state: WorkerState::Created,
            depends_on: vec![],
            is_periodic: false,
            interval_secs: None,
            last_execution: None,
            payload: Some(WorkerPayload::MoveFile { from, to }),
        }
    }
}

pub async fn execute_download_chapter(
    state: &AppState,
    connector_mapping_key: &str,
) -> Result<Vec<WorkerEntry>, anyhow::Error> {
    let chapter_mapping = sqlx::query_as::<_, crate::db::chapter_repo::ChapterConnectorMappingRow>(
        "SELECT key, manga_connector_name, obj_id, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_chapter WHERE key = $1"
    )
    .bind(connector_mapping_key)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| anyhow!("Chapter connector mapping not found: {connector_mapping_key}"))?;

    let chapter = ChapterRepo::get_by_id(&state.db, &chapter_mapping.entity_id)
        .await?
        .ok_or_else(|| anyhow!("Chapter not found: {}", chapter_mapping.entity_id))?;

    if chapter.downloaded {
        info!("Chapter {} already downloaded, skipping", chapter.id);
        return Ok(vec![]);
    }

    let manga = MangaRepo::get_by_id(&state.db, &chapter.manga_id)
        .await?
        .ok_or_else(|| anyhow!("Manga not found: {}", chapter.manga_id))?;

    let library_id = manga.file_library_id.as_deref()
        .ok_or_else(|| anyhow!("Manga {} has no library assigned", chapter.manga_id))?;

    let library = crate::db::library_repo::LibraryRepo::get_by_id(&state.db, library_id)
        .await?
        .ok_or_else(|| anyhow!("Library not found: {library_id}"))?;

    let connectors = state.connectors.read().await;
    let connector = connectors.get(&chapter_mapping.manga_connector_name)
        .ok_or_else(|| anyhow!("Connector not found: {}", chapter_mapping.manga_connector_name))?;

    let image_urls = connector.get_chapter_image_urls(
        &chapter_mapping.id_on_connector_site,
        &*state.http_client,
    ).await.context("Failed to get chapter image URLs")?;

    if image_urls.is_empty() {
        warn!("No image URLs for chapter {}, marking as downloaded to skip retry", chapter.id);
        ChapterRepo::mark_downloaded(&state.db, &chapter.id, "").await?;
        return Ok(vec![]);
    }

    let manga_dir = std::path::Path::new(&library.base_path)
        .join(clean_name_for_windows(&manga.name));
    tokio::fs::create_dir_all(&manga_dir).await?;

    let settings = state.config.settings.read().await.clone();
    let naming_ctx = ChapterNamingContext {
        manga_name: manga.name.clone(),
        volume_number: chapter.volume,
        chapter_number: chapter.chapter_number.clone(),
        chapter_title: chapter.title.clone(),
        author_name: manga.authors.first().map(|a| a.author_name.clone()),
        year: manga.year,
        always_include_volume: state.config.always_include_volume_in_filename,
    };
    let file_name = crate::naming::resolve_naming_scheme(
        &settings.chapter_naming_scheme,
        &naming_ctx,
    );
    let archive_path = manga_dir.join(&file_name);

    let comic_info_xml = if state.config.create_comicinfo_xml {
        Some(comicinfo::generate_comic_info_xml(&ComicInfoData {
            chapter_number: chapter.chapter_number.clone(),
            title: chapter.title.clone(),
            tags: manga.tags.clone(),
            volume_number: chapter.volume,
            authors: manga.authors.iter().map(|a| a.author_name.clone()).collect(),
            original_language: manga.original_language.clone(),
            description: if manga.description.is_empty() { String::new() } else { manga.description.clone() },
        }))
    } else {
        None
    };

    let ct = state.shutdown_token.clone();
    {
        let file = tokio::fs::File::create(&archive_path).await?;
        let mut zip = async_zip::tokio::write::ZipFileWriter::with_tokio(file);

        if let Some(ref xml) = comic_info_xml {
            let entry = async_zip::ZipEntryBuilder::new(
                "ComicInfo.xml".into(),
                async_zip::Compression::Deflate,
            );
            zip.write_entry_whole(entry, xml.as_bytes()).await?;
        }

    let referrer = connector.image_referrer();

    for (i, url) in image_urls.iter().enumerate() {
        let bytes = state.http_client.download_image(url, referrer, ct.clone()).await
            .with_context(|| format!("Failed to download image: {url}"))?
            .ok_or_else(|| anyhow!("Failed to download image: {url}"))?;

        let processed = crate::image::process_image(
            &bytes,
            settings.black_white_images,
            settings.image_compression,
        );

        let entry = async_zip::ZipEntryBuilder::new(
            format!("{i}.jpg").into(),
            async_zip::Compression::Deflate,
        );
        zip.write_entry_whole(entry, &processed).await?;
    }

    zip.close().await?;
    }

    let cover_copied = copy_cover_to_manga_dir(state, &manga, &manga_dir).await;
    if let Err(e) = cover_copied {
        warn!("Failed to copy cover: {e}");
    }

    let file_name_str = file_name.clone();
    ChapterRepo::mark_downloaded(&state.db, &chapter.id, &file_name_str).await?;

    let action_key = ActionId::new();
    ActionRepo::create(
        &state.db,
        &action_key.to_string(),
        "ChapterDownloaded",
        Some(&chapter.manga_id),
        Some(&chapter.id),
        Some(&file_name_str),
        None,
        None,
    ).await?;

    let notif_key = NotificationId::new();
    NotificationRepo::create(
        &state.db,
        &notif_key.to_string(),
        "Chapter downloaded",
        &format!("{} Ch. {} - {}", manga.name, chapter.chapter_number, file_name_str),
        "Normal",
    ).await?;

    info!("Downloaded chapter {} -> {}", chapter.id, archive_path.display());

    let mut children = Vec::new();
    if should_refresh_library(state, &settings.library_refresh_setting).await {
        {
            let mut last = state.last_library_refresh.lock().await;
            *last = std::time::Instant::now();
        }
        children.push(RefreshLibrariesWorker::create());
    }
    Ok(children)
}

async fn copy_cover_to_manga_dir(
    state: &AppState,
    manga: &crate::api::dto::MangaDetail,
    manga_dir: &std::path::Path,
) -> Result<(), anyhow::Error> {
    for ext in &["jpg", "jpeg", "png", "webp"] {
        let cover_path = manga_dir.join(format!("cover.{ext}"));
        if cover_path.exists() {
            return Ok(());
        }
    }

    let cache = CoverCache::new(state.config.working_dir.join("imageCache"));

    if let Some(ref cache_name) = manga.cover_file_name_in_cache
        && let Some(data) = cache.get_cover("original", cache_name).await {
        let ext = cache_name.rsplit('.').next().unwrap_or("jpg");
        let dest = manga_dir.join(format!("cover.{ext}"));
        tokio::fs::write(&dest, &data).await?;
        return Ok(());
    }

    Ok(())
}

async fn should_refresh_library(state: &AppState, setting: &str) -> bool {
    match setting {
        "AfterEveryChapter" => true,
        "AfterAllFinished" | "AfterMangaFinished" => {
            match crate::db::chapter_repo::ChapterRepo::count_missing_downloads(&state.db).await {
                Ok(count) => count == 0,
                Err(e) => {
                    warn!("Failed to count missing downloads: {e}");
                    false
                }
            }
        }
        "WhileDownloading" => {
            match crate::db::chapter_repo::ChapterRepo::count_missing_downloads(&state.db).await {
                Ok(0) => true,
                Ok(_) => {
                    let settings = state.config.settings.read().await;
                    let mins = settings.refresh_library_while_downloading_every_minutes as f64;
                    drop(settings);
                    let last = state.last_library_refresh.lock().await;
                    last.elapsed().as_secs_f64() / 60.0 > mins
                }
                Err(e) => {
                    warn!("Failed to count missing downloads: {e}");
                    false
                }
            }
        }
        _ => false,
    }
}

pub async fn execute_download_cover(
    state: &AppState,
    connector_mapping_key: &str,
) -> Result<Vec<WorkerEntry>, anyhow::Error> {
    let mapping = sqlx::query_as::<_, crate::db::manga_repo::ConnectorMappingRow>(
        "SELECT key, manga_connector_name, obj_id, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_manga WHERE key = $1"
    )
    .bind(connector_mapping_key)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| anyhow!("Manga connector mapping not found: {connector_mapping_key}"))?;

    let manga = MangaRepo::get_by_id(&state.db, &mapping.entity_id)
        .await?
        .ok_or_else(|| anyhow!("Manga not found: {}", mapping.entity_id))?;

    if manga.cover_url.is_empty() {
        warn!("No cover URL stored for manga {}, skipping", manga.id);
        return Ok(vec![]);
    }

    let cache_name = {
        let parsed = url::Url::parse(&manga.cover_url);
        let domain = parsed.map(|u| u.domain().unwrap_or("unknown").to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let ext = manga.cover_url.rsplit('.').next().unwrap_or("jpg");
        format!("{domain}-{}.{ext}", mapping.manga_connector_name)
    };

    if let Some(ref name) = manga.cover_file_name_in_cache {
        let cache = CoverCache::new(state.config.working_dir.join("imageCache"));
        if cache.get_cover("original", name).await.is_some() {
            info!("Cover already cached for manga {}", manga.id);
            return Ok(vec![]);
        }
    }

    let parsed = url::Url::parse(&manga.cover_url);
    let referrer = parsed.map(|u| format!("https://{}", u.domain().unwrap_or("unknown"))).ok();
    let ct = state.shutdown_token.clone();

    if let Ok(Some(bytes)) = state.http_client.download_image(&manga.cover_url, referrer.as_deref(), ct).await {
        let cache = CoverCache::new(state.config.working_dir.join("imageCache"));
        cache.ensure_dirs().await?;
        let cleaned = crate::utils::clean_name_for_windows(&cache_name);

        cache.save_original(&cleaned, &bytes).await?;
        cache.generate_cached_sizes(&cleaned, &bytes).await?;

        MangaRepo::update_cover_file_name(&state.db, &manga.id, &cleaned).await?;

        let action_key = ActionId::new();
        ActionRepo::create(
            &state.db,
            &action_key.to_string(),
            "CoverDownloaded",
            Some(&manga.id),
            None,
            None,
            None,
            None,
        ).await?;

        info!("Downloaded cover for manga {}", manga.id);
    }

    Ok(vec![])
}

pub async fn execute_retrieve_chapters(
    state: &AppState,
    connector_mapping_key: &str,
    language: &str,
) -> Result<Vec<WorkerEntry>, anyhow::Error> {
    let mapping = sqlx::query_as::<_, crate::db::manga_repo::ConnectorMappingRow>(
        "SELECT key, manga_connector_name, obj_id, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_manga WHERE key = $1"
    )
    .bind(connector_mapping_key)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| anyhow!("Manga connector mapping not found: {connector_mapping_key}"))?;

    let manga = MangaRepo::get_by_id(&state.db, &mapping.entity_id)
        .await?
        .ok_or_else(|| anyhow!("Manga not found: {}", mapping.entity_id))?;

    let connectors = state.connectors.read().await;
    let connector = connectors.get(&mapping.manga_connector_name)
        .ok_or_else(|| anyhow!("Connector not found: {}", mapping.manga_connector_name))?;

    let results = connector.get_chapters(
        &mapping.id_on_connector_site,
        Some(language),
        &*state.http_client,
    ).await.context("Failed to get chapters")?;

    let existing = ChapterRepo::get_all_for_manga(&state.db, &mapping.entity_id).await?;
    let existing_numbers: std::collections::HashSet<String> = existing.iter()
        .map(|c| c.chapter_number.clone())
        .collect();

    let mut new_count = 0u32;
    for result in &results {
        if existing_numbers.contains(&result.chapter_number) {
            continue;
        }

        let chapter_id = ChapterId::new();
        ChapterRepo::insert(
            &state.db,
            &chapter_id.to_string(),
            &mapping.entity_id,
            result.volume_number,
            &result.chapter_number,
            result.title.as_deref(),
        ).await?;

        let ch_mapping_key = ConnectorMappingId::new();
        ChapterRepo::insert_chapter_connector_mapping(
            &state.db,
            &ch_mapping_key.to_string(),
            &chapter_id.to_string(),
            &mapping.manga_connector_name,
            &result.connector_site_id,
            result.website_url.as_deref(),
            mapping.use_for_download != 0,
        ).await?;

        new_count += 1;
    }

    if new_count > 0 {
        let action_key = ActionId::new();
        ActionRepo::create(
            &state.db,
            &action_key.to_string(),
            "ChaptersRetrieved",
            Some(&mapping.entity_id),
            None,
            None,
            None,
            None,
        ).await?;

        info!("Retrieved {new_count} new chapters for manga {}", manga.id);
    }

    Ok(vec![])
}

pub async fn execute_refresh_libraries(
    state: &AppState,
) -> Result<Vec<WorkerEntry>, anyhow::Error> {
    let connectors = crate::db::connector_repo::ConnectorRepo::list_library_connectors(&state.db).await?;

    for conn in &connectors {
        let result = match conn.library_type.as_str() {
            "Komga" => {
                let c = crate::library_connectors::KomgaConnector::new(&conn.base_url, &conn.auth);
                c.update_library(state.shutdown_token.clone()).await
            }
            "Kavita" => {
                let c = crate::library_connectors::KavitaConnector::new(&conn.base_url, &conn.auth);
                c.update_library(state.shutdown_token.clone()).await
            }
            _ => {
                warn!("Unknown library type: {}", conn.library_type);
                continue;
            }
        };

        if let Err(e) = result {
            warn!("Failed to refresh library {}: {e}", conn.id);
        }
    }

    Ok(vec![])
}

pub async fn execute_move_file(
    state: &AppState,
    from: &str,
    to: &str,
) -> Result<Vec<WorkerEntry>, anyhow::Error> {
    let from_path = std::path::Path::new(from);
    let to_path = std::path::Path::new(to);

    if !from_path.exists() {
        return Err(anyhow!("Source path does not exist: {from}"));
    }
    if to_path.exists() {
        return Err(anyhow!("Destination path already exists: {to}"));
    }

    if let Some(parent) = to_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    tokio::fs::rename(from_path, to_path).await?;

    let action_key = ActionId::new();
    ActionRepo::create(
        &state.db,
        &action_key.to_string(),
        "DataMoved",
        None,
        None,
        None,
        Some(from),
        Some(to),
    ).await?;

    info!("Moved {from} -> {to}");
    Ok(vec![])
}
