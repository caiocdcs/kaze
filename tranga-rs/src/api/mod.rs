pub mod manga;
pub mod chapters;
pub mod search;
pub mod connectors;
pub mod metadata_fetchers;
pub mod library_connectors;
pub mod notification_connectors;
pub mod file_libraries;
pub mod workers_api;
pub mod actions;
pub mod settings;
pub mod maintenance;
pub mod authors;
pub mod stats;
pub mod dto;
pub mod extractors;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::app_state::AppState;

pub fn router(state: AppState) -> Router {
    let v2 = Router::new()
        .merge(manga::router())
        .merge(chapters::router())
        .merge(search::router())
        .merge(connectors::router())
        .merge(metadata_fetchers::router())
        .merge(library_connectors::router())
        .merge(notification_connectors::router())
        .merge(file_libraries::router())
        .merge(workers_api::router())
        .merge(actions::router())
        .merge(settings::router())
        .merge(maintenance::router())
        .merge(authors::router())
        .merge(stats::router())
        .with_state(state);

    Router::new()
        .nest("/v2", v2)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
