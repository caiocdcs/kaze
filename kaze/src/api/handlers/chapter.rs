use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use std::sync::Arc;

use crate::app::services::Services;

pub fn router() -> Router<Arc<Services>> {
    Router::new()
        .route("/chapters/{id}", get(get_one).delete(delete_one))
        .route("/chapters/{id}/recheck", post(recheck))
        .route("/chapters/{id}/connector/{source}", get(get_mapping))
}

async fn get_one(State(svc): State<Arc<Services>>, Path(id): Path<String>) -> impl IntoResponse {
    match svc.chapter.get(&id).await {
        Ok(ch) => (StatusCode::OK, Json(serde_json::to_value(ch).unwrap())).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn delete_one(State(svc): State<Arc<Services>>, Path(id): Path<String>) -> StatusCode {
    match svc.chapter.delete(&id).await {
        Ok(true) => StatusCode::OK,
        _ => StatusCode::NOT_FOUND,
    }
}

async fn recheck(
    State(svc): State<Arc<Services>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let count = svc.chapter.recheck(&id).await.unwrap_or(0);
    Json(serde_json::json!({"deleted": count}))
}

async fn get_mapping(
    State(svc): State<Arc<Services>>,
    Path((id, source)): Path<(String, String)>,
) -> impl IntoResponse {
    match svc.chapter.get_mapping(&id, &source).await {
        Ok(m) => (StatusCode::OK, Json(serde_json::to_value(m).unwrap())).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
