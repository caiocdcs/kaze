use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState};
use crate::{db, jobs::Job, library::WantedChapter};

#[derive(Deserialize)]
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
