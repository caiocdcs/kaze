//! In-process event bus: what is happening right now.
//!
//! A `broadcast` channel: every subscriber (SSE clients, the history
//! writer, notifiers) gets its own copy of each event. Slow subscribers
//! that fall too far behind skip events (`Lagged`) instead of slowing
//! down the workers.

use serde::Serialize;
use sqlx::SqlitePool;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::{db, jobs::Job};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(TS)]
#[ts(export)]
pub enum Event {
    JobQueued {
        job_id: i64,
        job: Job,
    },
    JobStarted {
        job_id: i64,
        job: Job,
    },
    JobProgress {
        job_id: i64,
        done: usize,
        total: usize,
    },
    JobFinished {
        job_id: i64,
    },
    JobFailed {
        job_id: i64,
        error: String,
        will_retry: bool,
    },
    JobCancelled {
        job_id: i64,
    },
    ChaptersFound {
        series_id: i64,
        series: String,
        count: usize,
    },
    ChapterDownloaded {
        series_id: i64,
        chapter_id: i64,
        series: String,
        chapter: String,
        path: String,
        /// The series' language, so only its library is scanned.
        language: String,
    },
    /// Files removed from the library. `series_id` is gone when the series
    /// was unfollowed with its folder.
    FilesDeleted {
        series_id: Option<i64>,
        series: String,
        count: usize,
        language: String,
    },
    /// One attempt failed; `JobFailed` for the same job says whether it retries.
    ChapterFailed {
        job_id: i64,
        series_id: i64,
        chapter_id: i64,
        series: String,
        chapter: String,
        error: String,
    },
    /// A source just reached `FAILING_AFTER` failures in a row. Sent once
    /// per outage: further failures stay quiet until a success resets it.
    SourceBroken {
        source: String,
        name: String,
        failures: i64,
        error: String,
    },
}

impl Event {
    /// Events worth keeping in the history table, as (kind, series, chapter, message).
    fn history(&self) -> Option<(&'static str, Option<i64>, Option<i64>, String)> {
        match self {
            Self::ChaptersFound {
                series_id,
                series,
                count,
            } => Some((
                "found",
                Some(*series_id),
                None,
                format!("{count} new chapter(s) of {series}"),
            )),
            Self::ChapterDownloaded {
                series_id,
                chapter_id,
                series,
                chapter,
                ..
            } => Some((
                "downloaded",
                Some(*series_id),
                Some(*chapter_id),
                format!("{series} chapter {chapter}"),
            )),
            Self::ChapterFailed {
                series_id,
                chapter_id,
                series,
                chapter,
                error,
                ..
            } => Some((
                "failed",
                Some(*series_id),
                Some(*chapter_id),
                format!("{series} chapter {chapter}: {error}"),
            )),
            Self::FilesDeleted {
                series_id,
                series,
                count,
                ..
            } => Some((
                "deleted",
                *series_id,
                None,
                format!("Deleted {count} file(s) of {series}"),
            )),
            Self::SourceBroken {
                name,
                failures,
                error,
                ..
            } => Some((
                "source_broken",
                None,
                None,
                format!("{name} failed {failures} times in a row: {error}"),
            )),
            _ => None,
        }
    }
}

/// Cheap to clone; all clones publish to the same channel.
#[derive(Debug, Clone)]
pub struct Events(broadcast::Sender<Event>);

impl Events {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self(broadcast::channel(capacity).0)
    }

    pub fn emit(&self, event: Event) {
        // An error only means nobody is listening right now, which is fine.
        let _ = self.0.send(event);
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.0.subscribe()
    }
}

/// Persist the interesting events until shutdown.
pub async fn record_history(
    db: SqlitePool,
    mut events: broadcast::Receiver<Event>,
    shutdown: CancellationToken,
) {
    loop {
        let event = tokio::select! {
            () = shutdown.cancelled() => return,
            event = events.recv() => event,
        };
        match event {
            Ok(event) => {
                if let Some((kind, series, chapter, message)) = event.history()
                    && let Err(err) = db::events::insert(&db, kind, series, chapter, &message).await
                {
                    tracing::warn!("could not record history: {err}");
                }
            }
            Err(RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "history writer fell behind");
            }
            Err(RecvError::Closed) => return,
        }
    }
}
