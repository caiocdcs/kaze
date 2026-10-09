//! SQLite storage. Queries are checked at compile time against the
//! migrations (`sqlx::query!`); see `just db` and `just sqlx-prepare`.
//!
//! Functions that run one statement take any `SqliteExecutor`, so callers
//! can pass the pool or a transaction (`&mut *tx`). Those that need several
//! statements take the pool and open their own transaction.

pub mod chapters;
pub mod events;
pub mod jobs;
pub mod series;

use std::{path::Path, time::Duration};

use sqlx::{
    SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

static MIGRATOR: Migrator = sqlx::migrate!();

/// Open (creating if needed) the database file and apply migrations.
pub async fn connect(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        // WAL lets the UI read while workers write.
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// A fresh, migrated in-memory database for tests.
///
/// Every SQLite connection to `:memory:` is a separate database, so the
/// pool is limited to one connection that is never closed.
#[cfg(test)]
pub async fn memory() -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .in_memory(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await
        .expect("open in-memory database");
    MIGRATOR.run(&pool).await.expect("run migrations");
    pool
}
