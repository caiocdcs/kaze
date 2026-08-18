use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::app::services::Services;

pub fn router() -> Router<Arc<Services>> {
    Router::new()
        .route("/libraries", get(list).post(create))
        .route("/libraries/{id}", get(get_one).delete(delete_one))
        .route("/libraries/{id}", patch(update))
}

#[derive(Deserialize)]
pub struct LibBody {
    pub base_path: String,
    pub library_name: String,
}

async fn list(State(svc): State<Arc<Services>>) -> impl IntoResponse {
    let libs = svc.library.list().await.unwrap_or_default();
    Json(libs)
}

async fn get_one(State(svc): State<Arc<Services>>, Path(id): Path<String>) -> impl IntoResponse {
    match svc.library.get(&id).await {
        Ok(l) => (StatusCode::OK, Json(serde_json::to_value(l).unwrap())).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn create(State(svc): State<Arc<Services>>, Json(body): Json<LibBody>) -> impl IntoResponse {
    match svc
        .library
        .create(&body.base_path, &body.library_name)
        .await
    {
        Ok(l) => (StatusCode::CREATED, Json(serde_json::to_value(l).unwrap())).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn update(
    State(svc): State<Arc<Services>>,
    Path(id): Path<String>,
    Json(body): Json<LibBody>,
) -> StatusCode {
    match svc
        .library
        .update(&id, &body.base_path, &body.library_name)
        .await
    {
        Ok(true) => StatusCode::OK,
        _ => StatusCode::NOT_FOUND,
    }
}

async fn delete_one(State(svc): State<Arc<Services>>, Path(id): Path<String>) -> StatusCode {
    match svc.library.delete(&id).await {
        Ok(true) => StatusCode::OK,
        _ => StatusCode::NOT_FOUND,
    }
}
