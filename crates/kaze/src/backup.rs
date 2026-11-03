//! Database backups kept in `data_dir/backups`, made on a schedule or by
//! hand. Each is a `VACUUM INTO` snapshot named after its time, so the
//! newest sorts last and no index is needed.
//!
//! Restoring can't swap the database under a running kaze, so a backup is
//! *staged* as `restore.db` in the same folder and swapped in on the next
//! start, before the database is opened.

use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime, TimeDelta, Timelike as _, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use ts_rs::TS;
use utoipa::ToSchema;

use crate::db;

const FORMAT: &str = "kaze-%Y%m%d-%H%M%S.db";
const STAGED: &str = "restore.db";
/// Where a staged file that failed its check on start is moved.
const REJECTED: &str = "restore.rejected.db";

/// A saved backup as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS, ToSchema)]
#[ts(export)]
pub struct BackupFile {
    pub name: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Why a file can't be restored.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("not a SQLite database")]
    NotSqlite,
    #[error("not a kaze database")]
    NotKaze,
    #[error("made by a newer kaze (schema {found}; this one knows up to {known})")]
    Newer { found: i64, known: i64 },
    #[error(transparent)]
    Backup(#[from] BackupError),
}

impl From<std::io::Error> for RestoreError {
    fn from(err: std::io::Error) -> Self {
        Self::Backup(err.into())
    }
}

/// Saved backups, newest first. Other files in the folder are ignored.
pub async fn list(dir: &Path) -> std::io::Result<Vec<BackupFile>> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut found = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(time) = NaiveDateTime::parse_from_str(&name, FORMAT) else {
            continue;
        };
        found.push(BackupFile {
            size_bytes: entry.metadata().await?.len(),
            created_at: time.and_utc(),
            name,
        });
    }
    found.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    Ok(found)
}

/// The path of the backup called `name`, matched against the listing so
/// a name from a request can't point anywhere else.
pub async fn find(dir: &Path, name: &str) -> std::io::Result<Option<PathBuf>> {
    Ok(list(dir)
        .await?
        .into_iter()
        .any(|b| b.name == name)
        .then(|| dir.join(name)))
}

/// Snapshot the database into a new backup.
pub async fn create(
    db: &SqlitePool,
    dir: &Path,
    now: DateTime<Utc>,
) -> Result<BackupFile, BackupError> {
    tokio::fs::create_dir_all(dir).await?;
    let name = now.format(FORMAT).to_string();
    let path = dir.join(&name);
    db::backup(db, &path).await?;
    Ok(BackupFile {
        size_bytes: tokio::fs::metadata(&path).await?.len(),
        // The name only keeps whole seconds.
        created_at: now.with_nanosecond(0).unwrap_or(now),
        name,
    })
}

/// Delete all but the newest `keep` backups; returns how many went.
pub async fn prune(dir: &Path, keep: usize) -> std::io::Result<usize> {
    let old = list(dir).await?.into_iter().skip(keep);
    let mut removed = 0;
    for backup in old {
        tokio::fs::remove_file(dir.join(&backup.name)).await?;
        removed += 1;
    }
    Ok(removed)
}

/// Whether `path` is a kaze database this version can open.
pub async fn check(path: &Path) -> Result<(), RestoreError> {
    use tokio::io::AsyncReadExt as _;
    let mut header = [0u8; 16];
    let mut file = tokio::fs::File::open(path).await?;
    if file.read_exact(&mut header).await.is_err() || &header != b"SQLite format 3\0" {
        return Err(RestoreError::NotSqlite);
    }
    match db::schema_version(path).await {
        Ok(Some(found)) if found > db::latest_migration() => Err(RestoreError::Newer {
            found,
            known: db::latest_migration(),
        }),
        Ok(Some(_)) => Ok(()),
        Ok(None) | Err(_) => Err(RestoreError::NotKaze),
    }
}

/// Whether a restore waits for the next start.
pub async fn is_staged(dir: &Path) -> bool {
    tokio::fs::try_exists(dir.join(STAGED))
        .await
        .unwrap_or(false)
}

/// Check `file` and stage a copy of it for the next start, replacing one
/// already staged.
pub async fn stage(dir: &Path, file: &Path) -> Result<(), RestoreError> {
    check(file).await?;
    let partial = dir.join(format!("{STAGED}.part"));
    tokio::fs::copy(file, &partial).await?;
    tokio::fs::rename(&partial, dir.join(STAGED)).await?;
    Ok(())
}

/// Stage an uploaded file: written beside the backups, checked, then staged.
pub async fn stage_upload(dir: &Path, bytes: &[u8]) -> Result<(), RestoreError> {
    tokio::fs::create_dir_all(dir).await?;
    let upload = dir.join(format!("{STAGED}.upload"));
    tokio::fs::write(&upload, bytes).await?;
    let checked = check(&upload).await;
    if checked.is_ok() {
        tokio::fs::rename(&upload, dir.join(STAGED)).await?;
    } else {
        tokio::fs::remove_file(&upload).await?;
    }
    checked
}

/// Drop a staged restore; false if there was none.
pub async fn unstage(dir: &Path) -> std::io::Result<bool> {
    match tokio::fs::remove_file(dir.join(STAGED)).await {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// What [`apply_staged`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    NothingStaged,
    /// `previous` is the backup of the replaced database; `None` when
    /// there was none yet.
    Restored {
        previous: Option<BackupFile>,
    },
}

/// On start, before the database is opened: swap a staged restore in.
/// The current database is saved first as an ordinary backup (so it can
/// be restored back), which also takes in its write-ahead log. A staged
/// file that fails its check is moved aside and the current database is
/// kept.
pub async fn apply_staged(
    database: &Path,
    dir: &Path,
    now: DateTime<Utc>,
) -> Result<Applied, RestoreError> {
    let staged = dir.join(STAGED);
    if !tokio::fs::try_exists(&staged).await? {
        return Ok(Applied::NothingStaged);
    }
    if let Err(err) = check(&staged).await {
        tokio::fs::rename(&staged, dir.join(REJECTED)).await?;
        return Err(err);
    }
    let previous = if tokio::fs::try_exists(database).await? {
        let current = db::connect(database).await.map_err(BackupError::from)?;
        let saved = create(&current, dir, now).await;
        current.close().await;
        Some(saved?)
    } else {
        None
    };
    // A stale log would be replayed onto the restored file.
    for suffix in ["-wal", "-shm"] {
        let mut side = database.as_os_str().to_owned();
        side.push(suffix);
        match tokio::fs::remove_file(&side).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    tokio::fs::rename(&staged, database).await?;
    Ok(Applied::Restored { previous })
}

/// Whether a scheduled backup is due: never made, or older than
/// `every_days`. `0` turns scheduled backups off.
#[must_use]
pub fn is_due(newest: Option<DateTime<Utc>>, every_days: u64, now: DateTime<Utc>) -> bool {
    let Ok(days) = i64::try_from(every_days) else {
        return false;
    };
    days > 0 && newest.is_none_or(|newest| now - newest >= TimeDelta::days(days))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn keeps_the_newest_and_only_its_own_files() {
        // `VACUUM INTO` writes nothing from an in-memory database.
        let dir = tempfile::tempdir().unwrap();
        let db = db::connect(&dir.path().join("live.db")).await.unwrap();
        let start = Utc::now();
        for minutes in 0..4 {
            create(&db, dir.path(), start + TimeDelta::minutes(minutes))
                .await
                .unwrap();
        }
        std::fs::write(dir.path().join("notes.txt"), "mine").unwrap();
        std::fs::write(dir.path().join("kaze.db"), "not a backup").unwrap();

        let all = list(dir.path()).await.unwrap();
        assert_eq!(all.len(), 4);
        assert!(all[0].created_at > all[1].created_at, "newest first");
        assert!(all[0].size_bytes > 0);

        assert_eq!(prune(dir.path(), 2).await.unwrap(), 2);
        let kept: Vec<_> = list(dir.path()).await.unwrap();
        assert_eq!(kept, all[..2]);
        assert!(dir.path().join("notes.txt").exists(), "other files stay");

        assert!(find(dir.path(), &kept[0].name).await.unwrap().is_some());
        for name in ["../kaze.db", "kaze.db", "notes.txt", &all[3].name] {
            assert!(find(dir.path(), name).await.unwrap().is_none(), "{name}");
        }
    }

    /// A file database holding `marker` in its settings, and its pool.
    async fn marked(path: &Path, marker: &str) -> SqlitePool {
        let db = db::connect(path).await.unwrap();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('marker', ?)")
            .bind(serde_json::to_string(marker).unwrap())
            .execute(&db)
            .await
            .unwrap();
        db
    }

    async fn marker(path: &Path) -> String {
        let db = db::connect(path).await.unwrap();
        let value: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'marker'")
            .fetch_one(&db)
            .await
            .unwrap();
        db.close().await;
        serde_json::from_str(&value).unwrap()
    }

    #[tokio::test]
    async fn only_kaze_databases_this_version_knows_can_be_staged() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("notes.db");
        std::fs::write(&text, "hello").unwrap();
        assert!(matches!(check(&text).await, Err(RestoreError::NotSqlite)));

        let other = dir.path().join("other.db");
        let pool = SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&other)
                .create_if_missing(true),
        )
        .await
        .unwrap();
        sqlx::query("CREATE TABLE t (x)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        assert!(matches!(check(&other).await, Err(RestoreError::NotKaze)));

        let newer = dir.path().join("newer.db");
        let pool = marked(&newer, "newer").await;
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
             VALUES (99999, 'future', 1, x'00', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
        assert!(matches!(
            check(&newer).await,
            Err(RestoreError::Newer { found: 99999, .. })
        ));

        let backups = dir.path().join("backups");
        assert!(stage_upload(&backups, b"not sqlite").await.is_err());
        assert!(
            !is_staged(&backups).await,
            "a rejected upload leaves nothing"
        );
    }

    #[tokio::test]
    async fn a_staged_backup_replaces_the_database_on_start() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        let database = dir.path().join("kaze.db");
        let now = Utc::now();
        assert_eq!(
            apply_staged(&database, &backups, now).await.unwrap(),
            Applied::NothingStaged
        );

        let old = marked(&dir.path().join("old.db"), "old").await;
        let saved = create(&old, &backups, now - TimeDelta::days(1))
            .await
            .unwrap();
        old.close().await;
        // The live database, with changes still in its write-ahead log.
        let live = marked(&database, "live").await;
        stage(&backups, &backups.join(&saved.name)).await.unwrap();
        assert!(is_staged(&backups).await);
        live.close().await;

        let Applied::Restored {
            previous: Some(previous),
        } = apply_staged(&database, &backups, now).await.unwrap()
        else {
            panic!("expected a restore that kept the live database");
        };
        assert_eq!(marker(&database).await, "old");
        assert_eq!(marker(&backups.join(&previous.name)).await, "live");
        assert!(!is_staged(&backups).await);
        assert_eq!(
            list(&backups).await.unwrap().len(),
            2,
            "both kept as backups"
        );
    }

    #[tokio::test]
    async fn a_bad_staged_file_is_set_aside_on_start() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        std::fs::write(backups.join(STAGED), "corrupt").unwrap();
        let database = dir.path().join("kaze.db");
        marked(&database, "live").await.close().await;

        assert!(apply_staged(&database, &backups, Utc::now()).await.is_err());
        assert_eq!(marker(&database).await, "live", "the database stays");
        assert!(!is_staged(&backups).await);
        assert!(backups.join(REJECTED).exists());
    }

    #[test]
    fn scheduled_backups_are_due_by_age() {
        let now = Utc::now();
        assert!(is_due(None, 7, now));
        assert!(!is_due(Some(now - TimeDelta::days(6)), 7, now));
        assert!(is_due(Some(now - TimeDelta::days(7)), 7, now));
        assert!(!is_due(None, 0, now), "0 turns them off");
    }
}
