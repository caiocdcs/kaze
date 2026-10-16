//! A snapshot of the database, downloaded from Settings → System.

use axum::{extract::State, http::header, response::IntoResponse};
use chrono::Utc;

use super::{ApiError, ApiResult, AppState};
use crate::db;

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
