//! An apprise-api server, for the services kaze doesn't speak natively.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;
use url::Url;

use super::{IntegrationError, Message, Trigger, checked, endpoint, filled, http_url};

/// `urls` (such as `ntfy://topic`) are sent with each message; without
/// them, `key` names a configuration saved on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Apprise {
    pub url: Url,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub key: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub urls: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub tags: Option<String>,
}

impl Apprise {
    pub(super) fn validate(&self) -> Result<(), String> {
        http_url(&self.url)?;
        if filled(self.key.as_deref()).is_none() && filled(self.urls.as_deref()).is_none() {
            return Err("give Apprise a configuration key or notification URLs".into());
        }
        Ok(())
    }

    pub(super) async fn send(
        &self,
        http: &Client,
        message: &Message,
    ) -> Result<(), IntegrationError> {
        let kind = match message.trigger {
            Trigger::Downloaded => "success",
            Trigger::Failed => "failure",
            Trigger::Found => "info",
        };
        let mut body = json!({ "title": message.title, "body": message.body, "type": kind });
        let url = if let Some(urls) = filled(self.urls.as_deref()) {
            body["urls"] = Value::from(urls);
            endpoint(&self.url, "notify/")?
        } else {
            let key = filled(self.key.as_deref())
                .ok_or_else(|| IntegrationError::Invalid("Apprise needs a key or URLs".into()))?;
            if let Some(tags) = filled(self.tags.as_deref()) {
                body["tag"] = Value::from(tags);
            }
            endpoint(&self.url, &format!("notify/{key}"))?
        };
        checked("Apprise", http.post(url).json(&body)).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::tests::{message, ok, server};

    #[tokio::test]
    async fn posts_to_the_notify_key() {
        let (url, seen) = server(ok).await;
        let apprise = Apprise {
            url,
            key: Some("kaze".into()),
            urls: None,
            tags: Some("phone".into()),
        };
        apprise.send(&Client::new(), &message()).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].path_and_query, "/notify/kaze");
        let body: Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(body["type"], "failure");
        assert_eq!(body["tag"], "phone");
    }

    #[tokio::test]
    async fn sends_urls_statelessly() {
        let (url, seen) = server(ok).await;
        let apprise = Apprise {
            url,
            key: None,
            urls: Some("ntfy://ntfy.sh/kaze-test".into()),
            tags: None,
        };
        apprise.send(&Client::new(), &message()).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].path_and_query, "/notify/");
        let body: Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(body["urls"], "ntfy://ntfy.sh/kaze-test");
        assert_eq!(body["title"], message().title);
    }

    #[test]
    fn needs_a_key_or_urls() {
        let apprise = Apprise {
            url: Url::parse("http://apprise:8000").unwrap(),
            key: Some("  ".into()),
            urls: None,
            tags: None,
        };
        assert!(apprise.validate().is_err());
    }
}
