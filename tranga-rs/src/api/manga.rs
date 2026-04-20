use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post},
    Json,
};
use serde::Deserialize;
use strsim::normalized_levenshtein;

use crate::app_state::AppState;
use crate::db::manga_repo::MangaRepo;
use crate::error::{AppError, AppResult};
use crate::api::dto::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/manga", get(list))
        .route("/manga/{manga_id}", get(get_manga).delete(delete_manga))
        .route("/manga/{manga_id}/cover/{size}", get(cover))
        .route("/manga/{manga_id}/move-to-library/{library_id}", post(move_to_library))
        .route("/manga/{manga_id}/add-to-library", post(add_to_library))
        .route("/manga/{manga_id}/connectors/{connector_name}/download", patch(toggle_download))
        .route("/manga/{manga_id}/search-on/{connector_name}", get(search_on_connector))
        .route("/manga/{manga_id}/merge-into/{target_id}", post(merge_into))
        .route("/manga/{manga_id}/similar", get(similar))
        .route("/manga/{manga_id}/recheck", post(recheck))
        .route("/manga/{manga_id}/ignore-before", patch(ignore_before))
        .route("/manga/{manga_id}/connectors/{connector_name}", get(get_connector_mapping))
        .route("/manga/{manga_id}/chapters", get(chapters_list))
        .route("/manga/{manga_id}/chapters/latest", get(chapters_latest))
        .route("/manga/{manga_id}/chapters/latest-downloaded", get(chapters_latest_downloaded))
}

#[derive(Deserialize)]
pub struct MangaListQuery {
    pub downloading: Option<bool>,
    pub tag: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

#[derive(Deserialize)]
pub struct ChapterListQuery {
    pub downloaded: Option<bool>,
    pub name: Option<String>,
    pub volume_number: Option<i32>,
    pub chapter_number: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

async fn list(
    State(state): State<AppState>,
    Query(query): Query<MangaListQuery>,
) -> AppResult<impl IntoResponse> {
    if query.downloading.unwrap_or(false) {
        let manga = MangaRepo::list_downloading(&state.db).await?;
        Ok(Json(manga))
    } else if let Some(tag) = query.tag {
        let manga = MangaRepo::get_by_tag(&state.db, &tag).await?;
        Ok(Json(manga))
    } else {
        let manga = MangaRepo::list(&state.db).await?;
        Ok(Json(manga))
    }
}

async fn get_manga(
    State(state): State<AppState>,
    Path(manga_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let manga = MangaRepo::get_by_id(&state.db, &manga_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Manga",
            id: manga_id,
        })?;
    Ok(Json(manga))
}

async fn delete_manga(
    State(state): State<AppState>,
    Path(manga_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    if !MangaRepo::delete(&state.db, &manga_id).await? {
        return Err(AppError::NotFound {
            entity: "Manga",
            id: manga_id,
        });
    }
    Ok(StatusCode::OK)
}

async fn cover(
    State(state): State<AppState>,
    Path((manga_id, size)): Path<(String, String)>,
) -> AppResult<impl IntoResponse> {
    let manga = MangaRepo::get_by_id(&state.db, &manga_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Manga",
            id: manga_id.clone(),
        })?;

    let filename = manga.cover_file_name_in_cache.unwrap_or_default();
    if filename.is_empty() {
        return Err(AppError::NotFound {
            entity: "Cover",
            id: manga_id,
        });
    }

    let cache = crate::image::cover_cache::CoverCache::new(state.config.working_dir.join("imageCache"));
    match cache.get_cover(&size, &filename).await {
        Some(data) => Ok((
            StatusCode::OK,
            [("content-type", "image/jpeg"), ("cache-control", "public")],
            data,
        )),
        None => Err(AppError::NotFound {
            entity: "Cover",
            id: manga_id,
        }),
    }
}

async fn move_to_library(
    State(state): State<AppState>,
    Path((manga_id, library_id)): Path<(String, String)>,
) -> AppResult<impl IntoResponse> {
    let manga = MangaRepo::get_by_id(&state.db, &manga_id)
        .await?
        .ok_or_else(|| AppError::NotFound { entity: "Manga", id: manga_id.clone() })?;

    let library = crate::db::library_repo::LibraryRepo::get_by_id(&state.db, &library_id)
        .await?
        .ok_or_else(|| AppError::NotFound { entity: "FileLibrary", id: library_id.clone() })?;

    if let Some(old_library_id) = &manga.file_library_id
        && old_library_id != &library_id {
            let old_library = crate::db::library_repo::LibraryRepo::get_by_id(&state.db, old_library_id).await.ok().flatten();
            if let (Some(old_lib), Some(dir_name)) = (old_library, manga.cover_file_name_in_cache.as_deref().or(Some(""))) {
                let from_path = std::path::Path::new(&old_lib.base_path).join(dir_name);
                let to_path = std::path::Path::new(&library.base_path).join(dir_name);
                if from_path.exists() && from_path != to_path {
                    let _ = tokio::fs::create_dir_all(to_path.parent().unwrap_or(std::path::Path::new("/"))).await;
                    let _ = tokio::fs::rename(&from_path, &to_path).await;
                }
            }
    }

    if !MangaRepo::update_library_id(&state.db, &manga_id, &library_id).await? {
        return Err(AppError::NotFound { entity: "Manga", id: manga_id });
    }
    Ok(StatusCode::OK)
}

async fn add_to_library(
    State(state): State<AppState>,
    Path(manga_id): Path<String>,
    Json(body): Json<AddToLibrary>,
) -> AppResult<impl IntoResponse> {
    let manga = MangaRepo::get_by_id(&state.db, &manga_id)
        .await?
        .ok_or_else(|| AppError::NotFound { entity: "Manga", id: manga_id.clone() })?;

    let library = crate::db::library_repo::LibraryRepo::get_by_id(&state.db, &body.library_id)
        .await?
        .ok_or_else(|| AppError::NotFound { entity: "FileLibrary", id: body.library_id.clone() })?;

    if manga.file_library_id.as_ref() != Some(&body.library_id) {
        if let Some(old_library_id) = &manga.file_library_id {
            let old_library = crate::db::library_repo::LibraryRepo::get_by_id(&state.db, old_library_id).await.ok().flatten();
            if let (Some(old_lib), Some(dir_name)) = (old_library, manga.cover_file_name_in_cache.as_deref().or(Some(""))) {
                let from_path = std::path::Path::new(&old_lib.base_path).join(dir_name);
                let to_path = std::path::Path::new(&library.base_path).join(dir_name);
                if from_path.exists() && from_path != to_path {
                    let _ = tokio::fs::create_dir_all(to_path.parent().unwrap_or(std::path::Path::new("/"))).await;
                    let _ = tokio::fs::rename(&from_path, &to_path).await;
                }
            }
        }

        if !MangaRepo::update_library_id(&state.db, &manga_id, &body.library_id).await? {
            return Err(AppError::NotFound { entity: "Manga", id: manga_id });
        }
    }

    Ok(StatusCode::OK)
}

async fn toggle_download(
    State(state): State<AppState>,
    Path((manga_id, connector_name)): Path<(String, String)>,
    Json(body): Json<ToggleDownload>,
) -> AppResult<impl IntoResponse> {
    let was_enabled = MangaRepo::get_connector_download_status(&state.db, &manga_id, &connector_name).await?;

    if let Some(was_enabled) = was_enabled {
        let changed = MangaRepo::set_connector_download(&state.db, &manga_id, &connector_name, body.enabled).await?;
        if !changed {
            return Err(AppError::NotFound {
                entity: "ConnectorMapping",
                id: format!("{manga_id}/{connector_name}"),
            });
        }

        if !was_enabled && body.enabled {
            let language = state.config.settings.read().await.download_language.clone();
            let mapping = MangaRepo::get_connector_mapping(&state.db, &manga_id, &connector_name).await?;
            if let Some(mapping) = mapping {
                let mut scheduler = state.scheduler.lock().await;
                tracing::info!("Submitting RetrieveChapters for mapping {}", mapping.id);
                scheduler.submit(crate::workers::download_chapter::RetrieveChaptersWorker::create(
                    mapping.id.clone(),
                    language,
                ));
                scheduler.submit(crate::workers::download_chapter::DownloadCoverWorker::create(
                    mapping.id.clone(),
                ));
            }
        }
    } else {
        return Err(AppError::NotFound {
            entity: "ConnectorMapping",
            id: format!("{manga_id}/{connector_name}"),
        });
    }

    Ok(StatusCode::OK)
}

async fn search_on_connector(
    State(state): State<AppState>,
    Path((manga_id, connector_name)): Path<(String, String)>,
) -> AppResult<impl IntoResponse> {
    let name = MangaRepo::get_name(&state.db, &manga_id)
        .await?
        .ok_or_else(|| AppError::NotFound { entity: "Manga", id: manga_id.clone() })?;

    let connectors = state.connectors.read().await;
    let connector = connectors.get(&connector_name)
        .ok_or_else(|| AppError::ConnectorNotFound { name: connector_name.clone() })?;

    if !connector.enabled() {
        return Err(AppError::ConnectorDisabled { name: connector_name });
    }

    let results = connector.search_manga(&name, &*state.http_client).await
        .unwrap_or_default();

    let minimal: Vec<MinimalManga> = results.into_iter().map(|r| MinimalManga {
        id: format!("manga_pending_{}", r.connector_site_id),
        name: r.name,
        description: r.description,
        release_status: r.release_status,
        connectors: vec![],
    }).collect();

    Ok(Json(minimal))
}

async fn merge_into(
    State(state): State<AppState>,
    Path((manga_id, target_id)): Path<(String, String)>,
) -> AppResult<impl IntoResponse> {
    if manga_id == target_id {
        return Err(AppError::BadRequest { detail: "Cannot merge manga into itself".into() });
    }

    MangaRepo::get_by_id(&state.db, &manga_id)
        .await?
        .ok_or_else(|| AppError::NotFound { entity: "Manga", id: manga_id.clone() })?;

    MangaRepo::get_by_id(&state.db, &target_id)
        .await?
        .ok_or_else(|| AppError::NotFound { entity: "Manga", id: target_id.clone() })?;

    MangaRepo::merge_manga(&state.db, &manga_id, &target_id).await?;
    Ok(StatusCode::OK)
}

async fn similar(
    State(state): State<AppState>,
    Path(manga_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let name = MangaRepo::get_name(&state.db, &manga_id)
        .await?
        .ok_or_else(|| AppError::NotFound { entity: "Manga", id: manga_id.clone() })?;

    let all = MangaRepo::list_all_names(&state.db).await?;
    let results: Vec<String> = all
        .into_iter()
        .filter(|(key, _)| key != &manga_id)
        .filter(|(_, other_name)| normalized_levenshtein(&name, other_name) > 0.8)
        .map(|(key, _)| key)
        .collect();

    Ok(Json(results))
}

async fn recheck(
    State(state): State<AppState>,
    Path(manga_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let count = crate::db::chapter_repo::ChapterRepo::recheck_manga(&state.db, Some(&manga_id)).await?;
    Ok(Json(count))
}

async fn ignore_before(
    State(state): State<AppState>,
    Path(manga_id): Path<String>,
    body: axum::body::Bytes,
) -> AppResult<impl IntoResponse> {
    let threshold: f32 = serde_json::from_slice(&body)
        .map_err(|_| AppError::BadRequest { detail: "invalid threshold".into() })?;
    if !crate::db::chapter_repo::ChapterRepo::update_ignore_before(&state.db, &manga_id, threshold).await? {
        return Err(AppError::NotFound { entity: "Manga", id: manga_id });
    }
    Ok(StatusCode::OK)
}

async fn get_connector_mapping(
    State(state): State<AppState>,
    Path((manga_id, connector_name)): Path<(String, String)>,
) -> AppResult<impl IntoResponse> {
    let mapping = MangaRepo::get_connector_mapping(&state.db, &manga_id, &connector_name)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ConnectorMapping",
            id: format!("{manga_id}/{connector_name}"),
        })?;
    Ok(Json(mapping))
}

async fn chapters_list(
    State(state): State<AppState>,
    Path(manga_id): Path<String>,
    Query(params): Query<ChapterListQuery>,
) -> AppResult<impl IntoResponse> {
    let filter = ChapterFilter {
        downloaded: params.downloaded,
        name: params.name,
        volume_number: params.volume_number,
        chapter_number: params.chapter_number,
    };
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(10).clamp(1, 100);
    let result = crate::db::chapter_repo::ChapterRepo::list_by_manga(
        &state.db, &manga_id, &filter, page, per_page,
    ).await?;
    Ok(Json(result))
}

async fn chapters_latest(
    State(state): State<AppState>,
    Path(manga_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let chapter = crate::db::chapter_repo::ChapterRepo::get_latest_by_manga(&state.db, &manga_id).await?;
    match chapter {
        Some(ch) => Ok(Json(ch)),
        None => Err(AppError::NotFound { entity: "Chapter", id: manga_id }),
    }
}

async fn chapters_latest_downloaded(
    State(state): State<AppState>,
    Path(manga_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let chapter = crate::db::chapter_repo::ChapterRepo::get_latest_downloaded(&state.db, &manga_id).await?;
    match chapter {
        Some(ch) => Ok(Json(ch)),
        None => Err(AppError::NotFound { entity: "Chapter", id: manga_id }),
    }
}
