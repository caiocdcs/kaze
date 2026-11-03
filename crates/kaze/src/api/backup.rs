//! Database backups, from Settings → System: a fresh snapshot to
//! download, and the saved ones (`crate::backup`).

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use chrono::Utc;
use serde::Serialize;
use ts_rs::TS;
use utoipa::ToSchema;

use super::{ApiError, ApiResult, AppState, ErrorBody};
use crate::{
    backup::{self, BackupFile},
    db,
};

#[utoipa::path(
    operation_id = "backup_download",
    summary = "Download a snapshot of the database",
    get,
    path = "/backup",
    tag = "system",
    responses((status = 200, description = "A snapshot of the SQLite database", content_type = "application/vnd.sqlite3", body = Vec<u8>))
)]
pub async fn download(State(app): State<AppState>) -> ApiResult<impl IntoResponse> {
    let now = Utc::now();
    // `VACUUM INTO` refuses an existing file, so the name must be fresh.
    let path = std::env::temp_dir().join(format!(
        "kaze-backup-{}-{}.db",
        std::process::id(),
        now.timestamp_nanos_opt().unwrap_or_default()
    ));
    db::backup(&app.ctx.db, &path).await?;
    let bytes = tokio::fs::read(&path).await;
    if let Err(err) = tokio::fs::remove_file(&path).await {
        tracing::warn!(path = %path.display(), "could not remove backup file: {err}");
    }
    let bytes = bytes.map_err(|err| {
        tracing::error!("could not read backup: {err}");
        ApiError::Internal
    })?;
    tracing::info!(bytes = bytes.len(), "database backup downloaded");
    let name = format!("kaze-{}.db", now.format("%Y%m%d-%H%M"));
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.sqlite3".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    ))
}

#[derive(Serialize, TS, ToSchema)]
#[ts(export)]
pub struct Backups {
    /// Newest first.
    saved: Vec<BackupFile>,
    /// A restore waits for kaze's next start.
    restore_staged: bool,
}

#[utoipa::path(
    operation_id = "backups_list",
    summary = "Saved backups, and whether a restore is staged",
    get,
    path = "/backups",
    tag = "system",
    responses((status = 200, description = "OK", body = Backups))
)]
pub async fn list(State(app): State<AppState>) -> ApiResult<Json<Backups>> {
    let dir = &app.ctx.backups;
    Ok(Json(Backups {
        saved: backup::list(dir).await.map_err(|e| io(&e))?,
        restore_staged: backup::is_staged(dir).await,
    }))
}

#[utoipa::path(
    operation_id = "backups_restore",
    summary = "Restore a saved backup on kaze's next start",
    post,
    path = "/backups/{name}/restore",
    tag = "system",
    params(("name" = String, Path, description = "Backup file name")),
    responses((status = 200, description = "Staged", body = Backups), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn restore(
    State(app): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Backups>> {
    let path = saved(&app, &name).await?;
    backup::stage(&app.ctx.backups, &path).await?;
    tracing::info!(name, "restore staged for the next start");
    list(State(app)).await
}

#[utoipa::path(
    operation_id = "backups_upload_restore",
    summary = "Restore an uploaded database file on kaze's next start",
    post,
    path = "/restore",
    tag = "system",
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "A kaze database or backup"),
    responses((status = 200, description = "Staged", body = Backups), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn upload(State(app): State<AppState>, body: Bytes) -> ApiResult<Json<Backups>> {
    backup::stage_upload(&app.ctx.backups, &body).await?;
    tracing::info!(
        bytes = body.len(),
        "uploaded restore staged for the next start"
    );
    list(State(app)).await
}

#[utoipa::path(
    operation_id = "backups_cancel_restore",
    summary = "Cancel a staged restore",
    delete,
    path = "/restore",
    tag = "system",
    responses((status = 200, description = "OK", body = Backups))
)]
pub async fn cancel_restore(State(app): State<AppState>) -> ApiResult<Json<Backups>> {
    if backup::unstage(&app.ctx.backups)
        .await
        .map_err(|e| io(&e))?
    {
        tracing::info!("staged restore cancelled");
    }
    list(State(app)).await
}

#[utoipa::path(
    operation_id = "backups_get",
    summary = "Download a saved backup",
    get,
    path = "/backups/{name}",
    tag = "system",
    params(("name" = String, Path, description = "Backup file name")),
    responses((status = 200, description = "The backup", content_type = "application/vnd.sqlite3", body = Vec<u8>), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn get(
    State(app): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let path = saved(&app, &name).await?;
    let bytes = tokio::fs::read(&path).await.map_err(|e| io(&e))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.sqlite3".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    ))
}

#[utoipa::path(
    operation_id = "backups_delete",
    summary = "Delete a saved backup",
    delete,
    path = "/backups/{name}",
    tag = "system",
    params(("name" = String, Path, description = "Backup file name")),
    responses((status = 204, description = "Deleted"), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn delete(
    State(app): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    let path = saved(&app, &name).await?;
    tokio::fs::remove_file(&path).await.map_err(|e| io(&e))?;
    tracing::info!(name, "backup deleted");
    Ok(StatusCode::NO_CONTENT)
}

/// The saved backup called `name`; anything else is a 404.
async fn saved(app: &AppState, name: &str) -> ApiResult<std::path::PathBuf> {
    backup::find(&app.ctx.backups, name)
        .await
        .map_err(|e| io(&e))?
        .ok_or(ApiError::NotFound("backup"))
}

fn io(err: &std::io::Error) -> ApiError {
    tracing::error!("backups folder: {err}");
    ApiError::Internal
}
