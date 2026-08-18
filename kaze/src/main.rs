use std::sync::Arc;
use tokio::signal;
use tracing::{error, info, warn};

use kaze::api;
use kaze::app::services::Services;
use kaze::app::workers::{WorkerPool, start_periodic};
use kaze::config::Config;
use kaze::db;
use kaze::domain::chapter::ChapterRepository;
use kaze::domain::library::LibraryRepository;
use kaze::domain::manga::MangaRepository;
use kaze::id::{EntityId, FileLibraryId};
use kaze::infra::connectors::chromium::BrowserManager;
use kaze::infra::connectors::{MangaDexSource, SourceRegistry, WeebCentralSource};
use kaze::infra::http::HttpDownloader;
use kaze::infra::sqlite::{
    SqliteChapterRepository, SqliteLibraryRepository, SqliteMangaRepository,
};

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("kaze starting (new architecture)");

    let config = Config::from_env();
    let config = Arc::new(config);

    // Ensure directories
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

    // Database
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

    // Bootstrap env library connectors (Komga from env vars)
    if let (Ok(url), Ok(key)) = (std::env::var("KOMGA_URL"), std::env::var("KOMGA_API_KEY")) {
        if let Err(e) = sqlx::query(
            r#"INSERT INTO library_connectors (key, library_type, base_url, auth)
               VALUES ('env_komga', 'Komga', $1, $2)
               ON CONFLICT (key) DO UPDATE SET base_url = $1, auth = $2"#,
        )
        .bind(&url)
        .bind(&key)
        .execute(&pool)
        .await
        {
            warn!("Failed to upsert Komga connector: {e}");
        }
    }

    // Ensure default file library
    let lib_repo = SqliteLibraryRepository::new(pool.clone());
    let download_path = config.download_location.display().to_string();
    if lib_repo.find_default().await.ok().flatten().is_none() {
        if let Err(e) = lib_repo
            .save(&kaze::domain::library::FileLibrary {
                id: FileLibraryId::new().to_string(),
                base_path: download_path.clone(),
                library_name: "Default".into(),
            })
            .await
        {
            warn!("Could not create default library: {e}");
        }
    }

    // Infrastructure
    let http = Arc::new(HttpDownloader::new(&config).await);
    let browser = Arc::new(BrowserManager::new(config.chromium_wait_time_secs));

    // Repositories
    let manga_repo: Arc<dyn MangaRepository> = Arc::new(SqliteMangaRepository::new(pool.clone()));
    let chapter_repo: Arc<dyn ChapterRepository> =
        Arc::new(SqliteChapterRepository::new(pool.clone()));
    let library_repo: Arc<dyn LibraryRepository> = Arc::new(lib_repo);

    // Sources
    let mut sources = SourceRegistry::new();
    sources.register(Arc::new(MangaDexSource::new(http.clone())));
    sources.register(Arc::new(WeebCentralSource::new(
        http.clone(),
        Some(browser),
    )));
    let sources = Arc::new(sources);

    // Services
    let services = Arc::new(Services::new(
        manga_repo.clone(),
        chapter_repo.clone(),
        library_repo.clone(),
        sources.clone(),
    ));

    // Workers
    let worker_count = std::env::var("WORKER_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| num_cpus().max(2));
    info!("Starting {worker_count} download workers");
    let _workers = WorkerPool::start(
        worker_count,
        chapter_repo.clone(),
        manga_repo.clone(),
        library_repo.clone(),
        sources.clone(),
        http.clone(),
        config.clone(),
    );

    // Periodic tasks
    start_periodic(
        chapter_repo.clone(),
        manga_repo.clone(),
        sources.clone(),
        http.clone(),
        config.clone(),
    );

    // HTTP server
    let app = api::new_router(services);

    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port)).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind to port {}: {e}", config.port);
            std::process::exit(1);
        }
    };

    info!("Listening on 0.0.0.0:{}", config.port);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| error!("Server error: {e}"));

    info!("kaze stopped");
}

async fn shutdown_signal() {
    signal::ctrl_c().await.expect("failed to listen for ctrl+c");
    info!("Received Ctrl+C, shutting down");
}
