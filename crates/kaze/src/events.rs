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
    },
    ChapterFailed {
        series_id: i64,
        chapter_id: i64,
        series: String,
        chapter: String,
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
            } => Some((
                "failed",
                Some(*series_id),
                Some(*chapter_id),
                format!("{series} chapter {chapter}: {error}"),
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
