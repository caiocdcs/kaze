//! Persistence for the job queue (see `crate::jobs`).

use chrono::{DateTime, Utc};
use kaze_core::ChapterNumber;
use serde::Serialize;
use sqlx::{SqliteExecutor, types::Json};
use ts_rs::TS;

use crate::jobs::Job;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
#[derive(TS)]
#[ts(export)]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct JobRecord {
    pub id: i64,
    #[ts(as = "Job")]
    pub job: Json<Job>,
    pub state: JobState,
    pub attempts: i64,
    pub max_attempts: i64,
    pub run_after: DateTime<Utc>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// What the job works on, for display; `None` for source-wide jobs.
    pub series_id: Option<i64>,
    pub series_title: Option<String>,
    pub chapter: Option<ChapterNumber>,
}

/// A job just claimed for running.
#[derive(Debug)]
pub struct Claimed {
    pub job: Job,
    pub attempts: i64,
    pub max_attempts: i64,
}

/// Queue a job unless an equivalent one is already queued or running.
/// Returns the new id, or `None` for a duplicate.
pub async fn insert(db: impl SqliteExecutor<'_>, job: &Job) -> sqlx::Result<Option<i64>> {
    let now = Utc::now();
    let payload = Json(job);
    let kind = job.kind();
    let key = job.dedupe_key();
    // The partial unique index on dedupe_key makes duplicates a no-op.
    sqlx::query_scalar!(
        r#"INSERT INTO jobs (kind, payload, dedupe_key, run_after, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT DO NOTHING
           RETURNING id AS "id!"
        "#,
        kind,
        payload,
        key,
        now,
        now,
        now,
    )
    .fetch_optional(db)
    .await
}

/// Atomically move a queued job to running. `None` if it was cancelled,
/// finished or claimed by someone else in the meantime.
pub async fn claim(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<Option<Claimed>> {
    let now = Utc::now();
    let row = sqlx::query!(
        r#"UPDATE jobs SET state = 'running', attempts = attempts + 1, updated_at = ?
           WHERE id = ? AND state = 'queued'
           RETURNING payload AS "payload: Json<Job>", attempts, max_attempts"#,
        now,
        id,
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| Claimed {
        job: r.payload.0,
        attempts: r.attempts,
        max_attempts: r.max_attempts,
    }))
}

pub async fn finish(
    db: impl SqliteExecutor<'_>,
    id: i64,
    state: JobState,
    error: Option<&str>,
) -> sqlx::Result<()> {
    let now = Utc::now();
    sqlx::query!(
        "UPDATE jobs SET state = ?, last_error = coalesce(?, last_error), updated_at = ? WHERE id = ?",
        state,
        error,
        now,
        id,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Put a job back in the queue to run again at `run_after`.
pub async fn requeue(
    db: impl SqliteExecutor<'_>,
    id: i64,
    run_after: DateTime<Utc>,
    error: Option<&str>,
) -> sqlx::Result<()> {
    let now = Utc::now();
    sqlx::query!(
        "UPDATE jobs SET state = 'queued', run_after = ?, last_error = coalesce(?, last_error), updated_at = ? WHERE id = ?",
        run_after,
        error,
        now,
        id,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Queued or running refreshes and downloads of one series.
pub async fn active_for_series(
    db: impl SqliteExecutor<'_>,
    series_id: i64,
) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar!(
        r#"SELECT id AS "id!" FROM jobs
           WHERE state IN ('queued', 'running')
             AND (json_extract(payload, '$.series_id') = ?1
                  OR json_extract(payload, '$.chapter_id') IN (SELECT id FROM chapters WHERE series_id = ?1))"#,
        series_id
    )
    .fetch_all(db)
    .await
}

/// Cancel a queued or running job. Returns whether anything changed.
pub async fn cancel(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<bool> {
    let now = Utc::now();
    let result = sqlx::query!(
        "UPDATE jobs SET state = 'cancelled', updated_at = ? WHERE id = ? AND state IN ('queued', 'running')",
        now,
        id,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Make a failed or cancelled job runnable again, with fresh attempts.
pub async fn retry(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<bool> {
    let now = Utc::now();
    let result = sqlx::query!(
        r#"UPDATE jobs SET state = 'queued', attempts = 0, run_after = ?, updated_at = ?
           WHERE id = ? AND state IN ('failed', 'cancelled')"#,
        now,
        now,
        id,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// After a restart: jobs that were running when the process died go back
/// to the queue. Returns every queued job with its due time.
pub async fn recover(db: &sqlx::SqlitePool) -> sqlx::Result<Vec<(i64, DateTime<Utc>)>> {
    let now = Utc::now();
    sqlx::query!(
        "UPDATE jobs SET state = 'queued', updated_at = ? WHERE state = 'running'",
        now
    )
    .execute(db)
    .await?;
    let rows = sqlx::query!(
        r#"SELECT id AS "id!", run_after AS "run_after: DateTime<Utc>" FROM jobs WHERE state = 'queued' ORDER BY id"#
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| (r.id, r.run_after)).collect())
}

/// Queued and running jobs, then the most recent finished ones.
pub async fn list(db: impl SqliteExecutor<'_>, limit: i64) -> sqlx::Result<Vec<JobRecord>> {
    sqlx::query_as!(
        JobRecord,
        r#"SELECT j.id, j.payload AS "job: Json<Job>", j.state AS "state: JobState", j.attempts,
                  j.max_attempts, j.run_after AS "run_after: _", j.last_error,
                  j.created_at AS "created_at: _", j.updated_at AS "updated_at: _",
                  s.id AS "series_id?", s.title AS "series_title?", c.number AS "chapter?: ChapterNumber"
           FROM jobs j
           LEFT JOIN chapters c ON c.id = j.payload ->> '$.chapter_id'
           LEFT JOIN series s ON s.id = coalesce(c.series_id, j.payload ->> '$.series_id')
           ORDER BY j.state NOT IN ('queued', 'running'), j.updated_at DESC
           LIMIT ?"#,
        limit
    )
    .fetch_all(db)
    .await
}

pub async fn get(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<Option<JobRecord>> {
    sqlx::query_as!(
        JobRecord,
        r#"SELECT j.id, j.payload AS "job: Json<Job>", j.state AS "state: JobState", j.attempts,
                  j.max_attempts, j.run_after AS "run_after: _", j.last_error,
                  j.created_at AS "created_at: _", j.updated_at AS "updated_at: _",
                  s.id AS "series_id?", s.title AS "series_title?", c.number AS "chapter?: ChapterNumber"
           FROM jobs j
           LEFT JOIN chapters c ON c.id = j.payload ->> '$.chapter_id'
           LEFT JOIN series s ON s.id = coalesce(c.series_id, j.payload ->> '$.series_id')
           WHERE j.id = ?"#,
        id
    )
    .fetch_optional(db)
    .await
}
