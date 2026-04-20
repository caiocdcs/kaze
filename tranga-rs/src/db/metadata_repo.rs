use sqlx::sqlite::SqlitePool;
use crate::error::AppResult;
use crate::api::dto::*;

pub struct MetadataRepo;

impl MetadataRepo {
    pub async fn list_entries(pool: &SqlitePool) -> AppResult<Vec<MetadataEntryDto>> {
        let rows = sqlx::query_as::<_, MetadataEntryRow>(
            "SELECT metadata_fetcher_name, identifier, manga_id FROM metadata_entries"
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| MetadataEntryDto {
            manga_id: r.manga_id,
            metadata_fetcher_name: r.metadata_fetcher_name,
            identifier: r.identifier,
        }).collect())
    }

    pub async fn list_for_manga(pool: &SqlitePool, manga_id: &str) -> AppResult<Vec<MetadataEntryDto>> {
        let rows = sqlx::query_as::<_, MetadataEntryRow>(
            "SELECT metadata_fetcher_name, identifier, manga_id FROM metadata_entries WHERE manga_id = $1"
        )
        .bind(manga_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| MetadataEntryDto {
            manga_id: r.manga_id,
            metadata_fetcher_name: r.metadata_fetcher_name,
            identifier: r.identifier,
        }).collect())
    }

    pub async fn link(pool: &SqlitePool, fetcher_name: &str, manga_id: &str, identifier: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO metadata_entries (metadata_fetcher_name, identifier, manga_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"
        )
        .bind(fetcher_name)
        .bind(identifier)
        .bind(manga_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn unlink(pool: &SqlitePool, fetcher_name: &str, manga_id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM metadata_entries WHERE metadata_fetcher_name = $1 AND manga_id = $2")
            .bind(fetcher_name)
            .bind(manga_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn get_by_fetcher_and_manga(pool: &SqlitePool, fetcher_name: &str, manga_id: &str) -> AppResult<Option<MetadataEntryDto>> {
        let row = sqlx::query_as::<_, MetadataEntryRow>(
            "SELECT metadata_fetcher_name, identifier, manga_id FROM metadata_entries WHERE metadata_fetcher_name = $1 AND manga_id = $2"
        )
        .bind(fetcher_name)
        .bind(manga_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| MetadataEntryDto {
            manga_id: r.manga_id,
            metadata_fetcher_name: r.metadata_fetcher_name,
            identifier: r.identifier,
        }))
    }

    pub async fn get_entries_for_downloading_manga(pool: &SqlitePool) -> AppResult<Vec<MetadataEntryDto>> {
        let rows = sqlx::query_as::<_, MetadataEntryRow>(
            r#"SELECT me.metadata_fetcher_name, me.identifier, me.manga_id
               FROM metadata_entries me
               INNER JOIN manga_connector_to_manga mcm ON mcm.obj_id = me.manga_id
                WHERE mcm.use_for_download = 1"#
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| MetadataEntryDto {
            manga_id: r.manga_id,
            metadata_fetcher_name: r.metadata_fetcher_name,
            identifier: r.identifier,
        }).collect())
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct MetadataEntryRow {
    pub metadata_fetcher_name: String,
    pub identifier: String,
    pub manga_id: String,
}
