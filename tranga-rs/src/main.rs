use std::sync::Arc;
use tokio::signal;
use tracing::{error, info, warn};
use tranga_rs::api;
use tranga_rs::app_state::AppState;
use tranga_rs::config::Config;
use tranga_rs::db;
use tranga_rs::http::ReqwestHttpClient;
use tranga_rs::connectors::chromium::BrowserManager;
use tranga_rs::workers::periodic;
use tranga_rs::workers::start_scheduler_loop;

async fn startup(config: &Config, state: &AppState) {
    if std::env::var("SKIP_STARTUP_TASKS").as_deref() == Ok("true") {
        info!("Skipping startup tasks (SKIP_STARTUP_TASKS=true)");
        return;
    }

    info!("Running startup tasks...");

    let mut scheduler = state.scheduler.lock().await;

    let check_interval_hours = std::env::var("HOURS_BETWEEN_NEW_CHAPTERS_CHECK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);

    if config.check_chapters_before_start {
        info!("Queueing initial chapter retrieval for tracked manga");
        scheduler.submit(periodic::check_new_chapters(0));
    }

    scheduler.submit(periodic::check_new_chapters(check_interval_hours));

    let work_cycle_secs = state.config.settings.read().await.work_cycle_timeout_ms as u64 / 1000;
    scheduler.submit(periodic::start_downloads(work_cycle_secs));
    scheduler.submit(periodic::update_metadata(6));
    scheduler.submit(periodic::update_covers(24));
    scheduler.submit(periodic::send_notifications(30));
    scheduler.submit(periodic::update_downloaded(6));
    scheduler.submit(periodic::cleanup_covers());
    scheduler.submit(periodic::cleanup_connector_ids());
    scheduler.submit(periodic::remove_old_notifications());

    drop(scheduler);
    info!("Startup tasks queued");
}

fn ensure_dirs(config: &Config) {
    let dirs = [
        &config.download_location,
        &config.working_dir,
        &config.cover_image_cache_original(),
        &config.cover_image_cache_large(),
        &config.cover_image_cache_medium(),
        &config.cover_image_cache_small(),
    ];

    for dir in &dirs {
        if let Err(e) = std::fs::create_dir_all(dir) {
            warn!("Could not create directory {}: {e}", dir.display());
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("tranga-rs starting");

    let config = Config::from_env();
    ensure_dirs(&config);

    let pool = match db::create_pool(&config.database_url).await {
        Ok(p) => {
            info!("Database connected: {}", config.database_url);
            p
        }
        Err(e) => {
            error!("Failed to connect to database: {e}");
            std::process::exit(1);
        }
    };

    let http_client: Arc<dyn tranga_rs::http::HttpClient> = Arc::new(ReqwestHttpClient::new(&config).await);
    let browser_manager = Arc::new(BrowserManager::new(15));

    let state = AppState::new(pool, config.clone(), http_client, browser_manager).await;

    let _cmd_tx = start_scheduler_loop(state.clone()).await;

    startup(&config, &state).await;

    let shutdown_token = state.shutdown_token.clone();
    let app = api::router(state);

    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port)).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind to port {}: {e}", config.port);
            std::process::exit(1);
        }
    };

    info!("Listening on 0.0.0.0:{}", config.port);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown_token))
        .await
        .unwrap_or_else(|e| error!("Server error: {e}"));

    info!("tranga-rs stopped");
}

async fn shutdown_signal(token: tokio_util::sync::CancellationToken) {
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("Received Ctrl+C, shutting down");
            token.cancel();
        }
        _ = token.cancelled() => {
            info!("Shutdown token cancelled");
        }
    }
}
