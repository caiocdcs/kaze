//! End-to-end tests of the job pipeline against an in-memory database and
//! a fake source: follow → refresh → downloads → events.

use std::{sync::Arc, time::Duration};

use tokio::sync::broadcast;

use super::*;
use crate::{
    app::{App, Context, Settings},
    db::{self, jobs::JobState},
    download,
    events::{Event, Events},
    jobs::scheduler::Schedule,
    library::Monitor,
    testing::FakeSource,
};

struct Harness {
    app: App,
    source: Arc<FakeSource>,
    events: broadcast::Receiver<Event>,
    library: tempfile::TempDir,
}

async fn start_with(source: FakeSource, db: sqlx::SqlitePool) -> Harness {
    let source = Arc::new(source);
    let library = tempfile::tempdir().unwrap();
    let events = Events::new(1024);
    let receiver = events.subscribe();
    let ctx = Context {
        db,
        sources: kaze_sources::Registry::from_sources(vec![source.clone()]),
        events,
        library: library.path().to_owned(),
        download: download::Options::default(),
    };
    let settings = Settings {
        retry: RetryPolicy {
            base: Duration::from_millis(20),
            max: Duration::from_millis(100),
        },
        // Keep the scheduler quiet; tests queue work themselves.
        schedule: Schedule {
            sync_latest_every: Duration::from_secs(3600),
            refresh_after: Duration::from_secs(3600),
            refresh_check_every: Duration::from_secs(3600),
        },
        ..Settings::default()
    };
    Harness {
        app: App::start(ctx, settings).await.unwrap(),
        source,
        events: receiver,
        library,
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
            .follow("fake", &"S1".into(), monitor, None)
            .await
            .unwrap()
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
async fn future_monitoring_only_takes_new_chapters() {
    let mut h = start(FakeSource::new().with_chapters(&["1", "2"])).await;
    let series_id = h.follow(Monitor::Future).await;
    h.wait_for(|e| matches!(e, Event::ChaptersFound { count: 2, .. }))
        .await;
    h.wait_for(|e| matches!(e, Event::JobFinished { .. })).await;

    h.source.add_chapter("3");
    h.app
        .jobs
        .submit(Job::RefreshSeries {
            series_id,
            source: "fake".into(),
        })
        .await
        .unwrap();
    assert_eq!(h.downloaded(1).await, ["3"]);
    assert_eq!(
        h.source.fetches(),
        2 + 1,
        "two pages of chapter 3 plus the cover"
    );
    h.app.shutdown().await;
}

#[tokio::test]
async fn transient_failures_are_retried() {
    let source = FakeSource::new().with_chapters(&["1"]);
    // Cover attempt + first page fail; the job retries with backoff.
    source
        .fail_images
        .store(2, std::sync::atomic::Ordering::SeqCst);
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
            Event::JobQueued { job_id, .. } => queued.push(job_id),
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
