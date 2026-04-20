use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get},
    Json,
};
use serde::Deserialize;

use crate::app_state::AppState;
use crate::db::action_repo::ActionRepo;
use crate::error::AppResult;
use crate::api::dto::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/actions/types", get(list_types))
        .route("/actions/filter", get(filter))
        .route("/actions", delete(cleanup))
}

#[derive(Deserialize)]
pub struct FilterQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub manga_id: Option<String>,
    pub chapter_id: Option<String>,
    pub action: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

async fn list_types() -> AppResult<impl IntoResponse> {
    let types = ActionRepo::list_types().await;
    Ok(Json(types))
}

async fn filter(
    State(state): State<AppState>,
    Query(query): Query<FilterQuery>,
) -> AppResult<impl IntoResponse> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).max(1);
    let actions_filter = ActionsFilter {
        start: query.start,
        end: query.end,
        manga_id: query.manga_id,
        chapter_id: query.chapter_id,
        action: query.action,
    };
    let result = ActionRepo::filter(&state.db, &actions_filter, page, per_page).await?;
    Ok(Json(result))
}

async fn cleanup(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    ActionRepo::cleanup(&state.db).await?;
    Ok(StatusCode::OK)
}
