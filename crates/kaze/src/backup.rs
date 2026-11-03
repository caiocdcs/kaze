//! Database backups kept in `data_dir/backups`, made on a schedule or by
//! hand. Each is a `VACUUM INTO` snapshot named after its time, so the
//! newest sorts last and no index is needed.

use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime, TimeDelta, Timelike as _, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use ts_rs::TS;
use utoipa::ToSchema;

use crate::db;

const FORMAT: &str = "kaze-%Y%m%d-%H%M%S.db";

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

    #[test]
    fn scheduled_backups_are_due_by_age() {
        let now = Utc::now();
        assert!(is_due(None, 7, now));
        assert!(!is_due(Some(now - TimeDelta::days(6)), 7, now));
        assert!(is_due(Some(now - TimeDelta::days(7)), 7, now));
        assert!(!is_due(None, 0, now), "0 turns them off");
    }
}
