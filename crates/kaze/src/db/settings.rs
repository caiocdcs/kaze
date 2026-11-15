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

/// Mark the generated API key as not yet handed out.
pub async fn mark_key_unclaimed(db: impl SqliteExecutor<'_>) -> sqlx::Result<()> {
    sqlx::query!("INSERT OR IGNORE INTO unclaimed_api_key (id) VALUES (1)")
        .execute(db)
        .await?;
    Ok(())
}

/// Take the unclaimed mark; only the first caller gets `true`.
pub async fn claim_key(db: impl SqliteExecutor<'_>) -> sqlx::Result<bool> {
    let claimed = sqlx::query!("DELETE FROM unclaimed_api_key")
        .execute(db)
        .await?;
    Ok(claimed.rows_affected() > 0)
}
