use std::collections::HashMap;

use chrono::{DateTime, Utc};
use kaze_core::{ChapterInfo, ChapterKey, ChapterNumber};
use sqlx::{SqliteExecutor, SqlitePool, types::Json};

use crate::library::{ChapterCopy, ChapterRecord, WantedChapter};

/// Store chapters found on the source; returns the ids of the ones that
/// were new. Known chapters (same source key) are left untouched, so a
/// refresh never resets a download or a monitor choice.
pub async fn insert_new(
    db: &SqlitePool,
    series_id: i64,
    source: &str,
    chapters: &[ChapterInfo],
    mut monitored: impl FnMut(&ChapterInfo) -> bool,
) -> sqlx::Result<Vec<i64>> {
    // One transaction: a thousand inserts commit (and fsync) once.
    let mut tx = db.begin().await?;
    let mut inserted = Vec::new();
    for c in chapters {
        let monitored = monitored(c);
        let volume = c.volume.map(i64::from);
        let url = c.url.as_str();
        let id = sqlx::query_scalar!(
            r#"INSERT INTO chapters
                   (series_id, source, source_key, number, volume, title, url, language, scanlator, published_at, monitored)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT (source, source_key) DO NOTHING
               RETURNING id AS "id!"
               "#,
            series_id,
            source,
            c.key,
            c.number,
            volume,
            c.title,
            url,
            c.language,
            c.scanlator,
            c.published_at,
            monitored,
        )
        .fetch_optional(&mut *tx)
        .await?;
        inserted.extend(id);
    }
    tx.commit().await?;
    Ok(inserted)
}

// `from "chapter_choice"` reads the ranked view (migrations 0009, 0010);
// its columns come back as nullable, hence the `!` overrides. The table
// has no `blocked` column, so its arm passes a constant.
macro_rules! select_chapters {
    (from $from:literal, blocked $blocked:literal, $rest:literal $(, $arg:expr)*) => {
        sqlx::query_as!(
            ChapterRecord,
            r#"SELECT id AS "id!", series_id AS "series_id!", source AS "source!",
                      source_key AS "source_key!: ChapterKey",
                      number AS "number!: _", volume, title, url AS "url!", language, scanlator,
                      published_at AS "published_at: _",
                      monitored AS "monitored!: bool",
                      file_path, size_bytes,
                      downloaded_at AS "downloaded_at: _",
                      discovered_at AS "discovered_at!: _",
                      "# + $blocked + r#" AS "blocked!: bool"
               FROM "# + $from + " " + $rest,
            $($arg),*
        )
    };
    ($rest:literal $(, $arg:expr)*) => {
        select_chapters!(from "chapters", blocked "0", $rest $(, $arg)*)
    };
}

pub async fn get(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<Option<ChapterRecord>> {
    select_chapters!("WHERE id = ?", id)
        .fetch_optional(db)
        .await
}

/// All chapters of a series in reading order.
pub async fn for_series(
    db: impl SqliteExecutor<'_>,
    series_id: i64,
) -> sqlx::Result<Vec<ChapterRecord>> {
    select_chapters!(
        "WHERE series_id = ? ORDER BY number_whole, number_frac, id",
        series_id
    )
    .fetch_all(db)
    .await
}

/// One chapter per number: the copy on disk, else the one to download.
pub async fn picks_for_series(
    db: impl SqliteExecutor<'_>,
    series_id: i64,
) -> sqlx::Result<Vec<ChapterRecord>> {
    select_chapters!(
        from "chapter_choice", blocked "blocked",
        "WHERE series_id = ? AND pick = 1 ORDER BY number_whole, number_frac, id",
        series_id
    )
    .fetch_all(db)
    .await
}

/// Monitored chapters without a file, oldest series first, in reading order.
/// Only the picked copy of each number counts.
pub async fn wanted(db: impl SqliteExecutor<'_>, limit: i64) -> sqlx::Result<Vec<ChapterRecord>> {
    select_chapters!(
        from "chapter_choice", blocked "blocked",
        "WHERE pick = 1 AND monitored = 1 AND file_path IS NULL AND NOT blocked
         ORDER BY series_id, number_whole, number_frac, id LIMIT ?",
        limit
    )
    .fetch_all(db)
    .await
}

/// The wanted chapters of one series in reading order, as (id, source).
pub async fn wanted_ids(
    db: impl SqliteExecutor<'_>,
    series_id: i64,
) -> sqlx::Result<Vec<(i64, String)>> {
    let rows = sqlx::query!(
        r#"SELECT id AS "id!", source AS "source!" FROM chapter_choice
           WHERE series_id = ? AND pick = 1 AND monitored = 1 AND file_path IS NULL AND NOT blocked
           ORDER BY number_whole, number_frac, id"#,
        series_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| (r.id, r.source)).collect())
}

/// The numbers a series already lists, each with whether it is monitored.
pub async fn known_numbers(
    db: impl SqliteExecutor<'_>,
    series_id: i64,
) -> sqlx::Result<HashMap<ChapterNumber, bool>> {
    let rows = sqlx::query!(
        r#"SELECT number AS "number!: ChapterNumber", MAX(monitored) AS "monitored!: bool"
           FROM chapters WHERE series_id = ? GROUP BY number"#,
        series_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| (r.number, r.monitored)).collect())
}

/// Wanted picks on other sources, in the series that use `source`: what
/// to download once `source` is broken and its copies rank last.
pub async fn picks_away_from(
    db: impl SqliteExecutor<'_>,
    source: &str,
) -> sqlx::Result<Vec<(i64, String)>> {
    let rows = sqlx::query!(
        r#"SELECT id AS "id!", source AS "source!" FROM chapter_choice
           WHERE pick = 1 AND monitored = 1 AND file_path IS NULL AND source != ?1 AND NOT blocked
             AND series_id IN (SELECT series_id FROM series_sources WHERE source = ?1)
           ORDER BY series_id, number_whole, number_frac"#,
        source
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| (r.id, r.source)).collect())
}

/// The wanted list with series titles, for the "Wanted" page.
pub async fn wanted_with_series(
    db: impl SqliteExecutor<'_>,
    limit: i64,
) -> sqlx::Result<Vec<WantedChapter>> {
    sqlx::query_as!(
        WantedChapter,
        r#"SELECT c.id AS "id!", c.series_id, s.title AS series_title, c.source,
                  c.number AS "number: _", c.volume, c.title, c.published_at AS "published_at: _",
                  j.updated_at AS "last_attempt_at?: _", j.state AS "last_attempt_state?",
                  j.last_error AS "last_error?"
           FROM chapter_choice c JOIN series s ON s.id = c.series_id
           -- The newest download job of each chapter (ids only grow).
           LEFT JOIN jobs j ON j.id = (
               SELECT max(id) FROM jobs WHERE dedupe_key = 'download:' || c.id
           )
           WHERE c.pick = 1 AND c.monitored = 1 AND c.file_path IS NULL AND NOT c.blocked
           ORDER BY s.title COLLATE NOCASE, c.number_whole, c.number_frac
           LIMIT ?"#,
        limit
    )
    .fetch_all(db)
    .await
}

pub async fn mark_downloaded(
    db: impl SqliteExecutor<'_>,
    id: i64,
    file_path: &str,
    size_bytes: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE chapters SET file_path = ?, size_bytes = ?,
                downloaded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
        file_path,
        size_bytes,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn clear_file(db: impl SqliteExecutor<'_>, id: i64, unmonitor: bool) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE chapters SET file_path = NULL, size_bytes = NULL, downloaded_at = NULL,
                monitored = CASE WHEN ? THEN 0 ELSE monitored END
         WHERE id = ?",
        unmonitor,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Every chapter with a file, as (id, path, recorded size).
pub async fn files(db: impl SqliteExecutor<'_>) -> sqlx::Result<Vec<(i64, String, Option<i64>)>> {
    let rows = sqlx::query!(
        r#"SELECT id AS "id!", file_path AS "file_path!", size_bytes FROM chapters
           WHERE file_path IS NOT NULL"#
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.id, r.file_path, r.size_bytes))
        .collect())
}

pub async fn set_size(
    db: impl SqliteExecutor<'_>,
    id: i64,
    size_bytes: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE chapters SET size_bytes = ? WHERE id = ?",
        size_bytes,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Monitor or unmonitor many chapters in one statement, with every other
/// copy of the same numbers, so a choice holds whichever copy is picked.
///
/// SQLite has no array parameters, so the ids travel as one JSON array
/// that `json_each` unpacks into rows.
pub async fn set_monitored(
    db: impl SqliteExecutor<'_>,
    ids: &[i64],
    monitored: bool,
) -> sqlx::Result<u64> {
    let ids = Json(ids);
    let result = sqlx::query!(
        "UPDATE chapters SET monitored = ?
         WHERE (series_id, number) IN (
             SELECT series_id, number FROM chapters WHERE id IN (SELECT value FROM json_each(?))
         )",
        monitored,
        ids
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

/// The wanted picks among the numbers of `ids`, as (id, source): what to
/// queue after monitoring those chapters.
pub async fn wanted_picks(
    db: impl SqliteExecutor<'_>,
    ids: &[i64],
) -> sqlx::Result<Vec<(i64, String)>> {
    let ids = Json(ids);
    let rows = sqlx::query!(
        r#"SELECT id AS "id!", source AS "source!" FROM chapter_choice
           WHERE pick = 1 AND monitored = 1 AND file_path IS NULL AND NOT blocked
             AND (series_id, number) IN (
                 SELECT series_id, number FROM chapters WHERE id IN (SELECT value FROM json_each(?))
             )"#,
        ids
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| (r.id, r.source)).collect())
}

/// Release dates of the picked chapters of monitored series, as (series,
/// title, date), grouped by series.
pub async fn release_dates(
    db: impl SqliteExecutor<'_>,
) -> sqlx::Result<Vec<(i64, String, DateTime<Utc>)>> {
    let rows = sqlx::query!(
        r#"SELECT c.series_id AS "series_id!", s.title,
                  c.published_at AS "published_at!: DateTime<Utc>"
           FROM chapter_choice c JOIN series s ON s.id = c.series_id
           WHERE c.pick = 1 AND c.published_at IS NOT NULL AND s.monitor != 'none'
           ORDER BY c.series_id"#
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.series_id, r.title, r.published_at))
        .collect())
}

/// Every copy of this chapter's number, best ranked first.
pub async fn copies(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<Vec<ChapterCopy>> {
    sqlx::query_as!(
        ChapterCopy,
        r#"SELECT c.id AS "id!", c.source AS "source!", c.scanlator, c.url AS "url!",
                  c.pick = 1 AS "picked!: bool",
                  c.file_path IS NOT NULL AS "on_disk!: bool",
                  c.blocked AS "blocked!: bool",
                  j.state AS "last_attempt_state?", j.last_error AS "last_error?"
           FROM chapter_choice c
           JOIN chapters me ON me.id = ?
           LEFT JOIN jobs j ON j.id = (
               SELECT max(id) FROM jobs WHERE dedupe_key = 'download:' || c.id
           )
           WHERE c.series_id = me.series_id AND c.number = me.number
           ORDER BY c.pick"#,
        id
    )
    .fetch_all(db)
    .await
}

/// Whether another copy of this chapter's number is already on disk.
pub async fn number_on_disk(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<bool> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS(
               SELECT 1 FROM chapters c JOIN chapters o
                 ON o.series_id = c.series_id AND o.number = c.number AND o.id != c.id
               WHERE c.id = ? AND o.file_path IS NOT NULL
           ) AS "on_disk!: bool""#,
        id
    )
    .fetch_one(db)
    .await
}

#[cfg(test)]
mod tests {
    use kaze_core::{Series, SeriesStatus};
    use url::Url;

    use super::*;
    use crate::{
        db::{self, series::NewSeries},
        library::{Monitor, Scanlators},
    };

    fn series() -> Series {
        Series {
            key: "S1".into(),
            title: "One Piece".into(),
            alt_titles: vec![],
            url: Url::parse("https://example.com/series/S1").unwrap(),
            cover_url: None,
            description: Some("Pirates".into()),
            authors: vec!["Oda".into()],
            tags: vec!["Action".into()],
            status: SeriesStatus::Ongoing,
            year: Some(1997),
            language: Some("en".into()),
        }
    }

    fn chapter(key: &str, number: &str) -> ChapterInfo {
        ChapterInfo {
            key: key.into(),
            number: number.parse().unwrap(),
            volume: None,
            title: None,
            url: Url::parse(&format!("https://example.com/chapters/{key}")).unwrap(),
            language: Some("en".into()),
            scanlator: None,
            published_at: None,
        }
    }

    async fn setup() -> (SqlitePool, i64) {
        let pool = db::memory().await;
        let s = series();
        let id = db::series::insert(
            &pool,
            &NewSeries {
                series: &s,
                source: "test",
                language: "en",
                folder: "One Piece",
                monitor: Monitor::All,
                monitor_from: None,
            },
        )
        .await
        .unwrap();
        (pool, id)
    }

    #[tokio::test]
    async fn series_round_trip() {
        let (pool, id) = setup().await;
        let record = db::series::get(&pool, id).await.unwrap().unwrap();
        assert_eq!(record.title, "One Piece");
        assert_eq!(record.authors.0, ["Oda"]);
        assert_eq!(record.status, SeriesStatus::Ongoing);
        assert_eq!(record.source_key.as_str(), "S1");
        assert_eq!(
            db::series::find_by_source(&pool, "test", &"S1".into(), "en")
                .await
                .unwrap(),
            Some(id)
        );
        assert_eq!(db::series::list(&pool).await.unwrap().len(), 1);

        assert!(
            db::series::set_monitor(&pool, id, Monitor::Future, Some(&"10".parse().unwrap()))
                .await
                .unwrap()
        );
        let record = db::series::get(&pool, id).await.unwrap().unwrap();
        assert_eq!(record.monitor, Monitor::Future);
        assert_eq!(record.monitor_from.unwrap().to_string(), "10");
    }

    #[tokio::test]
    async fn only_new_chapters_are_inserted() {
        let (pool, id) = setup().await;
        let first = [chapter("a", "1"), chapter("b", "2")];
        let new = insert_new(&pool, id, "test", &first, |_| true)
            .await
            .unwrap();
        assert_eq!(new.len(), 2);

        let second = [chapter("a", "1"), chapter("b", "2"), chapter("c", "3")];
        let new = insert_new(&pool, id, "test", &second, |_| true)
            .await
            .unwrap();
        assert_eq!(new.len(), 1);
        assert_eq!(
            get(&pool, new[0])
                .await
                .unwrap()
                .unwrap()
                .source_key
                .as_str(),
            "c"
        );
    }

    #[tokio::test]
    async fn chapters_sort_numerically() {
        let (pool, id) = setup().await;
        let list = ["10", "2.5", "1", "2.15", "2"].map(|n| chapter(n, n));
        insert_new(&pool, id, "test", &list, |_| true)
            .await
            .unwrap();
        let numbers: Vec<_> = for_series(&pool, id)
            .await
            .unwrap()
            .iter()
            .map(|c| c.number.to_string())
            .collect();
        assert_eq!(numbers, ["1", "2", "2.15", "2.5", "10"]);
    }

    #[tokio::test]
    async fn wanted_follows_monitoring_and_downloads() {
        let (pool, id) = setup().await;
        let list = [chapter("a", "1"), chapter("b", "2"), chapter("c", "3")];
        // Chapter 1 starts unmonitored.
        let ids = insert_new(&pool, id, "test", &list, |c| c.number.whole() > 1)
            .await
            .unwrap();
        assert_eq!(wanted(&pool, 100).await.unwrap().len(), 2);

        mark_downloaded(&pool, ids[1], "/lib/One Piece/Ch.2.cbz", Some(10))
            .await
            .unwrap();
        let wanted_now = wanted(&pool, 100).await.unwrap();
        assert_eq!(wanted_now.len(), 1);
        assert_eq!(wanted_now[0].number.to_string(), "3");

        assert_eq!(
            set_monitored(&pool, &[ids[0], ids[2]], false)
                .await
                .unwrap(),
            2
        );
        assert_eq!(wanted(&pool, 100).await.unwrap().len(), 0);
        let done = get(&pool, ids[1]).await.unwrap().unwrap();
        assert!(done.downloaded_at.is_some() && !done.is_wanted());
    }

    /// Follow the series on a second source too, ranked after "test".
    async fn add_source(pool: &SqlitePool, series_id: i64, source: &str) {
        sqlx::query(
            "INSERT INTO series_sources (series_id, source, source_key, language, url, priority)
             VALUES (?, ?, 'S1', 'en', 'https://example.com/S1', 1)",
        )
        .bind(series_id)
        .bind(source)
        .execute(pool)
        .await
        .unwrap();
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(n, s)| ((*n).to_owned(), (*s).to_owned()))
            .collect()
    }

    fn wanted_sources(list: &[ChapterRecord]) -> Vec<(String, String)> {
        list.iter()
            .map(|c| (c.number.to_string(), c.source.clone()))
            .collect()
    }

    #[tokio::test]
    async fn one_copy_per_number_is_wanted() {
        let (pool, id) = setup().await;
        add_source(&pool, id, "other").await;
        let ours = insert_new(
            &pool,
            id,
            "test",
            &[chapter("a", "1"), chapter("b", "2")],
            |_| true,
        )
        .await
        .unwrap();
        let theirs = [chapter("x", "1"), chapter("y", "2"), chapter("z", "3")];
        insert_new(&pool, id, "other", &theirs, |_| true)
            .await
            .unwrap();

        // The primary's copy wins; "other" fills the gap at 3.
        let expected = [("1", "test"), ("2", "test"), ("3", "other")]
            .map(|(n, s)| (n.to_owned(), s.to_owned()));
        assert_eq!(wanted_sources(&wanted(&pool, 100).await.unwrap()), expected);
        assert_eq!(picks_for_series(&pool, id).await.unwrap().len(), 3);
        assert_eq!(wanted_ids(&pool, id).await.unwrap().len(), 3);

        // A copy on disk is the pick, whichever source it came from.
        let other_1 = for_series(&pool, id).await.unwrap()[1].id;
        mark_downloaded(&pool, other_1, "/lib/Ch.1.cbz", None)
            .await
            .unwrap();
        assert!(number_on_disk(&pool, ours[0]).await.unwrap());
        let picks = picks_for_series(&pool, id).await.unwrap();
        assert_eq!(picks[0].id, other_1);
        assert_eq!(wanted(&pool, 100).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn monitoring_applies_to_every_copy() {
        let (pool, id) = setup().await;
        add_source(&pool, id, "other").await;
        let ours = insert_new(&pool, id, "test", &[chapter("a", "1")], |_| true)
            .await
            .unwrap();
        insert_new(&pool, id, "other", &[chapter("x", "1")], |_| true)
            .await
            .unwrap();

        assert_eq!(set_monitored(&pool, &ours, false).await.unwrap(), 2);
        assert_eq!(wanted(&pool, 100).await.unwrap().len(), 0);
        assert_eq!(wanted_picks(&pool, &ours).await.unwrap().len(), 0);

        set_monitored(&pool, &ours, true).await.unwrap();
        assert_eq!(
            wanted_picks(&pool, &ours).await.unwrap(),
            [(ours[0], "test".to_owned())]
        );
    }

    #[tokio::test]
    async fn broken_sources_and_failed_copies_go_last() {
        let (pool, id) = setup().await;
        add_source(&pool, id, "other").await;
        let ours = insert_new(
            &pool,
            id,
            "test",
            &[chapter("a", "1"), chapter("b", "2")],
            |_| true,
        )
        .await
        .unwrap();
        insert_new(
            &pool,
            id,
            "other",
            &[chapter("x", "1"), chapter("y", "2")],
            |_| true,
        )
        .await
        .unwrap();

        // Chapter 1's last download on "test" failed for good.
        sqlx::query(
            "INSERT INTO jobs (kind, payload, dedupe_key, state, run_after, created_at, updated_at)
             VALUES ('download_chapter', '{}', ?, 'failed', '', '', '')",
        )
        .bind(format!("download:{}", ours[0]))
        .execute(&pool)
        .await
        .unwrap();
        let picked = || async { wanted_sources(&wanted(&pool, 100).await.unwrap()) };
        let expect =
            |a: &str, b: &str| [("1", a), ("2", b)].map(|(n, s)| (n.to_owned(), s.to_owned()));
        assert_eq!(picked().await, expect("other", "test"));

        // One failure short of broken changes nothing; reaching it moves all.
        for n in 1..db::sources::FAILING_AFTER {
            db::sources::record_failure(&pool, "test", &format!("error {n}"))
                .await
                .unwrap();
        }
        assert_eq!(picked().await, expect("other", "test"));
        db::sources::record_failure(&pool, "test", "error")
            .await
            .unwrap();
        assert_eq!(picked().await, expect("other", "other"));
    }

    fn by(key: &str, number: &str, group: &str) -> ChapterInfo {
        ChapterInfo {
            scanlator: Some(group.to_owned()),
            ..chapter(key, number)
        }
    }

    async fn set_groups(pool: &SqlitePool, id: i64, preferred: &[&str], blocked: &[&str]) {
        let names = |list: &[&str]| {
            Scanlators::from(list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
        };
        db::series::set_scanlators(pool, id, &names(preferred), &names(blocked))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn preferred_groups_win_and_blocked_ones_are_never_wanted() {
        let (pool, id) = setup().await;
        add_source(&pool, id, "other").await;
        insert_new(
            &pool,
            id,
            "test",
            &[by("a", "1", "Alpha"), by("b", "2", "Alpha")],
            |_| true,
        )
        .await
        .unwrap();
        insert_new(&pool, id, "other", &[by("x", "1", "Beta")], |_| true)
            .await
            .unwrap();
        let picked = || async { wanted_sources(&wanted(&pool, 100).await.unwrap()) };

        // Preference beats source priority, case-insensitively.
        set_groups(&pool, id, &["beta"], &[]).await;
        assert_eq!(picked().await, pairs(&[("1", "other"), ("2", "test")]));

        // Blocked: the other copy is picked; a blocked only copy isn't wanted.
        set_groups(&pool, id, &[], &["ALPHA"]).await;
        assert_eq!(picked().await, pairs(&[("1", "other")]));
        let picks = picks_for_series(&pool, id).await.unwrap();
        assert!(picks[1].blocked && !picks[1].is_wanted());
        assert_eq!(wanted_ids(&pool, id).await.unwrap().len(), 1);

        // Health comes first: a preferred group on a broken source hands over.
        set_groups(&pool, id, &["Beta"], &[]).await;
        for n in 0..db::sources::FAILING_AFTER {
            db::sources::record_failure(&pool, "other", &format!("error {n}"))
                .await
                .unwrap();
        }
        assert_eq!(picked().await, pairs(&[("1", "test"), ("2", "test")]));
    }

    #[tokio::test]
    async fn deleting_series_cascades() {
        let (pool, id) = setup().await;
        insert_new(&pool, id, "test", &[chapter("a", "1")], |_| true)
            .await
            .unwrap();
        assert!(db::series::delete(&pool, id).await.unwrap());
        assert_eq!(wanted(&pool, 100).await.unwrap().len(), 0);
        assert!(
            db::series::find_by_source(&pool, "test", &"S1".into(), "en")
                .await
                .unwrap()
                .is_none()
        );
    }
}
