//! End-to-end tests of the job pipeline against an in-memory database and
//! a fake source: follow → refresh → downloads → events.

use std::{sync::Arc, time::Duration};

use tokio::sync::broadcast;

use super::*;
use crate::{
    app::{App, Context},
    db::{self, jobs::JobState},
    events::{Event, Events},
    library::Monitor,
    settings::SettingsStore,
    testing::FakeSource,
};

struct Harness {
    app: App,
    source: Arc<FakeSource>,
    /// Sources after the first, from `start_many`.
    others: Vec<Arc<FakeSource>>,
    events: broadcast::Receiver<Event>,
    library: tempfile::TempDir,
    /// The pt-BR root, beside `library`.
    library_pt: tempfile::TempDir,
}

async fn start_with(source: FakeSource, db: sqlx::SqlitePool) -> Harness {
    start_many(vec![source], db).await
}

async fn start_many(sources: Vec<FakeSource>, db: sqlx::SqlitePool) -> Harness {
    start_full(sources, db, crate::testing::empty_anilist().await).await
}

async fn start_full(
    sources: Vec<FakeSource>,
    db: sqlx::SqlitePool,
    anilist: kaze_sources::anilist::AniList,
) -> Harness {
    let mut sources: Vec<Arc<FakeSource>> = sources.into_iter().map(Arc::new).collect();
    let others = sources.split_off(1);
    let source = sources.remove(0);
    let library = tempfile::tempdir().unwrap();
    let library_pt = tempfile::tempdir().unwrap();
    let events = Events::new(1024);
    let receiver = events.subscribe();
    // Keep the scheduler quiet after its startup run; tests queue work themselves.
    let quiet = serde_json::json!({ "sync_latest_minutes": 600, "refresh_hours": 2000, "backup_days": 0, "check_updates": false });
    let ctx = Context {
        settings: SettingsStore::load(db.clone(), quiet.as_object().unwrap().clone())
            .await
            .unwrap(),
        db,
        sources: kaze_sources::Registry::from_sources(
            std::iter::once(&source)
                .chain(&others)
                .map(|s| Arc::clone(s) as Arc<dyn kaze_core::Source>)
                .collect(),
        ),
        events,
        libraries: crate::library::Libraries::new(
            library.path().to_owned(),
            [("pt-BR".to_owned(), library_pt.path().to_owned())].into(),
        )
        .unwrap(),
        http: reqwest::Client::new(),
        flaresolverr: kaze_sources::http::Http::new(kaze_sources::http::HttpConfig::default())
            .unwrap()
            .flaresolverr(),
        anilist,
        backups: library.path().join(".backups"),
        logs: crate::logs::RecentLogs::new(16),
        // Never GitHub from tests; the check is off there anyway.
        updates: crate::updates::Updates::new("http://127.0.0.1:9/".parse().unwrap()),
    };
    let retry = RetryPolicy {
        base: Duration::from_millis(20),
        max: Duration::from_millis(100),
    };
    Harness {
        // Short notification batches so integration tests don't wait 30 s.
        app: App::start_with(ctx, retry, notify_timing()).await.unwrap(),
        source,
        others,
        events: receiver,
        library,
        library_pt,
    }
}

fn notify_timing() -> crate::integrations::notifier::Timing {
    crate::integrations::notifier::Timing {
        quiet: Duration::from_millis(100),
        max_wait: Duration::from_secs(2),
    }
}

async fn start(source: FakeSource) -> Harness {
    start_with(source, db::memory().await).await
}

impl Harness {
    /// Wait for the first event matching `pred`, failing after 5 seconds.
    async fn wait_for(&mut self, mut pred: impl FnMut(&Event) -> bool) -> Event {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = self.events.recv().await.expect("event bus closed");
                if pred(&event) {
                    return event;
                }
            }
        })
        .await
        .expect("timed out waiting for event")
    }

    async fn downloaded(&mut self, count: usize) -> Vec<String> {
        let mut chapters = Vec::new();
        while chapters.len() < count {
            if let Event::ChapterDownloaded { chapter, .. } = self
                .wait_for(|e| matches!(e, Event::ChapterDownloaded { .. }))
                .await
            {
                chapters.push(chapter);
            }
        }
        chapters.sort();
        chapters
    }

    async fn follow(&self, monitor: Monitor) -> i64 {
        self.app
            .follow("fake", &"S1".into(), None, monitor, None)
            .await
            .unwrap()
    }

    /// The series once `pred` holds, failing after 5 seconds.
    async fn series_once(
        &self,
        id: i64,
        pred: impl Fn(&crate::library::SeriesRecord) -> bool,
    ) -> crate::library::SeriesRecord {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let record = db::series::get(&self.app.ctx.db, id)
                    .await
                    .unwrap()
                    .unwrap();
                if pred(&record) {
                    return record;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("timed out waiting for the series")
    }

    /// Refresh the series from its source again and wait for the check.
    async fn refresh_again(&self, id: i64) -> crate::library::SeriesRecord {
        let before = self.series_once(id, |_| true).await.last_checked_at;
        let job = Job::RefreshSeries {
            series_id: id,
            source: "fake".into(),
        };
        self.app.jobs.submit(job).await.unwrap();
        self.series_once(id, |r| r.last_checked_at > before).await
    }
}

#[tokio::test]
async fn following_downloads_every_chapter() {
    let mut h = start(FakeSource::new().with_chapters(&["1", "2", "3"])).await;
    let series_id = h.follow(Monitor::All).await;

    assert_eq!(h.downloaded(3).await, ["1", "2", "3"]);
    let folder = h.library.path().join("Test Series");
    for n in 1..=3 {
        assert!(folder.join(format!("Test Series - Ch.{n}.cbz")).exists());
    }
    assert!(folder.join("cover.jpg").exists());
    assert_eq!(
        db::chapters::wanted(&h.app.ctx.db, 10).await.unwrap().len(),
        0
    );
    let record = db::series::get(&h.app.ctx.db, series_id)
        .await
        .unwrap()
        .unwrap();
    assert!(record.last_checked_at.is_some());
    h.app.shutdown().await;
}

#[tokio::test]
async fn each_language_downloads_into_its_root() {
    let mut h = start(FakeSource::new().with_chapters(&["1"])).await;
    h.follow(Monitor::All).await;
    h.app
        .follow("fake", &"S1".into(), Some("pt-BR"), Monitor::All, None)
        .await
        .unwrap();
    h.downloaded(2).await;

    assert!(
        h.library
            .path()
            .join("Test Series/Test Series - Ch.1.cbz")
            .exists()
    );
    // Same title, so the folder name differs; the root keeps them apart.
    let pt = h.library_pt.path().join("Test Series (Fake)");
    assert!(pt.join("Test Series - Ch.1.cbz").exists());
    assert!(pt.join("cover.jpg").exists());
    h.app.shutdown().await;
}

#[tokio::test]
async fn disk_usage_is_recorded_and_remeasured() {
    let mut h = start(FakeSource::new().with_chapters(&["1", "2"])).await;
    let series_id = h.follow(Monitor::All).await;
    h.downloaded(2).await;
    h.wait_for(|e| matches!(e, Event::JobFinished { .. })).await;
    let chapters = db::chapters::for_series(&h.app.ctx.db, series_id)
        .await
        .unwrap();
    assert!(chapters.iter().all(|c| c.size_bytes.is_some_and(|n| n > 0)));
    let total: i64 = chapters.iter().filter_map(|c| c.size_bytes).sum();
    let summary = &db::series::summaries(&h.app.ctx.db).await.unwrap()[0];
    assert_eq!(summary.size_bytes, total);

    // A file removed behind kaze's back stops counting after a measure.
    std::fs::remove_file(chapters[0].file_path.as_ref().unwrap()).unwrap();
    let job_id = h
        .app
        .jobs
        .submit(Job::MeasureLibrary)
        .await
        .unwrap()
        .unwrap();
    h.wait_for(|e| matches!(e, Event::JobFinished { job_id: id } if *id == job_id))
        .await;
    let summary = &db::series::summaries(&h.app.ctx.db).await.unwrap()[0];
    assert_eq!(summary.size_bytes, chapters[1].size_bytes.unwrap());
    h.app.shutdown().await;
}

#[tokio::test]
async fn future_monitoring_only_takes_new_chapters() {
    let mut h = start(FakeSource::new().with_chapters(&["1", "2"])).await;
    let series_id = h.follow(Monitor::Future).await;
    let Event::JobQueued { job_id, .. } = h
        .wait_for(|e| {
            matches!(
                e,
                Event::JobQueued {
                    job: Job::RefreshSeries { .. },
                    ..
                }
            )
        })
        .await
    else {
        unreachable!()
    };
    h.wait_for(|e| matches!(e, Event::ChaptersFound { count: 2, .. }))
        .await;
    // The startup sync may finish in between; a refresh queued while this
    // one still runs would be dropped as a duplicate.
    h.wait_for(|e| matches!(e, Event::JobFinished { job_id: id } if *id == job_id))
        .await;

    h.source.add_chapter("3");
    h.app
        .jobs
        .submit(Job::RefreshSeries {
            series_id,
            source: "fake".into(),
        })
        .await
        .unwrap()
        .expect("the first refresh has finished");
    assert_eq!(h.downloaded(1).await, ["3"]);
    assert_eq!(
        h.source.fetches(),
        2 + 1,
        "two pages of chapter 3 plus the cover"
    );
    h.app.shutdown().await;
}

/// Primary "fake" with `ours`, and "other" (not linked yet) with `theirs`.
async fn two_sources(ours: &[&str], theirs: &[&str]) -> Harness {
    let other = FakeSource::new()
        .with_id("other", "Other")
        .with_chapters(theirs);
    start_many(
        vec![FakeSource::new().with_chapters(ours), other],
        db::memory().await,
    )
    .await
}

/// (number, source) of every chapter on disk, in reading order.
async fn on_disk(h: &Harness, series_id: i64) -> Vec<(String, String)> {
    db::chapters::for_series(&h.app.ctx.db, series_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|c| c.file_path.is_some())
        .map(|c| (c.number.to_string(), c.source))
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(n, s)| ((*n).to_owned(), (*s).to_owned()))
        .collect()
}

#[tokio::test]
async fn a_second_source_fills_the_gaps() {
    let mut h = two_sources(&["1", "2"], &["1", "2", "3"]).await;
    let series_id = h.follow(Monitor::All).await;
    assert_eq!(h.downloaded(2).await, ["1", "2"]);

    h.app
        .add_source(series_id, "other", &"S1".into())
        .await
        .unwrap();
    assert_eq!(h.downloaded(1).await, ["3"]);
    assert_eq!(
        on_disk(&h, series_id).await,
        pairs(&[("1", "fake"), ("2", "fake"), ("3", "other")])
    );
    h.app.shutdown().await;
}

#[tokio::test]
async fn a_broken_source_hands_over_to_the_next() {
    let mut h = two_sources(&["1"], &["1"]).await;
    h.source
        .fail_images
        .store(usize::MAX, std::sync::atomic::Ordering::SeqCst);
    let series_id = h.follow(Monitor::All).await;
    h.app
        .add_source(series_id, "other", &"S1".into())
        .await
        .unwrap();

    assert_eq!(h.downloaded(1).await, ["1"]);
    assert_eq!(on_disk(&h, series_id).await, pairs(&[("1", "other")]));
    h.app.shutdown().await;
}

#[tokio::test]
async fn a_source_added_later_does_not_backfill_future_series() {
    let mut h = two_sources(&["1", "2"], &["1", "2", "3"]).await;
    let series_id = h.follow(Monitor::Future).await;
    h.wait_for(|e| matches!(e, Event::ChaptersFound { count: 2, .. }))
        .await;

    h.app
        .add_source(series_id, "other", &"S1".into())
        .await
        .unwrap();
    // Chapter 3 predates "other"'s first check, like 1 and 2 did the series'.
    h.wait_for(|e| matches!(e, Event::ChaptersFound { count: 1, .. }))
        .await;
    assert_eq!(
        db::chapters::wanted(&h.app.ctx.db, 10).await.unwrap().len(),
        0
    );

    h.others[0].add_chapter("4");
    let refresh = Job::RefreshSeries {
        series_id,
        source: "other".into(),
    };
    // The first refresh of "other" may still be finishing.
    while h.app.jobs.submit(refresh.clone()).await.unwrap().is_none() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(h.downloaded(1).await, ["4"]);
    assert_eq!(on_disk(&h, series_id).await, pairs(&[("4", "other")]));
    h.app.shutdown().await;
}

#[tokio::test]
async fn removing_the_primary_refreshes_the_next_source() {
    let mut h = two_sources(&["1"], &["1", "2"]).await;
    let series_id = h.follow(Monitor::All).await;
    h.app
        .add_source(series_id, "other", &"S1".into())
        .await
        .unwrap();
    assert_eq!(h.downloaded(2).await, ["1", "2"]);

    h.app.remove_source(series_id, "fake").await.unwrap();
    h.wait_for(|e| {
        matches!(
            e,
            Event::JobQueued {
                job: Job::RefreshSeries { source, .. },
                ..
            } if source == "other"
        )
    })
    .await;
    let record = db::series::get(&h.app.ctx.db, series_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.source, "other");
    h.app.shutdown().await;
}

#[tokio::test]
async fn removing_a_source_stops_its_downloads() {
    let mut h = two_sources(&["1"], &["1", "2"]).await;
    h.others[0].pages_open.send_replace(false);
    let series_id = h.follow(Monitor::All).await;
    assert_eq!(h.downloaded(1).await, ["1"]);
    h.app
        .add_source(series_id, "other", &"S1".into())
        .await
        .unwrap();
    let Event::JobStarted { job_id, .. } = h
        .wait_for(|e| {
            matches!(
                e,
                Event::JobStarted {
                    job: Job::DownloadChapter { source, .. },
                    ..
                } if source == "other"
            )
        })
        .await
    else {
        unreachable!()
    };
    // Cancel while it waits for pages, not mid-query (see
    // `cancel_returns_once_the_job_stopped`).
    while h.others[0].pages_open.receiver_count() == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    h.app.remove_source(series_id, "other").await.unwrap();
    let job = db::jobs::get(&h.app.ctx.db, job_id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Cancelled);
    h.others[0].pages_open.send_replace(true);
    let folder = h.library.path().join("Test Series");
    assert!(!folder.join("Test Series - Ch.2.cbz").exists());
    assert_eq!(on_disk(&h, series_id).await, pairs(&[("1", "fake")]));
    h.app.shutdown().await;
}

#[tokio::test]
async fn transient_failures_are_retried() {
    let source = FakeSource::new().with_chapters(&["1"]);
    // Cover attempts (on refresh, then on download) and the first page
    // fail; the download job retries with backoff.
    source
        .fail_images
        .store(3, std::sync::atomic::Ordering::SeqCst);
    let mut h = start(source).await;
    h.follow(Monitor::All).await;

    h.wait_for(|e| {
        matches!(
            e,
            Event::JobFailed {
                will_retry: true,
                ..
            }
        )
    })
    .await;
    assert_eq!(h.downloaded(1).await, ["1"]);
    h.wait_for(|e| matches!(e, Event::JobFinished { .. })).await;
    let health = db::sources::get(&h.app.ctx.db, "fake").await.unwrap();
    assert!(health.last_error.is_some());
    assert!(health.last_success_at.is_some());
    assert_eq!(health.failures_in_row, 0, "the retry succeeded");
    h.app.shutdown().await;
}

#[tokio::test]
async fn a_failing_source_is_reported_once() {
    let source = FakeSource::new().with_chapters(&["1"]);
    source
        .fail_images
        .store(usize::MAX, std::sync::atomic::Ordering::SeqCst);
    let mut h = start(source).await;
    h.follow(Monitor::All).await;

    let mut broken = Vec::new();
    h.wait_for(|e| {
        if let Event::SourceBroken { name, failures, .. } = e {
            broken.push((name.clone(), *failures));
        }
        matches!(
            e,
            Event::JobFailed {
                will_retry: false,
                ..
            }
        )
    })
    .await;
    assert_eq!(broken, [("Fake".to_owned(), db::sources::FAILING_AFTER)]);
    let health = db::sources::get(&h.app.ctx.db, "fake").await.unwrap();
    assert!(health.failures_in_row > db::sources::FAILING_AFTER);
    h.app.shutdown().await;
}

#[tokio::test]
async fn cancelled_jobs_stop_retrying() {
    let source = FakeSource::new().with_chapters(&["1"]);
    source
        .fail_images
        .store(usize::MAX, std::sync::atomic::Ordering::SeqCst);
    let mut h = start(source).await;
    h.follow(Monitor::All).await;

    let Event::JobFailed { job_id, .. } =
        h.wait_for(|e| matches!(e, Event::JobFailed { .. })).await
    else {
        unreachable!()
    };
    assert!(h.app.jobs.cancel(job_id).await.unwrap());
    let fetches = h.source.fetches();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.source.fetches(), fetches, "no attempts after cancelling");
    let job = db::jobs::get(&h.app.ctx.db, job_id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Cancelled);
    h.app.shutdown().await;
}

#[tokio::test]
async fn cancel_returns_once_the_job_stopped() {
    let source = FakeSource::new().with_chapters(&["1"]);
    source.pages_open.send_replace(false);
    let mut h = start(source).await;
    h.follow(Monitor::All).await;
    let Event::JobStarted { job_id, .. } = h
        .wait_for(|e| {
            matches!(
                e,
                Event::JobStarted {
                    job: Job::DownloadChapter { .. },
                    ..
                }
            )
        })
        .await
    else {
        unreachable!()
    };
    // Cancel while it waits for pages, not mid-query: a dropped query
    // closes the connection, and with it this in-memory database.
    while h.source.pages_open.receiver_count() == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let stopped = tokio::time::timeout(Duration::from_secs(5), h.app.jobs.cancel(job_id))
        .await
        .expect("cancel hung");
    assert!(stopped.unwrap());
    let job = db::jobs::get(&h.app.ctx.db, job_id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Cancelled);
    assert!(!h.app.jobs.cancel(job_id).await.unwrap(), "already stopped");
    h.app.shutdown().await;
}

#[tokio::test]
async fn paused_sources_hold_their_jobs_until_resumed() {
    let mut h = start(FakeSource::new().with_chapters(&["1"])).await;
    let pause = |sources: serde_json::Value| {
        serde_json::json!({ "paused_sources": sources })
            .as_object()
            .unwrap()
            .clone()
    };
    h.app
        .ctx
        .settings
        .update(pause(serde_json::json!(["fake"])))
        .await
        .unwrap();
    h.follow(Monitor::All).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let jobs = db::jobs::list(&h.app.ctx.db, 10).await.unwrap();
    // Metadata comes from AniList, not the paused source.
    let jobs: Vec<_> = jobs.iter().filter(|j| j.job.source().is_some()).collect();
    assert!(!jobs.is_empty());
    assert!(
        jobs.iter().all(|j| j.state == JobState::Queued),
        "nothing ran"
    );

    h.app
        .ctx
        .settings
        .update(pause(serde_json::json!([])))
        .await
        .unwrap();
    assert_eq!(h.downloaded(1).await, ["1"]);
    h.app.shutdown().await;
}

#[tokio::test]
async fn jobs_waiting_for_a_slot_stay_queued() {
    let numbers: Vec<String> = (1..=10).map(|n| n.to_string()).collect();
    let numbers: Vec<&str> = numbers.iter().map(String::as_str).collect();
    let source = FakeSource::new().with_chapters(&numbers);
    source.pages_open.send_replace(false);
    let mut h = start(source).await;
    h.follow(Monitor::All).await;

    // The default per-source limit is 2: two downloads start, the rest wait.
    for _ in 0..2 {
        h.wait_for(|e| {
            matches!(
                e,
                Event::JobStarted {
                    job: Job::DownloadChapter { .. },
                    ..
                }
            )
        })
        .await;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    let jobs = db::jobs::list(&h.app.ctx.db, 100).await.unwrap();
    let downloads = |state| {
        jobs.iter()
            .filter(|j| j.state == state && matches!(j.job.0, Job::DownloadChapter { .. }))
            .count()
    };
    assert_eq!(downloads(JobState::Running), 2);
    assert_eq!(downloads(JobState::Queued), 8);
    assert!(
        jobs.iter()
            .filter(|j| j.state == JobState::Queued)
            .all(|j| j.attempts == 0),
        "waiting is not an attempt"
    );

    h.source.pages_open.send_replace(true);
    assert_eq!(h.downloaded(10).await.len(), 10);
    h.app.shutdown().await;
}

#[tokio::test]
async fn duplicate_jobs_are_ignored_while_active() {
    let db = db::memory().await;
    let job = Job::SyncLatest {
        source: "fake".into(),
    };
    let first = db::jobs::insert(&db, &job).await.unwrap().unwrap();
    assert_eq!(db::jobs::insert(&db, &job).await.unwrap(), None, "queued");

    db::jobs::claim(&db, first).await.unwrap().unwrap();
    assert_eq!(db::jobs::insert(&db, &job).await.unwrap(), None, "running");

    db::jobs::finish(&db, first, JobState::Done, None)
        .await
        .unwrap();
    assert!(
        db::jobs::insert(&db, &job).await.unwrap().is_some(),
        "done jobs do not block"
    );
}

#[tokio::test]
async fn latest_updates_refresh_followed_series() {
    let mut h = start(FakeSource::new()).await;
    let series_id = h.follow(Monitor::All).await;
    // Startup queues a sync and following queues a refresh; let both finish
    // so the jobs below are not deduplicated against them.
    let (mut pending, mut queued) = (2, Vec::new());
    while pending > 0 {
        match h
            .wait_for(|e| matches!(e, Event::JobQueued { .. } | Event::JobFinished { .. }))
            .await
        {
            // The metadata lookup that follows the refresh doesn't count.
            Event::JobQueued { job_id, job } if !matches!(job, Job::RefreshMetadata { .. }) => {
                queued.push(job_id);
            }
            Event::JobFinished { job_id } if queued.contains(&job_id) => pending -= 1,
            _ => {}
        }
    }

    h.source.add_chapter("1");
    *h.source.latest.lock().unwrap() = vec!["S1".into(), "UNKNOWN".into()];
    h.app
        .jobs
        .submit(Job::SyncLatest {
            source: "fake".into(),
        })
        .await
        .unwrap();

    h.wait_for(|e| matches!(e, Event::JobQueued { job: Job::RefreshSeries { series_id: id, .. }, .. } if *id == series_id))
        .await;
    assert_eq!(h.downloaded(1).await, ["1"]);
    h.app.shutdown().await;
}

#[tokio::test]
async fn interrupted_jobs_resume_after_restart() {
    let db = db::memory().await;
    let source = FakeSource::new().with_chapters(&["1"]);

    // Simulate a crash: a series with a chapter whose download job was
    // `running` when the process died.
    let series_id = db::series::insert(
        &db,
        &db::series::NewSeries {
            series: &source.series,
            source: "fake",
            language: "en",
            folder: "Test Series",
            monitor: Monitor::All,
            monitor_from: None,
        },
    )
    .await
    .unwrap();
    let chapters = source.chapters.lock().unwrap().clone();
    let ids = db::chapters::insert_new(&db, series_id, "fake", &chapters, |_| true)
        .await
        .unwrap();
    let job = Job::DownloadChapter {
        chapter_id: ids[0],
        source: "fake".into(),
    };
    let job_id = db::jobs::insert(&db, &job).await.unwrap().unwrap();
    db::jobs::claim(&db, job_id).await.unwrap().unwrap();

    let mut h = start_with(source, db).await;
    assert_eq!(h.downloaded(1).await, ["1"]);
    h.wait_for(|e| matches!(e, Event::JobFinished { job_id: id } if *id == job_id))
        .await;
    let job = db::jobs::get(&h.app.ctx.db, job_id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Done);
    assert_eq!(job.attempts, 2, "one before the crash, one after");
    h.app.shutdown().await;
}

#[tokio::test]
async fn downloads_are_announced_in_one_batched_webhook() {
    use axum::{Router, body::Bytes, routing::post};

    use crate::{
        db::integrations::NewIntegration,
        integrations::{Config, Trigger, WebhookMethod, webhook::Webhook},
    };

    let (tx, mut received) = tokio::sync::mpsc::unbounded_channel::<String>();
    let hook = Router::new().route(
        "/hook",
        post(move |body: Bytes| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(String::from_utf8_lossy(&body).into_owned());
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, hook).await.unwrap() });

    let db = db::memory().await;
    let config = Config::Webhook(Webhook {
        url: url.parse().unwrap(),
        method: WebhookMethod::Post,
        headers: std::collections::BTreeMap::new(),
        body: "{{ event }}|{{ title }}".into(),
    });
    let id = db::integrations::insert(
        &db,
        &NewIntegration {
            name: "hook",
            enabled: true,
            config: &config,
            events: &[Trigger::Downloaded],
        },
    )
    .await
    .unwrap();

    let mut h = start_with(FakeSource::new().with_chapters(&["1", "2"]), db).await;
    h.follow(Monitor::All).await;
    h.downloaded(2).await;

    let body = tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .expect("webhook called")
        .unwrap();
    assert_eq!(body, "downloaded|Downloaded 2 chapters");
    // `found` isn't subscribed: no second message.
    assert!(
        tokio::time::timeout(Duration::from_millis(300), received.recv())
            .await
            .is_err()
    );
    let integration = db::integrations::get(&h.app.ctx.db, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(integration.last_ok, Some(true));
    assert_eq!(integration.last_status.as_deref(), Some("Sent"));
    h.app.shutdown().await;
}

#[tokio::test]
async fn metadata_comes_from_anilist_once_linked() {
    let mut source = FakeSource::new().with_chapters(&["1"]);
    source.series.ids.anilist = Some(kaze_core::AniListId(42));
    let h = start_full(
        vec![source],
        db::memory().await,
        crate::testing::anilist_with("From AniList").await,
    )
    .await;
    let id = h.follow(Monitor::None).await;

    // Not just "checked": the startup lookup may check it before the
    // source's id links it.
    let record = h.series_once(id, |r| r.title == "From AniList").await;
    assert_eq!(record.anilist_id, Some(kaze_core::AniListId(42)));
    assert_eq!(record.metadata_link, crate::library::MetadataLink::Auto);
    assert_eq!(record.title, "From AniList");
    assert_eq!(record.description.as_deref(), Some("From AniList."));
    assert_eq!(record.authors.0, ["Mangaka"], "story & art: writer…");
    assert_eq!(record.artists.0, ["Mangaka"], "…and artist");
    // The cover is saved after the metadata, and the old one removed after
    // the new one is written, so wait for both.
    let folder = h.library.path().join("Test Series");
    let replaced = tokio::time::timeout(Duration::from_secs(5), async {
        while !folder.join("cover.png").exists() || folder.join("cover.jpg").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(replaced.is_ok(), "AniList's cover replaces the source's");

    let record = h.refresh_again(id).await;
    assert_eq!(
        record.title, "From AniList",
        "a source refresh keeps AniList's metadata"
    );
    h.app.shutdown().await;
}

#[tokio::test]
async fn a_series_without_an_id_is_linked_by_title() {
    let source = FakeSource::new().with_chapters(&["1"]);
    let h = start_full(
        vec![source],
        db::memory().await,
        crate::testing::anilist_with("The Test Series").await,
    )
    .await;
    let id = h.follow(Monitor::None).await;

    let record = h.series_once(id, |r| r.metadata_checked_at.is_some()).await;
    assert_eq!(
        record.anilist_id,
        Some(kaze_core::AniListId(42)),
        "\"The\" is ignored"
    );
    assert_eq!(record.title, "The Test Series");
    h.app.shutdown().await;
}

#[tokio::test]
async fn ambiguous_titles_stay_unlinked() {
    let anilist = crate::testing::fake_anilist(|vars, base| {
        vars.get("search").map(|_| {
            let twins = [41, 42].map(|id| crate::testing::anilist_media(id, "Test Series", base));
            serde_json::json!({ "Page": { "media": twins } })
        })
    })
    .await;
    let source = FakeSource::new().with_chapters(&["1"]);
    let h = start_full(vec![source], db::memory().await, anilist).await;
    let id = h.follow(Monitor::None).await;

    let record = h.series_once(id, |r| r.metadata_checked_at.is_some()).await;
    assert_eq!(record.anilist_id, None);
    assert_eq!(record.title, "Test Series");
    h.app.shutdown().await;
}

#[tokio::test]
async fn unlinking_by_hand_restores_source_metadata() {
    let mut source = FakeSource::new().with_chapters(&["1"]);
    source.series.ids.anilist = Some(kaze_core::AniListId(42));
    let h = start_full(
        vec![source],
        db::memory().await,
        crate::testing::anilist_with("From AniList").await,
    )
    .await;
    let id = h.follow(Monitor::None).await;
    h.series_once(id, |r| r.title == "From AniList").await;

    db::series::link_metadata(
        &h.app.ctx.db,
        id,
        None,
        crate::library::MetadataLink::Manual,
    )
    .await
    .unwrap();
    let record = h.refresh_again(id).await;
    assert_eq!(
        record.anilist_id, None,
        "the source's id doesn't relink a manual choice"
    );
    assert_eq!(record.title, "Test Series");
    h.app.shutdown().await;
}
