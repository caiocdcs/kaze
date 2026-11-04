use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;
use utoipa::IntoParams;

use super::{ApiResult, AppState};
use crate::logs::{LogLevel, LogLine};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LogsQuery {
    /// Only lines at this level or more severe; all of them without it.
    level: Option<LogLevel>,
}

#[utoipa::path(
    operation_id = "logs_recent",
    summary = "The last log lines kaze kept in memory, newest first",
    get,
    path = "/logs",
    tag = "system",
    params(LogsQuery),
    responses((status = 200, description = "OK", body = Vec<LogLine>))
)]
pub async fn recent(
    State(app): State<AppState>,
    Query(query): Query<LogsQuery>,
) -> ApiResult<Json<Vec<LogLine>>> {
    let level = query.level.unwrap_or(LogLevel::Trace);
    Ok(Json(app.ctx.logs.lines(level)))
}
