//! The history log behind "Activity → History".

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqliteExecutor;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub kind: String,
    pub series_id: Option<i64>,
    pub chapter_id: Option<i64>,
    pub message: String,
}

pub async fn insert(
    db: impl SqliteExecutor<'_>,
    kind: &str,
    series_id: Option<i64>,
    chapter_id: Option<i64>,
    message: &str,
) -> sqlx::Result<()> {
    let now = Utc::now();
    sqlx::query!(
        "INSERT INTO events (at, kind, series_id, chapter_id, message) VALUES (?, ?, ?, ?, ?)",
        now,
        kind,
        series_id,
        chapter_id,
        message
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn recent(db: impl SqliteExecutor<'_>, limit: i64) -> sqlx::Result<Vec<HistoryEntry>> {
    sqlx::query_as!(
        HistoryEntry,
        r#"SELECT id, at AS "at: _", kind, series_id, chapter_id, message
           FROM events ORDER BY at DESC, id DESC LIMIT ?"#,
        limit
    )
    .fetch_all(db)
    .await
}
