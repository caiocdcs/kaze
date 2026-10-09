use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use kaze_core::{ChapterNumber, SeriesKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState, patch::Patch};
use crate::{
    db,
    jobs::Job,
    library::{ChapterRecord, Monitor, SeriesRecord, SeriesSummary},
};

pub async fn list(State(app): State<AppState>) -> ApiResult<Json<Vec<SeriesSummary>>> {
    Ok(Json(db::series::summaries(&app.ctx.db).await?))
}

#[derive(Deserialize)]
pub struct FollowRequest {
    source: String,
    key: SeriesKey,
    #[serde(default)]
    monitor: Monitor,
    monitor_from: Option<ChapterNumber>,
}

pub async fn follow(
    State(app): State<AppState>,
    Json(req): Json<FollowRequest>,
) -> ApiResult<impl IntoResponse> {
    let id = app
        .follow(
            &req.source,
            &req.key,
            req.monitor,
            req.monitor_from.as_ref(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

#[derive(Serialize)]
pub struct SeriesDetail {
    #[serde(flatten)]
    series: SeriesRecord,
    chapters: Vec<ChapterRecord>,
}

async fn load(app: &AppState, id: i64) -> ApiResult<SeriesRecord> {
    db::series::get(&app.ctx.db, id)
        .await?
        .ok_or(ApiError::NotFound("series"))
}

pub async fn get(
    State(app): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<SeriesDetail>> {
    let series = load(&app, id).await?;
    let chapters = db::chapters::for_series(&app.ctx.db, id).await?;
    Ok(Json(SeriesDetail { series, chapters }))
}

/// Partial update: fields left out stay as they are.
#[derive(Deserialize)]
pub struct UpdateSeries {
    monitor: Option<Monitor>,
    #[serde(default)]
    monitor_from: Patch<ChapterNumber>,
    #[serde(default)]
    refresh_hours: Patch<u32>,
}

/// Change monitoring or the refresh interval. When monitoring changes,
/// chapters without a file are re-evaluated (as if just added) and newly
/// wanted ones are queued.
pub async fn update(
    State(app): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateSeries>,
) -> ApiResult<Json<SeriesDetail>> {
    let series = load(&app, id).await?;
    if !req.refresh_hours.is_keep() {
        let current = series.refresh_hours.and_then(|h| u32::try_from(h).ok());
        let hours = req.refresh_hours.apply(current).map(i64::from);
        db::series::set_refresh_hours(&app.ctx.db, id, hours).await?;
    }
    if req.monitor.is_some() || !req.monitor_from.is_keep() {
        let monitor = req.monitor.unwrap_or(series.monitor);
        let from = req.monitor_from.apply(series.monitor_from.clone());
        remonitor(&app, &series, monitor, from.as_ref()).await?;
    }
    get(State(app), Path(id)).await
}

async fn remonitor(
    app: &AppState,
    series: &SeriesRecord,
    monitor: Monitor,
    from: Option<&ChapterNumber>,
) -> ApiResult<()> {
    db::series::set_monitor(&app.ctx.db, series.id, monitor, from).await?;
    let chapters = db::chapters::for_series(&app.ctx.db, series.id).await?;
    let (wanted, unwanted): (Vec<_>, Vec<_>) = chapters
        .iter()
        .filter(|c| c.file_path.is_none())
        .partition(|c| monitor.wants(&c.number, from, true));
    let ids = |list: Vec<&ChapterRecord>| list.iter().map(|c| c.id).collect::<Vec<_>>();
    let wanted = ids(wanted);
    db::chapters::set_monitored(&app.ctx.db, &wanted, true).await?;
    db::chapters::set_monitored(&app.ctx.db, &ids(unwanted), false).await?;
    for chapter_id in wanted {
        app.jobs
            .submit(Job::DownloadChapter {
                chapter_id,
                source: series.source.clone(),
            })
            .await?;
    }
    Ok(())
}

/// Stop following; files already downloaded stay on disk.
pub async fn remove(State(app): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if db::series::delete(&app.ctx.db, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("series"))
    }
}

pub async fn refresh(
    State(app): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let series = load(&app, id).await?;
    let job_id = app
        .jobs
        .submit(Job::RefreshSeries {
            series_id: id,
            source: series.source,
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job_id": job_id }))))
}

/// The cover saved in the series folder, so the UI never hotlinks the site.
pub async fn cover(
    State(app): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    let series = load(&app, id).await?;
    let dir = app.ctx.library.join(&series.folder);
    for ext in ["jpg", "png", "webp", "gif", "avif"] {
        let path = dir.join(format!("cover.{ext}"));
        if let Ok(bytes) = tokio::fs::read(&path).await {
            let mime = if ext == "jpg" {
                "image/jpeg".to_owned()
            } else {
                format!("image/{ext}")
            };
            return Ok((
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "public, max-age=86400".to_owned()),
                ],
                bytes,
            ));
        }
    }
    Err(ApiError::NotFound("cover"))
}
