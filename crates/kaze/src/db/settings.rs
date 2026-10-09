//! Settings saved from the UI, as JSON values by key.

use serde_json::{Map, Value};
use sqlx::{SqliteExecutor, SqlitePool, types::Json};

pub async fn all(db: impl SqliteExecutor<'_>) -> sqlx::Result<Map<String, Value>> {
    let rows = sqlx::query!(r#"SELECT key AS "key!", value AS "value: Json<Value>" FROM settings"#)
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().map(|r| (r.key, r.value.0)).collect())
}

/// Apply changes in one transaction; a `null` value deletes the key
/// (back to the default).
pub async fn save(db: &SqlitePool, changes: &Map<String, Value>) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    for (key, value) in changes {
        if value.is_null() {
            sqlx::query!("DELETE FROM settings WHERE key = ?", key)
                .execute(&mut *tx)
                .await?;
        } else {
            let value = Json(value);
            sqlx::query!(
                "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                key,
                value
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await
}
