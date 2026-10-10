//! What kaze tracks: followed series and their chapters, as stored.

use std::{io, path::Path};

use chrono::{DateTime, Utc};
use kaze_core::{ChapterKey, ChapterNumber, SeriesKey, SeriesStatus};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use ts_rs::TS;

/// Which chapters of a series kaze should download (Sonarr's "monitor").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
#[derive(TS)]
#[ts(export)]
pub enum Monitor {
    /// Every chapter, past and future.
    #[default]
    All,
    /// Only chapters released after the series was added.
    Future,
    /// Nothing; the series is only tracked.
    None,
}

impl Monitor {
    /// Should a newly discovered chapter be monitored?
    ///
    /// `initial` is true for the chapters found when the series is first
    /// added; that is the only difference between `All` and `Future`.
    #[must_use]
    pub fn wants(
        self,
        number: &ChapterNumber,
        from: Option<&ChapterNumber>,
        initial: bool,
    ) -> bool {
        let in_range = from.is_none_or(|from| number >= from);
        match self {
            Self::All => in_range,
            Self::Future => in_range && !initial,
            Self::None => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct SeriesRecord {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    #[ts(as = "Vec<String>")]
    pub authors: Json<Vec<String>>,
    #[ts(as = "Vec<String>")]
    pub tags: Json<Vec<String>>,
    pub status: SeriesStatus,
    pub year: Option<i64>,
    pub cover_url: Option<String>,
    pub folder: String,
    pub monitor: Monitor,
    pub monitor_from: Option<ChapterNumber>,
    pub source: String,
    pub source_key: SeriesKey,
    pub url: String,
    pub added_at: DateTime<Utc>,
    pub last_checked_at: Option<DateTime<Utc>>,
    /// Hours between full refreshes; `None` = global setting, `Some(0)` = never.
    pub refresh_hours: Option<i64>,
}

/// A row of the library grid.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct SeriesSummary {
    pub id: i64,
    pub title: String,
    pub status: SeriesStatus,
    pub monitor: Monitor,
    pub source: String,
    pub url: String,
    pub chapters: i64,
    pub downloaded: i64,
    pub wanted: i64,
    pub latest_chapter_at: Option<DateTime<Utc>>,
    pub added_at: DateTime<Utc>,
}

/// A row of the wanted list.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct WantedChapter {
    pub id: i64,
    pub series_id: i64,
    pub series_title: String,
    pub source: String,
    pub number: ChapterNumber,
    pub volume: Option<i64>,
    pub title: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    /// The latest download job for this chapter, if it was ever tried.
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub last_attempt_state: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ChapterRecord {
    pub id: i64,
    pub series_id: i64,
    pub source: String,
    pub source_key: ChapterKey,
    pub number: ChapterNumber,
    pub volume: Option<i64>,
    pub title: Option<String>,
    pub url: String,
    pub language: Option<String>,
    pub scanlator: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub monitored: bool,
    pub file_path: Option<String>,
    pub downloaded_at: Option<DateTime<Utc>>,
    pub discovered_at: DateTime<Utc>,
}

impl ChapterRecord {
    #[must_use]
    pub fn is_wanted(&self) -> bool {
        self.monitored && self.file_path.is_none()
    }
}

/// Delete a file or folder, refusing anything that resolves outside the
/// library (a stray `..` or a symlink). Returns false if it was already gone.
pub async fn remove(library: &Path, path: &Path) -> io::Result<bool> {
    let root = tokio::fs::canonicalize(library).await?;
    let target = match tokio::fs::canonicalize(path).await {
        Ok(target) => target,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    if target == root || !target.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is outside the library", path.display()),
        ));
    }
    if tokio::fs::metadata(&target).await?.is_dir() {
        tokio::fs::remove_dir_all(&target).await?;
    } else {
        tokio::fs::remove_file(&target).await?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> ChapterNumber {
        s.parse().unwrap()
    }

    #[test]
    fn monitor_policy() {
        let ten = n("10");
        assert!(Monitor::All.wants(&n("1"), None, true));
        assert!(!Monitor::All.wants(&n("9.5"), Some(&ten), false));
        assert!(Monitor::All.wants(&n("10"), Some(&ten), true));
        assert!(!Monitor::Future.wants(&n("50"), None, true));
        assert!(Monitor::Future.wants(&n("51"), None, false));
        assert!(!Monitor::None.wants(&n("51"), None, false));
    }

    #[tokio::test]
    async fn remove_stays_inside_the_library() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(library.join("Series")).unwrap();
        std::fs::create_dir(&outside).unwrap();

        let escape = library.join("..").join("outside");
        assert!(remove(&library, &escape).await.is_err());
        assert!(remove(&library, &library).await.is_err());
        assert!(outside.exists() && library.exists());

        assert!(remove(&library, &library.join("Series")).await.unwrap());
        assert!(!remove(&library, &library.join("Series")).await.unwrap());
    }
}
