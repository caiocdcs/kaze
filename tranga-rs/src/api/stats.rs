use axum::{Router, extract::State, routing::get, Json, response::IntoResponse};

use crate::app_state::AppState;
use crate::api::dto::StatsDto;
use crate::error::AppResult;

pub fn router() -> Router<AppState> {
    Router::new().route("/stats", get(get_stats))
}

async fn get_stats(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let number_manga: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM mangas").fetch_one(&state.db).await?;
    let number_chapters: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM chapters").fetch_one(&state.db).await?;
    let downloaded_chapters: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM chapters WHERE downloaded=TRUE").fetch_one(&state.db).await?;
    let missing_chapters: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM chapters WHERE downloaded=FALSE").fetch_one(&state.db).await?;
    let sent_notifications: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM notifications WHERE is_sent=TRUE").fetch_one(&state.db).await?;
    let actions_taken: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM actions").fetch_one(&state.db).await?;
    let number_authors: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM authors").fetch_one(&state.db).await?;
    let number_tags: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM manga_tags").fetch_one(&state.db).await?;

    let stats = StatsDto {
        number_manga: number_manga.0,
        number_chapters: number_chapters.0,
        missing_chapters: missing_chapters.0,
        downloaded_chapters: downloaded_chapters.0,
        sent_notifications: sent_notifications.0,
        actions_taken: actions_taken.0,
        number_authors: number_authors.0,
        number_tags: number_tags.0,
    };

    Ok(Json(stats))
}
