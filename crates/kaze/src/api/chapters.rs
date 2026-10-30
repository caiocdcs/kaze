use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};
use ts_rs::TS;
use utoipa::{IntoParams, ToSchema};

use super::{ApiError, ApiResult, AppState, ErrorBody};
use crate::{
    db,
    events::Event,
    jobs::Job,
    library::{self, ChapterCopy, WantedChapter},
};

#[derive(Deserialize, TS, ToSchema)]
#[ts(export)]
pub struct SetMonitored {
    ids: Vec<i64>,
    monitored: bool,
}

/// Monitor or unmonitor chapters; newly monitored ones are queued.
#[utoipa::path(
    operation_id = "chapters_set_monitored",
    patch,
    path = "/chapters",
    tag = "chapters",
    request_body = SetMonitored,
    responses((status = 200, description = "Chapters changed, every copy included", body = Object, example = json!({"updated": 2})), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn set_monitored(
    State(app): State<AppState>,
    Json(req): Json<SetMonitored>,
) -> ApiResult<Json<Value>> {
    let updated = db::chapters::set_monitored(&app.ctx.db, &req.ids, req.monitored).await?;
    if req.monitored {
        for pick in db::chapters::wanted_picks(&app.ctx.db, &req.ids).await? {
            app.jobs.submit(pick.into()).await?;
        }
    }
    Ok(Json(json!({ "updated": updated })))
}

#[derive(Deserialize, TS, ToSchema)]
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

#[utoipa::path(
    operation_id = "chapters_delete_files",
    summary = "Delete chapter files from disk",
    post,
    path = "/chapters/delete-files",
    tag = "chapters",
    request_body = DeleteFiles,
    responses((status = 200, description = "Files deleted", body = Object, example = json!({"deleted": 2})), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
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
        let Some(series) = db::series::get(&app.ctx.db, chapter.series_id).await? else {
            continue;
        };
        library::remove(
            app.ctx.libraries.root(&series.language),
            std::path::Path::new(path),
        )
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
        let Some(series) = db::series::get(&app.ctx.db, series_id).await? else {
            continue;
        };
        tracing::info!(series = series.title, count, "deleted chapter files");
        app.ctx.events.emit(Event::FilesDeleted {
            series_id: Some(series_id),
            series: series.title,
            count,
            language: series.language,
        });
    }
    Ok(Json(json!({ "deleted": deleted })))
}

/// Download one chapter now, monitored or not.
#[utoipa::path(
    operation_id = "chapters_download",
    post,
    path = "/chapters/{id}/download",
    tag = "chapters",
    params(("id" = i64, Path, description = "Chapter id")),
    responses((status = 202, description = "Queued; null if already queued", body = Object, example = json!({"job_id": 1})), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn download(
    State(app): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let chapter = db::chapters::get(&app.ctx.db, id)
        .await?
        .ok_or(ApiError::NotFound("chapter"))?;
    // Files are never replaced by another copy; delete the file first.
    if chapter.file_path.is_none() && db::chapters::number_on_disk(&app.ctx.db, id).await? {
        return Err(ApiError::Conflict(format!(
            "another copy of chapter {} is on disk; delete it first",
            chapter.number
        )));
    }
    let job_id = app
        .jobs
        .submit(Job::DownloadChapter {
            chapter_id: chapter.id,
            source: chapter.source,
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job_id": job_id }))))
}

/// Every copy of a chapter number, across sources and groups.
#[utoipa::path(
    operation_id = "chapters_copies",
    get,
    path = "/chapters/{id}/copies",
    tag = "chapters",
    params(("id" = i64, Path, description = "Chapter id")),
    responses((status = 200, description = "OK", body = Vec<ChapterCopy>), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn copies(
    State(app): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<ChapterCopy>>> {
    let copies = db::chapters::copies(&app.ctx.db, id).await?;
    if copies.is_empty() {
        return Err(ApiError::NotFound("chapter"));
    }
    Ok(Json(copies))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct Limit {
    #[serde(default = "default_limit")]
    limit: i64,
}

const fn default_limit() -> i64 {
    500
}

#[utoipa::path(
    operation_id = "chapters_wanted",
    summary = "Monitored chapters not on disk yet",
    get,
    path = "/wanted",
    tag = "chapters",
    params(Limit),
    responses((status = 200, description = "OK", body = Vec<WantedChapter>))
)]
pub async fn wanted(
    State(app): State<AppState>,
    Query(q): Query<Limit>,
) -> ApiResult<Json<Vec<WantedChapter>>> {
    Ok(Json(
        db::chapters::wanted_with_series(&app.ctx.db, q.limit).await?,
    ))
}

/// Queue every wanted chapter.
#[utoipa::path(
    operation_id = "chapters_download_wanted",
    post,
    path = "/wanted/download",
    tag = "chapters",
    responses((status = 202, description = "Downloads queued", body = Object, example = json!({"queued": 3})))
)]
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
