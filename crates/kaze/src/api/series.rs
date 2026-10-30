use axum::{
    Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use chrono::Utc;
use kaze_core::{ChapterNumber, SeriesKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;
use utoipa::{IntoParams, ToSchema};

use super::{ApiError, ApiResult, AppState, ErrorBody, patch::Patch};
use crate::{
    db,
    events::Event,
    jobs::Job,
    library::{
        self, ChapterRecord, Monitor, Scanlators, SeriesRecord, SeriesSource, SeriesSummary,
    },
    upcoming::Upcoming,
};

#[utoipa::path(
    operation_id = "series_list",
    summary = "Every followed series with chapter counts",
    get,
    path = "/series",
    tag = "series",
    responses((status = 200, description = "OK", body = Vec<SeriesSummary>))
)]
pub async fn list(State(app): State<AppState>) -> ApiResult<Json<Vec<SeriesSummary>>> {
    Ok(Json(db::series::summaries(&app.ctx.db).await?))
}

#[derive(Deserialize, TS, ToSchema)]
#[ts(export)]
pub struct FollowRequest {
    source: String,
    key: SeriesKey,
    /// Defaults to the `default_language` setting.
    #[ts(optional)]
    language: Option<String>,
    #[serde(default)]
    monitor: Monitor,
    monitor_from: Option<ChapterNumber>,
}

#[utoipa::path(
    operation_id = "series_follow",
    summary = "Follow a series",
    post,
    path = "/series",
    tag = "series",
    request_body = FollowRequest,
    responses((status = 201, description = "Followed; its first refresh is queued", body = Object, example = json!({"id": 1})), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody), (status = 502, description = "The site failed", body = ErrorBody))
)]
pub async fn follow(
    State(app): State<AppState>,
    Json(req): Json<FollowRequest>,
) -> ApiResult<impl IntoResponse> {
    let id = app
        .follow(
            &req.source,
            &req.key,
            req.language.as_deref(),
            req.monitor,
            req.monitor_from.as_ref(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

#[derive(Serialize, TS, ToSchema)]
#[ts(export)]
pub struct SeriesDetail {
    #[serde(flatten)]
    series: SeriesRecord,
    /// Primary first.
    sources: Vec<LinkedSource>,
    /// Groups seen in the chapters, to choose preferences from.
    scanlators: Vec<String>,
    chapters: Vec<ChapterRecord>,
}

#[derive(Serialize, TS, ToSchema)]
#[ts(export)]
pub struct LinkedSource {
    #[serde(flatten)]
    link: SeriesSource,
    /// Broken sources' chapters come from the next source.
    failing: bool,
}

async fn load(app: &AppState, id: i64) -> ApiResult<SeriesRecord> {
    db::series::get(&app.ctx.db, id)
        .await?
        .ok_or(ApiError::NotFound("series"))
}

#[utoipa::path(
    operation_id = "series_get",
    summary = "A series with its sources and chapters",
    get,
    path = "/series/{id}",
    tag = "series",
    params(("id" = i64, Path, description = "Series id")),
    responses((status = 200, description = "OK", body = SeriesDetail), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn get(
    State(app): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<SeriesDetail>> {
    let series = load(&app, id).await?;
    let mut sources = Vec::new();
    for link in db::series::sources(&app.ctx.db, id).await? {
        let failing = db::sources::get(&app.ctx.db, &link.source)
            .await?
            .is_failing();
        sources.push(LinkedSource { link, failing });
    }
    let scanlators = db::series::scanlators(&app.ctx.db, id).await?;
    let chapters = db::chapters::picks_for_series(&app.ctx.db, id).await?;
    Ok(Json(SeriesDetail {
        series,
        sources,
        scanlators,
        chapters,
    }))
}

#[derive(Deserialize, TS, ToSchema)]
#[ts(export)]
pub struct AddSource {
    source: String,
    key: SeriesKey,
}

/// Also download the series from another source.
#[utoipa::path(
    operation_id = "series_add_source",
    post,
    path = "/series/{id}/sources",
    tag = "series",
    params(("id" = i64, Path, description = "Series id")),
    request_body = AddSource,
    responses((status = 200, description = "OK", body = SeriesDetail), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody), (status = 502, description = "The site failed", body = ErrorBody))
)]
pub async fn add_source(
    State(app): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<AddSource>,
) -> ApiResult<Json<SeriesDetail>> {
    app.add_source(id, &req.source, &req.key).await?;
    get(State(app), Path(id)).await
}

#[derive(Deserialize, TS, ToSchema)]
#[ts(export)]
pub struct ReorderSources {
    /// Source ids, primary first.
    order: Vec<String>,
}

#[utoipa::path(
    operation_id = "series_reorder_sources",
    summary = "Set the order sources are tried in",
    patch,
    path = "/series/{id}/sources",
    tag = "series",
    params(("id" = i64, Path, description = "Series id")),
    request_body = ReorderSources,
    responses((status = 200, description = "OK", body = SeriesDetail), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn reorder_sources(
    State(app): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<ReorderSources>,
) -> ApiResult<Json<SeriesDetail>> {
    app.reorder_sources(id, req.order).await?;
    get(State(app), Path(id)).await
}

#[utoipa::path(
    operation_id = "series_remove_source",
    summary = "Stop downloading a series from a source",
    delete,
    path = "/series/{id}/sources/{source}",
    tag = "series",
    params(("id" = i64, Path, description = "Series id"), ("source" = String, Path, description = "Source id")),
    responses((status = 200, description = "OK", body = SeriesDetail), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn remove_source(
    State(app): State<AppState>,
    Path((id, source)): Path<(i64, String)>,
) -> ApiResult<Json<SeriesDetail>> {
    app.remove_source(id, &source).await?;
    get(State(app), Path(id)).await
}

/// Partial update: fields left out stay as they are.
#[derive(Deserialize, TS, ToSchema)]
#[ts(export)]
pub struct UpdateSeries {
    #[ts(optional)]
    monitor: Option<Monitor>,
    // Patch: missing = keep, null = clear.
    #[serde(default)]
    #[ts(as = "Option<ChapterNumber>", optional = nullable)]
    #[schema(value_type = Option<ChapterNumber>, nullable)]
    monitor_from: Patch<ChapterNumber>,
    #[serde(default)]
    #[ts(as = "Option<u32>", optional = nullable)]
    #[schema(value_type = Option<u32>, nullable)]
    refresh_hours: Patch<u32>,
    #[ts(optional)]
    preferred_scanlators: Option<Scanlators>,
    #[ts(optional)]
    blocked_scanlators: Option<Scanlators>,
}

/// Change monitoring, the refresh interval or the group preferences. When
/// monitoring changes, chapters without a file are re-evaluated (as if
/// just added); either way newly wanted ones are queued.
#[utoipa::path(
    operation_id = "series_update",
    patch,
    path = "/series/{id}",
    tag = "series",
    params(("id" = i64, Path, description = "Series id")),
    request_body = UpdateSeries,
    responses((status = 200, description = "OK", body = SeriesDetail), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn update(
    State(app): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateSeries>,
) -> ApiResult<Json<SeriesDetail>> {
    let series = load(&app, id).await?;
    if req.preferred_scanlators.is_some() || req.blocked_scanlators.is_some() {
        let preferred = req
            .preferred_scanlators
            .unwrap_or_else(|| series.preferred_scanlators.0.clone());
        let blocked = req
            .blocked_scanlators
            .unwrap_or_else(|| series.blocked_scanlators.0.clone());
        if let Some(name) = preferred.overlap(&blocked) {
            return Err(ApiError::BadRequest(format!(
                "{name:?} can't be both preferred and blocked"
            )));
        }
        db::series::set_scanlators(&app.ctx.db, id, &preferred, &blocked).await?;
        // Another copy may be the pick now.
        for pick in db::chapters::wanted_for_series(&app.ctx.db, id).await? {
            app.jobs.submit(pick.into()).await?;
        }
    }
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
    for pick in db::chapters::wanted_picks(&app.ctx.db, &wanted).await? {
        app.jobs.submit(pick.into()).await?;
    }
    Ok(())
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RemoveOptions {
    #[serde(default)]
    delete_files: bool,
}

/// Stop following, and with `delete_files` remove the series folder too.
#[utoipa::path(
    operation_id = "series_remove",
    delete,
    path = "/series/{id}",
    tag = "series",
    params(("id" = i64, Path, description = "Series id"), RemoveOptions),
    responses((status = 204, description = "Unfollowed"), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn remove(
    State(app): State<AppState>,
    Path(id): Path<i64>,
    Query(opts): Query<RemoveOptions>,
) -> ApiResult<StatusCode> {
    let series = load(&app, id).await?;
    for job_id in db::jobs::active_for_series(&app.ctx.db, id).await? {
        // Returns once it stopped: a download mid-write would otherwise
        // put files back into the folder deleted below.
        app.jobs.cancel(job_id).await?;
    }
    let files = db::chapters::for_series(&app.ctx.db, id)
        .await?
        .iter()
        .filter(|c| c.file_path.is_some())
        .count();
    db::series::delete(&app.ctx.db, id).await?;
    tracing::info!(
        series = series.title,
        delete_files = opts.delete_files,
        "unfollowed"
    );
    if opts.delete_files {
        let root = app.ctx.libraries.root(&series.language);
        let folder = root.join(&series.folder);
        if library::remove(root, &folder)
            .await
            .map_err(|e| ApiError::Conflict(format!("deleting {}: {e}", folder.display())))?
        {
            app.ctx.events.emit(Event::FilesDeleted {
                series_id: None,
                series: series.title,
                count: files,
                language: series.language,
            });
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    operation_id = "series_refresh",
    summary = "Check every source of a series for chapters",
    post,
    path = "/series/{id}/refresh",
    tag = "series",
    params(("id" = i64, Path, description = "Series id")),
    responses((status = 202, description = "Refreshes queued; the primary source's job, if it wasn't queued already", body = Object, example = json!({"job_id": 1})), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn refresh(
    State(app): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    load(&app, id).await?;
    // Every source; the reply carries the primary's job (`None` when it
    // was already queued).
    let mut job_id = None;
    for (i, link) in db::series::sources(&app.ctx.db, id)
        .await?
        .into_iter()
        .enumerate()
    {
        let queued = app
            .jobs
            .submit(Job::RefreshSeries {
                series_id: id,
                source: link.source,
            })
            .await?;
        if i == 0 {
            job_id = queued;
        }
    }
    Ok((StatusCode::ACCEPTED, Json(json!({ "job_id": job_id }))))
}

/// Each monitored series with a guess at its next chapter, soonest first.
#[utoipa::path(
    operation_id = "series_upcoming",
    get,
    path = "/upcoming",
    tag = "series",
    responses((status = 200, description = "OK", body = Vec<Upcoming>))
)]
pub async fn upcoming(State(app): State<AppState>) -> ApiResult<Json<Vec<Upcoming>>> {
    let rows = db::chapters::release_dates(&app.ctx.db).await?;
    let now = Utc::now();
    let mut list: Vec<Upcoming> = rows
        .chunk_by(|a, b| a.0 == b.0)
        .filter_map(|series| {
            let (id, title, _) = &series[0];
            let dates: Vec<_> = series.iter().map(|(_, _, at)| *at).collect();
            Upcoming::new(*id, title.clone(), &dates, now)
        })
        .collect();
    list.sort_by_key(|u| (u.state, u.expected_at.unwrap_or(u.last_published_at)));
    Ok(Json(list))
}

/// The cover saved in the series folder, so the UI never hotlinks the site.
#[utoipa::path(
    operation_id = "series_cover",
    get,
    path = "/series/{id}/cover",
    tag = "series",
    params(("id" = i64, Path, description = "Series id")),
    responses((status = 200, description = "The saved cover image", content_type = "image/*", body = Vec<u8>), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn cover(
    State(app): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    let series = load(&app, id).await?;
    let dir = app
        .ctx
        .libraries
        .root(&series.language)
        .join(&series.folder);
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
