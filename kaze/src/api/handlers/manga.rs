use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::app::services::Services;
use crate::domain::chapter::ChapterFilter;

pub fn router() -> Router<Arc<Services>> {
    Router::new()
        .route("/manga", get(list))
        .route("/manga/{id}", get(get_one).delete(delete_one))
        .route("/manga/search/{source}/{query}", get(search_add))
        .route("/manga/from-url", get(from_url))
        .route("/manga/{id}/chapters", get(chapters))
        .route("/manga/{id}/chapters/latest", get(latest_chapter))
        .route(
            "/manga/{id}/chapters/latest-downloaded",
            get(latest_downloaded),
        )
        .route("/manga/{id}/merge/{target}", post(merge))
        .route("/manga/{id}/recheck", post(recheck))
        .route("/manga/{id}/similar", get(similar))
        .route("/manga/{id}/download/{source}", patch(toggle_download))
        .route("/manga/{id}/library/{library}", post(set_library))
        .route("/manga/{id}/ignore-before", patch(ignore_before))
        .route("/manga/{id}/connector/{source}", get(get_connector_mapping))
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub downloading: Option<bool>,
    pub tag: Option<String>,
}

async fn list(State(svc): State<Arc<Services>>, Query(q): Query<ListQuery>) -> impl IntoResponse {
    let manga = svc
        .manga
        .list(q.downloading, q.tag)
        .await
        .unwrap_or_default();
    Json(manga)
}

async fn get_one(State(svc): State<Arc<Services>>, Path(id): Path<String>) -> impl IntoResponse {
    match svc.manga.get(&id).await {
        Ok(m) => (StatusCode::OK, Json(serde_json::to_value(m).unwrap())).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn delete_one(State(svc): State<Arc<Services>>, Path(id): Path<String>) -> StatusCode {
    match svc.manga.delete(&id).await {
        Ok(true) => StatusCode::OK,
        _ => StatusCode::NOT_FOUND,
    }
}

async fn search_add(
    State(svc): State<Arc<Services>>,
    Path((source, query)): Path<(String, String)>,
) -> impl IntoResponse {
    match svc.manga.search_and_add(&source, &query).await {
        Ok(results) => {
            (StatusCode::OK, Json(serde_json::to_value(results).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct UrlQuery {
    pub url: String,
}

async fn from_url(
    State(svc): State<Arc<Services>>,
    Query(q): Query<UrlQuery>,
) -> impl IntoResponse {
    match svc.manga.search_by_url(&q.url).await {
        Ok(m) => (StatusCode::OK, Json(serde_json::to_value(m).unwrap())).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn chapters(State(svc): State<Arc<Services>>, Path(id): Path<String>) -> impl IntoResponse {
    let chapters = svc
        .manga
        .list_chapters(&id, &ChapterFilter::default())
        .await
        .unwrap_or_default();
    Json(chapters)
}

async fn latest_chapter(
    State(svc): State<Arc<Services>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match svc.manga.latest_chapter(&id).await {
        Ok(ch) => (StatusCode::OK, Json(serde_json::to_value(ch).unwrap())).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn latest_downloaded(
    State(svc): State<Arc<Services>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match svc.manga.latest_downloaded(&id).await {
        Ok(ch) => (StatusCode::OK, Json(serde_json::to_value(ch).unwrap())).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn merge(
    State(svc): State<Arc<Services>>,
    Path((source_id, target_id)): Path<(String, String)>,
) -> StatusCode {
    match svc.manga.merge(&source_id, &target_id).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

async fn recheck(
    State(svc): State<Arc<Services>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let count = svc.manga.recheck(&id).await.unwrap_or(0);
    Json(serde_json::json!({"deleted": count}))
}

async fn similar(State(svc): State<Arc<Services>>, Path(id): Path<String>) -> Json<Vec<String>> {
    let ids = svc.manga.similar(&id).await.unwrap_or_default();
    Json(ids)
}

#[derive(Deserialize)]
pub struct ToggleBody {
    pub enabled: bool,
}

async fn toggle_download(
    State(svc): State<Arc<Services>>,
    Path((id, source)): Path<(String, String)>,
    Json(body): Json<ToggleBody>,
) -> StatusCode {
    match svc.manga.set_download(&id, &source, body.enabled).await {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

async fn set_library(
    State(svc): State<Arc<Services>>,
    Path((id, library)): Path<(String, String)>,
) -> StatusCode {
    match svc.manga.move_to_library(&id, &library).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

async fn ignore_before(
    State(svc): State<Arc<Services>>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> StatusCode {
    let threshold: f64 = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return StatusCode::BAD_REQUEST,
    };
    match svc.manga.set_ignore_before(&id, threshold).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

async fn get_connector_mapping(
    State(svc): State<Arc<Services>>,
    Path((id, source)): Path<(String, String)>,
) -> impl IntoResponse {
    match svc.manga.get_connector_mapping(&id, &source).await {
        Ok(m) => (StatusCode::OK, Json(serde_json::to_value(m).unwrap())).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
