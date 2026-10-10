//! The JSON API under `/api/v1`, plus live events over SSE.

mod auth;
mod chapters;
mod error;
mod events;
mod health;
mod integrations;
mod jobs;
mod patch;
mod series;
mod settings;
mod sources;
#[cfg(test)]
mod tests;
mod ui;

use std::sync::Arc;

use axum::{
    Router, middleware,
    routing::{get, patch, post, put},
};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

use crate::app::App;

pub use self::error::{ApiError, ApiResult};

/// Handlers share the running app through axum's `State`.
pub type AppState = Arc<App>;

pub fn router(app: AppState) -> Router {
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
        .route("/settings", get(settings::get).patch(settings::update))
        .route("/health", get(health::check))
        .route("/tasks", get(jobs::tasks))
        .route(
            "/integrations",
            get(integrations::list).post(integrations::create),
        )
        .route("/integrations/presets", get(integrations::presets))
        .route("/integrations/test", post(integrations::test_unsaved))
        .route(
            "/integrations/{id}",
            put(integrations::replace).delete(integrations::remove),
        )
        .route("/integrations/{id}/test", post(integrations::test_saved))
        .route("/tasks/{task}/run", post(jobs::run_task))
        // Compressing an event stream would buffer it; only compress the rest.
        .layer(CompressionLayer::new())
        .route("/events", get(events::stream))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&app),
            auth::require_api_key,
        ));

    Router::new()
        .nest("/api/v1", api)
        .route("/healthz", get(|| async { "ok" }))
        .fallback_service(get(ui::serve).layer(CompressionLayer::new()))
        .layer(TraceLayer::new_for_http())
        .with_state(app)
}
