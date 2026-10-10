use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};
use ts_rs::TS;

use super::{ApiError, ApiResult, AppState};
use crate::{
    db,
    events::Event,
    jobs::Job,
    library::{self, WantedChapter},
};

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct SetMonitored {
    ids: Vec<i64>,
    monitored: bool,
}

/// Monitor or unmonitor chapters; newly monitored ones are queued.
pub async fn set_monitored(
    State(app): State<AppState>,
    Json(req): Json<SetMonitored>,
) -> ApiResult<Json<Value>> {
    let updated = db::chapters::set_monitored(&app.ctx.db, &req.ids, req.monitored).await?;
    if req.monitored {
        for id in &req.ids {
            if let Some(chapter) = db::chapters::get(&app.ctx.db, *id).await?
                && chapter.is_wanted()
            {
                app.jobs
                    .submit(Job::DownloadChapter {
                        chapter_id: chapter.id,
                        source: chapter.source,
                    })
                    .await?;
            }
        }
    }
    Ok(Json(json!({ "updated": updated })))
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct DeleteFiles {
    ids: Vec<i64>,
    /// Stop monitoring, so the chapters are not downloaded again. When
    /// false, monitored chapters are queued again (to replace a bad file).
    #[serde(default = "unmonitor_by_default")]
    unmonitor: bool,
}

const fn unmonitor_by_default() -> bool {
    true
}

pub async fn delete_files(
    State(app): State<AppState>,
    Json(req): Json<DeleteFiles>,
) -> ApiResult<Json<Value>> {
    let mut per_series: BTreeMap<i64, usize> = BTreeMap::new();
    for id in req.ids {
        let Some(chapter) = db::chapters::get(&app.ctx.db, id).await? else {
            continue;
        };
        let Some(path) = &chapter.file_path else {
            continue;
        };
        library::remove(&app.ctx.library, std::path::Path::new(path))
            .await
            .map_err(|e| ApiError::Conflict(format!("deleting {path}: {e}")))?;
        db::chapters::clear_file(&app.ctx.db, id, req.unmonitor).await?;
        *per_series.entry(chapter.series_id).or_default() += 1;
        if !req.unmonitor && chapter.monitored {
            app.jobs
                .submit(Job::DownloadChapter {
                    chapter_id: id,
                    source: chapter.source,
                })
                .await?;
        }
    }
    let deleted: usize = per_series.values().sum();
    for (series_id, count) in per_series {
        let series = db::series::get(&app.ctx.db, series_id)
            .await?
            .map(|s| s.title)
            .unwrap_or_default();
        tracing::info!(series, count, "deleted chapter files");
        app.ctx.events.emit(Event::FilesDeleted {
            series_id: Some(series_id),
            series,
            count,
        });
    }
    Ok(Json(json!({ "deleted": deleted })))
}

/// Download one chapter now, monitored or not.
pub async fn download(
    State(app): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let chapter = db::chapters::get(&app.ctx.db, id)
        .await?
        .ok_or(ApiError::NotFound("chapter"))?;
    let job_id = app
        .jobs
        .submit(Job::DownloadChapter {
            chapter_id: chapter.id,
            source: chapter.source,
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job_id": job_id }))))
}

#[derive(Deserialize)]
pub struct Limit {
    #[serde(default = "default_limit")]
    limit: i64,
}

const fn default_limit() -> i64 {
    500
}

pub async fn wanted(
    State(app): State<AppState>,
    Query(q): Query<Limit>,
) -> ApiResult<Json<Vec<WantedChapter>>> {
    Ok(Json(
        db::chapters::wanted_with_series(&app.ctx.db, q.limit).await?,
    ))
}

/// Queue every wanted chapter.
pub async fn download_wanted(State(app): State<AppState>) -> ApiResult<(StatusCode, Json<Value>)> {
    let mut queued = 0;
    for chapter in db::chapters::wanted(&app.ctx.db, i64::MAX).await? {
        let job = Job::DownloadChapter {
            chapter_id: chapter.id,
            source: chapter.source,
        };
        if app.jobs.submit(job).await?.is_some() {
            queued += 1;
        }
    }
    Ok((StatusCode::ACCEPTED, Json(json!({ "queued": queued }))))
}
