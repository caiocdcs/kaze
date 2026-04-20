use sqlx::sqlite::SqlitePool;
use crate::error::AppResult;
use crate::api::dto::*;

pub struct ChapterRepo;

impl ChapterRepo {
    pub async fn list_by_manga(
        pool: &SqlitePool,
        manga_id: &str,
        filter: &ChapterFilter,
        page: i32,
        per_page: i32,
    ) -> AppResult<PagedResponse<ChapterDto>> {
        let all = sqlx::query_as::<_, ChapterRow>(
            "SELECT key, parent_manga_id, volume_number, chapter_number, title, file_name, downloaded FROM chapters WHERE parent_manga_id = $1"
        )
        .bind(manga_id)
        .fetch_all(pool)
        .await?;

        let filtered: Vec<ChapterRow> = all
            .into_iter()
            .filter(|ch| {
                if let Some(d) = filter.downloaded
                    && (ch.downloaded != 0) != d {
                        return false;
                    }
                if let Some(ref name) = filter.name {
                    if let Some(ref title) = ch.title {
                        if !title.contains(name.as_str()) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                if let Some(vol) = filter.volume_number
                    && ch.volume_number != Some(vol) {
                        return false;
                    }
                if let Some(ref cn) = filter.chapter_number
                    && ch.chapter_number != cn.as_str() {
                        return false;
                    }
                true
            })
            .collect();

        let total_count = filtered.len() as i32;
        let total_pages = if total_count == 0 { 0 } else { (total_count - 1) / per_page + 1 };
        let skip = (page - 1) * per_page;
        let page_rows: Vec<ChapterRow> = filtered
            .into_iter()
            .skip(skip as usize)
            .take(per_page as usize)
            .collect();

        let mut page_data = Vec::with_capacity(page_rows.len());
        for ch in page_rows {
            let connector_ids = Self::get_chapter_connector_ids(pool, &ch.key).await?;
            page_data.push(ChapterDto {
                id: ch.key,
                manga_id: ch.parent_manga_id,
                volume: ch.volume_number,
                chapter_number: ch.chapter_number,
                title: ch.title,
                downloaded: ch.downloaded != 0,
                file_name: ch.file_name,
                connectors: connector_ids,
            });
        }

        Ok(PagedResponse {
            data: page_data,
            page,
            total_pages,
            total_count,
        })
    }

    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> AppResult<Option<ChapterDto>> {
        let row = sqlx::query_as::<_, ChapterRow>(
            "SELECT key, parent_manga_id, volume_number, chapter_number, title, file_name, downloaded FROM chapters WHERE key = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(ch) => {
                let connector_ids = Self::get_chapter_connector_ids(pool, &ch.key).await?;
                Ok(Some(ChapterDto {
                    id: ch.key,
                    manga_id: ch.parent_manga_id,
                    volume: ch.volume_number,
                    chapter_number: ch.chapter_number,
                    title: ch.title,
                    downloaded: ch.downloaded != 0,
                    file_name: ch.file_name,
                    connectors: connector_ids,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn delete(pool: &SqlitePool, id: &str) -> AppResult<bool> {
        let result = sqlx::query("DELETE FROM chapters WHERE key = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_latest_by_manga(pool: &SqlitePool, manga_id: &str) -> AppResult<Option<ChapterDto>> {
        let rows = sqlx::query_as::<_, ChapterRow>(
            "SELECT key, parent_manga_id, volume_number, chapter_number, title, file_name, downloaded FROM chapters WHERE parent_manga_id = $1"
        )
        .bind(manga_id)
        .fetch_all(pool)
        .await?;

        let max = rows.into_iter().max_by(|a, b| {
            compare_chapter_numbers(&a.chapter_number, &b.chapter_number)
        });

        match max {
            Some(ch) => {
                let connector_ids = Self::get_chapter_connector_ids(pool, &ch.key).await?;
                Ok(Some(ChapterDto {
                    id: ch.key,
                    manga_id: ch.parent_manga_id,
                    volume: ch.volume_number,
                    chapter_number: ch.chapter_number,
                    title: ch.title,
                    downloaded: ch.downloaded != 0,
                    file_name: ch.file_name,
                    connectors: connector_ids,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn get_latest_downloaded(pool: &SqlitePool, manga_id: &str) -> AppResult<Option<ChapterDto>> {
        let rows = sqlx::query_as::<_, ChapterRow>(
            "SELECT key, parent_manga_id, volume_number, chapter_number, title, file_name, downloaded FROM chapters WHERE parent_manga_id = $1 AND downloaded = 1"
        )
        .bind(manga_id)
        .fetch_all(pool)
        .await?;

        let max = rows.into_iter().max_by(|a, b| {
            compare_chapter_numbers(&a.chapter_number, &b.chapter_number)
        });

        match max {
            Some(ch) => {
                let connector_ids = Self::get_chapter_connector_ids(pool, &ch.key).await?;
                Ok(Some(ChapterDto {
                    id: ch.key,
                    manga_id: ch.parent_manga_id,
                    volume: ch.volume_number,
                    chapter_number: ch.chapter_number,
                    title: ch.title,
                    downloaded: ch.downloaded != 0,
                    file_name: ch.file_name,
                    connectors: connector_ids,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn update_ignore_before(pool: &SqlitePool, manga_id: &str, threshold: f32) -> AppResult<bool> {
        let result = sqlx::query("UPDATE mangas SET ignore_chapters_before = $1 WHERE key = $2")
            .bind(threshold)
            .bind(manga_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn recheck_manga(pool: &SqlitePool, manga_id: Option<&str>) -> AppResult<i64> {
        let query = match manga_id {
            Some(id) => sqlx::query("DELETE FROM manga_connector_to_chapter WHERE obj_id IN (SELECT key FROM chapters WHERE parent_manga_id = $1) AND (SELECT downloaded FROM chapters WHERE key = manga_connector_to_chapter.obj_id) = 1").bind(id),
            None => sqlx::query("DELETE FROM manga_connector_to_chapter WHERE obj_id IN (SELECT key FROM chapters WHERE downloaded = 1)"),
        };
        let result = query.execute(pool).await?;
        Ok(result.rows_affected() as i64)
    }

    pub async fn recheck_chapter(pool: &SqlitePool, chapter_id: &str) -> AppResult<i64> {
        let result = sqlx::query("DELETE FROM manga_connector_to_chapter WHERE obj_id = $1")
            .bind(chapter_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() as i64)
    }

    pub async fn count_missing_downloads(pool: &SqlitePool) -> AppResult<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM manga_connector_to_chapter WHERE use_for_download = 1 AND obj_id IN (SELECT key FROM chapters WHERE downloaded = 0)"
        )
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    pub async fn insert(
        pool: &SqlitePool,
        key: &str,
        parent_manga_id: &str,
        volume_number: Option<i32>,
        chapter_number: &str,
        title: Option<&str>,
    ) -> AppResult<()> {
        sqlx::query(
            r#"INSERT INTO chapters (key, parent_manga_id, volume_number, chapter_number, title, downloaded)
               VALUES ($1, $2, $3, $4, $5, 0)"#
        )
        .bind(key)
        .bind(parent_manga_id)
        .bind(volume_number)
        .bind(chapter_number)
        .bind(title)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn mark_downloaded(pool: &SqlitePool, chapter_id: &str, file_name: &str) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE chapters SET downloaded = 1, file_name = $1 WHERE key = $2"
        )
        .bind(file_name)
        .bind(chapter_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn mark_not_downloaded(pool: &SqlitePool, chapter_id: &str) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE chapters SET downloaded = 0, file_name = NULL WHERE key = $1"
        )
        .bind(chapter_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn insert_chapter_connector_mapping(
        pool: &SqlitePool,
        key: &str,
        chapter_id: &str,
        connector_name: &str,
        id_on_connector_site: &str,
        website_url: Option<&str>,
        use_for_download: bool,
    ) -> AppResult<()> {
        sqlx::query(
            r#"INSERT INTO manga_connector_to_chapter (key, obj_id, manga_connector_name, id_on_connector_site, website_url, use_for_download)
               VALUES ($1, $2, $3, $4, $5, $6)"#
        )
        .bind(key)
        .bind(chapter_id)
        .bind(connector_name)
        .bind(id_on_connector_site)
        .bind(website_url)
        .bind(use_for_download)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_chapter_connector_mapping(pool: &SqlitePool, chapter_id: &str, connector_name: &str) -> AppResult<Option<crate::api::dto::ConnectorMapping>> {
        let row = sqlx::query_as::<_, ChapterConnectorMappingRow>(
            "SELECT key, manga_connector_name, obj_id, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_chapter WHERE obj_id = $1 AND manga_connector_name = $2"
        )
        .bind(chapter_id)
        .bind(connector_name)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| crate::api::dto::ConnectorMapping {
            id: r.key,
            manga_connector_name: r.manga_connector_name,
            entity_id: r.entity_id,
            id_on_connector_site: r.id_on_connector_site,
            website_url: r.website_url,
            use_for_download: r.use_for_download != 0,
        }))
    }

    pub async fn toggle_chapter_connector_download(pool: &SqlitePool, chapter_id: &str, connector_name: &str) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE manga_connector_to_chapter SET use_for_download = CASE WHEN use_for_download = 0 THEN 1 ELSE 0 END WHERE obj_id = $1 AND manga_connector_name = $2"
        )
        .bind(chapter_id)
        .bind(connector_name)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_chapter_connector_mapping(pool: &SqlitePool, mapping_id: &str) -> AppResult<bool> {
        let result = sqlx::query("DELETE FROM manga_connector_to_chapter WHERE key = $1")
            .bind(mapping_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_missing_downloads(pool: &SqlitePool) -> AppResult<Vec<crate::api::dto::ConnectorMapping>> {
        let rows = sqlx::query_as::<_, ChapterConnectorMappingRow>(
            "SELECT mct.key, mct.manga_connector_name, mct.obj_id, mct.id_on_connector_site, mct.website_url, mct.use_for_download
            FROM manga_connector_to_chapter mct
            JOIN chapters c ON mct.obj_id = c.key
            JOIN mangas m ON c.parent_manga_id = m.key
            WHERE mct.use_for_download = 1 AND c.downloaded = 0 AND m.library_id IS NOT NULL"
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| crate::api::dto::ConnectorMapping {
            id: r.key,
            manga_connector_name: r.manga_connector_name,
            entity_id: r.entity_id,
            id_on_connector_site: r.id_on_connector_site,
            website_url: r.website_url,
            use_for_download: r.use_for_download != 0,
        }).collect())
    }

    pub async fn get_chapter_connector_ids(pool: &SqlitePool, chapter_id: &str) -> AppResult<Vec<crate::api::dto::ConnectorMapping>> {
        let rows = sqlx::query_as::<_, ChapterConnectorMappingRow>(
            "SELECT key, manga_connector_name, obj_id, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_chapter WHERE obj_id = $1"
        )
        .bind(chapter_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| crate::api::dto::ConnectorMapping {
            id: r.key,
            manga_connector_name: r.manga_connector_name,
            entity_id: r.entity_id,
            id_on_connector_site: r.id_on_connector_site,
            website_url: r.website_url,
            use_for_download: r.use_for_download != 0,
        }).collect())
    }

    pub async fn get_all_for_manga(pool: &SqlitePool, manga_id: &str) -> AppResult<Vec<ChapterRow>> {
        let rows = sqlx::query_as::<_, ChapterRow>(
            "SELECT key, parent_manga_id, volume_number, chapter_number, title, file_name, downloaded FROM chapters WHERE parent_manga_id = $1"
        )
        .bind(manga_id)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}

fn compare_chapter_numbers(a: &str, b: &str) -> std::cmp::Ordering {
    let parse_parts = |s: &str| -> Vec<i32> {
        s.split('.')
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    let a_parts = parse_parts(a);
    let b_parts = parse_parts(b);

    for (ai, bi) in a_parts.iter().zip(b_parts.iter()) {
        match ai.cmp(bi) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    a_parts.len().cmp(&b_parts.len())
}

#[derive(Debug, sqlx::FromRow)]
pub struct ChapterRow {
    pub key: String,
    pub parent_manga_id: String,
    pub volume_number: Option<i32>,
    pub chapter_number: String,
    pub title: Option<String>,
    pub file_name: Option<String>,
    pub downloaded: i32,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ChapterConnectorMappingRow {
    pub key: String,
    pub manga_connector_name: String,
    #[sqlx(rename = "obj_id")]
    pub entity_id: String,
    pub id_on_connector_site: String,
    pub website_url: Option<String>,
    pub use_for_download: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_chapter_numbers() {
        assert_eq!(compare_chapter_numbers("1", "2"), std::cmp::Ordering::Less);
        assert_eq!(compare_chapter_numbers("10", "2"), std::cmp::Ordering::Greater);
        assert_eq!(compare_chapter_numbers("1.5", "1.5"), std::cmp::Ordering::Equal);
        assert_eq!(compare_chapter_numbers("1.5", "2"), std::cmp::Ordering::Less);
        assert_eq!(compare_chapter_numbers("1.5", "1.6"), std::cmp::Ordering::Less);
    }
}
