//! Periodic work: latest-updates sync and full refreshes.
//!
//! Intervals come from the live settings; when they change, the timers
//! are rebuilt without a restart.

use std::{sync::Arc, time::Duration};

use chrono::Utc;
use tokio::time::{Instant, Interval, MissedTickBehavior, interval, interval_at};
use tokio_util::sync::CancellationToken;

use super::{Job, JobQueue};
use crate::{app::Context, db};

/// How often to look for series due a full refresh.
const REFRESH_CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

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

pub async fn run(ctx: Arc<Context>, queue: JobQueue, shutdown: CancellationToken) {
    let mut settings = ctx.settings.subscribe();
    let mut latest_period = Duration::from_secs(settings.borrow().sync_latest_minutes * 60);
    // The first tick of a fresh interval fires at once: sync on startup.
    let mut latest = every(latest_period, true);
    let mut refresh = every(REFRESH_CHECK_EVERY, true);

    loop {
        tokio::select! {
            () = shutdown.cancelled() => return,
            Ok(()) = settings.changed() => {
                let period = Duration::from_secs(settings.borrow_and_update().sync_latest_minutes * 60);
                if period != latest_period {
                    latest_period = period;
                    latest = every(period, false);
                }
            }
            _ = latest.tick() => {
                for source in ctx.sources.iter() {
                    submit(&queue, Job::SyncLatest { source: source.meta().id.to_owned() }).await;
                }
            }
            _ = refresh.tick() => refresh_due(&ctx, &queue).await,
        }
    }
}

/// Queue a refresh for every monitored series whose last check is older
/// than the refresh interval.
async fn refresh_due(ctx: &Context, queue: &JobQueue) {
    let hours = ctx.settings.current().refresh_hours;
    let cutoff = Utc::now() - chrono::Duration::hours(i64::try_from(hours).unwrap_or(i64::MAX));
    match db::series::stale(&ctx.db, cutoff).await {
        Ok(stale) => {
            for (series_id, source) in stale {
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
