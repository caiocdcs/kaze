//! Background jobs: a persistent queue driven by a tokio `mpsc` channel.
//!
//! * [`JobQueue`] is the cloneable handle producers use (API, scheduler,
//!   other jobs). `submit` writes the job to SQLite, then sends its id
//!   down the channel.
//! * The [`dispatcher`] is a single task that owns the receiving end and
//!   all mutable state (running jobs, retry timers), so it needs no locks.
//! * The database is the source of truth: the channel only says "job N is
//!   ready", and a restart reloads whatever was queued.

mod dispatcher;
mod handlers;
pub mod scheduler;

use std::time::Duration;
use ts_rs::TS;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};

pub use self::dispatcher::Dispatcher;
use crate::{
    db,
    events::{Event, Events},
    integrations::Message,
};

/// A unit of background work. Stored as JSON in `jobs.payload`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(TS)]
#[ts(export)]
pub enum Job {
    /// Fetch the chapter list and queue downloads for wanted chapters.
    RefreshSeries { series_id: i64, source: String },
    /// Download one chapter into the library.
    DownloadChapter { chapter_id: i64, source: String },
    /// Check a source's latest updates for followed series.
    SyncLatest { source: String },
    /// Send a message to a notification integration.
    Notify {
        integration_id: i64,
        message: Message,
    },
    /// Ask a library server (Komga, Kavita) to scan.
    ScanLibrary { integration_id: i64 },
}

impl Job {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::RefreshSeries { .. } => "refresh_series",
            Self::DownloadChapter { .. } => "download_chapter",
            Self::SyncLatest { .. } => "sync_latest",
            Self::Notify { .. } => "notify",
            Self::ScanLibrary { .. } => "scan_library",
        }
    }

    /// Two jobs with the same key are the same work; only one may be
    /// queued or running at a time.
    #[must_use]
    pub fn dedupe_key(&self) -> String {
        match self {
            Self::RefreshSeries { series_id, .. } => format!("refresh:{series_id}"),
            Self::DownloadChapter { chapter_id, .. } => format!("download:{chapter_id}"),
            Self::SyncLatest { source } => format!("latest:{source}"),
            // Every message is its own; nothing to merge.
            Self::Notify { integration_id, .. } => format!(
                "notify:{integration_id}:{}",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ),
            // A scan picks up everything on disk, so one pending scan is enough.
            Self::ScanLibrary { integration_id } => format!("scan:{integration_id}"),
        }
    }

    /// The source this job talks to, for per-source concurrency limits.
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::RefreshSeries { source, .. }
            | Self::DownloadChapter { source, .. }
            | Self::SyncLatest { source } => Some(source),
            Self::Notify { .. } | Self::ScanLibrary { .. } => None,
        }
    }
}

/// Messages from [`JobQueue`] handles to the dispatcher.
#[derive(Debug)]
enum Command {
    /// A queued job is ready to run.
    Run(i64),
    /// Stop a running job (already marked cancelled in the database);
    /// `stopped` fires once it no longer runs.
    Cancel {
        id: i64,
        stopped: Option<oneshot::Sender<()>>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("the job dispatcher has stopped")]
    Stopped,
}

/// Handle for submitting and controlling jobs. Cheap to clone.
#[derive(Debug, Clone)]
pub struct JobQueue {
    db: SqlitePool,
    events: Events,
    commands: mpsc::Sender<Command>,
}

impl JobQueue {
    /// Queue a job; `None` if an identical job is already queued or running.
    pub async fn submit(&self, job: Job) -> Result<Option<i64>, QueueError> {
        let Some(id) = db::jobs::insert(&self.db, &job).await? else {
            return Ok(None);
        };
        self.events.emit(Event::JobQueued { job_id: id, job });
        // `send` waits when the channel is full: natural backpressure.
        self.commands
            .send(Command::Run(id))
            .await
            .map_err(|_| QueueError::Stopped)?;
        Ok(Some(id))
    }

    pub async fn cancel(&self, id: i64) -> Result<bool, QueueError> {
        self.cancel_with(id, None).await
    }

    /// Cancel, then wait until the job has really stopped, its file writes
    /// included. For callers about to delete what the job writes to.
    pub async fn cancel_and_wait(&self, id: i64) -> Result<bool, QueueError> {
        let (tx, rx) = oneshot::channel();
        let cancelled = self.cancel_with(id, Some(tx)).await?;
        if cancelled {
            rx.await.map_err(|_| QueueError::Stopped)?;
        }
        Ok(cancelled)
    }

    async fn cancel_with(
        &self,
        id: i64,
        stopped: Option<oneshot::Sender<()>>,
    ) -> Result<bool, QueueError> {
        if !db::jobs::cancel(&self.db, id).await? {
            return Ok(false);
        }
        self.commands
            .send(Command::Cancel { id, stopped })
            .await
            .map_err(|_| QueueError::Stopped)?;
        self.events.emit(Event::JobCancelled { job_id: id });
        Ok(true)
    }

    /// Run a failed or cancelled job again.
    pub async fn retry(&self, id: i64) -> Result<bool, QueueError> {
        if !db::jobs::retry(&self.db, id).await? {
            return Ok(false);
        }
        self.commands
            .send(Command::Run(id))
            .await
            .map_err(|_| QueueError::Stopped)?;
        Ok(true)
    }
}

/// How long to wait before retry number `attempt` (1-based): doubles each
/// time from `base`, capped at `max`.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub base: Duration,
    pub max: Duration,
}

impl RetryPolicy {
    #[must_use]
    pub fn delay(&self, attempt: i64) -> Duration {
        let exponent = u32::try_from(attempt.saturating_sub(1))
            .unwrap_or(u32::MAX)
            .min(16);
        self.base.saturating_mul(1 << exponent).min(self.max)
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(60),
            max: Duration::from_secs(60 * 60),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delays_double_and_cap() {
        let policy = RetryPolicy {
            base: Duration::from_secs(10),
            max: Duration::from_secs(60),
        };
        let delays: Vec<u64> = (1..=5).map(|a| policy.delay(a).as_secs()).collect();
        assert_eq!(delays, [10, 20, 40, 60, 60]);
        assert_eq!(policy.delay(1000).as_secs(), 60);
    }

    #[test]
    fn job_json_is_tagged() {
        let job = Job::DownloadChapter {
            chapter_id: 7,
            source: "weebcentral".into(),
        };
        let json = serde_json::to_string(&job).unwrap();
        assert_eq!(
            json,
            r#"{"type":"download_chapter","chapter_id":7,"source":"weebcentral"}"#
        );
        assert_eq!(job.dedupe_key(), "download:7");
    }
}

#[cfg(test)]
mod pipeline_tests;
