pub mod manga_repo;
pub mod chapter_repo;
pub mod library_repo;
pub mod connector_repo;
pub mod notification_repo;
pub mod action_repo;
pub mod metadata_repo;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

pub async fn create_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    #[allow(clippy::manual_strip)]
    if database_url.starts_with("sqlite://") && !std::path::Path::new(&database_url[9..]).exists() {
        std::fs::File::create(&database_url[9..])?;
    }

    let opts = SqlitePoolOptions::new()
        .max_connections(20);
    let pool = opts.connect(database_url).await?;

    sqlx::query("PRAGMA journal_mode=WAL;")
        .execute(&pool)
        .await?;
    sqlx::query("PRAGMA foreign_keys=ON;")
        .execute(&pool)
        .await?;

    run_migrations(&pool).await?;

    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let migration_sql = include_str!("../../migrations/001_initial.sql");

    for statement in migration_sql.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() {
            sqlx::query(trimmed).execute(pool).await?;
        }
    }

    Ok(())
}