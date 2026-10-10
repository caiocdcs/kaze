use kaze_core::{ChapterInfo, ChapterKey};
use sqlx::{SqliteExecutor, SqlitePool, types::Json};

use crate::library::{ChapterRecord, WantedChapter};

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

macro_rules! select_chapters {
    ($rest:literal $(, $arg:expr)*) => {
        sqlx::query_as!(
            ChapterRecord,
            r#"SELECT id, series_id, source, source_key AS "source_key: ChapterKey",
                      number AS "number: _", volume, title, url, language, scanlator,
                      published_at AS "published_at: _",
                      monitored AS "monitored: bool",
                      file_path,
                      downloaded_at AS "downloaded_at: _",
                      discovered_at AS "discovered_at: _"
               FROM chapters
               "# + $rest,
            $($arg),*
        )
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

/// Monitored chapters without a file, oldest series first, in reading order.
pub async fn wanted(db: impl SqliteExecutor<'_>, limit: i64) -> sqlx::Result<Vec<ChapterRecord>> {
    select_chapters!(
        "WHERE monitored = 1 AND file_path IS NULL ORDER BY series_id, number_whole, number_frac, id LIMIT ?",
        limit
    )
    .fetch_all(db)
    .await
}

/// Ids of the wanted chapters of one series, in reading order.
pub async fn wanted_ids(db: impl SqliteExecutor<'_>, series_id: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar!(
        r#"SELECT id AS "id!" FROM chapters
           WHERE series_id = ? AND monitored = 1 AND file_path IS NULL
           ORDER BY number_whole, number_frac, id"#,
        series_id
    )
    .fetch_all(db)
    .await
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
           FROM chapters c JOIN series s ON s.id = c.series_id
           -- The newest download job of each chapter (ids only grow).
           LEFT JOIN jobs j ON j.id = (
               SELECT max(id) FROM jobs WHERE dedupe_key = 'download:' || c.id
           )
           WHERE c.monitored = 1 AND c.file_path IS NULL
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
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE chapters SET file_path = ?, downloaded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
        file_path,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn clear_file(db: impl SqliteExecutor<'_>, id: i64, unmonitor: bool) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE chapters SET file_path = NULL, downloaded_at = NULL,
                monitored = CASE WHEN ? THEN 0 ELSE monitored END
         WHERE id = ?",
        unmonitor,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Monitor or unmonitor many chapters in one statement.
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
        "UPDATE chapters SET monitored = ? WHERE id IN (SELECT value FROM json_each(?))",
        monitored,
        ids
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use kaze_core::{Series, SeriesStatus};
    use url::Url;

    use super::*;
    use crate::{
        db::{self, series::NewSeries},
        library::Monitor,
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
            db::series::find_by_source(&pool, "test", &"S1".into())
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

        mark_downloaded(&pool, ids[1], "/lib/One Piece/Ch.2.cbz")
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

    #[tokio::test]
    async fn deleting_series_cascades() {
        let (pool, id) = setup().await;
        insert_new(&pool, id, "test", &[chapter("a", "1")], |_| true)
            .await
            .unwrap();
        assert!(db::series::delete(&pool, id).await.unwrap());
        assert_eq!(wanted(&pool, 100).await.unwrap().len(), 0);
        assert!(
            db::series::find_by_source(&pool, "test", &"S1".into())
                .await
                .unwrap()
                .is_none()
        );
    }
}
