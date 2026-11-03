use axum::{
    Json,
    extract::{Path, Query, State},
};
use kaze_core::{AniListId, SeriesStatus};
use kaze_sources::anilist::{Format, Media};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use url::Url;
use utoipa::{IntoParams, ToSchema};

use super::{
    ApiError, ApiResult, AppState, ErrorBody,
    series::{self, SeriesDetail},
};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MetadataQuery {
    q: String,
}

/// An AniList entry to link a series to.
#[derive(Serialize, TS, ToSchema)]
#[ts(export)]
pub struct MetadataHit {
    anilist_id: AniListId,
    title: String,
    /// The romanized title, when it differs.
    romaji: Option<String>,
    year: Option<u16>,
    status: SeriesStatus,
    /// One-shots often share a series' title.
    one_shot: bool,
    cover_url: Option<Url>,
    url: Url,
}

impl From<Media> for MetadataHit {
    fn from(media: Media) -> Self {
        let title = media.title();
        Self {
            anilist_id: media.id,
            romaji: media.titles.romaji.clone().filter(|r| *r != title),
            url: media.url(),
            title,
            year: media.year,
            status: media.status,
            one_shot: media.format == Some(Format::OneShot),
            cover_url: media.cover_url,
        }
    }
}

#[utoipa::path(
    operation_id = "metadata_search",
    summary = "Search AniList for a series' entry",
    get,
    path = "/metadata/search",
    tag = "series",
    params(MetadataQuery),
    responses((status = 200, description = "OK", body = Vec<MetadataHit>), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody), (status = 502, description = "AniList failed", body = ErrorBody))
)]
pub async fn search(
    State(app): State<AppState>,
    Query(query): Query<MetadataQuery>,
) -> ApiResult<Json<Vec<MetadataHit>>> {
    let q = query.q.trim();
    if q.is_empty() {
        return Err(ApiError::BadRequest("empty search".into()));
    }
    let hits = app
        .ctx
        .anilist
        .search(q)
        .await
        .map_err(|e| ApiError::Upstream(e.to_string()))?;
    Ok(Json(hits.into_iter().map(MetadataHit::from).collect()))
}

#[derive(Deserialize, TS, ToSchema)]
#[ts(export)]
pub struct LinkMetadata {
    /// `null` unlinks: the series takes its metadata from its source again.
    anilist_id: Option<AniListId>,
}

#[utoipa::path(
    operation_id = "series_link_metadata",
    summary = "Link a series to an AniList entry by hand, or unlink it",
    put,
    path = "/series/{id}/metadata",
    tag = "series",
    params(("id" = i64, Path, description = "Series id")),
    request_body = LinkMetadata,
    responses((status = 200, description = "OK; the metadata refresh is queued", body = SeriesDetail), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody), (status = 502, description = "AniList failed", body = ErrorBody))
)]
pub async fn link(
    State(app): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<LinkMetadata>,
) -> ApiResult<Json<SeriesDetail>> {
    app.link_metadata(id, req.anilist_id).await?;
    series::get(State(app), Path(id)).await
}
