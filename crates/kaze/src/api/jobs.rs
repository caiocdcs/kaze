use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use super::{ApiError, ApiResult, AppState};
use crate::db::{self, events::HistoryEntry, jobs::JobRecord};

#[derive(Deserialize)]
pub struct Limit {
    #[serde(default = "default_limit")]
    limit: i64,
}

const fn default_limit() -> i64 {
    200
}

/// Queued and running jobs first, then recently finished ones.
pub async fn list(
    State(app): State<AppState>,
    Query(q): Query<Limit>,
) -> ApiResult<Json<Vec<JobRecord>>> {
    Ok(Json(db::jobs::list(&app.ctx.db, q.limit).await?))
}

pub async fn cancel(State(app): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if app.jobs.cancel(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("active job"))
    }
}

pub async fn retry(State(app): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if app.jobs.retry(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("failed job"))
    }
}

pub async fn history(
    State(app): State<AppState>,
    Query(q): Query<Limit>,
) -> ApiResult<Json<Vec<HistoryEntry>>> {
    Ok(Json(db::events::recent(&app.ctx.db, q.limit).await?))
}
