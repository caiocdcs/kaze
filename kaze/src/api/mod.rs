pub mod handlers;

use axum::Router;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::app::services::Services;

/// Build the new API router using services (no AppState).
pub fn new_router(services: Arc<Services>) -> Router {
    let v2 = Router::new()
        .merge(handlers::manga::router())
        .merge(handlers::chapter::router())
        .merge(handlers::library::router())
        .merge(handlers::source::router())
        .merge(handlers::settings::router())
        .with_state(services);

    Router::new()
        .nest("/v2", v2)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
