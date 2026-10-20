//! Periodic work: latest-updates sync and full refreshes.
//!
//! The scheduler is an actor like the dispatcher: one task owns the
//! timers, and the [`Tasks`] handle talks to it over an mpsc channel
//! ("run now") while reading the shared status for the UI. Intervals come
//! from the live settings and are rebuilt when they change.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use ts_rs::TS;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::mpsc,
    time::{Instant, Interval, MissedTickBehavior, interval, interval_at},
};
use tokio_util::sync::CancellationToken;

use super::{Job, JobQueue};
use crate::{app::Context, db};

/// How often to look for series due a full refresh.
const REFRESH_CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// Downloads record their own size; this catches older and outside changes.
const MEASURE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The first measure waits for startup work (sync, refreshes) to settle.
const MEASURE_FIRST_AFTER: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(TS)]
#[ts(export)]
pub enum Task {
    /// Check every source's latest updates for followed series.
    SyncLatest,
    /// Refresh series whose full refresh is due ("run now": all of them).
    RefreshSeries,
    /// Re-read the size of every chapter file.
    MeasureLibrary,
}

impl Task {
    const ALL: [Self; 3] = [Self::SyncLatest, Self::RefreshSeries, Self::MeasureLibrary];

    fn name(self) -> &'static str {
        match self {
            Self::SyncLatest => "Check latest updates",
            Self::RefreshSeries => "Refresh series",
            Self::MeasureLibrary => "Measure library",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Status {
    every: Duration,
    last_run: Option<DateTime<Utc>>,
    next_run: Option<DateTime<Utc>>,
}

/// A scheduled task as the API shows it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TaskInfo {
    pub task: Task,
    pub name: &'static str,
    pub every_secs: u64,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
}

/// Handle to the scheduler: list tasks, run one now. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Tasks {
    run_now: mpsc::Sender<Task>,
    // A std (not tokio) mutex: it is never held across an `.await`.
    status: Arc<Mutex<HashMap<Task, Status>>>,
}

impl Tasks {
    #[must_use]
    pub fn list(&self) -> Vec<TaskInfo> {
        let status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        Task::ALL
            .iter()
            .map(|&task| {
                let s = status.get(&task).cloned().unwrap_or_default();
                TaskInfo {
                    task,
                    name: task.name(),
                    every_secs: s.every.as_secs(),
                    last_run: s.last_run,
                    next_run: s.next_run,
                }
            })
            .collect()
    }

    /// Ask the scheduler to run a task now; false if it has stopped.
    pub async fn run_now(&self, task: Task) -> bool {
        self.run_now.send(task).await.is_ok()
    }

    fn record(&self, task: Task, every: Duration, ran: bool) {
        let now = Utc::now();
        let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = status.entry(task).or_default();
        entry.every = every;
        if ran {
            entry.last_run = Some(now);
        }
        entry.next_run = Some(now + chrono::Duration::from_std(every).unwrap_or_default());
    }
}

fn every(period: Duration, start_now: bool) -> Interval {
    let mut timer = if start_now {
        interval(period)
    } else {
        interval_at(Instant::now() + period, period)
    };
    // After a long pause (laptop asleep), tick once instead of catching up.
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    timer
}

/// The handle plus the receiving end the scheduler task needs.
#[must_use]
pub fn channel() -> (Tasks, mpsc::Receiver<Task>) {
    let (tx, rx) = mpsc::channel(8);
    let tasks = Tasks {
        run_now: tx,
        status: Arc::default(),
    };
    (tasks, rx)
}

pub async fn run(
    ctx: Arc<Context>,
    queue: JobQueue,
    tasks: Tasks,
    mut run_now: mpsc::Receiver<Task>,
    shutdown: CancellationToken,
) {
    let mut settings = ctx.settings.subscribe();
    let mut latest_period = Duration::from_secs(settings.borrow().sync_latest_minutes * 60);
    // The first tick of a fresh interval fires at once: sync on startup.
    let mut latest = every(latest_period, true);
    let mut refresh = every(REFRESH_CHECK_EVERY, true);
    let mut measure = interval_at(Instant::now() + MEASURE_FIRST_AFTER, MEASURE_EVERY);
    measure.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            () = shutdown.cancelled() => return,
            Ok(()) = settings.changed() => {
                let period = Duration::from_secs(settings.borrow_and_update().sync_latest_minutes * 60);
                if period != latest_period {
                    latest_period = period;
                    latest = every(period, false);
                    tasks.record(Task::SyncLatest, period, false);
                }
            }
            _ = latest.tick() => {
                sync_latest(&ctx, &queue).await;
                tasks.record(Task::SyncLatest, latest_period, true);
            }
            _ = refresh.tick() => {
                refresh_due(&ctx, &queue).await;
                tasks.record(Task::RefreshSeries, REFRESH_CHECK_EVERY, true);
            }
            _ = measure.tick() => {
                submit(&queue, Job::MeasureLibrary).await;
                tasks.record(Task::MeasureLibrary, MEASURE_EVERY, true);
            }
            Some(task) = run_now.recv() => {
                match task {
                    Task::SyncLatest => sync_latest(&ctx, &queue).await,
                    Task::RefreshSeries => refresh_all(&ctx, &queue).await,
                    Task::MeasureLibrary => submit(&queue, Job::MeasureLibrary).await,
                }
                let mut status = tasks.status.lock().unwrap_or_else(PoisonError::into_inner);
                status.entry(task).or_default().last_run = Some(Utc::now());
            }
        }
    }
}

async fn sync_latest(ctx: &Context, queue: &JobQueue) {
    for source in ctx.sources.iter() {
        let job = Job::SyncLatest {
            source: source.meta().id.to_owned(),
        };
        submit(queue, job).await;
    }
}

/// Queue a refresh for every monitored series that is due one.
async fn refresh_due(ctx: &Context, queue: &JobQueue) {
    let hours = ctx.settings.current().refresh_hours;
    match db::series::due_for_refresh(&ctx.db, hours, Utc::now()).await {
        Ok(due) => {
            for (series_id, source) in due {
                submit(queue, Job::RefreshSeries { series_id, source }).await;
            }
        }
        Err(err) => tracing::error!("could not list series to refresh: {err}"),
    }
}

async fn refresh_all(ctx: &Context, queue: &JobQueue) {
    match db::series::monitored(&ctx.db).await {
        Ok(all) => {
            for (series_id, source) in all {
                submit(queue, Job::RefreshSeries { series_id, source }).await;
            }
        }
        Err(err) => tracing::error!("could not list series to refresh: {err}"),
    }
}

async fn submit(queue: &JobQueue, job: Job) {
    if let Err(err) = queue.submit(job).await {
        tracing::error!("could not queue scheduled job: {err}");
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration as Hours;

    use super::*;
    use crate::{
        db::{self, series::NewSeries},
        library::Monitor,
        testing::FakeSource,
    };

    #[tokio::test]
    async fn per_series_refresh_interval() {
        let db = db::memory().await;
        let source = FakeSource::new();
        let mut ids = Vec::new();
        for (folder, hours) in [("a", None), ("b", Some(0)), ("c", Some(2))] {
            let id = db::series::insert(
                &db,
                &NewSeries {
                    series: &source.series,
                    source: folder, // distinct (source, key) pairs
                    language: "en",
                    folder,
                    monitor: Monitor::All,
                    monitor_from: None,
                },
            )
            .await
            .unwrap();
            db::series::set_refresh_hours(&db, id, hours).await.unwrap();
            db::series::mark_checked(&db, id).await.unwrap();
            ids.push(id);
        }
        let due = |hours_later: i64| {
            let db = db.clone();
            async move {
                let now = Utc::now() + Hours::hours(hours_later);
                let mut ids: Vec<i64> = db::series::due_for_refresh(&db, 24, now)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect();
                ids.sort_unstable();
                ids
            }
        };
        assert_eq!(due(1).await, Vec::<i64>::new());
        assert_eq!(due(3).await, [ids[2]], "custom 2h interval");
        assert_eq!(
            due(25).await,
            [ids[0], ids[2]],
            "global 24h; 0 never refreshes"
        );
    }
}
