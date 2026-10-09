//! The actor that runs jobs.

use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::Utc;
use futures_util::StreamExt as _;
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinSet,
};
use tokio_util::{sync::CancellationToken, time::DelayQueue};

use super::{Command, JobQueue, RetryPolicy, handlers};
use crate::{app::Context, db, db::jobs::JobState, events::Event};

/// How much runs at once.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Jobs running at the same time, across all sources.
    pub global: usize,
    /// Jobs running at the same time against one source.
    pub per_source: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            global: 4,
            per_source: 2,
        }
    }
}

/// How a job attempt ended.
#[derive(Debug)]
enum Outcome {
    Done,
    /// Not runnable any more (cancelled or finished before it started).
    Skipped,
    /// Stopped by a cancel request or by shutdown.
    Interrupted,
    Failed {
        error: String,
        retryable: bool,
        attempts: i64,
        max_attempts: i64,
    },
}

/// Concurrency permits, shared with the job tasks.
struct Lanes {
    global: Arc<Semaphore>,
    per_source: HashMap<String, Arc<Semaphore>>,
}

pub struct Dispatcher {
    ctx: Arc<Context>,
    commands: mpsc::Receiver<Command>,
    lanes: Arc<Lanes>,
    retry: RetryPolicy,
    shutdown: CancellationToken,
    /// Jobs in flight; each task returns its id and outcome.
    tasks: JoinSet<(i64, Outcome)>,
    /// Cancel handles of the jobs in flight.
    running: HashMap<i64, CancellationToken>,
    /// Jobs waiting for a retry (or a future `run_after`).
    delayed: DelayQueue<i64>,
}

impl Dispatcher {
    /// Create the dispatcher and the queue handle that feeds it.
    #[must_use]
    pub fn new(
        ctx: Arc<Context>,
        limits: Limits,
        retry: RetryPolicy,
        shutdown: CancellationToken,
    ) -> (Self, JobQueue) {
        let (tx, rx) = mpsc::channel(256);
        let queue = JobQueue {
            db: ctx.db.clone(),
            events: ctx.events.clone(),
            commands: tx,
        };
        let lanes = Arc::new(Lanes {
            global: Arc::new(Semaphore::new(limits.global)),
            per_source: ctx
                .sources
                .iter()
                .map(|s| {
                    (
                        s.meta().id.to_owned(),
                        Arc::new(Semaphore::new(limits.per_source)),
                    )
                })
                .collect(),
        });
        let dispatcher = Self {
            ctx,
            commands: rx,
            lanes,
            retry,
            shutdown,
            tasks: JoinSet::new(),
            running: HashMap::new(),
            delayed: DelayQueue::new(),
        };
        (dispatcher, queue)
    }

    /// Run until shutdown. The `queue` handle is needed so jobs can queue
    /// follow-up jobs (a refresh queues downloads).
    pub async fn run(mut self, queue: JobQueue) {
        self.recover().await;

        loop {
            // `select!` waits on all branches at once and runs the first
            // that is ready; the `if` guards skip empty collections, whose
            // streams would otherwise return `None` in a busy loop.
            tokio::select! {
                () = self.shutdown.cancelled() => break,
                Some(command) = self.commands.recv() => match command {
                    Command::Run(id) => self.start(id, &queue),
                    Command::Cancel(id) => {
                        if let Some(token) = self.running.get(&id) {
                            token.cancel();
                        }
                    }
                },
                Some(expired) = self.delayed.next(), if !self.delayed.is_empty() => {
                    self.start(expired.into_inner(), &queue);
                }
                Some(joined) = self.tasks.join_next(), if !self.tasks.is_empty() => match joined {
                    Ok((id, outcome)) => self.finished(id, outcome).await,
                    Err(err) => tracing::error!("job task panicked: {err}"),
                },
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

    /// Re-queue jobs left over from the previous run.
    async fn recover(&mut self) {
        match db::jobs::recover(&self.ctx.db).await {
            Ok(jobs) => {
                if !jobs.is_empty() {
                    tracing::info!(count = jobs.len(), "resuming queued jobs");
                }
                let now = Utc::now();
                for (id, run_after) in jobs {
                    let delay = (run_after - now).to_std().unwrap_or(Duration::ZERO);
                    self.delayed.insert(id, delay);
                }
            }
            Err(err) => tracing::error!("could not load queued jobs: {err}"),
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

    async fn finished(&mut self, id: i64, outcome: Outcome) {
        self.running.remove(&id);
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
            // Cancelled: the cancel already updated the row. Skipped: nothing ran.
            Outcome::Interrupted | Outcome::Skipped => Ok(()),
            Outcome::Failed {
                error,
                retryable,
                attempts,
                max_attempts,
            } => {
                let will_retry =
                    retryable && attempts < max_attempts && !self.shutdown.is_cancelled();
                tracing::warn!(job = id, attempts, will_retry, "job failed: {error}");
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
    let claimed = match db::jobs::claim(&ctx.db, id).await {
        Ok(Some(claimed)) => claimed,
        Ok(None) => return Outcome::Skipped,
        Err(err) => {
            return Outcome::Failed {
                error: err.to_string(),
                retryable: true,
                attempts: 0,
                max_attempts: 1,
            };
        }
    };

    // Wait for a free slot, globally and for this source. Permits are
    // released when the guards drop at the end of this function.
    let permits = async {
        let global = Arc::clone(&lanes.global).acquire_owned().await;
        let source = match lanes.per_source.get(claimed.job.source()) {
            Some(lane) => Some(Arc::clone(lane).acquire_owned().await),
            None => None,
        };
        (global, source)
    };
    let _permits = tokio::select! {
        () = token.cancelled() => return Outcome::Interrupted,
        permits = permits => permits,
    };

    ctx.events.emit(Event::JobStarted {
        job_id: id,
        job: claimed.job.clone(),
    });
    // Dropping a future cancels it: when the token fires, `select!` drops
    // the handler mid-flight, wherever it is awaiting.
    let result = tokio::select! {
        () = token.cancelled() => return Outcome::Interrupted,
        result = handlers::run(ctx, queue, id, &claimed.job) => result,
    };
    match result {
        Ok(()) => Outcome::Done,
        Err(err) => Outcome::Failed {
            retryable: err.is_retryable(),
            error: err.to_string(),
            attempts: claimed.attempts,
            max_attempts: claimed.max_attempts,
        },
    }
}
