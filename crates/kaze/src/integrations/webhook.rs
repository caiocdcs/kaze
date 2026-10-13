//! Any HTTP endpoint, with a minijinja-templated body and headers.

use std::collections::BTreeMap;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use url::Url;

use super::{
    Config, IntegrationError, Message, Trigger, checked, http_url, keep, keep_url, mask, mask_url,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Webhook {
    pub url: Url,
    #[serde(default)]
    pub method: WebhookMethod,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "UPPERCASE")]
#[ts(export)]
pub enum WebhookMethod {
    #[default]
    Post,
    Put,
}

/// Headers that usually carry credentials; the rest (`Content-Type`) stay readable.
fn is_secret_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["auth", "token", "key", "secret", "password", "cookie"]
        .iter()
        .any(|word| name.contains(word))
}

impl Webhook {
    pub(super) fn mask(&mut self) {
        mask_url(&mut self.url);
        for (name, value) in &mut self.headers {
            if is_secret_header(name) {
                mask(value);
            }
        }
    }

    pub(super) fn keep_secrets(&mut self, stored: &Self) {
        keep_url(&mut self.url, &stored.url);
        for (name, value) in &mut self.headers {
            if let Some(old) = stored.headers.get(name) {
                keep(value, old);
            }
        }
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        http_url(&self.url)?;
        let env = template_env();
        env.template_from_str(&self.body)
            .map_err(|e| format!("body template: {e}"))?;
        for (name, value) in &self.headers {
            env.template_from_str(value)
                .map_err(|e| format!("header {name}: {e}"))?;
        }
        Ok(())
    }

    pub(super) async fn send(
        &self,
        http: &Client,
        message: &Message,
    ) -> Result<(), IntegrationError> {
        let body = render(&self.body, message)?;
        let mut request = match self.method {
            WebhookMethod::Post => http.post(self.url.clone()),
            WebhookMethod::Put => http.put(self.url.clone()),
        };
        let mut has_type = false;
        for (name, value) in &self.headers {
            has_type |= name.eq_ignore_ascii_case("content-type");
            request = request.header(name, render(value, message)?);
        }
        if !has_type {
            request = request.header("content-type", "application/json");
        }
        checked("webhook", request.body(body)).await?;
        Ok(())
    }
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

/// Webhook presets for services without a kind of their own. The UI copies
/// one into a new integration to edit.
#[must_use]
#[allow(clippy::expect_used, reason = "literal URLs, covered by tests")]
pub fn presets() -> Vec<Preset> {
    let url = |s: &str| Url::parse(s).expect("preset URL");
    vec![Preset {
        id: "discord",
        name: "Discord",
        hint: "Paste the channel's webhook URL (Server settings → Integrations → Webhooks).",
        config: Config::Webhook(Webhook {
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
        }),
        events: vec![Trigger::Downloaded, Trigger::Failed],
    }]
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

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use serde_json::{Value, json};

    use super::*;
    use crate::integrations::tests::{message, ok, server};

    fn hook(url: Url, body: &str) -> Webhook {
        Webhook {
            url,
            method: WebhookMethod::Post,
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }

    #[test]
    fn every_preset_renders_valid_json() {
        for preset in presets() {
            let Config::Webhook(webhook) = &preset.config else {
                panic!("{} is not a webhook", preset.id);
            };
            let rendered = render(&webhook.body, &message()).unwrap();
            let parsed: Value = serde_json::from_str(&rendered)
                .unwrap_or_else(|e| panic!("{}: {e}\n{rendered}", preset.id));
            assert!(
                rendered.contains(r#"Berserk \"374\""#),
                "{}: title escaped",
                preset.id
            );
            assert!(parsed.is_object());
            preset.config.validate().unwrap();
        }
    }

    #[test]
    fn validation_catches_bad_templates_and_urls() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert!(hook(url("https://x.test"), "{{ title").validate().is_err());
        assert!(hook(url("ftp://x.test"), "{}").validate().is_err());
        assert!(
            hook(url("https://x.test"), "{{ title | tojson }}")
                .validate()
                .is_ok()
        );
        // Unknown variables fail at render time, not silently as "".
        assert!(render("{{ nope }}", &message()).is_err());
    }

    #[tokio::test]
    async fn posts_the_rendered_template() {
        let (url, seen) = server(ok).await;
        let webhook = Webhook {
            url: url.join("hook").unwrap(),
            method: WebhookMethod::Put,
            headers: BTreeMap::from([("X-Title".into(), "{{ title }}".into())]),
            body: r#"{"text": {{ message | tojson }}, "event": "{{ event }}"}"#.into(),
        };
        webhook.send(&Client::new(), &message()).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].method, Method::PUT);
        assert_eq!(seen[0].path_and_query, "/hook");
        assert_eq!(seen[0].headers["x-title"], "Berserk \"374\"");
        assert_eq!(seen[0].headers["content-type"], "application/json");
        let body: Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(
            body,
            json!({ "text": "line one\nline two", "event": "failed" })
        );
    }

    #[tokio::test]
    async fn server_errors_retry_client_errors_do_not() {
        let (url, _) = server(|path| match path {
            "/busy" => (StatusCode::SERVICE_UNAVAILABLE, json!({})),
            _ => (StatusCode::UNAUTHORIZED, json!({ "error": "bad key" })),
        })
        .await;
        let client = Client::new();
        let busy = hook(url.join("busy").unwrap(), "{}")
            .send(&client, &message())
            .await
            .unwrap_err();
        assert!(busy.is_retryable());
        let denied = hook(url.join("denied").unwrap(), "{}")
            .send(&client, &message())
            .await
            .unwrap_err();
        assert!(!denied.is_retryable());
        assert!(denied.to_string().contains("401"), "{denied}");
    }
}
