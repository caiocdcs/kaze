use std::time::Duration;
use ts_rs::TS;

use axum::{
    Json,
    extract::{Query, State},
};
use kaze_core::{Series, SeriesHit, SeriesKey, UrlTarget};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{ApiError, ApiResult, AppState};
use crate::db::{self, sources::SourceHealth};

#[derive(Serialize, TS)]
#[ts(export)]
pub struct SourceInfo {
    id: &'static str,
    name: &'static str,
    base_url: Url,
    languages: &'static [&'static str],
    health: SourceHealth,
    /// Several jobs in a row failed: the site probably changed or is down.
    failing: bool,
}

pub async fn list(State(app): State<AppState>) -> ApiResult<Json<Vec<SourceInfo>>> {
    let mut sources = Vec::new();
    for source in app.ctx.sources.iter() {
        let meta = source.meta();
        let health = db::sources::get(&app.ctx.db, meta.id).await?;
        sources.push(SourceInfo {
            id: meta.id,
            name: meta.name,
            base_url: meta.base_url.clone(),
            languages: meta.languages,
            failing: health.is_failing(),
            health,
        });
    }
    Ok(Json(sources))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
    source: Option<String>,
}

/// Results of one source; a failing source reports its error instead.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SourceResults {
    source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    hits: Option<Vec<SeriesHit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    error: Option<String>,
}

pub async fn search(
    State(app): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Vec<SourceResults>>> {
    let q = query.q.trim();
    if q.is_empty() {
        return Err(ApiError::BadRequest("empty search".into()));
    }
    let sources = &app.ctx.sources;
    let outcomes = match &query.source {
        Some(id) => {
            let source = sources.get(id).ok_or(ApiError::NotFound("source"))?;
            vec![kaze_sources::SearchOutcome {
                source: source.meta().id,
                result: source.search(q).await,
            }]
        }
        None => sources.search_all(q, Duration::from_secs(20)).await,
    };
    Ok(Json(
        outcomes
            .into_iter()
            .map(|o| match o.result {
                Ok(hits) => SourceResults {
                    source: o.source,
                    hits: Some(hits),
                    error: None,
                },
                Err(err) => SourceResults {
                    source: o.source,
                    hits: None,
                    error: Some(err.to_string()),
                },
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct LookupQuery {
    url: Url,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct Lookup {
    source: &'static str,
    key: SeriesKey,
    series: Series,
    /// Set when this series is already followed.
    followed_id: Option<i64>,
}

/// Resolve a pasted URL to a series preview.
pub async fn lookup(
    State(app): State<AppState>,
    Query(query): Query<LookupQuery>,
) -> ApiResult<Json<Lookup>> {
    let Some((source, target)) = app.ctx.sources.resolve(&query.url) else {
        return Err(ApiError::BadRequest("no source recognises this URL".into()));
    };
    let UrlTarget::Series(key) = target else {
        return Err(ApiError::BadRequest(
            "this is a chapter URL; paste the series URL".into(),
        ));
    };
    let id = source.meta().id;
    let series = source.series(&key).await?;
    let followed_id = db::series::find_by_source(&app.ctx.db, id, &key).await?;
    Ok(Json(Lookup {
        source: id,
        key,
        series,
        followed_id,
    }))
}
