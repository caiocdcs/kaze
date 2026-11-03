//! The JSON API under `/api/v1`, plus live events over SSE.

mod auth;
mod backup;
mod chapters;
mod error;
mod events;
mod health;
mod import;
mod integrations;
mod jobs;
mod metadata;
mod patch;
mod series;
mod settings;
mod sources;
#[cfg(test)]
mod tests;
mod ui;

use std::{sync::Arc, time::Instant};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request},
    http::Method,
    middleware::{self, Next},
    response::{Redirect, Response},
    routing::get,
};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{app::App, config::UrlBase};

/// Largest database accepted for a restore.
const UPLOAD_LIMIT: usize = 1024 * 1024 * 1024;

pub use self::error::{ApiError, ApiResult, ErrorBody};

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

/// The spec's header and its API-key schemes; the paths come from the
/// handlers' `#[utoipa::path]` as they are routed.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "kaze",
        description = "A self-hosted manga downloader. Send the API key, when one is set, \
                       as `X-Api-Key` or as `?apikey=`."
    ),
    servers((url = "/", description = "This kaze instance")),
    modifiers(&ApiKeySchemes),
    security(("api_key_header" = []), ("api_key_query" = []))
)]
struct ApiDoc;

struct ApiKeySchemes;

impl Modify for ApiKeySchemes {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "api_key_header",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("X-Api-Key"))),
        );
        // For `<img>` and `EventSource`, which cannot set headers.
        components.add_security_scheme(
            "api_key_query",
            SecurityScheme::ApiKey(ApiKey::Query(ApiKeyValue::new("apikey"))),
        );
    }
}

/// Every `/api/v1` route with its docs: a handler can't be routed without
/// appearing in the spec.
fn api_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(sources::list))
        .routes(routes!(sources::search))
        .routes(routes!(sources::lookup))
        .routes(routes!(series::list, series::follow))
        .routes(routes!(series::get, series::update, series::remove))
        .routes(routes!(series::refresh))
        .routes(routes!(series::add_source, series::reorder_sources))
        .routes(routes!(series::remove_source))
        .routes(routes!(series::cover))
        .routes(routes!(metadata::link))
        .routes(routes!(metadata::search))
        .routes(routes!(chapters::set_monitored))
        .routes(routes!(chapters::download))
        .routes(routes!(chapters::copies))
        .routes(routes!(chapters::delete_files))
        .routes(routes!(chapters::wanted))
        .routes(routes!(series::upcoming))
        .routes(routes!(chapters::download_wanted))
        .routes(routes!(jobs::list))
        .routes(routes!(jobs::cancel))
        .routes(routes!(jobs::retry))
        .routes(routes!(jobs::history))
        .routes(routes!(settings::get, settings::update))
        .routes(routes!(health::check))
        .routes(routes!(health::problems))
        .routes(routes!(backup::download))
        .routes(routes!(backup::list))
        .routes(routes!(backup::get, backup::delete))
        .routes(routes!(backup::restore))
        .routes(routes!(backup::cancel_restore))
        // A whole database is larger than axum's default 2 MB body limit.
        .merge(
            OpenApiRouter::new()
                .routes(routes!(backup::upload))
                .layer(DefaultBodyLimit::max(UPLOAD_LIMIT)),
        )
        .routes(routes!(import::candidates, import::import))
        .routes(routes!(import::matches))
        .routes(routes!(jobs::tasks))
        .routes(routes!(integrations::list, integrations::create))
        .routes(routes!(integrations::presets))
        .routes(routes!(integrations::test_unsaved))
        .routes(routes!(integrations::replace, integrations::remove))
        .routes(routes!(integrations::test_saved))
        .routes(routes!(jobs::run_task))
        // Compressing an event stream would buffer it; only compress the rest.
        .layer(CompressionLayer::new())
        .routes(routes!(events::stream))
}

/// The OpenAPI document of `/api/v1`.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/api/v1", api_routes())
        .into_openapi()
}

/// The whole app, under `base` when one is set; `/healthz` also answers
/// at the root for container health checks.
pub fn router(app: AppState, base: &UrlBase) -> Router {
    let api = api_routes()
        .layer(middleware::from_fn_with_state(
            Arc::clone(&app),
            auth::require_api_key,
        ))
        .layer(middleware::from_fn(log_request));
    let (routes, spec) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/api/v1", api)
        .split_for_parts();

    let routes: Router = routes
        // Outside the API key: the spec holds no secrets.
        .route(
            "/api/v1/openapi.json",
            get(move || std::future::ready(Json(spec.clone()))),
        )
        .route("/healthz", get(|| async { "ok" }))
        .fallback_service({
            let base = base.as_str().to_owned();
            get(move |uri: axum::http::Uri| std::future::ready(ui::serve(&uri, &base)))
                .layer(CompressionLayer::new())
        })
        .with_state(app);
    let routes = if base.as_str().is_empty() {
        routes
    } else {
        // SvelteKit's home under a base is `/kaze`, without the slash.
        let home = base.as_str().to_owned();
        let to_home = move || std::future::ready(Redirect::to(&home));
        Router::new()
            .nest(base.as_str(), routes)
            .route("/", get(to_home.clone()))
            .route(&format!("{}/", base.as_str()), get(to_home))
            .route("/healthz", get(|| async { "ok" }))
    };
    routes.layer(TraceLayer::new_for_http())
}
