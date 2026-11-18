//! Settings that can change while kaze runs.
//!
//! Three layers, later ones winning:
//! 1. defaults ([`Settings::default`]),
//! 2. values saved from the UI (the `settings` table),
//! 3. values under `[settings]` in `kaze.toml` / `KAZE_SETTINGS__*`.
//!
//! Layer 3 is *locked*: the UI shows it but cannot change it, so a
//! NixOS module or compose file stays the source of truth for whatever
//! it sets. The layers are merged as JSON objects and deserialized once
//! into [`Settings`], so "is this valid?" is the same question as "does
//! it deserialize?" plus a range check.
//!
//! The effective settings are published on a `tokio::sync::watch`
//! channel: the scheduler and the dispatcher hold receivers and react
//! to changes without a restart.

use std::{fmt::Write as _, sync::Arc};
use ts_rs::TS;
use utoipa::ToSchema;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, watch};

use crate::{auth::oidc::OidcConfig, db, download::naming::Template, integrations::MASK};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(TS, ToSchema)]
#[ts(export)]
pub struct Settings {
    /// Chapter file name template.
    #[ts(type = "string")]
    #[schema(value_type = String)]
    pub naming_template: Template,
    /// When set, API requests must send it as `X-Api-Key` (or `?apikey=`).
    pub api_key: Option<String>,
    /// Source jobs (downloads, refreshes) running at once.
    pub concurrency: usize,
    /// Jobs running at once against one source.
    pub per_source: usize,
    /// Pages of one chapter fetched at once.
    pub page_concurrency: usize,
    /// How often each source's latest updates are checked.
    pub sync_latest_minutes: u64,
    /// How often a series' full chapter list is re-read (series can override).
    pub refresh_hours: u64,
    /// A FlareSolverr instance for sites behind Cloudflare challenges.
    pub flaresolverr_url: Option<url::Url>,
    /// Sources whose jobs wait instead of running, while a site is broken.
    pub paused_sources: Vec<String>,
    /// What new series are followed in, when the source offers it.
    pub default_language: String,
    /// Days between scheduled database backups; `0` turns them off.
    pub backup_days: u64,
    /// Scheduled and manual backups kept; older ones are deleted.
    pub backup_keep: usize,
    /// Ask GitHub once a day whether a newer kaze is out.
    pub check_updates: bool,
    /// Signing in with an OpenID Connect provider, besides the password.
    pub oidc: Option<OidcConfig>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            naming_template: Template::default(),
            api_key: None,
            concurrency: 4,
            per_source: 2,
            page_concurrency: 4,
            sync_latest_minutes: 15,
            refresh_hours: 24,
            flaresolverr_url: None,
            paused_sources: Vec::new(),
            default_language: "en".into(),
            backup_days: 7,
            backup_keep: 4,
            check_updates: true,
            oidc: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("{0} is set in kaze.toml or the environment and cannot be changed here")]
    Locked(String),
    #[error("invalid settings: {0}")]
    Invalid(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("generating an API key: {0}")]
    Random(#[from] getrandom::Error),
}

impl Settings {
    #[must_use]
    pub fn is_paused(&self, source: &str) -> bool {
        self.paused_sources.iter().any(|s| s == source)
    }

    fn check(&self) -> Result<(), SettingsError> {
        let range = |name: &str, value: u64, min: u64, max: u64| {
            if (min..=max).contains(&value) {
                Ok(())
            } else {
                Err(SettingsError::Invalid(format!(
                    "{name} must be between {min} and {max}"
                )))
            }
        };
        range("concurrency", self.concurrency as u64, 1, 32)?;
        range("per_source", self.per_source as u64, 1, 16)?;
        range("page_concurrency", self.page_concurrency as u64, 1, 16)?;
        range(
            "sync_latest_minutes",
            self.sync_latest_minutes,
            1,
            7 * 24 * 60,
        )?;
        range("refresh_hours", self.refresh_hours, 1, 90 * 24)?;
        range("backup_days", self.backup_days, 0, 365)?;
        range("backup_keep", self.backup_keep as u64, 1, 100)?;
        if self
            .flaresolverr_url
            .as_ref()
            .is_some_and(|u| !matches!(u.scheme(), "http" | "https"))
        {
            return Err(SettingsError::Invalid(
                "flaresolverr_url must start with http:// or https://".into(),
            ));
        }
        if kaze_core::language::normalize(&self.default_language).as_ref()
            != Some(&self.default_language)
        {
            return Err(SettingsError::Invalid(
                "default_language must be a language tag like en or pt-BR".into(),
            ));
        }
        if let Some(oidc) = &self.oidc {
            oidc.check().map_err(SettingsError::Invalid)?;
        }
        if self.api_key.as_ref().is_some_and(|k| k.trim().len() < 8) {
            return Err(SettingsError::Invalid(
                "api_key must be at least 8 characters".into(),
            ));
        }
        Ok(())
    }

    /// Defaults with each layer of overrides applied in order.
    fn from_layers(layers: &[&Map<String, Value>]) -> Result<Self, SettingsError> {
        let Ok(Value::Object(mut merged)) = serde_json::to_value(Self::default()) else {
            return Err(SettingsError::Invalid("defaults are not an object".into()));
        };
        for layer in layers {
            for (key, value) in *layer {
                merged.insert(key.clone(), value.clone());
            }
        }
        let mut settings: Self = serde_json::from_value(Value::Object(merged))
            .map_err(|e| SettingsError::Invalid(e.to_string()))?;
        // An empty key is saved on purpose: no authentication, and no new
        // key generated on the next start.
        if settings
            .api_key
            .as_deref()
            .is_some_and(|k| k.trim().is_empty())
        {
            settings.api_key = None;
        }
        settings.check()?;
        Ok(settings)
    }
}

/// Owns the stored settings and broadcasts the effective ones.
#[derive(Debug)]
pub struct SettingsStore {
    db: SqlitePool,
    locked: Map<String, Value>,
    /// What the UI saved; the mutex serializes read-modify-write updates.
    saved: Mutex<Map<String, Value>>,
    current: watch::Sender<Arc<Settings>>,
}

/// Settings as the API shows them.
#[derive(Debug, Serialize, TS, ToSchema)]
#[ts(export)]
pub struct SettingsView {
    pub settings: Settings,
    /// Keys set in kaze.toml / the environment.
    pub locked: Vec<String>,
}

impl SettingsStore {
    /// Load saved settings and apply the locked ones from the config file.
    pub async fn load(db: SqlitePool, locked: Map<String, Value>) -> Result<Self, SettingsError> {
        let saved = db::settings::all(&db).await?;
        let settings = Settings::from_layers(&[&saved, &locked]).or_else(|err| {
            // A bad value saved earlier must not stop kaze from starting.
            tracing::warn!("ignoring saved settings: {err}");
            Settings::from_layers(&[&locked])
        })?;
        Ok(Self {
            db,
            locked,
            saved: Mutex::new(saved),
            current: watch::Sender::new(Arc::new(settings)),
        })
    }

    #[must_use]
    pub fn current(&self) -> Arc<Settings> {
        self.current.borrow().clone()
    }

    /// A receiver that sees every future change.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<Arc<Settings>> {
        self.current.subscribe()
    }

    /// The OIDC client secret reads as [`MASK`], like integration secrets.
    #[must_use]
    pub fn view(&self) -> SettingsView {
        let mut settings = (*self.current()).clone();
        if let Some(oidc) = &mut settings.oidc {
            MASK.clone_into(&mut oidc.client_secret);
        }
        SettingsView {
            settings,
            locked: self.locked.keys().cloned().collect(),
        }
    }

    /// Apply changes from the UI. `null` resets a key to its default.
    pub async fn update(
        &self,
        mut changes: Map<String, Value>,
    ) -> Result<Arc<Settings>, SettingsError> {
        if let Some(key) = changes.keys().find(|k| self.locked.contains_key(*k)) {
            return Err(SettingsError::Locked(key.clone()));
        }
        // The form sends the masked secret back unchanged.
        if let Some(Value::Object(oidc)) = changes.get_mut("oidc")
            && oidc.get("client_secret").and_then(Value::as_str) == Some(MASK)
            && let Some(stored) = &self.current().oidc
        {
            oidc.insert(
                "client_secret".into(),
                Value::String(stored.client_secret.clone()),
            );
        }
        let mut saved = self.saved.lock().await;
        let mut next = saved.clone();
        for (key, value) in &changes {
            if value.is_null() {
                next.remove(key);
            } else {
                next.insert(key.clone(), value.clone());
            }
        }
        // Validate before writing anything.
        let settings = Arc::new(Settings::from_layers(&[&next, &self.locked])?);
        db::settings::save(&self.db, &changes).await?;
        *saved = next;
        self.current.send_replace(Arc::clone(&settings));
        Ok(settings)
    }

    /// On the first start, generate an API key, as Sonarr and Radarr do;
    /// until it is claimed, the first browser may create the account.
    /// Returns whether one was made. Any `api_key`, saved or locked, even
    /// an empty one, keeps it from running.
    pub async fn ensure_api_key(&self) -> Result<bool, SettingsError> {
        if self.locked.contains_key("api_key") || self.saved.lock().await.contains_key("api_key") {
            return Ok(false);
        }
        let mut bytes = [0u8; 18];
        getrandom::fill(&mut bytes)?;
        let key = bytes.iter().fold(String::from("kz_"), |mut key, b| {
            let _ = write!(key, "{b:02x}");
            key
        });
        // Marked first: a crash before the key is saved just makes a new one.
        db::settings::mark_key_unclaimed(&self.db).await?;
        let mut changes = Map::new();
        changes.insert("api_key".into(), Value::String(key));
        self.update(changes).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn map(value: Value) -> Map<String, Value> {
        let Value::Object(map) = value else {
            panic!("not an object")
        };
        map
    }

    #[tokio::test]
    async fn layers_saved_and_locked_values() {
        let db = db::memory().await;
        let store = SettingsStore::load(db.clone(), map(json!({ "per_source": 1 })))
            .await
            .unwrap();
        assert_eq!(store.current().per_source, 1);
        assert_eq!(store.current().concurrency, 4);

        let mut rx = store.subscribe();
        store
            .update(map(json!({ "concurrency": 8 })))
            .await
            .unwrap();
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update().concurrency, 8);

        // Saved values survive a restart; locked ones still win.
        let reloaded = SettingsStore::load(db, map(json!({ "per_source": 1 })))
            .await
            .unwrap();
        assert_eq!(reloaded.current().concurrency, 8);
        assert_eq!(reloaded.view().locked, ["per_source"]);
    }

    #[tokio::test]
    async fn rejects_locked_and_invalid_changes() {
        let store = SettingsStore::load(db::memory().await, map(json!({ "per_source": 1 })))
            .await
            .unwrap();
        assert!(matches!(
            store.update(map(json!({ "per_source": 3 }))).await,
            Err(SettingsError::Locked(_))
        ));
        assert!(matches!(
            store.update(map(json!({ "concurrency": 0 }))).await,
            Err(SettingsError::Invalid(_))
        ));
        assert!(matches!(
            store.update(map(json!({ "naming_template": "%M" }))).await,
            Err(SettingsError::Invalid(_))
        ));
        assert!(matches!(
            store.update(map(json!({ "no_such_setting": 1 }))).await,
            Err(SettingsError::Invalid(_))
        ));
        assert_eq!(
            *store.current(),
            Settings {
                per_source: 1,
                ..Settings::default()
            }
        );
    }

    #[tokio::test]
    async fn first_start_generates_a_key_to_claim_once() {
        let db = db::memory().await;
        let store = SettingsStore::load(db.clone(), Map::new()).await.unwrap();
        assert!(store.ensure_api_key().await.unwrap());
        let key = store.current().api_key.clone().unwrap();
        assert!(key.starts_with("kz_") && key.len() == 39, "{key}");

        assert!(db::settings::claim_key(&db).await.unwrap());
        assert!(!db::settings::claim_key(&db).await.unwrap(), "claimed once");

        // The saved key survives a restart and is not replaced.
        let reloaded = SettingsStore::load(db, Map::new()).await.unwrap();
        assert!(!reloaded.ensure_api_key().await.unwrap());
        assert_eq!(reloaded.current().api_key, Some(key));
    }

    fn oidc(secret: &str) -> Value {
        json!({ "oidc": {
            "issuer": "https://auth.example.com",
            "client_id": "kaze",
            "client_secret": secret,
            "public_url": "https://kaze.example.com",
        }})
    }

    #[tokio::test]
    async fn the_oidc_secret_stays_on_the_server() {
        let store = SettingsStore::load(db::memory().await, Map::new())
            .await
            .unwrap();
        store.update(map(oidc("s3cret"))).await.unwrap();
        let shown = store.view().settings.oidc.unwrap();
        assert_eq!(shown.client_secret, MASK);

        // The form sends the mask back with another change: the secret stays.
        let mut changes = oidc(MASK);
        changes["oidc"]["name"] = json!("Authelia");
        store.update(map(changes)).await.unwrap();
        let saved = store.current().oidc.clone().unwrap();
        assert_eq!(saved.client_secret, "s3cret");
        assert_eq!(saved.name.as_deref(), Some("Authelia"));

        let mut bad = oidc("s3cret");
        bad["oidc"]["issuer"] = json!("not a url");
        assert!(matches!(
            store.update(map(bad)).await,
            Err(SettingsError::Invalid(_))
        ));
        store.update(map(json!({ "oidc": null }))).await.unwrap();
        assert!(store.current().oidc.is_none(), "null turns it off");

        let locked = SettingsStore::load(db::memory().await, map(oidc("s3cret")))
            .await
            .unwrap();
        assert!(matches!(
            locked.update(map(oidc("other"))).await,
            Err(SettingsError::Locked(_))
        ));
    }

    #[tokio::test]
    async fn an_empty_key_opts_out_of_generation() {
        let locked = SettingsStore::load(db::memory().await, map(json!({ "api_key": "" })))
            .await
            .unwrap();
        assert!(!locked.ensure_api_key().await.unwrap());
        assert_eq!(locked.current().api_key, None);

        // Removing the key in the UI saves it empty, so it stays removed.
        let db = db::memory().await;
        let store = SettingsStore::load(db.clone(), Map::new()).await.unwrap();
        store.ensure_api_key().await.unwrap();
        store.update(map(json!({ "api_key": "" }))).await.unwrap();
        assert_eq!(store.current().api_key, None);
        let reloaded = SettingsStore::load(db, Map::new()).await.unwrap();
        assert!(!reloaded.ensure_api_key().await.unwrap());
        assert_eq!(reloaded.current().api_key, None);
    }

    #[tokio::test]
    async fn null_resets_to_default() {
        let store = SettingsStore::load(db::memory().await, Map::new())
            .await
            .unwrap();
        store
            .update(map(json!({ "refresh_hours": 6 })))
            .await
            .unwrap();
        store
            .update(map(json!({ "refresh_hours": null })))
            .await
            .unwrap();
        assert_eq!(store.current().refresh_hours, 24);
    }

    #[tokio::test]
    async fn bad_locked_value_is_an_error() {
        let result =
            SettingsStore::load(db::memory().await, map(json!({ "concurrency": "lots" }))).await;
        assert!(matches!(result, Err(SettingsError::Invalid(_))));
    }
}
