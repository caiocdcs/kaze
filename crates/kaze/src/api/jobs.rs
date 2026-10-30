use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use utoipa::IntoParams;

use super::{ApiError, ApiResult, AppState, ErrorBody};
use crate::{
    db::{self, events::HistoryEntry, jobs::JobRecord},
    jobs::scheduler::{Task, TaskInfo},
};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct Limit {
    #[serde(default = "default_limit")]
    limit: i64,
}

const fn default_limit() -> i64 {
    200
}

/// Queued and running jobs first, then recently finished ones.
#[utoipa::path(
    operation_id = "jobs_list",
    get,
    path = "/jobs",
    tag = "jobs",
    params(Limit),
    responses((status = 200, description = "OK", body = Vec<JobRecord>))
)]
pub async fn list(
    State(app): State<AppState>,
    Query(q): Query<Limit>,
) -> ApiResult<Json<Vec<JobRecord>>> {
    Ok(Json(db::jobs::list(&app.ctx.db, q.limit).await?))
}

#[utoipa::path(
    operation_id = "jobs_cancel",
    summary = "Cancel a job",
    post,
    path = "/jobs/{id}/cancel",
    tag = "jobs",
    params(("id" = i64, Path, description = "Job id")),
    responses((status = 204, description = "Cancelled"), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn cancel(State(app): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if app.jobs.cancel(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("active job"))
    }
}

#[utoipa::path(
    operation_id = "jobs_retry",
    summary = "Run a failed or cancelled job again",
    post,
    path = "/jobs/{id}/retry",
    tag = "jobs",
    params(("id" = i64, Path, description = "Job id")),
    responses((status = 204, description = "Queued again"), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn retry(State(app): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if app.jobs.retry(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("failed job"))
    }
}

#[utoipa::path(
    operation_id = "jobs_history",
    summary = "Recent downloads, failures and new chapters",
    get,
    path = "/history",
    tag = "jobs",
    params(Limit),
    responses((status = 200, description = "OK", body = Vec<HistoryEntry>))
)]
pub async fn history(
    State(app): State<AppState>,
    Query(q): Query<Limit>,
) -> ApiResult<Json<Vec<HistoryEntry>>> {
    Ok(Json(db::events::recent(&app.ctx.db, q.limit).await?))
}

/// Scheduled tasks with their last and next run.
#[utoipa::path(
    operation_id = "jobs_tasks",
    get,
    path = "/tasks",
    tag = "jobs",
    responses((status = 200, description = "OK", body = Vec<TaskInfo>))
)]
pub async fn tasks(State(app): State<AppState>) -> Json<Vec<TaskInfo>> {
    Json(app.scheduled.list())
}

/// Run a scheduled task now. The path segment deserializes into `Task`,
/// so axum rejects an unknown name before this runs.
#[utoipa::path(
    operation_id = "jobs_run_task",
    post,
    path = "/tasks/{task}/run",
    tag = "jobs",
    params(("task" = Task, Path, description = "Scheduled task")),
    responses((status = 202, description = "Started"), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
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
