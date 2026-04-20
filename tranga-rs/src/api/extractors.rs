use axum::{Router, http::StatusCode, response::IntoResponse, routing::post, Json};
use serde_json::json;

use crate::app_state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/extractors/cbz", post(extract_cbz))
}

async fn extract_cbz() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "type": "/errors/not-implemented",
            "title": "Not Implemented",
            "status": 501,
            "detail": "CBZ extraction is not yet implemented"
        })),
    )
}
