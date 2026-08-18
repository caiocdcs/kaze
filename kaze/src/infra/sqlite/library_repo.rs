use async_trait::async_trait;
use sqlx::sqlite::SqlitePool;

use crate::domain::DomainResult;
use crate::domain::error::{DomainError, StorageError, StorageKind};
use crate::domain::library::*;

pub struct SqliteLibraryRepository {
    pool: SqlitePool,
}

impl SqliteLibraryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn map_sqlx_error(e: sqlx::Error) -> DomainError {
        DomainError::Storage(StorageError {
            kind: StorageKind::Query,
            detail: e.to_string(),
        })
    }
}

#[async_trait]
impl LibraryRepository for SqliteLibraryRepository {
    async fn list(&self) -> DomainResult<Vec<FileLibrary>> {
        let rows = sqlx::query_as::<_, LibraryRow>(
            "SELECT key, base_path, library_name FROM file_libraries ORDER BY library_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|r| FileLibrary {
                id: r.key,
                base_path: r.base_path,
                library_name: r.library_name,
            })
            .collect())
    }

    async fn find_by_id(&self, id: &str) -> DomainResult<Option<FileLibrary>> {
        let row = sqlx::query_as::<_, LibraryRow>(
            "SELECT key, base_path, library_name FROM file_libraries WHERE key = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(row.map(|r| FileLibrary {
            id: r.key,
            base_path: r.base_path,
            library_name: r.library_name,
        }))
    }

    async fn find_default(&self) -> DomainResult<Option<FileLibrary>> {
        let row = sqlx::query_as::<_, LibraryRow>(
            "SELECT key, base_path, library_name FROM file_libraries LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;

        Ok(row.map(|r| FileLibrary {
            id: r.key,
            base_path: r.base_path,
            library_name: r.library_name,
        }))
    }

    async fn save(&self, library: &FileLibrary) -> DomainResult<()> {
        sqlx::query(
            "INSERT INTO file_libraries (key, base_path, library_name) VALUES ($1, $2, $3)",
        )
        .bind(&library.id)
        .bind(&library.base_path)
        .bind(&library.library_name)
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;
        Ok(())
    }

    async fn update(&self, id: &str, base_path: &str, name: &str) -> DomainResult<bool> {
        let result = sqlx::query(
            "UPDATE file_libraries SET base_path = $1, library_name = $2 WHERE key = $3",
        )
        .bind(base_path)
        .bind(name)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(Self::map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete(&self, id: &str) -> DomainResult<bool> {
        let result = sqlx::query("DELETE FROM file_libraries WHERE key = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }
}

#[derive(Debug, sqlx::FromRow)]
struct LibraryRow {
    key: String,
    base_path: String,
    library_name: String,
}
