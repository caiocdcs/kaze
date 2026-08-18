use axum::{Json, Router, routing::get};
use std::sync::Arc;

use crate::app::services::Services;

pub fn router() -> Router<Arc<Services>> {
    Router::new().route("/ping", get(ping))
}

async fn ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}
