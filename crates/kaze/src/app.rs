//! Wiring: shared context plus the background tasks around it.

use std::{path::PathBuf, sync::Arc};

use kaze_core::{ChapterNumber, SeriesKey, SourceError};
use kaze_sources::{Registry, flaresolverr::FlareSolverr};
use sqlx::SqlitePool;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    db::{self, series::NewSeries},
    download::naming::sanitize_file_name,
    events::{self, Events},
    import,
    integrations::notifier,
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
    /// For integrations (Komga, webhooks, ...); sources have their own.
    pub http: reqwest::Client,
    /// The sources' FlareSolverr fallback, pointed at the configured URL.
    pub flaresolverr: FlareSolverr,
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
    #[error("another series already uses the folder {0:?}")]
    FolderTaken(String),
    #[error("reading the folder: {0}")]
    Io(#[from] std::io::Error),
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
        Self::start_with(ctx, retry, notifier::Timing::default()).await
    }

    pub async fn start_with(
        ctx: Context,
        retry: RetryPolicy,
        notify: notifier::Timing,
    ) -> sqlx::Result<Self> {
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
        tasks.spawn(apply_flaresolverr(
            ctx.settings.subscribe(),
            ctx.flaresolverr.clone(),
            shutdown.clone(),
        ));
        tasks.spawn(notifier::run(
            ctx.db.clone(),
            ctx.events.subscribe(),
            jobs.clone(),
            notify,
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

    /// Follow a series whose files already sit in `folder` of the library:
    /// store its chapters, mark the ones on disk as downloaded and queue
    /// the rest that `monitor` wants. Returns the series id and how many
    /// chapters were matched to files.
    pub async fn import(
        &self,
        folder: &str,
        source_id: &str,
        key: &SeriesKey,
        monitor: Monitor,
        from: Option<&ChapterNumber>,
    ) -> Result<(i64, usize), FollowError> {
        let source = self
            .ctx
            .sources
            .get(source_id)
            .ok_or_else(|| FollowError::UnknownSource(source_id.to_owned()))?;
        if let Some(id) = db::series::find_by_source(&self.ctx.db, source_id, key).await? {
            return Err(FollowError::AlreadyFollowing(id));
        }
        if db::series::folder_taken(&self.ctx.db, folder).await? {
            return Err(FollowError::FolderTaken(folder.to_owned()));
        }
        let dir = self.ctx.library.join(folder);
        let files = {
            let dir = dir.clone();
            tokio::task::spawn_blocking(move || {
                import::cbz_files(&dir).map(|files| import::chapter_files(&files))
            })
            .await
            .map_err(|e| FollowError::Io(std::io::Error::other(e)))??
        };

        let (series, chapters) = tokio::try_join!(source.series(key), source.chapters(key))?;
        let id = db::series::insert(
            &self.ctx.db,
            &NewSeries {
                series: &series,
                source: source_id,
                folder,
                monitor,
                monitor_from: from,
            },
        )
        .await?;
        // Chapters already on disk are never wanted; the rest follow `monitor`
        // as for a newly followed series.
        db::chapters::insert_new(&self.ctx.db, id, source_id, &chapters, |c| {
            !files.contains_key(&c.number) && monitor.wants(&c.number, from, true)
        })
        .await?;
        let mut matched = 0;
        for chapter in db::chapters::for_series(&self.ctx.db, id).await? {
            if let Some(path) = files.get(&chapter.number).and_then(|paths| paths.first()) {
                db::chapters::mark_downloaded(&self.ctx.db, chapter.id, &path.to_string_lossy())
                    .await?;
                matched += 1;
            }
        }
        db::series::mark_checked(&self.ctx.db, id).await?;
        for chapter_id in db::chapters::wanted_ids(&self.ctx.db, id).await? {
            self.jobs
                .submit(Job::DownloadChapter {
                    chapter_id,
                    source: source_id.to_owned(),
                })
                .await?;
        }
        Ok((id, matched))
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

/// Keep the sources' FlareSolverr URL in step with the settings.
async fn apply_flaresolverr(
    mut settings: tokio::sync::watch::Receiver<Arc<crate::settings::Settings>>,
    flaresolverr: FlareSolverr,
    shutdown: CancellationToken,
) {
    loop {
        let url = settings.borrow_and_update().flaresolverr_url.clone();
        flaresolverr.set_url(url);
        tokio::select! {
            () = shutdown.cancelled() => return,
            changed = settings.changed() => if changed.is_err() { return },
        }
    }
}
