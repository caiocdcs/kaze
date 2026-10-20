use kaze_core::{ChapterNumber, Series, SeriesKey};
use sqlx::{SqliteExecutor, SqlitePool, types::Json};

use crate::library::{Monitor, SeriesRecord, SeriesSummary};

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
                      s.refresh_hours
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

/// Refresh metadata from the source; keeps the folder and monitor settings.
pub async fn update_metadata(db: impl SqliteExecutor<'_>, id: i64, s: &Series) -> sqlx::Result<()> {
    let authors = Json(&s.authors);
    let tags = Json(&s.tags);
    let year = s.year.map(i64::from);
    let cover = s.cover_url.as_ref().map(url::Url::as_str);
    sqlx::query!(
        r#"UPDATE series SET title = ?, description = ?, authors = ?, tags = ?, status = ?, year = ?, cover_url = ?
           WHERE id = ?"#,
        s.title,
        s.description,
        authors,
        tags,
        s.status,
        year,
        cover,
        id,
    )
    .execute(db)
    .await?;
    Ok(())
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

pub async fn mark_checked(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<()> {
    let now = chrono::Utc::now();
    sqlx::query!(
        "UPDATE series SET last_checked_at = ? WHERE id = ?",
        now,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Monitored series whose full refresh is due, with their source.
///
/// Each series uses its own `refresh_hours` or the global `default_hours`;
/// `0` turns refreshing off. Few rows, so the date math happens in Rust.
pub async fn due_for_refresh(
    db: impl SqliteExecutor<'_>,
    default_hours: u64,
    now: chrono::DateTime<chrono::Utc>,
) -> sqlx::Result<Vec<(i64, String)>> {
    let rows = sqlx::query!(
        r#"SELECT s.id AS "id!", ss.source, s.refresh_hours,
                  s.last_checked_at AS "last_checked_at: chrono::DateTime<chrono::Utc>"
           FROM series s JOIN series_sources ss ON ss.series_id = s.id AND ss.priority = 0
           WHERE s.monitor != 'none'"#
    )
    .fetch_all(db)
    .await?;
    let default_hours = i64::try_from(default_hours).unwrap_or(i64::MAX);
    Ok(rows
        .into_iter()
        .filter(|r| {
            let hours = r.refresh_hours.unwrap_or(default_hours);
            hours > 0
                && r.last_checked_at
                    .is_none_or(|checked| now - checked >= chrono::Duration::hours(hours))
        })
        .map(|r| (r.id, r.source))
        .collect())
}

/// Every monitored series with its source ("refresh everything now").
pub async fn monitored(db: impl SqliteExecutor<'_>) -> sqlx::Result<Vec<(i64, String)>> {
    let rows = sqlx::query!(
        r#"SELECT s.id AS "id!", ss.source
           FROM series s JOIN series_sources ss ON ss.series_id = s.id AND ss.priority = 0
           WHERE s.monitor != 'none'"#
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
                  COALESCE(SUM(c.monitored = 1 AND c.file_path IS NULL), 0) AS "wanted!: i64",
                  COALESCE(SUM(c.size_bytes), 0) AS "size_bytes!: i64",
                  MAX(c.published_at) AS "latest_chapter_at: _",
                  s.added_at AS "added_at!: _"
           FROM series s
           JOIN series_sources ss ON ss.series_id = s.id AND ss.priority = 0
           LEFT JOIN chapters c ON c.series_id = s.id
           GROUP BY s.id
           ORDER BY s.title COLLATE NOCASE"#
    )
    .fetch_all(db)
    .await
}
