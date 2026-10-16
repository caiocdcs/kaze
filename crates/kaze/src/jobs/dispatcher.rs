//! The actor that runs jobs.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use chrono::{DateTime, Utc};
use futures_util::StreamExt as _;
use tokio::{
    sync::{Semaphore, mpsc, oneshot, watch},
    task::JoinSet,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker, time::DelayQueue};
use tracing::Instrument as _;

use super::{Command, Job, JobQueue, RetryPolicy, handlers};
use crate::{app::Context, db, db::jobs::JobState, events::Event, settings::Settings};

/// How a job attempt ended.
#[derive(Debug)]
enum Outcome {
    Done,
    /// Not runnable any more (cancelled or finished before it started).
    Skipped,
    /// Stopped by a cancel request or by shutdown.
    Interrupted,
    /// Its source is paused; it waits, still queued, until it resumes.
    Paused,
    Failed {
        kind: &'static str,
        error: String,
        retryable: bool,
        attempts: i64,
        max_attempts: i64,
    },
}

/// A semaphore whose number of permits can change while jobs hold some.
struct Lane {
    semaphore: Arc<Semaphore>,
    size: AtomicUsize,
}

impl Lane {
    fn new(size: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(size)),
            size: AtomicUsize::new(size),
        }
    }

    fn resize(&self, size: usize) {
        let old = self.size.swap(size, Ordering::Relaxed);
        if size > old {
            self.semaphore.add_permits(size - old);
        } else if size < old {
            // Permits held by running jobs cannot be taken back; instead
            // wait for them to be returned and then throw them away.
            let surplus = u32::try_from(old - size).unwrap_or(u32::MAX);
            let semaphore = Arc::clone(&self.semaphore);
            tokio::spawn(async move {
                if let Ok(permits) = semaphore.acquire_many_owned(surplus).await {
                    permits.forget();
                }
            });
        }
    }
}

/// Concurrency permits, shared with the job tasks.
struct Lanes {
    global: Lane,
    per_source: HashMap<String, Lane>,
}

impl Lanes {
    fn resize(&self, settings: &Settings) {
        self.global.resize(settings.concurrency);
        for lane in self.per_source.values() {
            lane.resize(settings.per_source);
        }
    }
}

pub struct Dispatcher {
    ctx: Arc<Context>,
    commands: mpsc::Receiver<Command>,
    lanes: Arc<Lanes>,
    settings: watch::Receiver<Arc<Settings>>,
    retry: RetryPolicy,
    shutdown: CancellationToken,
    /// Jobs in flight; each task returns its id and outcome.
    tasks: JoinSet<(i64, Outcome)>,
    /// Cancel handles of the jobs in flight.
    running: HashMap<i64, CancellationToken>,
    /// Callers waiting for a cancelled job to stop.
    stopping: HashMap<i64, Vec<oneshot::Sender<()>>>,
    /// Queued jobs of paused sources.
    parked: Vec<i64>,
    /// Jobs waiting for a retry (or a future `run_after`).
    delayed: DelayQueue<i64>,
}

impl Dispatcher {
    /// Create the dispatcher and the queue handle that feeds it.
    #[must_use]
    pub fn new(
        ctx: Arc<Context>,
        retry: RetryPolicy,
        shutdown: CancellationToken,
    ) -> (Self, JobQueue) {
        let (tx, rx) = mpsc::channel(256);
        let queue = JobQueue {
            db: ctx.db.clone(),
            events: ctx.events.clone(),
            commands: tx,
        };
        let settings = ctx.settings.subscribe();
        let current = settings.borrow().clone();
        let lanes = Arc::new(Lanes {
            global: Lane::new(current.concurrency),
            per_source: ctx
                .sources
                .iter()
                .map(|s| (s.meta().id.to_owned(), Lane::new(current.per_source)))
                .collect(),
        });
        let dispatcher = Self {
            ctx,
            commands: rx,
            lanes,
            settings,
            retry,
            shutdown,
            tasks: JoinSet::new(),
            running: HashMap::new(),
            stopping: HashMap::new(),
            parked: Vec::new(),
            delayed: DelayQueue::new(),
        };
        (dispatcher, queue)
    }

    /// Run until shutdown. The `queue` handle is needed so jobs can queue
    /// follow-up jobs (a refresh queues downloads); `leftover` are the jobs
    /// still queued from the previous run, with their due times.
    pub async fn run(mut self, queue: JobQueue, leftover: Vec<(i64, DateTime<Utc>)>) {
        let now = Utc::now();
        for (id, run_after) in leftover {
            let delay = (run_after - now).to_std().unwrap_or(Duration::ZERO);
            self.delayed.insert(id, delay);
        }

        loop {
            // `select!` waits on all branches at once and runs the first
            // that is ready; the `if` guards skip empty collections, whose
            // streams would otherwise return `None` in a busy loop.
            tokio::select! {
                () = self.shutdown.cancelled() => break,
                Some(command) = self.commands.recv() => match command {
                    Command::Run(id) => self.start(id, &queue),
                    Command::Cancel { id, stopped } => self.cancel(id, stopped),
                },
                Some(expired) = self.delayed.next(), if !self.delayed.is_empty() => {
                    self.start(expired.into_inner(), &queue);
                }
                Some(joined) = self.tasks.join_next(), if !self.tasks.is_empty() => match joined {
                    Ok((id, outcome)) => self.finished(id, outcome).await,
                    Err(err) => tracing::error!("job task panicked: {err}"),
                },
                // Settings changed in the UI: apply new concurrency limits.
                Ok(()) = self.settings.changed() => {
                    let settings = self.settings.borrow_and_update().clone();
                    self.lanes.resize(&settings);
                    // A source may have resumed; the rest park themselves again.
                    for id in std::mem::take(&mut self.parked) {
                        self.start(id, &queue);
                    }
                }
            }
        }

        // Shutdown: running jobs see their token cancelled (it is a child
        // of `shutdown`); wait for them so they can put themselves back.
        tracing::info!(running = self.tasks.len(), "stopping job dispatcher");
        while let Some(joined) = self.tasks.join_next().await {
            if let Ok((id, outcome)) = joined {
                self.finished(id, outcome).await;
            }
        }
    }

    fn start(&mut self, id: i64, queue: &JobQueue) {
        if self.running.contains_key(&id) {
            return;
        }
        let token = self.shutdown.child_token();
        self.running.insert(id, token.clone());

        let ctx = Arc::clone(&self.ctx);
        let lanes = Arc::clone(&self.lanes);
        let queue = queue.clone();
        self.tasks.spawn(async move {
            let outcome = run_one(&ctx, &lanes, &queue, id, &token).await;
            (id, outcome)
        });
    }

    fn cancel(&mut self, id: i64, stopped: oneshot::Sender<()>) {
        if let Some(token) = self.running.get(&id) {
            token.cancel();
            self.stopping.entry(id).or_default().push(stopped);
        } else {
            // Queued or waiting for a retry: it never starts now.
            let _ = stopped.send(());
        }
    }

    async fn finished(&mut self, id: i64, outcome: Outcome) {
        self.running.remove(&id);
        for tx in self.stopping.remove(&id).unwrap_or_default() {
            let _ = tx.send(());
        }
        let db = &self.ctx.db;
        let events = &self.ctx.events;
        let result = match outcome {
            Outcome::Done => {
                events.emit(Event::JobFinished { job_id: id });
                db::jobs::finish(db, id, JobState::Done, None).await
            }
            Outcome::Interrupted if self.shutdown.is_cancelled() => {
                // Not the job's fault: run it again after the restart.
                db::jobs::requeue(db, id, Utc::now(), None).await
            }
            Outcome::Paused => {
                self.parked.push(id);
                Ok(())
            }
            // Cancelled: the cancel already updated the row. Skipped: nothing ran.
            Outcome::Interrupted | Outcome::Skipped => Ok(()),
            Outcome::Failed {
                kind,
                error,
                retryable,
                attempts,
                max_attempts,
            } => {
                let will_retry =
                    retryable && attempts < max_attempts && !self.shutdown.is_cancelled();
                tracing::warn!(job = id, kind, attempts, will_retry, "job failed: {error}");
                events.emit(Event::JobFailed {
                    job_id: id,
                    error: error.clone(),
                    will_retry,
                });
                if will_retry {
                    let delay = self.retry.delay(attempts);
                    self.delayed.insert(id, delay);
                    let run_after =
                        Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default();
                    db::jobs::requeue(db, id, run_after, Some(&error)).await
                } else {
                    db::jobs::finish(db, id, JobState::Failed, Some(&error)).await
                }
            }
        };
        if let Err(err) = result {
            tracing::error!(job = id, "could not update job: {err}");
        }
    }
}

/// One attempt at one job, inside its own task.
async fn run_one(
    ctx: &Context,
    lanes: &Lanes,
    queue: &JobQueue,
    id: i64,
    token: &CancellationToken,
) -> Outcome {
    let db_error = |err: sqlx::Error| Outcome::Failed {
        kind: "unknown",
        error: err.to_string(),
        retryable: true,
        attempts: 0,
        max_attempts: 1,
    };
    let queued = match db::jobs::get(&ctx.db, id).await {
        Ok(Some(record)) if record.state == JobState::Queued => record.job.0,
        Ok(_) => return Outcome::Skipped,
        Err(err) => return db_error(err),
    };
    if queued
        .source()
        .is_some_and(|s| ctx.settings.current().is_paused(s))
    {
        return Outcome::Paused;
    }

    // Wait for a free slot, globally and for this source, before claiming:
    // a job stays `queued` while it waits, so a big refresh does not show
    // as hundreds of running jobs. Permits are released when the guards
    // drop at the end of this function.
    let permits = async {
        let global = Arc::clone(&lanes.global.semaphore).acquire_owned().await;
        let lane = queued.source().and_then(|s| lanes.per_source.get(s));
        let source = match lane {
            Some(lane) => Some(Arc::clone(&lane.semaphore).acquire_owned().await),
            None => None,
        };
        (global, source)
    };
    let _permits = tokio::select! {
        () = token.cancelled() => return Outcome::Interrupted,
        permits = permits => permits,
    };

    let claimed = match db::jobs::claim(&ctx.db, id).await {
        Ok(Some(claimed)) => claimed,
        // Cancelled while waiting for a slot.
        Ok(None) => return Outcome::Skipped,
        Err(err) => return db_error(err),
    };

    ctx.events.emit(Event::JobStarted {
        job_id: id,
        job: claimed.job.clone(),
    });
    // Dropping a future cancels it: when the token fires, `select!` drops
    // the handler mid-flight, wherever it is awaiting.
    let started = std::time::Instant::now();
    let writes = TaskTracker::new();
    // Every log line of the job carries these; handlers fill in `series`.
    let span = tracing::info_span!(
        "job",
        job_id = id,
        kind = claimed.job.kind(),
        source = claimed.job.source(),
        series = tracing::field::Empty,
    );
    let handler = handlers::run(ctx, queue, id, &claimed.job, &writes).instrument(span);
    let result = tokio::select! {
        () = token.cancelled() => {
            // The handler is gone, but a CBZ write on a blocking thread is not.
            writes.close();
            writes.wait().await;
            return Outcome::Interrupted;
        }
        result = handler => result,
    };
    if let Some(source) = claimed.job.source() {
        record_health(ctx, source, &result).await;
    }
    let kind = claimed.job.kind();
    match result {
        Ok(()) => {
            let elapsed_ms = started.elapsed().as_millis();
            // Refreshes run on a timer for every series.
            if matches!(
                claimed.job,
                Job::RefreshSeries { .. } | Job::SyncLatest { .. }
            ) {
                tracing::debug!(job = id, kind, elapsed_ms, "job done");
            } else {
                tracing::info!(job = id, kind, elapsed_ms, "job done");
            }
            Outcome::Done
        }
        Err(err) => Outcome::Failed {
            kind,
            retryable: err.is_retryable(),
            error: err.to_string(),
            attempts: claimed.attempts,
            max_attempts: claimed.max_attempts,
        },
    }
}

async fn record_health(ctx: &Context, source: &str, result: &Result<(), handlers::JobError>) {
    let recorded = match result {
        Ok(()) => db::sources::record_success(&ctx.db, source).await,
        Err(err) if err.is_source_failure() => {
            let error = err.to_string();
            db::sources::record_failure(&ctx.db, source, &error)
                .await
                .map(|failures| {
                    // Exactly at the threshold, so one outage is one alert.
                    if failures == db::sources::FAILING_AFTER {
                        let name = ctx
                            .sources
                            .get(source)
                            .map_or_else(|| source.to_owned(), |s| s.meta().name.to_owned());
                        ctx.events.emit(Event::SourceBroken {
                            source: source.to_owned(),
                            name,
                            failures,
                            error,
                        });
                    }
                })
        }
        Err(_) => Ok(()),
    };
    if let Err(err) = recorded {
        tracing::warn!(source, "could not record source health: {err}");
    }
}
