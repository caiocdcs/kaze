//! What kaze tracks: followed series and their chapters, as stored.

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use kaze_core::{AniListId, ChapterKey, ChapterNumber, SeriesKey, SeriesStatus};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use ts_rs::TS;
use utoipa::ToSchema;

/// Which chapters of a series kaze should download (Sonarr's "monitor").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
#[derive(TS, ToSchema)]
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

/// Who decided a series' AniList link.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
#[derive(TS, ToSchema)]
#[ts(export)]
pub enum MetadataLink {
    /// kaze: by the source's AniList id, else by title. Relinked when the
    /// source's id changes.
    #[default]
    Auto,
    /// The user linked or unlinked it; kaze leaves it alone.
    Manual,
}

/// Scanlation group names: trimmed, without blanks, and without repeats
/// that differ only in case (the database compares them the same way).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, ToSchema)]
#[serde(from = "Vec<String>")]
#[ts(as = "Vec<String>")]
pub struct Scanlators(Vec<String>);

impl From<Vec<String>> for Scanlators {
    fn from(names: Vec<String>) -> Self {
        let mut kept: Vec<String> = Vec::new();
        for name in names {
            let name = name.trim();
            if !name.is_empty() && !kept.iter().any(|k| k.to_lowercase() == name.to_lowercase()) {
                kept.push(name.to_owned());
            }
        }
        Self(kept)
    }
}

impl Scanlators {
    /// A name found in both lists, if any.
    #[must_use]
    pub fn overlap<'a>(&'a self, other: &Self) -> Option<&'a str> {
        self.0
            .iter()
            .find(|a| other.0.iter().any(|b| a.to_lowercase() == b.to_lowercase()))
            .map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS, ToSchema)]
#[ts(export)]
pub struct SeriesRecord {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    #[ts(as = "Vec<String>")]
    #[schema(value_type = Vec<String>)]
    pub authors: Json<Vec<String>>,
    #[ts(as = "Vec<String>")]
    #[schema(value_type = Vec<String>)]
    pub artists: Json<Vec<String>>,
    #[ts(as = "Vec<String>")]
    #[schema(value_type = Vec<String>)]
    pub tags: Json<Vec<String>>,
    pub status: SeriesStatus,
    pub year: Option<i64>,
    pub cover_url: Option<String>,
    pub folder: String,
    pub monitor: Monitor,
    pub monitor_from: Option<ChapterNumber>,
    pub source: String,
    pub source_key: SeriesKey,
    /// BCP 47 tag, e.g. `pt-BR`.
    pub language: String,
    pub url: String,
    pub added_at: DateTime<Utc>,
    pub last_checked_at: Option<DateTime<Utc>>,
    /// Hours between full refreshes; `None` = global setting, `Some(0)` = never.
    pub refresh_hours: Option<i64>,
    /// Picked over other groups' copies of a chapter.
    #[ts(as = "Scanlators")]
    #[schema(value_type = Scanlators)]
    pub preferred_scanlators: Json<Scanlators>,
    /// Never downloaded.
    #[ts(as = "Scanlators")]
    #[schema(value_type = Scanlators)]
    pub blocked_scanlators: Json<Scanlators>,
    /// While set, metadata comes from this AniList entry, not the source.
    pub anilist_id: Option<AniListId>,
    pub metadata_link: MetadataLink,
    pub metadata_checked_at: Option<DateTime<Utc>>,
}

/// A row of the library grid.
#[derive(Debug, Clone, PartialEq, Serialize, TS, ToSchema)]
#[ts(export)]
pub struct SeriesSummary {
    pub id: i64,
    pub title: String,
    pub status: SeriesStatus,
    pub monitor: Monitor,
    pub source: String,
    pub language: String,
    pub url: String,
    pub chapters: i64,
    pub downloaded: i64,
    pub wanted: i64,
    /// Bytes on disk, as last measured.
    pub size_bytes: i64,
    pub latest_chapter_at: Option<DateTime<Utc>>,
    pub added_at: DateTime<Utc>,
}

/// A site a series is followed on; priority 0 is the primary, which
/// also provides the metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS, ToSchema)]
#[ts(export)]
pub struct SeriesSource {
    pub source: String,
    pub source_key: SeriesKey,
    pub url: String,
    pub priority: i64,
    pub checked_at: Option<DateTime<Utc>>,
}

/// Source ids in the order a series tries them: each of its sources once,
/// so priorities can't skip or repeat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOrder(Vec<String>);

impl SourceOrder {
    /// `None` unless `order` lists each of `linked` exactly once.
    #[must_use]
    pub fn new(order: Vec<String>, linked: &[SeriesSource]) -> Option<Self> {
        let mut given: Vec<&str> = order.iter().map(String::as_str).collect();
        let mut current: Vec<&str> = linked.iter().map(|l| l.source.as_str()).collect();
        given.sort_unstable();
        current.sort_unstable();
        (!order.is_empty() && given == current).then_some(Self(order))
    }

    /// The source that comes first and gives the metadata.
    #[must_use]
    pub fn primary(&self) -> &str {
        &self.0[0]
    }

    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

/// The copy of a chapter to download, and the source it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub chapter_id: i64,
    pub source: String,
}

/// One copy of a chapter number: from one source, maybe by one group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS, ToSchema)]
#[ts(export)]
pub struct ChapterCopy {
    pub id: i64,
    pub source: String,
    pub scanlator: Option<String>,
    pub url: String,
    /// The copy kaze shows and downloads.
    pub picked: bool,
    pub on_disk: bool,
    /// By a group the series never downloads by itself.
    pub blocked: bool,
    /// The latest download job for this copy, if it was ever tried.
    pub last_attempt_state: Option<String>,
    pub last_error: Option<String>,
}

/// A row of the wanted list.
#[derive(Debug, Clone, PartialEq, Serialize, TS, ToSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, TS, ToSchema)]
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
    pub size_bytes: Option<i64>,
    pub downloaded_at: Option<DateTime<Utc>>,
    pub discovered_at: DateTime<Utc>,
    /// By a group the series never downloads; only the ranked view sets it.
    pub blocked: bool,
}

impl ChapterRecord {
    #[must_use]
    pub fn is_wanted(&self) -> bool {
        self.monitored && self.file_path.is_none() && !self.blocked
    }
}

/// Where series folders live: a root per language that has its own (so
/// each is its own Komga/Kavita library) and `default` for the rest.
#[derive(Debug, Clone)]
pub struct Libraries {
    default: PathBuf,
    by_language: BTreeMap<String, PathBuf>,
}

impl Libraries {
    /// Keys may be spelled any way (`pt-br`); they are normalized. Roots
    /// must not sit inside one another, or a library server would show
    /// one language's series in the other's library.
    pub fn new(default: PathBuf, by_language: BTreeMap<String, PathBuf>) -> Result<Self, String> {
        let mut normalized = BTreeMap::new();
        for (tag, root) in by_language {
            let language = kaze_core::language::normalize(&tag)
                .ok_or_else(|| format!("library_dirs: {tag:?} is not a language tag"))?;
            normalized.insert(language, root);
        }
        let all: Vec<&PathBuf> = std::iter::once(&default)
            .chain(normalized.values())
            .collect();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                if a.starts_with(b) || b.starts_with(a) {
                    return Err(format!(
                        "library folders {} and {} overlap; put each language's folder beside the others, not inside",
                        a.display(),
                        b.display()
                    ));
                }
            }
        }
        Ok(Self {
            default,
            by_language: normalized,
        })
    }

    #[must_use]
    pub fn single(default: PathBuf) -> Self {
        Self {
            default,
            by_language: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn root(&self, language: &str) -> &Path {
        self.by_language.get(language).unwrap_or(&self.default)
    }

    /// Every root, with its language (`None` for the default one).
    pub fn roots(&self) -> impl Iterator<Item = (Option<&str>, &Path)> {
        std::iter::once((None, self.default.as_path())).chain(
            self.by_language
                .iter()
                .map(|(l, root)| (Some(l.as_str()), root.as_path())),
        )
    }
}

/// A file's size for disk usage; `None` if it is gone or unreadable.
pub async fn file_size(path: impl AsRef<Path>) -> Option<i64> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    i64::try_from(meta.len()).ok()
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

    #[test]
    fn source_order_lists_each_source_once() {
        let linked: Vec<SeriesSource> = ["a", "b"]
            .into_iter()
            .zip(0..)
            .map(|(source, priority)| SeriesSource {
                source: source.into(),
                source_key: "S1".into(),
                url: String::new(),
                priority,
                checked_at: None,
            })
            .collect();
        let order =
            |ids: &[&str]| SourceOrder::new(ids.iter().map(|s| (*s).to_owned()).collect(), &linked);
        let swapped = order(&["b", "a"]).unwrap();
        assert_eq!(swapped.primary(), "b");
        assert_eq!(order(&["a"]), None, "missing");
        assert_eq!(order(&["a", "b", "c"]), None, "extra");
        assert_eq!(order(&["a", "a"]), None, "repeated");
        assert_eq!(order(&[]), None);
    }

    #[test]
    fn scanlator_names_are_tidied() {
        let names: Scanlators =
            serde_json::from_str(r#"[" Knox ", "", "knox", "Asura", "  "]"#).unwrap();
        assert_eq!(names, Scanlators(vec!["Knox".into(), "Asura".into()]));
        let other = Scanlators::from(vec!["ASURA".to_owned()]);
        assert_eq!(names.overlap(&other), Some("Asura"));
        assert_eq!(names.overlap(&Scanlators::default()), None);
    }

    #[test]
    fn libraries_per_language() {
        let libraries = Libraries::new(
            "/manga".into(),
            BTreeMap::from([("pt-br".into(), "/manga-pt".into())]),
        )
        .unwrap();
        assert_eq!(libraries.root("pt-BR"), Path::new("/manga-pt"));
        assert_eq!(libraries.root("en"), Path::new("/manga"));
        assert_eq!(libraries.roots().count(), 2);

        let nested = BTreeMap::from([("pt-BR".into(), "/manga/pt-br".into())]);
        assert!(Libraries::new("/manga".into(), nested).is_err());
        let junk = BTreeMap::from([("portuguese".into(), "/pt".into())]);
        assert!(Libraries::new("/manga".into(), junk).is_err());
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
