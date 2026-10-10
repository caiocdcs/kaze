//! The JSON API under `/api/v1`, plus live events over SSE.

mod auth;
mod chapters;
mod error;
mod events;
mod health;
mod import;
mod integrations;
mod jobs;
mod patch;
mod series;
mod settings;
mod sources;
#[cfg(test)]
mod tests;
mod ui;

use std::{sync::Arc, time::Instant};

use axum::{
    Router,
    extract::Request,
    http::Method,
    middleware::{self, Next},
    response::Response,
    routing::{get, patch, post, put},
};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

use crate::app::App;

pub use self::error::{ApiError, ApiResult};

/// Handlers share the running app through axum's `State`.
pub type AppState = Arc<App>;

/// Successful reads are debug-only: the UI polls them.
async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16();
    let elapsed_ms = started.elapsed().as_millis();
    if status >= 500 {
        tracing::error!(%method, path, status, elapsed_ms, "request failed");
    } else if status >= 400 {
        tracing::warn!(%method, path, status, elapsed_ms, "request rejected");
    } else if method == Method::GET {
        tracing::debug!(%method, path, status, elapsed_ms, "request");
    } else {
        tracing::info!(%method, path, status, elapsed_ms, "request");
    }
    response
}

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
        .route("/chapters/delete-files", post(chapters::delete_files))
        .route("/wanted", get(chapters::wanted))
        .route("/wanted/download", post(chapters::download_wanted))
        .route("/jobs", get(jobs::list))
        .route("/jobs/{id}/cancel", post(jobs::cancel))
        .route("/jobs/{id}/retry", post(jobs::retry))
        .route("/history", get(jobs::history))
        .route("/settings", get(settings::get).patch(settings::update))
        .route("/health", get(health::check))
        .route("/import", get(import::candidates).post(import::import))
        .route("/import/match", get(import::matches))
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
        ))
        .layer(middleware::from_fn(log_request));

    Router::new()
        .nest("/api/v1", api)
        .route("/healthz", get(|| async { "ok" }))
        .fallback_service(get(ui::serve).layer(CompressionLayer::new()))
        .layer(TraceLayer::new_for_http())
        .with_state(app)
}
