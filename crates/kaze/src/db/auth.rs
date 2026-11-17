//! The local account and signed-in sessions.

use chrono::{DateTime, Utc};
use sqlx::SqliteExecutor;

#[derive(Debug, Clone)]
pub struct Account {
    pub username: String,
    /// Argon2, in PHC string form.
    pub password_hash: String,
}

pub async fn account(db: impl SqliteExecutor<'_>) -> sqlx::Result<Option<Account>> {
    sqlx::query_as!(
        Account,
        "SELECT username, password_hash FROM account WHERE id = 1"
    )
    .fetch_optional(db)
    .await
}

pub async fn has_account(db: impl SqliteExecutor<'_>) -> sqlx::Result<bool> {
    Ok(account(db).await?.is_some())
}

/// Create the account; `false` when one exists already.
pub async fn create_account(
    db: impl SqliteExecutor<'_>,
    username: &str,
    password_hash: &str,
) -> sqlx::Result<bool> {
    let created = sqlx::query!(
        "INSERT INTO account (id, username, password_hash) VALUES (1, ?, ?) ON CONFLICT (id) DO NOTHING",
        username,
        password_hash,
    )
    .execute(db)
    .await?;
    Ok(created.rows_affected() > 0)
}

/// Create the account or replace its name and password.
pub async fn set_account(
    db: impl SqliteExecutor<'_>,
    username: &str,
    password_hash: &str,
) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO account (id, username, password_hash) VALUES (1, ?1, ?2)
           ON CONFLICT (id) DO UPDATE SET username = ?1, password_hash = ?2"#,
        username,
        password_hash,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// `false` when there is no account.
pub async fn set_password(db: impl SqliteExecutor<'_>, password_hash: &str) -> sqlx::Result<bool> {
    let updated = sqlx::query!(
        "UPDATE account SET password_hash = ? WHERE id = 1",
        password_hash
    )
    .execute(db)
    .await?;
    Ok(updated.rows_affected() > 0)
}

pub async fn create_session(
    db: impl SqliteExecutor<'_>,
    token_hash: &[u8],
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO sessions (token_hash, created_at, expires_at) VALUES (?, ?, ?)",
        token_hash,
        created_at,
        expires_at,
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn session_expiry(
    db: impl SqliteExecutor<'_>,
    token_hash: &[u8],
) -> sqlx::Result<Option<DateTime<Utc>>> {
    sqlx::query_scalar!(
        r#"SELECT expires_at AS "expires_at: DateTime<Utc>" FROM sessions WHERE token_hash = ?"#,
        token_hash
    )
    .fetch_optional(db)
    .await
}

pub async fn delete_session(db: impl SqliteExecutor<'_>, token_hash: &[u8]) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM sessions WHERE token_hash = ?", token_hash)
        .execute(db)
        .await?;
    Ok(())
}

/// Sign out every browser but `keep`, if given.
pub async fn delete_sessions(db: impl SqliteExecutor<'_>, keep: Option<&[u8]>) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM sessions WHERE token_hash IS NOT ?", keep)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn prune_sessions(db: impl SqliteExecutor<'_>, now: DateTime<Utc>) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM sessions WHERE expires_at <= ?", now)
        .execute(db)
        .await?;
    Ok(())
}
