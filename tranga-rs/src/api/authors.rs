use axum::{Router, extract::{Path, Query, State}, routing::get, Json, response::IntoResponse};
use serde::Deserialize;

use crate::app_state::AppState;
use crate::db::manga_repo::MangaRepo;
use crate::error::{AppError, AppResult};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/authors", get(list))
        .route("/authors/{author_id}", get(get_author))
        .route("/authors/{author_id}/manga", get(manga_by_author))
}

#[derive(Deserialize)]
pub struct AuthorQuery {
    pub manga_id: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(query): Query<AuthorQuery>,
) -> AppResult<impl IntoResponse> {
    if let Some(manga_id) = query.manga_id {
        let authors = MangaRepo::get_authors_public(&state.db, &manga_id).await?;
        Ok(Json(authors))
    } else {
        let authors = MangaRepo::list_authors(&state.db).await?;
        Ok(Json(authors))
    }
}

async fn get_author(
    State(state): State<AppState>,
    Path(author_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let author = MangaRepo::get_author_by_id(&state.db, &author_id).await?
        .ok_or_else(|| AppError::NotFound { entity: "Author", id: author_id })?;
    Ok(Json(author))
}

async fn manga_by_author(
    State(state): State<AppState>,
    Path(author_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let manga = MangaRepo::get_manga_by_author(&state.db, &author_id).await?;
    Ok(Json(manga))
}
