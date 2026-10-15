//! Per-source health: the outcome of the jobs that talk to each source.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqliteExecutor;
use ts_rs::TS;

/// Failures in a row before a source counts as broken: one failed
/// attempt is often a blip that the retry fixes.
pub const FAILING_AFTER: i64 = 3;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SourceHealth {
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub failures_in_row: i64,
}

impl SourceHealth {
    #[must_use]
    pub const fn is_failing(&self) -> bool {
        self.failures_in_row >= FAILING_AFTER
    }
}

pub async fn record_success(db: impl SqliteExecutor<'_>, source: &str) -> sqlx::Result<()> {
    let now = Utc::now();
    sqlx::query!(
        r#"INSERT INTO source_health (source, last_success_at) VALUES (?1, ?2)
           ON CONFLICT (source) DO UPDATE SET last_success_at = ?2, failures_in_row = 0"#,
        source,
        now,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Returns the failures in a row, this one included.
pub async fn record_failure(
    db: impl SqliteExecutor<'_>,
    source: &str,
    error: &str,
) -> sqlx::Result<i64> {
    let now = Utc::now();
    sqlx::query_scalar!(
        r#"INSERT INTO source_health (source, last_error_at, last_error, failures_in_row)
           VALUES (?1, ?2, ?3, 1)
           ON CONFLICT (source) DO UPDATE
             SET last_error_at = ?2, last_error = ?3, failures_in_row = failures_in_row + 1
           RETURNING failures_in_row"#,
        source,
        now,
        error,
    )
    .fetch_one(db)
    .await
}

pub async fn get(db: impl SqliteExecutor<'_>, source: &str) -> sqlx::Result<SourceHealth> {
    let health = sqlx::query_as!(
        SourceHealth,
        r#"SELECT last_success_at AS "last_success_at: _", last_error_at AS "last_error_at: _",
                  last_error, failures_in_row
           FROM source_health WHERE source = ?"#,
        source
    )
    .fetch_optional(db)
    .await?;
    Ok(health.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[tokio::test]
    async fn failures_count_until_a_success() {
        let db = db::memory().await;
        assert_eq!(get(&db, "fake").await.unwrap(), SourceHealth::default());

        for n in 1..=FAILING_AFTER {
            assert_eq!(record_failure(&db, "fake", "503").await.unwrap(), n);
        }
        let health = get(&db, "fake").await.unwrap();
        assert!(health.is_failing());
        assert_eq!(health.last_error.as_deref(), Some("503"));

        record_success(&db, "fake").await.unwrap();
        let health = get(&db, "fake").await.unwrap();
        assert!(!health.is_failing());
        assert!(health.last_success_at.is_some());
        assert_eq!(health.last_error.as_deref(), Some("503"), "kept for the UI");
    }
}
