use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json,
};

use crate::app_state::AppState;
use crate::db::metadata_repo::MetadataRepo;
use crate::db::manga_repo::MangaRepo;
use crate::error::{AppError, AppResult};
use crate::metadata::MetadataFetcher;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/metadata-fetchers", get(list))
        .route("/metadata-fetchers/links", get(list_all_links))
        .route("/metadata-fetchers/links/{manga_id}", get(list_for_manga))
        .route(
            "/metadata-fetchers/{fetcher_name}/search/{manga_id}",
            post(search_manga),
        )
        .route(
            "/metadata-fetchers/{fetcher_name}/link/{manga_id}",
            post(link),
        )
        .route(
            "/metadata-fetchers/{fetcher_name}/unlink/{manga_id}",
            post(unlink),
        )
        .route(
            "/metadata-fetchers/{fetcher_name}/update/{manga_id}",
            post(update_metadata),
        )
}

async fn list(
    State(_state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let fetchers = [crate::metadata::MyAnimeListFetcher::new()];
    let names: Vec<String> = fetchers.iter().map(|f| f.name().to_string()).collect();
    Ok(Json(names))
}

async fn list_all_links(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let entries = MetadataRepo::list_entries(&state.db).await?;
    Ok(Json(entries))
}

async fn list_for_manga(
    State(state): State<AppState>,
    Path(manga_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let entries = MetadataRepo::list_for_manga(&state.db, &manga_id).await?;
    Ok(Json(entries))
}

async fn search_manga(
    State(state): State<AppState>,
    Path((fetcher_name, manga_id)): Path<(String, String)>,
    body: String,
) -> AppResult<impl IntoResponse> {
    let search_term = if body.is_empty() { None } else { Some(body) };

    let name = match search_term {
        Some(term) => term,
        None => MangaRepo::get_name(&state.db, &manga_id)
            .await?
            .ok_or_else(|| AppError::NotFound { entity: "Manga", id: manga_id })?,
    };

    let fetcher = match fetcher_name.to_lowercase().as_str() {
        "myanimelist" | "mal" => crate::metadata::MyAnimeListFetcher::new(),
        _ => return Err(AppError::NotFound { entity: "MetadataFetcher", id: fetcher_name }),
    };

    let results = fetcher
        .search_by_manga_name(&name, &*state.http_client)
        .await
        .map_err(crate::error::AppError::Internal)?;

    Ok(Json(results))
}

async fn link(
    State(state): State<AppState>,
    Path((fetcher_name, manga_id)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> AppResult<impl IntoResponse> {
    let identifier = String::from_utf8_lossy(&body).to_string();
    MetadataRepo::link(&state.db, &fetcher_name, &manga_id, &identifier).await?;
    Ok(StatusCode::OK)
}

async fn unlink(
    State(state): State<AppState>,
    Path((fetcher_name, manga_id)): Path<(String, String)>,
) -> AppResult<impl IntoResponse> {
    MetadataRepo::unlink(&state.db, &fetcher_name, &manga_id).await?;
    Ok(StatusCode::OK)
}

async fn update_metadata(
    State(state): State<AppState>,
    Path((fetcher_name, manga_id)): Path<(String, String)>,
) -> AppResult<impl IntoResponse> {
    let entry = MetadataRepo::get_by_fetcher_and_manga(&state.db, &fetcher_name, &manga_id)
        .await?
        .ok_or_else(|| AppError::PreconditionRequired {
            detail: format!("No {fetcher_name} link found for manga {manga_id}"),
        })?;
    let identifier = entry.identifier;

    let fetcher = match fetcher_name.to_lowercase().as_str() {
        "myanimelist" | "mal" => crate::metadata::MyAnimeListFetcher::new(),
        _ => return Err(AppError::NotFound { entity: "MetadataFetcher", id: fetcher_name }),
    };

    if let Some(update) = fetcher
        .update_metadata(&identifier, &*state.http_client)
        .await
        .map_err(crate::error::AppError::Internal)?
    {
        if !update.name.is_empty() {
            MangaRepo::update_manga_field(&state.db, &manga_id, "name", &update.name).await?;
        }
        if !update.description.is_empty() {
            MangaRepo::update_manga_field(&state.db, &manga_id, "description", &update.description).await?;
        }
    }

    Ok(StatusCode::OK)
}
