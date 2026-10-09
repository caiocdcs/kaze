//! The JSON API under `/api/v1`, plus live events over SSE.

mod auth;
mod chapters;
mod error;
mod events;
mod jobs;
mod series;
mod sources;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::{
    Router, middleware,
    routing::{get, patch, post},
};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

use crate::app::App;

pub use self::error::{ApiError, ApiResult};

/// Handlers share the running app through axum's `State`.
pub type AppState = Arc<App>;

pub fn router(app: AppState, api_key: Option<String>) -> Router {
    let key = api_key.map(Arc::<str>::from);

    let api = Router::new()
        .route("/sources", get(sources::list))
        .route("/search", get(sources::search))
        .route("/lookup", get(sources::lookup))
        .route("/series", get(series::list).post(series::follow))
        .route(
            "/series/{id}",
            get(series::get)
                .patch(series::update)
                .delete(series::remove),
        )
        .route("/series/{id}/refresh", post(series::refresh))
        .route("/series/{id}/cover", get(series::cover))
        .route("/chapters", patch(chapters::set_monitored))
        .route("/chapters/{id}/download", post(chapters::download))
        .route("/wanted", get(chapters::wanted))
        .route("/wanted/download", post(chapters::download_wanted))
        .route("/jobs", get(jobs::list))
        .route("/jobs/{id}/cancel", post(jobs::cancel))
        .route("/jobs/{id}/retry", post(jobs::retry))
        .route("/history", get(jobs::history))
        // Compressing an event stream would buffer it; only compress the rest.
        .layer(CompressionLayer::new())
        .route("/events", get(events::stream))
        .layer(middleware::from_fn_with_state(key, auth::require_api_key));

    Router::new()
        .nest("/api/v1", api)
        .route("/healthz", get(|| async { "ok" }))
        .layer(TraceLayer::new_for_http())
        .with_state(app)
}
