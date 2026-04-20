use sqlx::sqlite::SqlitePool;
use crate::error::AppResult;

pub struct NotificationRepo;

impl NotificationRepo {
    pub async fn create(
        pool: &SqlitePool,
        key: &str,
        title: &str,
        message: &str,
        urgency: &str,
    ) -> AppResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO notifications (key, title, message, urgency, date, is_sent) VALUES ($1, $2, $3, $4, $5, 0)"
        )
        .bind(key)
        .bind(title)
        .bind(message)
        .bind(urgency)
        .bind(now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn list_unsent(pool: &SqlitePool) -> AppResult<Vec<NotificationDto>> {
        let rows = sqlx::query_as::<_, NotificationRow>(
            "SELECT key, urgency, title, message, date, is_sent FROM notifications WHERE is_sent = 0"
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| NotificationDto {
            key: r.key,
            urgency: r.urgency,
            title: r.title,
            message: r.message,
            date: r.date,
            is_sent: r.is_sent != 0,
        }).collect())
    }

    pub async fn mark_sent(pool: &SqlitePool) -> AppResult<u64> {
        let result = sqlx::query("UPDATE notifications SET is_sent = 1 WHERE is_sent = 0")
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn mark_sent_by_title(pool: &SqlitePool, title: &str) -> AppResult<u64> {
        let result = sqlx::query("UPDATE notifications SET is_sent = 1 WHERE title = $1 AND is_sent = 0")
            .bind(title)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn count_sent(pool: &SqlitePool) -> AppResult<i64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM notifications WHERE is_sent = 1")
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }

    pub async fn delete_sent(pool: &SqlitePool) -> AppResult<u64> {
        let result = sqlx::query("DELETE FROM notifications WHERE is_sent = 1")
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct NotificationRow {
    pub key: String,
    pub urgency: String,
    pub title: String,
    pub message: String,
    pub date: String,
    pub is_sent: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationDto {
    pub key: String,
    pub urgency: String,
    pub title: String,
    pub message: String,
    pub date: String,
    pub is_sent: bool,
}

use serde::{Serialize, Deserialize};
