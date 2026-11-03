use chrono::{DateTime, Utc};
use kaze_core::{AniListId, ChapterNumber, Series, SeriesKey};
use sqlx::{SqliteExecutor, SqlitePool, types::Json};

use crate::library::{
    MetadataLink, Monitor, Scanlators, SeriesRecord, SeriesSource, SeriesSummary, SourceOrder,
};

/// A series about to be followed.
#[derive(Debug)]
pub struct NewSeries<'a> {
    pub series: &'a Series,
    pub source: &'a str,
    /// What the series is followed in; a normalized BCP 47 tag.
    pub language: &'a str,
    pub folder: &'a str,
    pub monitor: Monitor,
    pub monitor_from: Option<&'a ChapterNumber>,
}

/// Insert a series and its source mapping; returns the new id.
pub async fn insert(db: &SqlitePool, new: &NewSeries<'_>) -> sqlx::Result<i64> {
    let s = new.series;
    let authors = Json(&s.authors);
    let tags = Json(&s.tags);
    let year = s.year.map(i64::from);
    let cover = s.cover_url.as_ref().map(url::Url::as_str);
    let url = s.url.as_str();

    let mut tx = db.begin().await?;
    let id = sqlx::query_scalar!(
        r#"INSERT INTO series (title, description, authors, tags, status, year, cover_url, folder, monitor, monitor_from)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           RETURNING id AS "id!"
           "#,
        s.title,
        s.description,
        authors,
        tags,
        s.status,
        year,
        cover,
        new.folder,
        new.monitor,
        new.monitor_from,
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO series_sources (series_id, source, source_key, language, url) VALUES (?, ?, ?, ?, ?)",
        id,
        new.source,
        s.key,
        new.language,
        url,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

// `query_as!` maps columns to struct fields by name; the `"name: Type"`
// aliases tell sqlx which Rust type to decode into.
macro_rules! select_series {
    ($where:literal $(, $arg:expr)*) => {
        sqlx::query_as!(
            SeriesRecord,
            r#"SELECT s.id, s.title, s.description,
                      s.authors AS "authors: Json<Vec<String>>",
                      s.tags AS "tags: Json<Vec<String>>",
                      s.status AS "status: _",
                      s.year, s.cover_url, s.folder,
                      s.monitor AS "monitor: Monitor",
                      s.monitor_from AS "monitor_from: _",
                      ss.source, ss.source_key AS "source_key: SeriesKey", ss.language, ss.url,
                      s.added_at AS "added_at: _",
                      s.last_checked_at AS "last_checked_at: _",
                      s.refresh_hours,
                      s.preferred_scanlators AS "preferred_scanlators: Json<Scanlators>",
                      s.blocked_scanlators AS "blocked_scanlators: Json<Scanlators>",
                      s.anilist_id AS "anilist_id: AniListId",
                      s.metadata_link AS "metadata_link: MetadataLink",
                      s.metadata_checked_at AS "metadata_checked_at: _"
               FROM series s
               JOIN series_sources ss ON ss.series_id = s.id AND ss.priority = 0
               "# + $where,
            $($arg),*
        )
    };
}

pub async fn get(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<Option<SeriesRecord>> {
    select_series!("WHERE s.id = ?", id)
        .fetch_optional(db)
        .await
}

pub async fn list(db: impl SqliteExecutor<'_>) -> sqlx::Result<Vec<SeriesRecord>> {
    select_series!("ORDER BY s.title COLLATE NOCASE")
        .fetch_all(db)
        .await
}

/// The series followed with this key on this source in `language`, if any.
pub async fn find_by_source(
    db: impl SqliteExecutor<'_>,
    source: &str,
    key: &SeriesKey,
    language: &str,
) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar!(
        "SELECT series_id FROM series_sources WHERE source = ? AND source_key = ? AND language = ?",
        source,
        key,
        language,
    )
    .fetch_optional(db)
    .await
}

/// Every series followed with this key on this source, one per language.
pub async fn find_all_by_source(
    db: impl SqliteExecutor<'_>,
    source: &str,
    key: &SeriesKey,
) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar!(
        "SELECT series_id FROM series_sources WHERE source = ? AND source_key = ?",
        source,
        key,
    )
    .fetch_all(db)
    .await
}

pub async fn folder_taken(db: impl SqliteExecutor<'_>, folder: &str) -> sqlx::Result<bool> {
    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM series WHERE folder = ?) AS "taken: bool""#,
        folder
    )
    .fetch_one(db)
    .await?;
    Ok(taken)
}

/// Where new metadata comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataFrom {
    /// The primary source: ignored while the series is linked to AniList.
    Source,
    AniList,
}

/// Refresh metadata; keeps the folder and monitor settings. False when a
/// source's metadata was ignored because the series is linked. The check
/// is in the `UPDATE`, so a link made meanwhile is never overwritten.
pub async fn update_metadata(
    db: impl SqliteExecutor<'_>,
    id: i64,
    s: &Series,
    from: MetadataFrom,
) -> sqlx::Result<bool> {
    let authors = Json(&s.authors);
    let tags = Json(&s.tags);
    let year = s.year.map(i64::from);
    let cover = s.cover_url.as_ref().map(url::Url::as_str);
    let linked_too = from == MetadataFrom::AniList;
    let result = sqlx::query!(
        r#"UPDATE series SET title = ?, description = ?, authors = ?, tags = ?, status = ?, year = ?, cover_url = ?
           WHERE id = ? AND (? OR anilist_id IS NULL)"#,
        s.title,
        s.description,
        authors,
        tags,
        s.status,
        year,
        cover,
        id,
        linked_too,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Link the series to an AniList entry, or unlink it with `None`.
pub async fn link_metadata(
    db: impl SqliteExecutor<'_>,
    id: i64,
    anilist: Option<AniListId>,
    link: MetadataLink,
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE series SET anilist_id = ?, metadata_link = ? WHERE id = ?",
        anilist,
        link,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Link an entry found by title, unless the series got a link meanwhile
/// (the source's id wins) or is handled by hand. True if linked.
pub async fn link_found_metadata(
    db: impl SqliteExecutor<'_>,
    id: i64,
    anilist: AniListId,
) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        "UPDATE series SET anilist_id = ?
         WHERE id = ? AND anilist_id IS NULL AND metadata_link = 'auto'",
        anilist,
        id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn mark_metadata_checked(
    db: impl SqliteExecutor<'_>,
    id: i64,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE series SET metadata_checked_at = ? WHERE id = ?",
        now,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Series whose metadata was last checked before `cutoff`, or never.
/// Unlinked by hand means "no metadata": those are left out.
pub async fn metadata_due(
    db: impl SqliteExecutor<'_>,
    cutoff: DateTime<Utc>,
) -> sqlx::Result<Vec<i64>> {
    let rows = sqlx::query!(
        r#"SELECT id AS "id!", metadata_checked_at AS "checked_at: DateTime<Utc>"
           FROM series
           WHERE NOT (anilist_id IS NULL AND metadata_link = 'manual')
           ORDER BY id"#
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|r| r.checked_at.is_none_or(|checked| checked < cutoff))
        .map(|r| r.id)
        .collect())
}

pub async fn set_monitor(
    db: impl SqliteExecutor<'_>,
    id: i64,
    monitor: Monitor,
    from: Option<&ChapterNumber>,
) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        "UPDATE series SET monitor = ?, monitor_from = ? WHERE id = ?",
        monitor,
        from,
        id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Record a check of one of the series' sources. `checked_at` tells a
/// source's first check apart, `last_checked_at` is any source's last.
pub async fn mark_checked(db: &SqlitePool, id: i64, source: &str) -> sqlx::Result<()> {
    let now = Utc::now();
    let mut tx = db.begin().await?;
    sqlx::query!(
        "UPDATE series SET last_checked_at = ? WHERE id = ?",
        now,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE series_sources SET checked_at = ? WHERE series_id = ? AND source = ?",
        now,
        id,
        source
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// The sources of a series, primary first.
pub async fn sources(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<Vec<SeriesSource>> {
    sqlx::query_as!(
        SeriesSource,
        r#"SELECT source, source_key AS "source_key: SeriesKey", url, priority,
                  checked_at AS "checked_at: _"
           FROM series_sources WHERE series_id = ? ORDER BY priority"#,
        id
    )
    .fetch_all(db)
    .await
}

/// Follow a series on one more source, after the others.
pub async fn add_source(
    db: impl SqliteExecutor<'_>,
    id: i64,
    source: &str,
    key: &SeriesKey,
    language: &str,
    url: &str,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO series_sources (series_id, source, source_key, language, url, priority)
         VALUES (?1, ?2, ?3, ?4, ?5,
                 (SELECT COALESCE(MAX(priority), -1) + 1 FROM series_sources WHERE series_id = ?1))",
        id,
        source,
        key,
        language,
        url,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Give the sources priorities 0, 1, … in `order`.
pub async fn reorder_sources(db: &SqlitePool, id: i64, order: &SourceOrder) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    for (priority, source) in (0_i64..).zip(order.as_slice()) {
        sqlx::query!(
            "UPDATE series_sources SET priority = ? WHERE series_id = ? AND source = ?",
            priority,
            id,
            source
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Stop using a source for a series. Its chapters without a file go;
/// downloaded ones stay, so their files are still known.
pub async fn remove_source(db: &SqlitePool, id: i64, source: &str) -> sqlx::Result<bool> {
    let mut tx = db.begin().await?;
    let removed = sqlx::query!(
        "DELETE FROM series_sources WHERE series_id = ? AND source = ?",
        id,
        source
    )
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    sqlx::query!(
        "DELETE FROM chapters WHERE series_id = ? AND source = ? AND file_path IS NULL",
        id,
        source
    )
    .execute(&mut *tx)
    .await?;
    // Close the gap, so the next source becomes the primary if needed.
    sqlx::query!(
        "UPDATE series_sources
         SET priority = (SELECT COUNT(*) FROM series_sources o
                         WHERE o.series_id = series_sources.series_id
                           AND o.priority < series_sources.priority)
         WHERE series_id = ?",
        id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(removed)
}

/// Sources of monitored series whose full refresh is due, as
/// (series, source): each source of a series is refreshed on its own.
///
/// Each series uses its own `refresh_hours` or the global `default_hours`;
/// `0` turns refreshing off. Few rows, so the date math happens in Rust.
pub async fn due_for_refresh(
    db: impl SqliteExecutor<'_>,
    default_hours: u64,
    now: DateTime<Utc>,
) -> sqlx::Result<Vec<(i64, String)>> {
    let rows = sqlx::query!(
        r#"SELECT s.id AS "id!", ss.source, s.refresh_hours,
                  ss.checked_at AS "checked_at: DateTime<Utc>"
           FROM series s JOIN series_sources ss ON ss.series_id = s.id
           WHERE s.monitor != 'none'
           ORDER BY s.id, ss.priority"#
    )
    .fetch_all(db)
    .await?;
    let default_hours = i64::try_from(default_hours).unwrap_or(i64::MAX);
    Ok(rows
        .into_iter()
        .filter(|r| {
            let hours = r.refresh_hours.unwrap_or(default_hours);
            hours > 0
                && r.checked_at
                    .is_none_or(|checked| now - checked >= chrono::Duration::hours(hours))
        })
        .map(|r| (r.id, r.source))
        .collect())
}

/// Every source of every monitored series ("refresh everything now").
pub async fn monitored(db: impl SqliteExecutor<'_>) -> sqlx::Result<Vec<(i64, String)>> {
    let rows = sqlx::query!(
        r#"SELECT s.id AS "id!", ss.source
           FROM series s JOIN series_sources ss ON ss.series_id = s.id
           WHERE s.monitor != 'none'
           ORDER BY s.id, ss.priority"#
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| (r.id, r.source)).collect())
}

pub async fn set_refresh_hours(
    db: impl SqliteExecutor<'_>,
    id: i64,
    hours: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE series SET refresh_hours = ? WHERE id = ?",
        hours,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_scanlators(
    db: impl SqliteExecutor<'_>,
    id: i64,
    preferred: &Scanlators,
    blocked: &Scanlators,
) -> sqlx::Result<()> {
    let (preferred, blocked) = (Json(preferred), Json(blocked));
    sqlx::query!(
        "UPDATE series SET preferred_scanlators = ?, blocked_scanlators = ? WHERE id = ?",
        preferred,
        blocked,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// The groups seen in a series' chapters, for picking preferences.
pub async fn scanlators(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar!(
        r#"SELECT DISTINCT scanlator AS "scanlator!" FROM chapters
           WHERE series_id = ? AND scanlator IS NOT NULL
           ORDER BY scanlator COLLATE NOCASE"#,
        id
    )
    .fetch_all(db)
    .await
}

/// Delete a series with its chapters (files on disk are left alone).
pub async fn delete(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<bool> {
    let result = sqlx::query!("DELETE FROM series WHERE id = ?", id)
        .execute(db)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Every followed series with chapter counts, for the library grid.
pub async fn summaries(db: impl SqliteExecutor<'_>) -> sqlx::Result<Vec<SeriesSummary>> {
    sqlx::query_as!(
        SeriesSummary,
        // Aggregates over a LEFT JOIN make sqlx assume every column may be
        // NULL; `!` asserts the ones that cannot be.
        r#"SELECT s.id AS "id!", s.title AS "title!", s.status AS "status!: _", s.monitor AS "monitor!: Monitor",
                  ss.source AS "source!", ss.language AS "language!", ss.url AS "url!",
                  COUNT(c.id) AS "chapters!: i64",
                  COALESCE(SUM(c.file_path IS NOT NULL), 0) AS "downloaded!: i64",
                  COALESCE(SUM(c.monitored = 1 AND c.file_path IS NULL AND NOT c.blocked), 0) AS "wanted!: i64",
                  COALESCE(SUM(c.size_bytes), 0) AS "size_bytes!: i64",
                  MAX(c.published_at) AS "latest_chapter_at: _",
                  s.added_at AS "added_at!: _"
           FROM series s
           JOIN series_sources ss ON ss.series_id = s.id AND ss.priority = 0
           -- One copy per number, so other sources' copies don't count twice.
           LEFT JOIN chapter_choice c ON c.series_id = s.id AND c.pick = 1
           GROUP BY s.id
           ORDER BY s.title COLLATE NOCASE"#
    )
    .fetch_all(db)
    .await
}
