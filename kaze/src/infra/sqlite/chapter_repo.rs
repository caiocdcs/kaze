use async_trait::async_trait;
use sqlx::sqlite::SqlitePool;

use crate::domain::DomainResult;
use crate::domain::chapter::*;
use crate::domain::error::{DomainError, StorageError, StorageKind};
use crate::domain::manga::ConnectorMapping;
use crate::id::EntityId;

pub struct SqliteChapterRepository {
    pool: SqlitePool,
}

impl SqliteChapterRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn map_sqlx_error(e: sqlx::Error) -> DomainError {
        DomainError::Storage(StorageError {
            kind: StorageKind::Query,
            detail: e.to_string(),
        })
    }

    fn status_to_db(status: ChapterStatus) -> i32 {
        match status {
            ChapterStatus::Pending => 0,
            ChapterStatus::Downloading => 2,
            ChapterStatus::Completed => 1,
            ChapterStatus::Failed => 3,
        }
    }

    fn status_from_db(val: i32) -> ChapterStatus {
        match val {
            1 => ChapterStatus::Completed,
            2 => ChapterStatus::Downloading,
            3 => ChapterStatus::Failed,
            _ => ChapterStatus::Pending,
        }
    }

    async fn get_connectors(&self, chapter_id: &str) -> DomainResult<Vec<ConnectorMapping>> {
        let rows = sqlx::query_as::<_, ChapterConnectorRow>(
            "SELECT key, manga_connector_name, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_chapter WHERE obj_id = $1"
        )
        .bind(chapter_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|r| ConnectorMapping {
                id: r.key,
                connector_name: r.manga_connector_name,
                site_id: r.id_on_connector_site,
                website_url: r.website_url,
                use_for_download: r.use_for_download == 1,
            })
            .collect())
    }

    fn row_to_chapter(&self, row: ChapterRow, connectors: Vec<ConnectorMapping>) -> Chapter {
        Chapter {
            id: row.key,
            manga_id: row.parent_manga_id,
            volume: row.volume_number,
            chapter_number: row.chapter_number,
            title: row.title,
            connectors,
            status: Self::status_from_db(row.downloaded),
            file_name: row.file_name,
        }
    }

    async fn build_chapter(&self, row: ChapterRow) -> DomainResult<Chapter> {
        let connectors = self.get_connectors(&row.key).await?;
        Ok(self.row_to_chapter(row, connectors))
    }
}

#[async_trait]
impl ChapterRepository for SqliteChapterRepository {
    async fn find_by_id(&self, id: &str) -> DomainResult<Option<Chapter>> {
        let row = sqlx::query_as::<_, ChapterRow>(
            "SELECT key, parent_manga_id, volume_number, chapter_number, title, file_name, downloaded FROM chapters WHERE key = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        match row {
            Some(r) => Ok(Some(self.build_chapter(r).await?)),
            None => Ok(None),
        }
    }

    async fn list_by_manga(
        &self,
        manga_id: &str,
        _filter: &ChapterFilter,
    ) -> DomainResult<Vec<Chapter>> {
        let rows = sqlx::query_as::<_, ChapterRow>(
            "SELECT key, parent_manga_id, volume_number, chapter_number, title, file_name, downloaded FROM chapters WHERE parent_manga_id = $1"
        )
        .bind(manga_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            result.push(self.build_chapter(row).await?);
        }
        Ok(result)
    }

    async fn count_by_manga(&self, manga_id: &str, _filter: &ChapterFilter) -> DomainResult<i64> {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM chapters WHERE parent_manga_id = $1")
                .bind(manga_id)
                .fetch_one(&self.pool)
                .await
                .map_err(Self::map_sqlx_error)?;
        Ok(row.0)
    }

    async fn latest_by_manga(&self, manga_id: &str) -> DomainResult<Option<Chapter>> {
        let row = sqlx::query_as::<_, ChapterRow>(
            "SELECT key, parent_manga_id, volume_number, chapter_number, title, file_name, downloaded FROM chapters WHERE parent_manga_id = $1 ORDER BY chapter_number DESC LIMIT 1"
        )
        .bind(manga_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        match row {
            Some(r) => Ok(Some(self.build_chapter(r).await?)),
            None => Ok(None),
        }
    }

    async fn latest_downloaded(&self, manga_id: &str) -> DomainResult<Option<Chapter>> {
        let row = sqlx::query_as::<_, ChapterRow>(
            "SELECT key, parent_manga_id, volume_number, chapter_number, title, file_name, downloaded FROM chapters WHERE parent_manga_id = $1 AND downloaded = 1 ORDER BY chapter_number DESC LIMIT 1"
        )
        .bind(manga_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        match row {
            Some(r) => Ok(Some(self.build_chapter(r).await?)),
            None => Ok(None),
        }
    }

    async fn save(&self, chapter: &Chapter) -> DomainResult<()> {
        sqlx::query(
            r#"INSERT INTO chapters (key, parent_manga_id, volume_number, chapter_number, title, downloaded)
               VALUES ($1, $2, $3, $4, $5, $6)
               ON CONFLICT (key) DO UPDATE SET
               volume_number = $3, chapter_number = $4, title = $5, downloaded = $6"#
        )
        .bind(&chapter.id)
        .bind(&chapter.manga_id)
        .bind(chapter.volume)
        .bind(&chapter.chapter_number)
        .bind(&chapter.title)
        .bind(Self::status_to_db(chapter.status))
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> DomainResult<bool> {
        let result = sqlx::query("DELETE FROM chapters WHERE key = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn claim_next_pending(&self) -> DomainResult<Option<Chapter>> {
        // Transaction: find next pending, claim it atomically
        let mut tx = self.pool.begin().await.map_err(Self::map_sqlx_error)?;

        let row = sqlx::query_as::<_, ChapterRow>(
            "SELECT key, parent_manga_id, volume_number, chapter_number, title, file_name, downloaded FROM chapters WHERE downloaded = 0 LIMIT 1"
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Self::map_sqlx_error)?;

        let chapter = match row {
            Some(r) => r,
            None => {
                tx.commit().await.map_err(Self::map_sqlx_error)?;
                return Ok(None);
            }
        };

        sqlx::query("UPDATE chapters SET downloaded = 2 WHERE key = $1")
            .bind(&chapter.key)
            .execute(&mut *tx)
            .await
            .map_err(Self::map_sqlx_error)?;

        tx.commit().await.map_err(Self::map_sqlx_error)?;

        let connectors = self.get_connectors(&chapter.key).await?;
        Ok(Some(self.row_to_chapter(chapter, connectors)))
    }

    async fn mark_completed(&self, id: &str, file_name: &str) -> DomainResult<()> {
        sqlx::query("UPDATE chapters SET downloaded = 1, file_name = $1 WHERE key = $2")
            .bind(file_name)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn mark_failed(&self, id: &str) -> DomainResult<()> {
        sqlx::query("UPDATE chapters SET downloaded = 3 WHERE key = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn reset_failed(&self, _older_than_secs: u64) -> DomainResult<u64> {
        // SQLite doesn't track update timestamps natively, so reset all failed
        let result = sqlx::query("UPDATE chapters SET downloaded = 0 WHERE downloaded = 3")
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(result.rows_affected())
    }

    async fn add_connector(
        &self,
        chapter_id: &str,
        connector: &str,
        site_id: &str,
        website_url: Option<&str>,
    ) -> DomainResult<ConnectorMapping> {
        let key = crate::id::ConnectorMappingId::new().to_string();
        sqlx::query(
            r#"INSERT INTO manga_connector_to_chapter (key, obj_id, manga_connector_name, id_on_connector_site, website_url, use_for_download)
               VALUES ($1, $2, $3, $4, $5, 0)"#
        )
        .bind(&key)
        .bind(chapter_id)
        .bind(connector)
        .bind(site_id)
        .bind(website_url)
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(ConnectorMapping {
            id: key,
            connector_name: connector.to_string(),
            site_id: site_id.to_string(),
            website_url: website_url.map(String::from),
            use_for_download: false,
        })
    }

    async fn get_connector(
        &self,
        chapter_id: &str,
        connector: &str,
    ) -> DomainResult<Option<ConnectorMapping>> {
        let row = sqlx::query_as::<_, ChapterConnectorRow>(
            "SELECT key, manga_connector_name, id_on_connector_site, website_url, use_for_download FROM manga_connector_to_chapter WHERE obj_id = $1 AND manga_connector_name = $2"
        )
        .bind(chapter_id)
        .bind(connector)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(row.map(|r| ConnectorMapping {
            id: r.key,
            connector_name: r.manga_connector_name,
            site_id: r.id_on_connector_site,
            website_url: r.website_url,
            use_for_download: r.use_for_download == 1,
        }))
    }

    async fn delete_connector_mapping(&self, mapping_id: &str) -> DomainResult<bool> {
        let result = sqlx::query("DELETE FROM manga_connector_to_chapter WHERE key = $1")
            .bind(mapping_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn recheck_chapter(&self, chapter_id: &str) -> DomainResult<i64> {
        let result = sqlx::query("DELETE FROM manga_connector_to_chapter WHERE obj_id = $1")
            .bind(chapter_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(result.rows_affected() as i64)
    }

    async fn pending_count(&self) -> DomainResult<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM manga_connector_to_chapter WHERE use_for_download = 1 AND obj_id IN (SELECT key FROM chapters WHERE downloaded = 0)"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;
        Ok(row.0)
    }

    async fn get_missing_downloads(&self) -> DomainResult<Vec<ConnectorMapping>> {
        let rows = sqlx::query_as::<_, ChapterConnectorRow>(
            r#"SELECT mct.key, mct.manga_connector_name, mct.id_on_connector_site, mct.website_url, mct.use_for_download
               FROM manga_connector_to_chapter mct
               INNER JOIN chapters c ON c.key = mct.obj_id
               WHERE mct.use_for_download = 1 AND c.downloaded = 0"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|r| ConnectorMapping {
                id: r.key,
                connector_name: r.manga_connector_name,
                site_id: r.id_on_connector_site,
                website_url: r.website_url,
                use_for_download: r.use_for_download == 1,
            })
            .collect())
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ChapterRow {
    key: String,
    parent_manga_id: String,
    volume_number: Option<i32>,
    chapter_number: String,
    title: Option<String>,
    file_name: Option<String>,
    downloaded: i32,
}

#[derive(Debug, sqlx::FromRow)]
struct ChapterConnectorRow {
    key: String,
    manga_connector_name: String,
    id_on_connector_site: String,
    website_url: Option<String>,
    use_for_download: i32,
}
