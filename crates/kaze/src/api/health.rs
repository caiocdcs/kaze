//! System health for the UI: can we reach each source, is there disk
//! space, did the integrations' last sends work.

use std::time::{Duration, Instant};

use axum::{Json, extract::State};
use futures_util::future::join_all;
use serde::Serialize;
use ts_rs::TS;
use utoipa::ToSchema;

use super::{ApiResult, AppState};
use crate::db;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS, ToSchema)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum HealthState {
    Ok,
    Slow,
    Down,
    Off,
    /// Worth a look, not a fault (a new kaze is out).
    Notice,
}

#[derive(Debug, Serialize, TS, ToSchema)]
#[ts(export)]
pub struct HealthCheck {
    pub name: String,
    pub kind: &'static str,
    pub state: HealthState,
    pub detail: String,
    /// Where to read more, e.g. release notes.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub link: Option<String>,
}

const SLOW: Duration = Duration::from_secs(3);
const TIMEOUT: Duration = Duration::from_secs(10);
/// Below this, downloads may start failing soon.
const LOW_DISK: u64 = 5 * 1024 * 1024 * 1024;

#[utoipa::path(
    operation_id = "health_check",
    summary = "Check the sources and integrations",
    get,
    path = "/health",
    tag = "system",
    responses((status = 200, description = "OK", body = Vec<HealthCheck>))
)]
pub async fn check(State(app): State<AppState>) -> ApiResult<Json<Vec<HealthCheck>>> {
    let mut known = Vec::new();
    for source in app.ctx.sources.iter() {
        known.push(known_source_state(&app, source.meta()).await?);
    }
    // Broken or paused sources say so; only the others are pinged.
    let sources = app.ctx.sources.iter().zip(known).map(|(source, known)| {
        let meta = source.meta();
        let http = app.ctx.http.clone();
        async move {
            if let Some(check) = known {
                return check;
            }
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
                link: None,
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
        link: None,
    });
    checks.extend(stored_checks(&app).await?);
    checks.push(update_check(&app));
    Ok(Json(checks))
}

/// The update check's last result. Not in `problems`: a new version is
/// news, not a fault.
fn update_check(app: &AppState) -> HealthCheck {
    let current = env!("CARGO_PKG_VERSION");
    let latest = app.ctx.updates.latest();
    let (state, detail, link) = if app.ctx.settings.current().check_updates {
        match latest {
            Some(r) if r.is_newer_than(current) => (
                HealthState::Notice,
                format!("{current}: {} is out", r.tag),
                Some(r.url.to_string()),
            ),
            Some(_) => (HealthState::Ok, format!("{current}, up to date"), None),
            None => (
                HealthState::Off,
                format!("{current}; not checked yet"),
                None,
            ),
        }
    } else {
        (
            HealthState::Off,
            format!("{current}; update check off"),
            None,
        )
    };
    HealthCheck {
        name: "kaze".into(),
        kind: "update",
        state,
        detail,
        link,
    }
}

#[utoipa::path(
    operation_id = "health_problems",
    summary = "Problems kaze knows of without checking the network",
    description = "Broken sources, low disk space and integrations whose last send failed: cheap enough for the UI to poll.",
    get,
    path = "/health/problems",
    tag = "system",
    responses((status = 200, description = "OK", body = Vec<HealthCheck>))
)]
pub async fn problems(State(app): State<AppState>) -> ApiResult<Json<Vec<HealthCheck>>> {
    let mut checks = Vec::new();
    for source in app.ctx.sources.iter() {
        checks.extend(known_source_state(&app, source.meta()).await?);
    }
    checks.extend(stored_checks(&app).await?);
    // Paused and unused things are choices, not problems.
    checks.retain(|c| matches!(c.state, HealthState::Down | HealthState::Slow));
    Ok(Json(checks))
}

/// A source's state from what kaze recorded: paused, or broken by
/// failures in a row. `None` when neither, so it needs a ping.
async fn known_source_state(
    app: &AppState,
    meta: &kaze_core::SourceMeta,
) -> sqlx::Result<Option<HealthCheck>> {
    let (state, detail) = if app.ctx.settings.current().is_paused(meta.id) {
        (HealthState::Off, "paused".to_owned())
    } else {
        let health = db::sources::get(&app.ctx.db, meta.id).await?;
        if !health.is_failing() {
            return Ok(None);
        }
        let failures = format!("failed {} times in a row", health.failures_in_row);
        let detail = health
            .last_error
            .map_or_else(|| failures.clone(), |e| format!("{failures}: {e}"));
        (HealthState::Down, detail)
    };
    Ok(Some(HealthCheck {
        name: meta.name.to_owned(),
        kind: "source",
        state,
        detail,
        link: None,
    }))
}

/// Library disks and integrations: no requests to other hosts.
async fn stored_checks(app: &AppState) -> sqlx::Result<Vec<HealthCheck>> {
    let mut checks = Vec::new();
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
            link: None,
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
            link: None,
        });
    }
    Ok(checks)
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
