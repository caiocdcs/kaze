use std::{collections::HashSet, time::Duration};

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use kaze_core::{ChapterNumber, SeriesKey};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::{IntoParams, ToSchema};

use super::{ApiError, ApiResult, AppState, ErrorBody};
use crate::{
    db,
    import::{self, Candidate, Match},
    library::Monitor,
};

/// Library folders kaze doesn't track yet.
#[utoipa::path(
    operation_id = "import_candidates",
    get,
    path = "/import",
    tag = "import",
    responses((status = 200, description = "OK", body = Vec<Candidate>))
)]
pub async fn candidates(State(app): State<AppState>) -> ApiResult<Json<Vec<Candidate>>> {
    let followed: HashSet<String> = db::series::list(&app.ctx.db)
        .await?
        .into_iter()
        .map(|s| s.folder)
        .collect();
    let libraries = app.ctx.libraries.clone();
    let found = tokio::task::spawn_blocking(move || {
        let mut found = Vec::new();
        for (language, root) in libraries.roots() {
            for mut candidate in import::scan(root, &followed)? {
                candidate.language = language.map(ToOwned::to_owned);
                found.push(candidate);
            }
        }
        std::io::Result::Ok(found)
    })
    .await
    .map_err(|_| ApiError::Internal)?
    .map_err(|e| ApiError::BadRequest(format!("reading the library: {e}")))?;
    Ok(Json(found))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MatchQuery {
    folder: String,
}

/// Search every source for a folder's name, closest titles first.
#[utoipa::path(
    operation_id = "import_matches",
    get,
    path = "/import/match",
    tag = "import",
    params(MatchQuery),
    responses((status = 200, description = "OK", body = Vec<Match>))
)]
pub async fn matches(
    State(app): State<AppState>,
    Query(q): Query<MatchQuery>,
) -> ApiResult<Json<Vec<Match>>> {
    let outcomes = app
        .ctx
        .sources
        .search_all(&q.folder, None, Duration::from_secs(20))
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

#[derive(Deserialize, TS, ToSchema)]
#[ts(export)]
pub struct ImportRequest {
    folder: String,
    source: String,
    key: SeriesKey,
    /// Defaults to the `default_language` setting.
    #[ts(optional)]
    language: Option<String>,
    #[serde(default)]
    monitor: Monitor,
    monitor_from: Option<ChapterNumber>,
}

#[derive(Serialize, TS, ToSchema)]
#[ts(export)]
pub struct Imported {
    id: i64,
    /// Chapters matched to files on disk.
    matched: usize,
}

#[utoipa::path(
    operation_id = "import_import",
    summary = "Follow a series whose files are already in the library",
    post,
    path = "/import",
    tag = "import",
    request_body = ImportRequest,
    responses((status = 201, description = "Imported", body = Imported), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody), (status = 502, description = "The site failed", body = ErrorBody))
)]
pub async fn import(
    State(app): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> ApiResult<(StatusCode, Json<Imported>)> {
    let (id, matched) = app
        .import(
            &req.folder,
            &req.source,
            &req.key,
            req.language.as_deref(),
            req.monitor,
            req.monitor_from.as_ref(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(Imported { id, matched })))
}
