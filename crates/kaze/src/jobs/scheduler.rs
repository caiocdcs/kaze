//! Periodic work: latest-updates sync and full refreshes.

use std::{sync::Arc, time::Duration};

use chrono::Utc;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;

use super::{Job, JobQueue};
use crate::{app::Context, db};

#[derive(Debug, Clone, Copy)]
pub struct Schedule {
    /// How often to poll each source's latest-updates page.
    pub sync_latest_every: Duration,
    /// How old a series' last check may get before a full refresh.
    pub refresh_after: Duration,
    /// How often to look for series due a refresh.
    pub refresh_check_every: Duration,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            sync_latest_every: Duration::from_secs(15 * 60),
            refresh_after: Duration::from_secs(24 * 60 * 60),
            refresh_check_every: Duration::from_secs(60 * 60),
        }
    }
}

pub async fn run(
    ctx: Arc<Context>,
    queue: JobQueue,
    schedule: Schedule,
    shutdown: CancellationToken,
) {
    let mut latest = interval(schedule.sync_latest_every);
    let mut refresh = interval(schedule.refresh_check_every);
    // After a long pause (laptop asleep), tick once instead of catching up.
    latest.set_missed_tick_behavior(MissedTickBehavior::Skip);
    refresh.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            () = shutdown.cancelled() => return,
            _ = latest.tick() => {
                for source in ctx.sources.iter() {
                    submit(&queue, Job::SyncLatest { source: source.meta().id.to_owned() }).await;
                }
            }
            _ = refresh.tick() => {
                let cutoff = Utc::now() - chrono::Duration::from_std(schedule.refresh_after).unwrap_or_default();
                match db::series::stale(&ctx.db, cutoff).await {
                    Ok(stale) => {
                        for (series_id, source) in stale {
                            submit(&queue, Job::RefreshSeries { series_id, source }).await;
                        }
                    }
                    Err(err) => tracing::error!("could not list series to refresh: {err}"),
                }
            }
        }
    }
}

async fn submit(queue: &JobQueue, job: Job) {
    if let Err(err) = queue.submit(job).await {
        tracing::error!("could not queue scheduled job: {err}");
    }
}
