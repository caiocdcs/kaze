use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use super::{ApiError, ApiResult, AppState};
use crate::{
    db::{self, events::HistoryEntry, jobs::JobRecord},
    jobs::scheduler::{Task, TaskInfo},
};

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

/// Scheduled tasks with their last and next run.
pub async fn tasks(State(app): State<AppState>) -> Json<Vec<TaskInfo>> {
    Json(app.scheduled.list())
}

/// Run a scheduled task now. The path segment deserializes into `Task`,
/// so axum rejects an unknown name before this runs.
pub async fn run_task(
    State(app): State<AppState>,
    Path(task): Path<Task>,
) -> ApiResult<StatusCode> {
    if app.scheduled.run_now(task).await {
        Ok(StatusCode::ACCEPTED)
    } else {
        Err(ApiError::Internal)
    }
}
