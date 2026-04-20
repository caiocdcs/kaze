use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json,
};

use crate::app_state::AppState;
use crate::db::library_repo::LibraryRepo;
use crate::error::{AppError, AppResult};
use crate::id::{EntityId, FileLibraryId};
use crate::api::dto::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/file-libraries", get(list).post(create))
        .route(
            "/file-libraries/{id}",
            get(get_one).patch(update).delete(delete_lib),
        )
}

async fn list(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let libraries = LibraryRepo::list(&state.db).await?;
    Ok(Json(libraries))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let library = LibraryRepo::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "FileLibrary",
            id: id.clone(),
        })?;
    Ok(Json(library))
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateLibrary>,
) -> AppResult<impl IntoResponse> {
    let key = FileLibraryId::new();
    let library =
        LibraryRepo::create(&state.db, &key.to_string(), &body.base_path, &body.library_name)
            .await?;
    Ok((StatusCode::CREATED, Json(library)))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateLibrary>,
) -> AppResult<impl IntoResponse> {
    if !LibraryRepo::update(&state.db, &id, &body.base_path, &body.library_name).await? {
        return Err(AppError::NotFound {
            entity: "FileLibrary",
            id,
        });
    }
    Ok(StatusCode::OK)
}

async fn delete_lib(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    if !LibraryRepo::delete(&state.db, &id).await? {
        return Err(AppError::NotFound {
            entity: "FileLibrary",
            id,
        });
    }
    Ok(StatusCode::OK)
}
