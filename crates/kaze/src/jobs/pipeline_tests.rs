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
    events: broadcast::Receiver<Event>,
    library: tempfile::TempDir,
}

async fn start_with(source: FakeSource, db: sqlx::SqlitePool) -> Harness {
    let source = Arc::new(source);
    let library = tempfile::tempdir().unwrap();
    let events = Events::new(1024);
    let receiver = events.subscribe();
    // Keep the scheduler quiet after its startup run; tests queue work themselves.
    let quiet = serde_json::json!({ "sync_latest_minutes": 600, "refresh_hours": 2000 });
    let ctx = Context {
        settings: SettingsStore::load(db.clone(), quiet.as_object().unwrap().clone())
            .await
            .unwrap(),
        db,
        sources: kaze_sources::Registry::from_sources(vec![source.clone()]),
        events,
        library: library.path().to_owned(),
        http: reqwest::Client::new(),
        flaresolverr: kaze_sources::http::Http::new(kaze_sources::http::HttpConfig::default())
            .unwrap()
            .flaresolverr(),
    };
    let retry = RetryPolicy {
        base: Duration::from_millis(20),
        max: Duration::from_millis(100),
    };
    Harness {
        // Short notification batches so integration tests don't wait 30 s.
        app: App::start_with(ctx, retry, notify_timing()).await.unwrap(),
        source,
        events: receiver,
        library,
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
async fn cancel_and_wait_returns_once_the_job_stopped() {
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

    let stopped = tokio::time::timeout(Duration::from_secs(5), h.app.jobs.cancel_and_wait(job_id))
        .await
        .expect("cancel_and_wait hung");
    assert!(stopped.unwrap());
    let job = db::jobs::get(&h.app.ctx.db, job_id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Cancelled);
    assert!(
        !h.app.jobs.cancel_and_wait(job_id).await.unwrap(),
        "already stopped"
    );
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
