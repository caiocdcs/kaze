use std::{collections::HashSet, time::Duration};

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use kaze_core::{ChapterNumber, SeriesKey};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{ApiError, ApiResult, AppState};
use crate::{
    db,
    import::{self, Candidate, Match},
    library::Monitor,
};

/// Library folders kaze doesn't track yet.
pub async fn candidates(State(app): State<AppState>) -> ApiResult<Json<Vec<Candidate>>> {
    let followed: HashSet<String> = db::series::list(&app.ctx.db)
        .await?
        .into_iter()
        .map(|s| s.folder)
        .collect();
    let library = app.ctx.library.clone();
    let found = tokio::task::spawn_blocking(move || import::scan(&library, &followed))
        .await
        .map_err(|_| ApiError::Internal)?
        .map_err(|e| ApiError::BadRequest(format!("reading the library: {e}")))?;
    Ok(Json(found))
}

#[derive(Deserialize)]
pub struct MatchQuery {
    folder: String,
}

/// Search every source for a folder's name, closest titles first.
pub async fn matches(
    State(app): State<AppState>,
    Query(q): Query<MatchQuery>,
) -> ApiResult<Json<Vec<Match>>> {
    let outcomes = app
        .ctx
        .sources
        .search_all(&q.folder, Duration::from_secs(20))
        .await;
    let hits = outcomes
        .into_iter()
        .filter_map(|o| o.result.ok().map(|hits| (o.source, hits)))
        .flat_map(|(source, hits)| hits.into_iter().map(move |h| (source.to_owned(), h)))
        .collect();
    let mut ranked = import::rank(&q.folder, hits);
    ranked.truncate(8);
    Ok(Json(ranked))
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct ImportRequest {
    folder: String,
    source: String,
    key: SeriesKey,
    #[serde(default)]
    monitor: Monitor,
    monitor_from: Option<ChapterNumber>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct Imported {
    id: i64,
    /// Chapters matched to files on disk.
    matched: usize,
}

pub async fn import(
    State(app): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> ApiResult<(StatusCode, Json<Imported>)> {
    let (id, matched) = app
        .import(
            &req.folder,
            &req.source,
            &req.key,
            req.monitor,
            req.monitor_from.as_ref(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(Imported { id, matched })))
}
