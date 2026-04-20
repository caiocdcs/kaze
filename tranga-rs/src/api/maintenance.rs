use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json,
};

use crate::app_state::AppState;
use crate::db::{chapter_repo::ChapterRepo, library_repo::LibraryRepo, manga_repo::MangaRepo};
use crate::error::AppResult;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/maintenance/ensure-default-library",
            post(ensure_default_library),
        )
        .route(
            "/maintenance/missing-downloads",
            get(count_missing_downloads),
        )
        .route(
            "/maintenance/cleanup-no-download-manga",
            post(cleanup_no_download_manga),
        )
}

async fn ensure_default_library(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let default_path = state
        .config
        .download_location
        .to_str()
        .unwrap_or("/Manga");
    LibraryRepo::ensure_default(&state.db, default_path).await?;
    Ok(StatusCode::OK)
}

async fn count_missing_downloads(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let count = ChapterRepo::count_missing_downloads(&state.db).await?;
    Ok(Json(count))
}

async fn cleanup_no_download_manga(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let removed = MangaRepo::delete_no_download_manga(&state.db).await?;
    Ok(Json(removed))
}
