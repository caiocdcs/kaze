use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::connectors::{ConnectorRegistry, add_global_connector};
use crate::db::connector_repo::ConnectorRepo;
use crate::http::HttpClient;
use crate::workers::WorkerScheduler;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::sqlite::SqlitePool,
    pub config: Config,
    pub connectors: Arc<RwLock<ConnectorRegistry>>,
    pub scheduler: Arc<Mutex<WorkerScheduler>>,
    pub http_client: Arc<dyn HttpClient>,
    pub download_semaphore: Arc<Semaphore>,
    pub worker_semaphore: Arc<Semaphore>,
    pub write_semaphore: Arc<Semaphore>,
    pub shutdown_token: CancellationToken,
    pub last_library_refresh: Arc<Mutex<Instant>>,
}

impl AppState {
    pub async fn new(
        db: sqlx::sqlite::SqlitePool,
        config: Config,
        http_client: Arc<dyn HttpClient>,
        browser_manager: Arc<crate::connectors::chromium::BrowserManager>,
    ) -> Self {
        let num_cpus = num_cpus_get();
        let settings = config.settings.read().await;

        let max_workers = if settings.max_concurrent_workers > 0 {
            settings.max_concurrent_workers as usize
        } else {
            num_cpus.max(4)
        };

        let max_downloads = if settings.max_concurrent_downloads > 0 {
            settings.max_concurrent_downloads as usize
        } else {
            ((num_cpus as f64 * 0.75).floor() as usize).max(1)
        };

        drop(settings);

        let connectors = Arc::new(RwLock::new(ConnectorRegistry::new(Some(browser_manager))));
        add_global_connector(&connectors).await;

        if let Ok(states) = ConnectorRepo::get_all_manga_connector_states(&db).await {
            let mut reg = connectors.write().await;
            for (name, enabled) in states {
                if let Some(connector) = reg.get_mut(&name) {
                    connector.set_enabled(enabled);
                }
            }
        }

        Self {
            db,
            config,
            connectors,
            scheduler: Arc::new(Mutex::new(WorkerScheduler::new())),
            http_client,
            download_semaphore: Arc::new(Semaphore::new(max_downloads)),
            worker_semaphore: Arc::new(Semaphore::new(max_workers)),
            write_semaphore: Arc::new(Semaphore::new(1)),
            shutdown_token: CancellationToken::new(),
            last_library_refresh: Arc::new(Mutex::new(Instant::now())),
        }
    }
}

fn num_cpus_get() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
