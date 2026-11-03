//! Wiring: shared context plus the background tasks around it.

use std::sync::Arc;

use kaze_core::{AniListId, ChapterNumber, SeriesKey, Source, SourceError};
use kaze_sources::{Registry, anilist::AniList, flaresolverr::FlareSolverr};
use sqlx::SqlitePool;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    db::{self, series::NewSeries},
    download::{self, DownloadError, naming::sanitize_file_name},
    events::{self, Events},
    import,
    integrations::notifier,
    jobs::{Dispatcher, Job, JobQueue, QueueError, RetryPolicy, scheduler},
    library::{self, Libraries, MetadataLink, Monitor, SourceOrder},
    settings::SettingsStore,
};

/// Everything a job or request handler needs, shared behind an `Arc`.
#[derive(Debug)]
pub struct Context {
    pub db: SqlitePool,
    pub sources: Registry,
    pub events: Events,
    /// Library roots: each series gets a folder in its language's root.
    pub libraries: Libraries,
    pub settings: SettingsStore,
    /// For integrations (Komga, webhooks, ...); sources have their own.
    pub http: reqwest::Client,
    /// The sources' FlareSolverr fallback, pointed at the configured URL.
    pub flaresolverr: FlareSolverr,
    /// Where series metadata comes from once linked.
    pub anilist: AniList,
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
    #[error("{name} has no {language}; it offers {offered}")]
    Language {
        name: &'static str,
        language: String,
        offered: String,
    },
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

/// Why a series' sources could not be changed.
#[derive(Debug, thiserror::Error)]
pub enum SeriesSourceError {
    #[error("series not found")]
    SeriesNotFound,
    #[error("the series does not use {0:?}")]
    NotLinked(String),
    #[error(transparent)]
    Follow(#[from] FollowError),
    #[error("the series already uses {0:?}")]
    AlreadyLinked(String),
    #[error("a series needs at least one source")]
    LastSource,
    #[error("the new order must list each of the series' sources once")]
    BadOrder,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Queue(#[from] QueueError),
}

/// Why a series' AniList link could not be changed.
#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("series not found")]
    SeriesNotFound,
    #[error("AniList has no manga {0}")]
    NotOnAniList(AniListId),
    #[error("AniList failed: {0}")]
    AniList(SourceError),
    #[error(transparent)]
    Cover(#[from] DownloadError),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Queue(#[from] QueueError),
}

impl From<SourceError> for SeriesSourceError {
    fn from(err: SourceError) -> Self {
        Self::Follow(err.into())
    }
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

    /// The language to follow a series in: `wanted` (any spelling), else the
    /// default setting, else the source's main language.
    pub fn language_for(
        &self,
        source: &dyn Source,
        wanted: Option<&str>,
    ) -> Result<&'static str, FollowError> {
        let meta = source.meta();
        let offered = |tag: &str| meta.languages.iter().copied().find(|l| *l == tag);
        let Some(wanted) = wanted else {
            let default = self.ctx.settings.current().default_language.clone();
            return Ok(offered(&default).unwrap_or_else(|| meta.main_language()));
        };
        kaze_core::language::normalize(wanted)
            .and_then(|tag| offered(&tag))
            .ok_or_else(|| FollowError::Language {
                name: meta.name,
                language: wanted.to_owned(),
                offered: meta.languages.join(", "),
            })
    }

    /// Follow a series and queue its first refresh. Returns the series id.
    pub async fn follow(
        &self,
        source_id: &str,
        key: &SeriesKey,
        language: Option<&str>,
        monitor: Monitor,
        from: Option<&ChapterNumber>,
    ) -> Result<i64, FollowError> {
        let source = self
            .ctx
            .sources
            .get(source_id)
            .ok_or_else(|| FollowError::UnknownSource(source_id.to_owned()))?;
        let language = self.language_for(source.as_ref(), language)?;
        if let Some(id) = db::series::find_by_source(&self.ctx.db, source_id, key, language).await?
        {
            return Err(FollowError::AlreadyFollowing(id));
        }
        let series = source.series(key, language).await?;

        // Two series may share a title on different sources or languages:
        // keep folders apart.
        let name = source.meta().name;
        let mut folder = String::new();
        for candidate in [
            series.title.clone(),
            format!("{} ({name})", series.title),
            format!("{} ({name} {language})", series.title),
        ] {
            folder = sanitize_file_name(&candidate);
            if !db::series::folder_taken(&self.ctx.db, &folder).await? {
                break;
            }
        }
        let id = db::series::insert(
            &self.ctx.db,
            &NewSeries {
                series: &series,
                source: source_id,
                language,
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

    /// Also download a followed series from `source_id`, after its other
    /// sources: it fills their gaps and takes over when they break.
    pub async fn add_source(
        &self,
        series_id: i64,
        source_id: &str,
        key: &SeriesKey,
    ) -> Result<(), SeriesSourceError> {
        let series = db::series::get(&self.ctx.db, series_id)
            .await?
            .ok_or(SeriesSourceError::SeriesNotFound)?;
        let source = self
            .ctx
            .sources
            .get(source_id)
            .ok_or_else(|| FollowError::UnknownSource(source_id.to_owned()))?;
        let language = self.language_for(source.as_ref(), Some(&series.language))?;
        let linked = db::series::sources(&self.ctx.db, series_id).await?;
        if linked.iter().any(|l| l.source == source_id) {
            return Err(SeriesSourceError::AlreadyLinked(source_id.to_owned()));
        }
        // Merging two followed series is not supported.
        if let Some(id) = db::series::find_by_source(&self.ctx.db, source_id, key, language).await?
        {
            return Err(FollowError::AlreadyFollowing(id).into());
        }
        let found = source.series(key, language).await?;
        db::series::add_source(
            &self.ctx.db,
            series_id,
            source_id,
            &found.key,
            language,
            found.url.as_str(),
        )
        .await?;
        tracing::info!(series = series.title, source = source_id, "source added");
        self.jobs
            .submit(Job::RefreshSeries {
                series_id,
                source: source_id.to_owned(),
            })
            .await?;
        Ok(())
    }

    /// Set the order sources are tried in; the first becomes the primary.
    pub async fn reorder_sources(
        &self,
        series_id: i64,
        order: Vec<String>,
    ) -> Result<(), SeriesSourceError> {
        let linked = db::series::sources(&self.ctx.db, series_id).await?;
        if linked.is_empty() {
            return Err(SeriesSourceError::SeriesNotFound);
        }
        let order = SourceOrder::new(order, &linked).ok_or(SeriesSourceError::BadOrder)?;
        db::series::reorder_sources(&self.ctx.db, series_id, &order).await?;
        // A new primary brings its metadata.
        if linked[0].source != order.primary() {
            self.jobs
                .submit(Job::RefreshSeries {
                    series_id,
                    source: order.primary().to_owned(),
                })
                .await?;
        }
        Ok(())
    }

    /// Stop downloading a series from `source_id`; files it gave stay.
    pub async fn remove_source(
        &self,
        series_id: i64,
        source_id: &str,
    ) -> Result<(), SeriesSourceError> {
        let linked = db::series::sources(&self.ctx.db, series_id).await?;
        if linked.is_empty() {
            return Err(SeriesSourceError::SeriesNotFound);
        }
        if !linked.iter().any(|l| l.source == source_id) {
            return Err(SeriesSourceError::NotLinked(source_id.to_owned()));
        }
        if linked.len() == 1 {
            return Err(SeriesSourceError::LastSource);
        }
        // Wait for its downloads to stop: a file written after its chapter
        // rows are gone would sit in the folder untracked.
        for job_id in db::jobs::active_downloads_from(&self.ctx.db, series_id, source_id).await? {
            self.jobs.cancel(job_id).await?;
        }
        db::series::remove_source(&self.ctx.db, series_id, source_id).await?;
        tracing::info!(series_id, source = source_id, "source removed");
        // The next source is the primary now and brings its metadata.
        if linked[0].source == source_id {
            self.jobs
                .submit(Job::RefreshSeries {
                    series_id,
                    source: linked[1].source.clone(),
                })
                .await?;
        }
        // Chapters it was going to download now come from the others.
        for pick in db::chapters::wanted_for_series(&self.ctx.db, series_id).await? {
            self.jobs.submit(pick.into()).await?;
        }
        Ok(())
    }

    /// Link a series to an AniList entry by hand, or unlink it with `None`.
    /// Either way, automatic linking leaves it alone from now on.
    pub async fn link_metadata(
        &self,
        series_id: i64,
        anilist: Option<AniListId>,
    ) -> Result<(), MetadataError> {
        let series = db::series::get(&self.ctx.db, series_id)
            .await?
            .ok_or(MetadataError::SeriesNotFound)?;
        if let Some(id) = anilist {
            // Checked now, so a wrong id fails here, not in a background job.
            self.ctx.anilist.media(id).await.map_err(|err| match err {
                SourceError::NotFound { .. } => MetadataError::NotOnAniList(id),
                err => MetadataError::AniList(err),
            })?;
        }
        db::series::link_metadata(&self.ctx.db, series_id, anilist, MetadataLink::Manual).await?;
        tracing::info!(series = series.title, anilist = ?anilist, "metadata linked by hand");
        let job = if anilist.is_some() {
            Job::RefreshMetadata { series_id }
        } else {
            // The source's metadata and cover come back with its refresh.
            let dir = self
                .ctx
                .libraries
                .root(&series.language)
                .join(&series.folder);
            download::remove_cover(&dir).await?;
            Job::RefreshSeries {
                series_id,
                source: series.source,
            }
        };
        self.jobs.submit(job).await?;
        Ok(())
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
        language: Option<&str>,
        monitor: Monitor,
        from: Option<&ChapterNumber>,
    ) -> Result<(i64, usize), FollowError> {
        let source = self
            .ctx
            .sources
            .get(source_id)
            .ok_or_else(|| FollowError::UnknownSource(source_id.to_owned()))?;
        let language = self.language_for(source.as_ref(), language)?;
        if let Some(id) = db::series::find_by_source(&self.ctx.db, source_id, key, language).await?
        {
            return Err(FollowError::AlreadyFollowing(id));
        }
        if db::series::folder_taken(&self.ctx.db, folder).await? {
            return Err(FollowError::FolderTaken(folder.to_owned()));
        }
        let dir = self.ctx.libraries.root(language).join(folder);
        let files = {
            let dir = dir.clone();
            tokio::task::spawn_blocking(move || {
                import::cbz_files(&dir).map(|files| import::chapter_files(&files))
            })
            .await
            .map_err(|e| FollowError::Io(std::io::Error::other(e)))??
        };

        let (series, chapters) =
            tokio::try_join!(source.series(key, language), source.chapters(key, language))?;
        let id = db::series::insert(
            &self.ctx.db,
            &NewSeries {
                series: &series,
                source: source_id,
                language,
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
                let size = library::file_size(path).await;
                db::chapters::mark_downloaded(
                    &self.ctx.db,
                    chapter.id,
                    &path.to_string_lossy(),
                    size,
                )
                .await?;
                matched += 1;
            }
        }
        db::series::mark_checked(&self.ctx.db, id, source_id).await?;
        for pick in db::chapters::wanted_for_series(&self.ctx.db, id).await? {
            self.jobs.submit(pick.into()).await?;
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
