//! Wiring: shared context plus the background tasks around it.

use std::{path::PathBuf, sync::Arc};

use kaze_core::{ChapterNumber, SeriesKey, SourceError};
use kaze_sources::Registry;
use sqlx::SqlitePool;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    db::{self, series::NewSeries},
    download::naming::sanitize_file_name,
    events::{self, Events},
    jobs::{Dispatcher, Job, JobQueue, QueueError, RetryPolicy, scheduler},
    library::Monitor,
    settings::SettingsStore,
};

/// Everything a job or request handler needs, shared behind an `Arc`.
#[derive(Debug)]
pub struct Context {
    pub db: SqlitePool,
    pub sources: Registry,
    pub events: Events,
    /// Library root: each series gets a folder inside.
    pub library: PathBuf,
    pub settings: SettingsStore,
}

/// The running application.
pub struct App {
    pub ctx: Arc<Context>,
    pub jobs: JobQueue,
    /// Scheduled tasks: list them, run one now.
    pub scheduled: scheduler::Tasks,
    shutdown: CancellationToken,
    tasks: TaskTracker,
}

#[derive(Debug, thiserror::Error)]
pub enum FollowError {
    #[error("unknown source {0:?}")]
    UnknownSource(String),
    #[error("already following this series (id {0})")]
    AlreadyFollowing(i64),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Queue(#[from] QueueError),
}

impl App {
    /// Start the dispatcher, scheduler and history writer.
    pub async fn start(ctx: Context, retry: RetryPolicy) -> sqlx::Result<Self> {
        // Before anything can queue new work: whatever is queued now was
        // left over from the previous run.
        let leftover = db::jobs::recover(&ctx.db).await?;
        if !leftover.is_empty() {
            tracing::info!(count = leftover.len(), "resuming queued jobs");
        }
        let ctx = Arc::new(ctx);
        let shutdown = CancellationToken::new();
        // TaskTracker: spawn background tasks, later wait for all of them.
        let tasks = TaskTracker::new();

        let (dispatcher, jobs) = Dispatcher::new(Arc::clone(&ctx), retry, shutdown.clone());
        tasks.spawn(dispatcher.run(jobs.clone(), leftover));
        let (scheduled, run_now) = scheduler::channel();
        tasks.spawn(scheduler::run(
            Arc::clone(&ctx),
            jobs.clone(),
            scheduled.clone(),
            run_now,
            shutdown.clone(),
        ));
        tasks.spawn(events::record_history(
            ctx.db.clone(),
            ctx.events.subscribe(),
            shutdown.clone(),
        ));
        tasks.close();

        Ok(Self {
            ctx,
            jobs,
            scheduled,
            shutdown,
            tasks,
        })
    }

    /// Follow a series and queue its first refresh. Returns the series id.
    pub async fn follow(
        &self,
        source_id: &str,
        key: &SeriesKey,
        monitor: Monitor,
        from: Option<&ChapterNumber>,
    ) -> Result<i64, FollowError> {
        let source = self
            .ctx
            .sources
            .get(source_id)
            .ok_or_else(|| FollowError::UnknownSource(source_id.to_owned()))?;
        if let Some(id) = db::series::find_by_source(&self.ctx.db, source_id, key).await? {
            return Err(FollowError::AlreadyFollowing(id));
        }
        let series = source.series(key).await?;

        // Two series may share a title on different sources: keep folders apart.
        let mut folder = sanitize_file_name(&series.title);
        if db::series::folder_taken(&self.ctx.db, &folder).await? {
            folder = sanitize_file_name(&format!("{} ({})", series.title, source.meta().name));
        }
        let id = db::series::insert(
            &self.ctx.db,
            &NewSeries {
                series: &series,
                source: source_id,
                folder: &folder,
                monitor,
                monitor_from: from,
            },
        )
        .await?;
        self.jobs
            .submit(Job::RefreshSeries {
                series_id: id,
                source: source_id.to_owned(),
            })
            .await?;
        Ok(id)
    }

    /// Fires when shutdown starts; long-lived responses (SSE) end with it.
    #[must_use]
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// Stop background work and wait for it; running jobs are re-queued.
    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        self.tasks.wait().await;
    }
}
