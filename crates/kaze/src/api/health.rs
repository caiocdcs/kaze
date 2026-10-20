//! System health for the UI: can we reach each source, is there disk
//! space, did the integrations' last sends work.

use std::time::{Duration, Instant};

use axum::{Json, extract::State};
use futures_util::future::join_all;
use serde::Serialize;
use ts_rs::TS;

use super::{ApiResult, AppState};
use crate::db;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum HealthState {
    Ok,
    Slow,
    Down,
    Off,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct HealthCheck {
    pub name: String,
    pub kind: &'static str,
    pub state: HealthState,
    pub detail: String,
}

const SLOW: Duration = Duration::from_secs(3);
const TIMEOUT: Duration = Duration::from_secs(10);
/// Below this, downloads may start failing soon.
const LOW_DISK: u64 = 5 * 1024 * 1024 * 1024;

pub async fn check(State(app): State<AppState>) -> ApiResult<Json<Vec<HealthCheck>>> {
    let sources = app.ctx.sources.iter().map(|source| {
        let meta = source.meta();
        let http = app.ctx.http.clone();
        async move {
            let started = Instant::now();
            let result = http
                .get(meta.base_url.clone())
                .timeout(TIMEOUT)
                .send()
                .await;
            let took = started.elapsed();
            let (state, detail) = match result {
                // Cloudflare challenges answer 403/503: reachable but blocked.
                Ok(r) if r.status().is_server_error() || r.status().as_u16() == 403 => {
                    (HealthState::Down, format!("HTTP {}", r.status().as_u16()))
                }
                Ok(_) if took > SLOW => (HealthState::Slow, format!("{:.1} s", took.as_secs_f64())),
                Ok(_) => (HealthState::Ok, format!("{} ms", took.as_millis())),
                Err(err) if err.is_timeout() => (HealthState::Down, "timed out".into()),
                Err(_) => (HealthState::Down, "unreachable".into()),
            };
            HealthCheck {
                name: meta.name.to_owned(),
                kind: "source",
                state,
                detail,
            }
        }
    });
    let mut checks = join_all(sources).await;

    let (state, detail) = if app.ctx.flaresolverr.url().is_none() {
        (HealthState::Off, "not set".to_owned())
    } else {
        match app.ctx.flaresolverr.check().await {
            Ok(version) => (HealthState::Ok, version),
            Err(err) => (HealthState::Down, err),
        }
    };
    checks.push(HealthCheck {
        name: "FlareSolverr".into(),
        kind: "flaresolverr",
        state,
        detail,
    });

    for (language, root) in app.ctx.libraries.roots() {
        let root = root.to_owned();
        let space = tokio::task::spawn_blocking(move || fs4::available_space(&root))
            .await
            .ok()
            .and_then(Result::ok);
        checks.push(HealthCheck {
            name: language.map_or_else(|| "Library disk".into(), |l| format!("Library disk ({l})")),
            kind: "disk",
            state: match space {
                Some(free) if free < LOW_DISK => HealthState::Slow,
                Some(_) => HealthState::Ok,
                None => HealthState::Down,
            },
            detail: space.map_or_else(
                || "can't read free space".into(),
                |b| format!("{} free", size(b)),
            ),
        });
    }

    for i in db::integrations::list(&app.ctx.db).await? {
        let state = match (i.enabled, i.last_ok) {
            (false, _) | (true, None) => HealthState::Off,
            (true, Some(true)) => HealthState::Ok,
            (true, Some(false)) => HealthState::Down,
        };
        let detail = if i.enabled {
            i.last_status.unwrap_or_else(|| "not used yet".into())
        } else {
            "paused".into()
        };
        checks.push(HealthCheck {
            name: i.name,
            kind: "integration",
            state,
            detail,
        });
    }
    Ok(Json(checks))
}

#[allow(clippy::cast_precision_loss)] // display only
fn size(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let gb = bytes as f64 / GB;
    if gb >= 1024.0 {
        format!("{:.1} TB", gb / 1024.0)
    } else if gb >= 10.0 {
        format!("{gb:.0} GB")
    } else {
        format!("{gb:.1} GB")
    }
}

#[cfg(test)]
mod tests {
    use super::size;

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(size(412 * 1024 * 1024 * 1024), "412 GB");
        assert_eq!(size(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
        assert_eq!(size(2 * 1024 * 1024 * 1024 * 1024), "2.0 TB");
    }
}
