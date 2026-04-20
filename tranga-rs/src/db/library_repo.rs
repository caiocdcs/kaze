use sqlx::sqlite::SqlitePool;
use crate::error::AppResult;
use crate::id::EntityId;
use crate::api::dto::*;

pub struct LibraryRepo;

impl LibraryRepo {
    pub async fn list(pool: &SqlitePool) -> AppResult<Vec<FileLibraryDto>> {
        let rows = sqlx::query_as::<_, FileLibraryRow>(
            "SELECT key, base_path, library_name FROM file_libraries"
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| FileLibraryDto {
                id: r.key,
                base_path: r.base_path,
                library_name: r.library_name,
            })
            .collect())
    }

    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> AppResult<Option<FileLibraryDto>> {
        let row = sqlx::query_as::<_, FileLibraryRow>(
            "SELECT key, base_path, library_name FROM file_libraries WHERE key = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| FileLibraryDto {
            id: r.key,
            base_path: r.base_path,
            library_name: r.library_name,
        }))
    }

    pub async fn create(pool: &SqlitePool, key: &str, base_path: &str, library_name: &str) -> AppResult<FileLibraryDto> {
        sqlx::query(
            "INSERT INTO file_libraries (key, base_path, library_name) VALUES ($1, $2, $3)"
        )
        .bind(key)
        .bind(base_path)
        .bind(library_name)
        .execute(pool)
        .await?;

        Ok(FileLibraryDto {
            id: key.to_string(),
            base_path: base_path.to_string(),
            library_name: library_name.to_string(),
        })
    }

    pub async fn update(pool: &SqlitePool, id: &str, base_path: &str, library_name: &str) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE file_libraries SET base_path = $1, library_name = $2 WHERE key = $3"
        )
        .bind(base_path)
        .bind(library_name)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete(pool: &SqlitePool, id: &str) -> AppResult<bool> {
        let result = sqlx::query("DELETE FROM file_libraries WHERE key = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn ensure_default(pool: &SqlitePool, default_path: &str) -> AppResult<()> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM file_libraries")
            .fetch_one(pool)
            .await?;

        if count.0 == 0 {
            let key = crate::id::FileLibraryId::new();
            Self::create(pool, &key.to_string(), default_path, "Default").await?;
        }
        Ok(())
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct FileLibraryRow {
    pub key: String,
    pub base_path: String,
    pub library_name: String,
}
