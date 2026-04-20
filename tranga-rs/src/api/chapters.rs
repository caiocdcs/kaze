use axum::{extract::{Path, State}, routing::{delete, get, patch, post}, Json, http::StatusCode, response::IntoResponse, Router};

use crate::app_state::AppState;
use crate::db::chapter_repo::ChapterRepo;
use crate::error::{AppError, AppResult};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/chapters/{chapter_id}", get(get_chapter).delete(delete_chapter))
        .route("/chapters/{chapter_id}/connectors/{connector_name}/download", patch(toggle_download))
        .route("/chapters/{chapter_id}/connectors/{connector_name}", get(get_mapping))
        .route("/chapters/connectors/{mapping_id}", delete(delete_mapping))
        .route("/chapters/{chapter_id}/recheck", post(recheck))
}

async fn get_chapter(State(state): State<AppState>, Path(chapter_id): Path<String>) -> AppResult<impl IntoResponse> {
    match ChapterRepo::get_by_id(&state.db, &chapter_id).await? {
        Some(ch) => Ok(Json(ch)),
        None => Err(AppError::NotFound { entity: "Chapter", id: chapter_id }),
    }
}

async fn delete_chapter(State(state): State<AppState>, Path(chapter_id): Path<String>) -> AppResult<impl IntoResponse> {
    if ChapterRepo::delete(&state.db, &chapter_id).await? {
        Ok(StatusCode::OK)
    } else {
        Err(AppError::NotFound { entity: "Chapter", id: chapter_id })
    }
}

async fn toggle_download(
    State(state): State<AppState>,
    Path((chapter_id, connector_name)): Path<(String, String)>,
    _body: axum::body::Bytes,
) -> AppResult<impl IntoResponse> {
    if !ChapterRepo::toggle_chapter_connector_download(&state.db, &chapter_id, &connector_name).await? {
        return Err(AppError::NotFound {
            entity: "ConnectorMapping",
            id: format!("{chapter_id}/{connector_name}"),
        });
    }
    Ok(StatusCode::OK)
}

async fn get_mapping(
    State(state): State<AppState>,
    Path((chapter_id, connector_name)): Path<(String, String)>,
) -> AppResult<impl IntoResponse> {
    let mapping = ChapterRepo::get_chapter_connector_mapping(&state.db, &chapter_id, &connector_name)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ConnectorMapping",
            id: format!("{chapter_id}/{connector_name}"),
        })?;
    Ok(Json(mapping))
}

async fn delete_mapping(
    State(state): State<AppState>,
    Path(mapping_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    if !ChapterRepo::delete_chapter_connector_mapping(&state.db, &mapping_id).await? {
        return Err(AppError::NotFound { entity: "ConnectorMapping", id: mapping_id });
    }
    Ok(StatusCode::OK)
}

async fn recheck(State(state): State<AppState>, Path(chapter_id): Path<String>) -> AppResult<impl IntoResponse> {
    let count = ChapterRepo::recheck_chapter(&state.db, &chapter_id).await?;
    Ok(Json(count))
}
