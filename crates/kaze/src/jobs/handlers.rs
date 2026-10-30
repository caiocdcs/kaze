//! What each kind of job actually does.

use std::{collections::HashSet, path::Path};

use kaze_core::{Source, SourceError};
use tokio_util::task::TaskTracker;

use super::{Job, JobQueue, QueueError};
use crate::{
    app::Context,
    db,
    download::{self, ChapterMeta, DownloadError, Outcome},
    events::Event,
    integrations::{self, IntegrationError, Message},
    library::{self, SeriesRecord},
};

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Queue(#[from] QueueError),
    #[error(transparent)]
    Integration(#[from] IntegrationError),
    /// Something the job needs no longer exists; retrying will not help.
    #[error("{0}")]
    Gone(String),
}

impl JobError {
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Source(e) | Self::Download(DownloadError::Source(e)) => e.is_retryable(),
            // A page that is not an image is usually a transient CDN error.
            Self::Download(DownloadError::NotAnImage { .. }) | Self::Db(_) => true,
            Self::Integration(e) => e.is_retryable(),
            Self::Download(_) | Self::Queue(_) | Self::Gone(_) => false,
        }
    }

    /// Whether the site itself misbehaved, as opposed to kaze (disk,
    /// database) or the library. Only these count against source health.
    pub const fn is_source_failure(&self) -> bool {
        matches!(
            self,
            Self::Source(_)
                | Self::Download(
                    DownloadError::Source(_)
                        | DownloadError::NotAnImage { .. }
                        | DownloadError::NoPages
                )
        )
    }
}

/// `writes` tracks blocking file writes, which outlive a cancelled job.
pub async fn run(
    ctx: &Context,
    queue: &JobQueue,
    job_id: i64,
    job: &Job,
    writes: &TaskTracker,
) -> Result<(), JobError> {
    match job {
        Job::RefreshSeries { series_id, source } => {
            refresh_series(ctx, queue, *series_id, source).await
        }
        Job::DownloadChapter { chapter_id, .. } => {
            download_chapter(ctx, job_id, *chapter_id, writes).await
        }
        Job::SyncLatest { source } => sync_latest(ctx, queue, source).await,
        Job::Notify {
            integration_id,
            message,
        } => deliver(ctx, *integration_id, Some(message), None).await,
        Job::ScanLibrary {
            integration_id,
            language,
        } => deliver(ctx, *integration_id, None, language.as_deref()).await,
        Job::MeasureLibrary => measure_library(ctx).await,
    }
}

/// Re-read the size of every chapter file: fills in files from before
/// sizes were recorded, and anything changed outside kaze.
async fn measure_library(ctx: &Context) -> Result<(), JobError> {
    let files = db::chapters::files(&ctx.db).await?;
    let (mut total, mut changed) = (0_i64, 0_usize);
    for (id, path, recorded) in &files {
        let size = library::file_size(path).await;
        total += size.unwrap_or(0);
        if size != *recorded {
            db::chapters::set_size(&ctx.db, *id, size).await?;
            changed += 1;
        }
    }
    tracing::info!(
        files = files.len(),
        changed,
        total_bytes = total,
        "measured library"
    );
    Ok(())
}

fn source<'a>(ctx: &'a Context, id: &str) -> Result<&'a dyn Source, JobError> {
    ctx.sources
        .get(id)
        .map(AsRef::as_ref)
        .ok_or_else(|| JobError::Gone(format!("source {id:?} is not available")))
}

async fn load_series(ctx: &Context, id: i64) -> Result<SeriesRecord, JobError> {
    let series = db::series::get(&ctx.db, id)
        .await?
        .ok_or_else(|| JobError::Gone(format!("series {id} was removed")))?;
    // Fills the job span's empty `series` field (see the dispatcher).
    tracing::Span::current().record("series", series.title.as_str());
    Ok(series)
}

/// Pull one source's chapters (and, from the primary, the metadata), store
/// new ones and queue every wanted one.
async fn refresh_series(
    ctx: &Context,
    queue: &JobQueue,
    series_id: i64,
    source_id: &str,
) -> Result<(), JobError> {
    let record = load_series(ctx, series_id).await?;
    let link = db::series::sources(&ctx.db, series_id)
        .await?
        .into_iter()
        .find(|l| l.source == source_id)
        .ok_or_else(|| JobError::Gone(format!("series {series_id} no longer uses {source_id}")))?;
    let source = source(ctx, source_id)?;
    let chapters = source.chapters(&link.source_key, &record.language);
    let (title, chapters) = if link.priority == 0 {
        let (series, chapters) =
            tokio::try_join!(source.series(&link.source_key, &record.language), chapters)?;
        db::series::update_metadata(&ctx.db, series_id, &series).await?;
        // Saved now, not with the first chapter, so the library shows it at once.
        let cover = series.cover_url.as_ref().map(ToString::to_string);
        let dir = ctx.libraries.root(&record.language).join(&record.folder);
        save_cover(source, &dir, cover.as_deref()).await;
        (series.title, chapters)
    } else {
        (record.title, chapters.await?)
    };

    // A number the series already lists keeps its monitor choice on every
    // source. For new numbers, a source's first check decides between
    // `All` and `Future`, so a source added later doesn't backfill.
    let known = db::chapters::known_numbers(&ctx.db, series_id).await?;
    let initial = link.checked_at.is_none();
    let from = record.monitor_from.as_ref();
    db::chapters::insert_new(&ctx.db, series_id, source_id, &chapters, |c| {
        known
            .get(&c.number)
            .copied()
            .unwrap_or_else(|| record.monitor.wants(&c.number, from, initial))
    })
    .await?;
    db::series::mark_checked(&ctx.db, series_id, source_id).await?;

    // Copies of known numbers are not news.
    let found: HashSet<_> = chapters
        .iter()
        .map(|c| &c.number)
        .filter(|n| !known.contains_key(*n))
        .collect();
    if !found.is_empty() {
        ctx.events.emit(Event::ChaptersFound {
            series_id,
            series: title,
            count: found.len(),
        });
    }
    // All wanted chapters, not only new ones, so earlier failures get
    // another chance; duplicates are ignored by the queue.
    queue_wanted(ctx, queue, series_id).await
}

/// Queue the download of every wanted chapter of a series, each from the
/// source of its picked copy.
async fn queue_wanted(ctx: &Context, queue: &JobQueue, series_id: i64) -> Result<(), JobError> {
    for pick in db::chapters::wanted_for_series(&ctx.db, series_id).await? {
        queue.submit(pick.into()).await?;
    }
    Ok(())
}

/// Best effort: a missing cover never fails the job.
async fn save_cover(source: &dyn Source, dir: &Path, url: Option<&str>) {
    let Some(url) = url.and_then(|u| u.parse().ok()) else {
        return;
    };
    if let Err(err) = download::cover(source, &url, dir).await {
        tracing::warn!(dir = %dir.display(), "could not save cover: {err}");
    }
}

async fn download_chapter(
    ctx: &Context,
    job_id: i64,
    chapter_id: i64,
    writes: &TaskTracker,
) -> Result<(), JobError> {
    let chapter = db::chapters::get(&ctx.db, chapter_id)
        .await?
        .ok_or_else(|| JobError::Gone(format!("chapter {chapter_id} was removed")))?;
    // Another copy of the number may have landed since this one was queued.
    if chapter.file_path.is_some() || db::chapters::number_on_disk(&ctx.db, chapter_id).await? {
        return Ok(());
    }
    let series = load_series(ctx, chapter.series_id).await?;
    let source = source(ctx, &chapter.source)?;
    let dir = ctx.libraries.root(&series.language).join(&series.folder);

    save_cover(source, &dir, series.cover_url.as_deref()).await;

    let meta = ChapterMeta {
        series_title: series.title.clone(),
        number: chapter.number.clone(),
        volume: chapter.volume.and_then(|v| u32::try_from(v).ok()),
        title: chapter.title.clone(),
        authors: series.authors.0.clone(),
        tags: series.tags.0.clone(),
        year: series.year.and_then(|y| u16::try_from(y).ok()),
        summary: series.description.clone(),
        language: chapter.language.clone(),
        url: Some(chapter.url.clone()),
    };
    let progress = |done, total| {
        ctx.events.emit(Event::JobProgress {
            job_id,
            done,
            total,
        });
    };
    let settings = ctx.settings.current();
    let options = download::Options {
        template: settings.naming_template.clone(),
        concurrency: settings.page_concurrency,
        writes: writes.clone(),
    };
    let result = download::chapter(
        source,
        &chapter.source_key,
        &meta,
        &dir,
        &options,
        &progress,
    )
    .await;

    let outcome = match result {
        Ok(outcome) => outcome,
        Err(err) => {
            ctx.events.emit(Event::ChapterFailed {
                job_id,
                series_id: series.id,
                chapter_id,
                series: series.title.clone(),
                chapter: chapter.number.to_string(),
                error: err.to_string(),
            });
            return Err(err.into());
        }
    };
    let size = library::file_size(outcome.path()).await;
    let path = outcome.path().to_string_lossy().into_owned();
    db::chapters::mark_downloaded(&ctx.db, chapter_id, &path, size).await?;
    if matches!(outcome, Outcome::Downloaded { .. }) {
        tracing::info!(series = series.title, chapter = %chapter.number, path, "downloaded");
        ctx.events.emit(Event::ChapterDownloaded {
            series_id: series.id,
            chapter_id,
            series: series.title,
            chapter: chapter.number.to_string(),
            path,
            language: series.language,
        });
    }
    Ok(())
}

/// One request to the source's "latest updates" instead of one per series.
async fn sync_latest(ctx: &Context, queue: &JobQueue, source_id: &str) -> Result<(), JobError> {
    let keys = match source(ctx, source_id)?.latest_updates().await {
        Ok(keys) => keys,
        Err(SourceError::Unsupported(_)) => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    for key in keys {
        for series_id in db::series::find_all_by_source(&ctx.db, source_id, &key).await? {
            queue
                .submit(Job::RefreshSeries {
                    series_id,
                    source: source_id.to_owned(),
                })
                .await?;
        }
    }
    Ok(())
}

/// Send a message, or scan `language`'s library when `message` is `None`,
/// and remember the outcome on the integration for the UI.
async fn deliver(
    ctx: &Context,
    id: i64,
    message: Option<&Message>,
    language: Option<&str>,
) -> Result<(), JobError> {
    let Some(integration) = db::integrations::get(&ctx.db, id).await? else {
        return Ok(()); // removed since; nothing to do
    };
    if !integration.enabled {
        return Ok(());
    }
    let config = &integration.config.0;
    let result = match message {
        Some(message) => integrations::send(&ctx.http, config, message).await,
        None => integrations::scan(&ctx.http, config, language).await,
    };
    let status = match &result {
        Ok(()) if message.is_some() => "Sent".to_owned(),
        Ok(()) => "Scanned".to_owned(),
        Err(err) => err.to_string(),
    };
    let action = if message.is_some() { "send" } else { "scan" };
    match &result {
        Ok(()) => tracing::info!(integration = integration.name, action, "{status}"),
        Err(err) => tracing::warn!(integration = integration.name, action, "{err}"),
    }
    db::integrations::record(&ctx.db, id, result.is_ok(), &status).await?;
    Ok(result?)
}
