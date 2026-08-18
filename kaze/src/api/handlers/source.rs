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
        .route("/sources", get(list))
        .route("/sources/{name}", get(get_one))
        .route("/sources/{name}/enabled", patch(toggle))
}

async fn list(State(svc): State<Arc<Services>>) -> impl IntoResponse {
    let sources = svc.sources.all();
    let infos: Vec<_> = sources
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name(),
            })
        })
        .collect();
    Json(infos)
}

async fn get_one(State(svc): State<Arc<Services>>, Path(name): Path<String>) -> impl IntoResponse {
    match svc.sources.get(&name) {
        Some(s) => (StatusCode::OK, Json(serde_json::json!({"name": s.name()}))).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
pub struct ToggleBody {
    pub enabled: bool,
}

async fn toggle(
    State(_svc): State<Arc<Services>>,
    Path(_name): Path<String>,
    Json(_body): Json<ToggleBody>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
