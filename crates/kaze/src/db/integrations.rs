use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{SqliteExecutor, types::Json};
use ts_rs::TS;

use crate::integrations::{Config, Trigger};

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct Integration {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    #[ts(as = "Config")]
    pub config: Json<Config>,
    /// Which events a notifier hears about; library servers ignore it.
    #[ts(as = "Vec<Trigger>")]
    pub events: Json<Vec<Trigger>>,
    pub last_ok: Option<bool>,
    pub last_status: Option<String>,
    pub last_sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Integration {
    /// Without its secrets, for the API.
    #[must_use]
    pub fn masked(mut self) -> Self {
        self.config = Json(self.config.0.masked());
        self
    }
}

pub struct NewIntegration<'a> {
    pub name: &'a str,
    pub enabled: bool,
    pub config: &'a Config,
    pub events: &'a [Trigger],
}

pub async fn insert(db: impl SqliteExecutor<'_>, new: &NewIntegration<'_>) -> sqlx::Result<i64> {
    let now = Utc::now();
    let config = Json(new.config);
    let events = Json(new.events);
    sqlx::query_scalar!(
        r#"INSERT INTO integrations (name, enabled, config, events, created_at)
           VALUES (?, ?, ?, ?, ?) RETURNING id AS "id!""#,
        new.name,
        new.enabled,
        config,
        events,
        now,
    )
    .fetch_one(db)
    .await
}

pub async fn update(
    db: impl SqliteExecutor<'_>,
    id: i64,
    new: &NewIntegration<'_>,
) -> sqlx::Result<bool> {
    let config = Json(new.config);
    let events = Json(new.events);
    let result = sqlx::query!(
        "UPDATE integrations SET name = ?, enabled = ?, config = ?, events = ? WHERE id = ?",
        new.name,
        new.enabled,
        config,
        events,
        id,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn get(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<Option<Integration>> {
    sqlx::query_as!(
        Integration,
        r#"SELECT id, name, enabled AS "enabled: bool", config AS "config: Json<Config>",
                  events AS "events: Json<Vec<Trigger>>", last_ok AS "last_ok: bool", last_status,
                  last_sent_at AS "last_sent_at: _", created_at AS "created_at: _"
           FROM integrations WHERE id = ?"#,
        id
    )
    .fetch_optional(db)
    .await
}

pub async fn list(db: impl SqliteExecutor<'_>) -> sqlx::Result<Vec<Integration>> {
    sqlx::query_as!(
        Integration,
        r#"SELECT id, name, enabled AS "enabled: bool", config AS "config: Json<Config>",
                  events AS "events: Json<Vec<Trigger>>", last_ok AS "last_ok: bool", last_status,
                  last_sent_at AS "last_sent_at: _", created_at AS "created_at: _"
           FROM integrations ORDER BY id"#
    )
    .fetch_all(db)
    .await
}

pub async fn delete(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<bool> {
    let result = sqlx::query!("DELETE FROM integrations WHERE id = ?", id)
        .execute(db)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Remember how the latest send, scan or test went.
pub async fn record(
    db: impl SqliteExecutor<'_>,
    id: i64,
    ok: bool,
    status: &str,
) -> sqlx::Result<()> {
    let now = Utc::now();
    sqlx::query!(
        "UPDATE integrations SET last_ok = ?, last_status = ?, last_sent_at = ? WHERE id = ?",
        ok,
        status,
        now,
        id,
    )
    .execute(db)
    .await?;
    Ok(())
}
