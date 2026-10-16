//! SQLite storage. Queries are checked at compile time against the
//! migrations (`sqlx::query!`); see `just db` and `just sqlx-prepare`.
//!
//! Functions that run one statement take any `SqliteExecutor`, so callers
//! can pass the pool or a transaction (`&mut *tx`). Those that need several
//! statements take the pool and open their own transaction.

pub mod chapters;
pub mod events;
pub mod integrations;
pub mod jobs;
pub mod series;
pub mod settings;
pub mod sources;

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

/// Write a consistent copy of the database to `path`, which must not
/// exist. `VACUUM INTO` is safe while workers keep writing.
pub async fn backup(db: &SqlitePool, path: &Path) -> Result<(), sqlx::Error> {
    sqlx::query("VACUUM INTO ?")
        .bind(path.to_string_lossy())
        .execute(db)
        .await?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn backup_copies_the_database() {
        // A file database: `VACUUM INTO` from `:memory:` writes no file.
        let dir = tempfile::tempdir().unwrap();
        let db = connect(&dir.path().join("kaze.db")).await.unwrap();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('marker', '1')")
            .execute(&db)
            .await
            .unwrap();
        let path = dir.path().join("copy.db");
        backup(&db, &path).await.unwrap();
        assert!(backup(&db, &path).await.is_err(), "never overwrites");

        let copy = connect(&path).await.unwrap();
        let marker: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'marker'")
            .fetch_one(&copy)
            .await
            .unwrap();
        assert_eq!(marker, "1");
    }
}
