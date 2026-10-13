//! Talking to the outside world: library servers to scan (Komga, Kavita)
//! and notification targets (ntfy, Gotify, email, a templated webhook,
//! Apprise).
//!
//! The [`notifier`] task turns events into `Notify` / `ScanLibrary` jobs,
//! so every send goes through the job queue and gets its retries.

pub mod apprise;
pub mod email;
pub mod gotify;
pub mod kavita;
pub mod komga;
pub mod notifier;
pub mod ntfy;
pub mod webhook;

use reqwest::{Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use url::Url;

pub use self::webhook::{Preset, WebhookMethod, presets, render};

/// Where an integration sends things, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Config {
    Komga(komga::Komga),
    Kavita(kavita::Kavita),
    Ntfy(ntfy::Ntfy),
    Gotify(gotify::Gotify),
    Email(email::Email),
    Webhook(webhook::Webhook),
    Apprise(apprise::Apprise),
}

impl Config {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Komga(_) => "komga",
            Self::Kavita(_) => "kavita",
            Self::Ntfy(_) => "ntfy",
            Self::Gotify(_) => "gotify",
            Self::Email(_) => "email",
            Self::Webhook(_) => "webhook",
            Self::Apprise(_) => "apprise",
        }
    }

    /// Library servers get scans; everything else gets messages.
    #[must_use]
    pub const fn is_library(&self) -> bool {
        matches!(self, Self::Komga(_) | Self::Kavita(_))
    }

    /// A copy fit for the API: every secret that is set reads as [`MASK`].
    #[must_use]
    pub fn masked(&self) -> Self {
        let mut config = self.clone();
        match &mut config {
            Self::Komga(c) => mask(&mut c.api_key),
            Self::Kavita(c) => mask(&mut c.api_key),
            Self::Ntfy(c) => c.mask(),
            Self::Gotify(c) => c.mask(),
            Self::Email(c) => c.mask(),
            Self::Webhook(c) => c.mask(),
            Self::Apprise(_) => {}
        }
        config
    }

    /// Put `stored`'s secrets back where the UI sent a [`MASK`] unchanged.
    pub fn keep_secrets(&mut self, stored: &Self) {
        match (self, stored) {
            (Self::Komga(c), Self::Komga(s)) => keep(&mut c.api_key, &s.api_key),
            (Self::Kavita(c), Self::Kavita(s)) => keep(&mut c.api_key, &s.api_key),
            (Self::Ntfy(c), Self::Ntfy(s)) => c.keep_secrets(s),
            (Self::Gotify(c), Self::Gotify(s)) => c.keep_secrets(s),
            (Self::Email(c), Self::Email(s)) => c.keep_secrets(s),
            (Self::Webhook(c), Self::Webhook(s)) => c.keep_secrets(s),
            _ => {}
        }
    }

    /// Check what a user typed before saving it.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Komga(c) => c.validate(),
            Self::Kavita(c) => c.validate(),
            Self::Ntfy(c) => c.validate(),
            Self::Gotify(c) => c.validate(),
            Self::Email(c) => c.validate(),
            Self::Webhook(c) => c.validate(),
            Self::Apprise(c) => c.validate(),
        }
    }
}

/// What a notification integration can be told about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Trigger {
    /// Chapters landed in the library.
    Downloaded,
    /// Chapters failed for good (no retries left).
    Failed,
    /// New chapters were found on a source.
    Found,
}

/// One notification, already summarised (a batch of events becomes one
/// message). Stored in the `Notify` job's payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Message {
    pub trigger: Trigger,
    pub title: String,
    pub body: String,
    /// How many chapters (or series, for `Found`) this covers.
    pub count: usize,
    /// Series involved, for templates that want them.
    pub series: Vec<String>,
    /// Their ids, for links into the UI. Missing from jobs queued before 0.2.
    #[serde(default)]
    pub series_ids: Vec<i64>,
}

impl Message {
    #[must_use]
    pub fn test() -> Self {
        Self {
            trigger: Trigger::Downloaded,
            title: "kaze test notification".into(),
            body: "If you can read this, notifications work.".into(),
            count: 1,
            series: vec!["Test Series".into()],
            series_ids: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    #[error("request failed: {}", with_causes(.0))]
    Http(#[from] reqwest::Error),
    #[error("{service} answered HTTP {status}: {body}")]
    Status {
        service: &'static str,
        status: u16,
        body: String,
    },
    #[error("SMTP: {}", with_causes(.0))]
    Smtp(#[from] lettre::transport::smtp::Error),
    #[error("template: {0}")]
    Template(#[from] minijinja::Error),
    #[error("{0}")]
    Invalid(String),
}

impl IntegrationError {
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http(e) => e.is_timeout() || e.is_connect() || e.is_request(),
            Self::Status { status, .. } => *status == 429 || *status >= 500,
            // Refused connections and 4xx replies; not bad logins or TLS setups.
            Self::Smtp(e) => !(e.is_permanent() || e.is_tls() || e.is_client()),
            Self::Template(_) | Self::Invalid(_) => false,
        }
    }
}

/// reqwest's message alone ("error sending request") hides why; the
/// causes say "connection refused" or "dns error".
fn with_causes(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

/// Stands in for a stored secret in API responses.
pub const MASK: &str = "********";

fn mask(secret: &mut String) {
    if !secret.is_empty() {
        MASK.clone_into(secret);
    }
}

fn mask_opt(secret: &mut Option<String>) {
    if let Some(secret) = secret {
        mask(secret);
    }
}

fn keep_opt(secret: &mut Option<String>, stored: Option<&String>) {
    if secret.as_deref() == Some(MASK) {
        secret.clone_from(&stored.cloned());
    }
}

fn keep(secret: &mut String, stored: &str) {
    if secret == MASK {
        stored.clone_into(secret);
    }
}

fn filled(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

fn http_url(url: &Url) -> Result<(), String> {
    if matches!(url.scheme(), "http" | "https") {
        Ok(())
    } else {
        Err("the URL must start with http:// or https://".into())
    }
}

/// `base` + `path`, keeping any path prefix of `base` (`http://h/komga` →
/// `http://h/komga/api/...`); `Url::join` would drop its last segment.
fn endpoint(base: &Url, path: &str) -> Result<Url, IntegrationError> {
    let mut base = base.clone();
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    base.join(path)
        .map_err(|e| IntegrationError::Invalid(format!("bad URL: {e}")))
}

async fn checked(
    service: &'static str,
    request: RequestBuilder,
) -> Result<Response, IntegrationError> {
    let response = request.send().await?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let mut body = response.text().await.unwrap_or_default();
    body.truncate(200);
    Err(IntegrationError::Status {
        service,
        status: status.as_u16(),
        body,
    })
}

/// Send a message to a notification integration.
pub async fn send(
    http: &Client,
    config: &Config,
    message: &Message,
) -> Result<(), IntegrationError> {
    match config {
        Config::Ntfy(c) => c.send(http, message).await,
        Config::Gotify(c) => c.send(http, message).await,
        Config::Email(c) => c.send(message).await,
        Config::Webhook(c) => c.send(http, message).await,
        Config::Apprise(c) => c.send(http, message).await,
        Config::Komga(_) | Config::Kavita(_) => Err(IntegrationError::Invalid(
            "library servers get scans, not messages".into(),
        )),
    }
}

/// Ask a library server to scan for new files.
pub async fn scan(http: &Client, config: &Config) -> Result<(), IntegrationError> {
    match config {
        Config::Komga(c) => c.scan(http).await,
        Config::Kavita(c) => c.scan(http).await,
        _ => Err(IntegrationError::Invalid(
            "only library servers can scan".into(),
        )),
    }
}

/// Check that an integration works: log in to a library server (without
/// scanning), or send a test message.
pub async fn test(http: &Client, config: &Config) -> Result<(), IntegrationError> {
    match config {
        Config::Komga(c) => c.test(http).await,
        Config::Kavita(c) => c.test(http).await,
        _ => send(http, config, &Message::test()).await,
    }
}

#[cfg(test)]
mod tests;
