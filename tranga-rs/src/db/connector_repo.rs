use sqlx::sqlite::SqlitePool;
use crate::error::AppResult;
use crate::api::dto::*;

pub struct ConnectorRepo;

impl ConnectorRepo {
    pub async fn list_library_connectors(pool: &SqlitePool) -> AppResult<Vec<LibraryConnectorDto>> {
        let rows = sqlx::query_as::<_, LibraryConnectorRow>(
            "SELECT key, library_type, base_url, auth FROM library_connectors"
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| LibraryConnectorDto {
                id: r.key,
                library_type: r.library_type,
                base_url: r.base_url,
                auth: r.auth,
            })
            .collect())
    }

    pub async fn get_library_connector(pool: &SqlitePool, id: &str) -> AppResult<Option<LibraryConnectorDto>> {
        let row = sqlx::query_as::<_, LibraryConnectorRow>(
            "SELECT key, library_type, base_url, auth FROM library_connectors WHERE key = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| LibraryConnectorDto {
            id: r.key,
            library_type: r.library_type,
            base_url: r.base_url,
            auth: r.auth,
        }))
    }

    pub async fn create_library_connector(
        pool: &SqlitePool,
        key: &str,
        library_type: &str,
        base_url: &str,
        auth: &str,
    ) -> AppResult<LibraryConnectorDto> {
        sqlx::query(
            "INSERT INTO library_connectors (key, library_type, base_url, auth) VALUES ($1, $2, $3, $4)"
        )
        .bind(key)
        .bind(library_type)
        .bind(base_url)
        .bind(auth)
        .execute(pool)
        .await?;

        Ok(LibraryConnectorDto {
            id: key.to_string(),
            library_type: library_type.to_string(),
            base_url: base_url.to_string(),
            auth: auth.to_string(),
        })
    }

    pub async fn delete_library_connector(pool: &SqlitePool, id: &str) -> AppResult<bool> {
        let result = sqlx::query("DELETE FROM library_connectors WHERE key = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_notification_connectors(pool: &SqlitePool) -> AppResult<Vec<NotificationConnectorDto>> {
        let rows = sqlx::query_as::<_, NotificationConnectorRow>(
            "SELECT name, url, headers, http_method, body FROM notification_connectors"
        )
        .fetch_all(pool)
        .await?;

        let mut result = Vec::new();
        for r in rows {
            let headers: HashMap<String, String> = serde_json::from_str(&r.headers).unwrap_or_default();
            result.push(NotificationConnectorDto {
                name: r.name,
                url: r.url,
                headers,
                http_method: r.http_method,
                body: r.body,
            });
        }
        Ok(result)
    }

    pub async fn get_notification_connector(pool: &SqlitePool, name: &str) -> AppResult<Option<NotificationConnectorDto>> {
        let row = sqlx::query_as::<_, NotificationConnectorRow>(
            "SELECT name, url, headers, http_method, body FROM notification_connectors WHERE name = $1"
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| {
            let headers: HashMap<String, String> = serde_json::from_str(&r.headers).unwrap_or_default();
            NotificationConnectorDto {
                name: r.name,
                url: r.url,
                headers,
                http_method: r.http_method,
                body: r.body,
            }
        }))
    }

    pub async fn upsert_notification_connector(
        pool: &SqlitePool,
        name: &str,
        url: &str,
        headers: &str,
        http_method: &str,
        body: &str,
    ) -> AppResult<NotificationConnectorDto> {
        sqlx::query(
            r#"INSERT INTO notification_connectors (name, url, headers, http_method, body)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (name) DO UPDATE SET url = $2, headers = $3, http_method = $4, body = $5"#
        )
        .bind(name)
        .bind(url)
        .bind(headers)
        .bind(http_method)
        .bind(body)
        .execute(pool)
        .await?;

        let hdrs: HashMap<String, String> = serde_json::from_str(headers).unwrap_or_default();
        Ok(NotificationConnectorDto {
            name: name.to_string(),
            url: url.to_string(),
            headers: hdrs,
            http_method: http_method.to_string(),
            body: body.to_string(),
        })
    }

    pub async fn delete_notification_connector(pool: &SqlitePool, name: &str) -> AppResult<bool> {
        let result = sqlx::query("DELETE FROM notification_connectors WHERE name = $1")
            .bind(name)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_manga_connector_enabled(pool: &SqlitePool, name: &str, enabled: bool) -> AppResult<()> {
        sqlx::query(
            r#"INSERT INTO manga_connectors (name, enabled) VALUES ($1, $2)
               ON CONFLICT (name) DO UPDATE SET enabled = $2"#
        )
        .bind(name)
        .bind(enabled)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_manga_connector_enabled(pool: &SqlitePool, name: &str) -> AppResult<Option<bool>> {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT enabled FROM manga_connectors WHERE name = $1"
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|r| r.0 != 0))
    }

    pub async fn get_all_manga_connector_states(pool: &SqlitePool) -> AppResult<HashMap<String, bool>> {
        let rows = sqlx::query_as::<_, (String, i32)>(
            "SELECT name, enabled FROM manga_connectors"
        )
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(name, enabled)| (name, enabled != 0)).collect())
    }
}

use std::collections::HashMap;

#[derive(Debug, sqlx::FromRow)]
pub struct LibraryConnectorRow {
    pub key: String,
    pub library_type: String,
    pub base_url: String,
    pub auth: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct NotificationConnectorRow {
    pub name: String,
    pub url: String,
    pub headers: String,
    pub http_method: String,
    pub body: String,
}
