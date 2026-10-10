//! Talking to the outside world: library servers to scan (Komga, Kavita)
//! and notification targets (a templated webhook, Apprise).
//!
//! The [`notifier`] task turns events into `Notify` / `ScanLibrary` jobs,
//! so every send goes through the job queue and gets its retries.

pub mod notifier;

use std::collections::BTreeMap;

use reqwest::{Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;
use url::Url;

/// Where an integration sends things, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Config {
    /// Komga: scan a library (or all of them) after downloads.
    Komga {
        url: Url,
        api_key: String,
        #[serde(default)]
        #[ts(optional = nullable)]
        library_id: Option<String>,
    },
    /// Kavita: scan a library (or all of them) after downloads.
    Kavita {
        url: Url,
        api_key: String,
        #[serde(default)]
        #[ts(optional = nullable)]
        library_id: Option<i64>,
    },
    /// Any HTTP endpoint, with a minijinja-templated body and headers.
    Webhook {
        url: Url,
        #[serde(default)]
        method: WebhookMethod,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        body: String,
    },
    /// An apprise-api server, reaching 100+ services. Either a `key` for
    /// a configuration saved on the server, or Apprise `urls` (such as
    /// `ntfy://topic`) sent along with each message.
    Apprise {
        url: Url,
        #[serde(default)]
        #[ts(optional = nullable)]
        key: Option<String>,
        #[serde(default)]
        #[ts(optional = nullable)]
        urls: Option<String>,
        #[serde(default)]
        #[ts(optional = nullable)]
        tags: Option<String>,
    },
}

impl Config {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Komga { .. } => "komga",
            Self::Kavita { .. } => "kavita",
            Self::Webhook { .. } => "webhook",
            Self::Apprise { .. } => "apprise",
        }
    }

    /// Library servers get scans; everything else gets messages.
    #[must_use]
    pub const fn is_library(&self) -> bool {
        matches!(self, Self::Komga { .. } | Self::Kavita { .. })
    }

    /// Check what a user typed before saving it.
    pub fn validate(&self) -> Result<(), String> {
        let url = match self {
            Self::Komga { url, .. }
            | Self::Kavita { url, .. }
            | Self::Webhook { url, .. }
            | Self::Apprise { url, .. } => url,
        };
        if !matches!(url.scheme(), "http" | "https") {
            return Err("the URL must start with http:// or https://".into());
        }
        if let Self::Webhook { body, headers, .. } = self {
            let env = template_env();
            env.template_from_str(body)
                .map_err(|e| format!("body template: {e}"))?;
            for (name, value) in headers {
                env.template_from_str(value)
                    .map_err(|e| format!("header {name}: {e}"))?;
            }
        }
        if let Self::Apprise { key, urls, .. } = self
            && filled(key.as_deref()).is_none()
            && filled(urls.as_deref()).is_none()
        {
            return Err("give Apprise a configuration key or notification URLs".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "UPPERCASE")]
#[ts(export)]
pub enum WebhookMethod {
    #[default]
    Post,
    Put,
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

fn filled(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// A ready-made webhook setup for a popular service.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    /// What the user still has to fill in.
    pub hint: &'static str,
    pub config: Config,
    pub events: Vec<Trigger>,
}

/// Webhook presets. The UI copies one into a new integration to edit.
#[must_use]
pub fn presets() -> Vec<Preset> {
    let url = |s: &str| Url::parse(s).expect("preset URL");
    let all = vec![Trigger::Downloaded, Trigger::Failed];
    vec![
        Preset {
            id: "ntfy",
            name: "ntfy",
            hint: "Change the topic in the body; for a private server, change the URL.",
            config: Config::Webhook {
                url: url("https://ntfy.sh"),
                method: WebhookMethod::Post,
                headers: BTreeMap::new(),
                body: r#"{
  "topic": "[YOUR TOPIC]",
  "title": {{ title | tojson }},
  "message": {{ message | tojson }},
  "tags": [{% if event == "failed" %}"warning"{% else %}"books"{% endif %}]
}"#
                .into(),
            },
            events: all.clone(),
        },
        Preset {
            id: "discord",
            name: "Discord",
            hint: "Paste the channel's webhook URL (Server settings → Integrations → Webhooks).",
            config: Config::Webhook {
                url: url("https://discord.com/api/webhooks/[ID]/[TOKEN]"),
                method: WebhookMethod::Post,
                headers: BTreeMap::new(),
                body: r#"{
  "username": "kaze",
  "embeds": [{
    "title": {{ title | tojson }},
    "description": {{ message | tojson }},
    "color": {% if event == "failed" %}16747660{% else %}16739133{% endif %}
  }]
}"#
                .into(),
            },
            events: all.clone(),
        },
        Preset {
            id: "gotify",
            name: "Gotify",
            hint: "Use your server's URL and an application token.",
            config: Config::Webhook {
                url: url("https://gotify.example.com/message?token=[APP TOKEN]"),
                method: WebhookMethod::Post,
                headers: BTreeMap::new(),
                body: r#"{
  "title": {{ title | tojson }},
  "message": {{ message | tojson }},
  "priority": {% if event == "failed" %}8{% else %}5{% endif %}
}"#
                .into(),
            },
            events: all,
        },
    ]
}

fn template_env() -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();
    // Bodies are JSON, not HTML: values go in through `tojson` instead.
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env
}

/// Render a webhook template with a message's fields.
pub fn render(template: &str, message: &Message) -> Result<String, minijinja::Error> {
    let env = template_env();
    let ctx = minijinja::context! {
        title => &message.title,
        message => &message.body,
        event => message.trigger,
        count => message.count,
        series => &message.series,
    };
    env.render_str(template, ctx)
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
        Config::Webhook {
            url,
            method,
            headers,
            body,
        } => {
            let body = render(body, message)?;
            let mut request = match method {
                WebhookMethod::Post => http.post(url.clone()),
                WebhookMethod::Put => http.put(url.clone()),
            };
            let mut has_type = false;
            for (name, value) in headers {
                has_type |= name.eq_ignore_ascii_case("content-type");
                request = request.header(name, render(value, message)?);
            }
            if !has_type {
                request = request.header("content-type", "application/json");
            }
            checked("webhook", request.body(body)).await?;
        }
        Config::Apprise {
            url,
            key,
            urls,
            tags,
        } => {
            let kind = match message.trigger {
                Trigger::Downloaded => "success",
                Trigger::Failed => "failure",
                Trigger::Found => "info",
            };
            let mut body = json!({ "title": message.title, "body": message.body, "type": kind });
            let url = if let Some(urls) = filled(urls.as_deref()) {
                // Stateless: apprise-api needs no saved configuration.
                body["urls"] = Value::from(urls);
                endpoint(url, "notify/")?
            } else {
                let key = filled(key.as_deref()).ok_or_else(|| {
                    IntegrationError::Invalid("Apprise needs a key or URLs".into())
                })?;
                if let Some(tags) = filled(tags.as_deref()) {
                    body["tag"] = Value::from(tags);
                }
                endpoint(url, &format!("notify/{key}"))?
            };
            checked("Apprise", http.post(url).json(&body)).await?;
        }
        Config::Komga { .. } | Config::Kavita { .. } => {
            return Err(IntegrationError::Invalid(
                "library servers get scans, not messages".into(),
            ));
        }
    }
    Ok(())
}

/// Ask a library server to scan for new files.
pub async fn scan(http: &Client, config: &Config) -> Result<(), IntegrationError> {
    match config {
        Config::Komga {
            url,
            api_key,
            library_id,
        } => {
            let ids = if let Some(id) = library_id.as_deref().filter(|id| !id.is_empty()) {
                vec![id.to_owned()]
            } else {
                #[derive(Deserialize)]
                struct Library {
                    id: String,
                }
                let request = http
                    .get(endpoint(url, "api/v1/libraries")?)
                    .header("X-API-Key", api_key);
                let libraries: Vec<Library> = checked("Komga", request).await?.json().await?;
                libraries.into_iter().map(|l| l.id).collect()
            };
            for id in ids {
                let request = http
                    .post(endpoint(url, &format!("api/v1/libraries/{id}/scan"))?)
                    .header("X-API-Key", api_key);
                checked("Komga", request).await?;
            }
        }
        Config::Kavita {
            url,
            api_key,
            library_id,
        } => {
            let token = kavita_token(http, url, api_key).await?;
            let request = match library_id {
                Some(id) => http
                    .post(endpoint(url, "api/Library/scan")?)
                    .query(&[("libraryId", id.to_string())]),
                None => http.post(endpoint(url, "api/Library/scan-all")?),
            };
            checked("Kavita", request.bearer_auth(token)).await?;
        }
        Config::Webhook { .. } | Config::Apprise { .. } => {
            return Err(IntegrationError::Invalid(
                "only library servers can scan".into(),
            ));
        }
    }
    Ok(())
}

/// Kavita trades the user's API key for a short-lived JWT.
async fn kavita_token(http: &Client, url: &Url, api_key: &str) -> Result<String, IntegrationError> {
    #[derive(Deserialize)]
    struct Auth {
        token: String,
    }
    let request = http
        .post(endpoint(url, "api/Plugin/authenticate")?)
        .query(&[("apiKey", api_key), ("pluginName", "kaze")]);
    let auth: Auth = checked("Kavita", request).await?.json().await?;
    Ok(auth.token)
}

/// Check that an integration works: log in to a library server (without
/// scanning), or send a test message.
pub async fn test(http: &Client, config: &Config) -> Result<(), IntegrationError> {
    match config {
        Config::Komga { url, api_key, .. } => {
            let request = http
                .get(endpoint(url, "api/v2/users/me")?)
                .header("X-API-Key", api_key);
            checked("Komga", request).await?;
        }
        Config::Kavita { url, api_key, .. } => {
            kavita_token(http, url, api_key).await?;
        }
        Config::Webhook { .. } | Config::Apprise { .. } => {
            send(http, config, &Message::test()).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
