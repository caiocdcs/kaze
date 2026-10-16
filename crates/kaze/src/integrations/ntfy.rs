//! ntfy, on ntfy.sh or your own server, through its JSON publish API.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;
use url::Url;

use super::{
    IntegrationError, Message, Trigger, checked, endpoint, filled, http_url, keep_opt, mask_opt,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Ntfy {
    /// The server, such as `https://ntfy.sh`.
    pub url: Url,
    pub topic: String,
    /// An access token; or `username` and `password`.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub token: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub username: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub password: Option<String>,
    /// Where kaze's UI is reached, so tapping a notification opens it.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub click_url: Option<Url>,
}

impl Ntfy {
    pub(super) fn validate(&self) -> Result<(), String> {
        http_url(&self.url)?;
        if let Some(click) = &self.click_url {
            http_url(click).map_err(|e| format!("kaze address: {e}"))?;
        }
        if self.topic.trim().is_empty() {
            return Err("give ntfy a topic".into());
        }
        if filled(self.token.as_deref()).is_some() && filled(self.username.as_deref()).is_some() {
            return Err("use either a token or a username and password, not both".into());
        }
        Ok(())
    }

    pub(super) fn mask(&mut self) {
        mask_opt(&mut self.token);
        mask_opt(&mut self.password);
    }

    pub(super) fn keep_secrets(&mut self, stored: &Self) {
        keep_opt(&mut self.token, stored.token.as_ref());
        keep_opt(&mut self.password, stored.password.as_ref());
    }

    pub(super) async fn send(
        &self,
        http: &Client,
        message: &Message,
    ) -> Result<(), IntegrationError> {
        let (priority, tag) = match message.trigger {
            Trigger::Failed | Trigger::SourceBroken => (4, "warning"),
            Trigger::Downloaded => (3, "books"),
            Trigger::Found => (3, "new"),
        };
        let mut body = json!({
            "topic": self.topic.trim(),
            "title": message.title,
            "message": message.body,
            "priority": priority,
            "tags": [tag],
        });
        if let Some(click) = &self.click_url {
            // One series opens its page; a mix of series opens the activity log.
            let path = match message.series_ids.as_slice() {
                [id] => format!("series/{id}"),
                _ => "activity".into(),
            };
            body["click"] = Value::from(endpoint(click, &path)?.as_str());
        }
        let mut request = http.post(self.url.clone()).json(&body);
        if let Some(token) = filled(self.token.as_deref()) {
            request = request.bearer_auth(token);
        } else if let Some(username) = filled(self.username.as_deref()) {
            request = request.basic_auth(username, self.password.as_deref());
        }
        checked("ntfy", request).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::tests::{message, ok, server};

    fn ntfy(url: Url) -> Ntfy {
        Ntfy {
            url,
            topic: "manga".into(),
            token: None,
            username: None,
            password: None,
            click_url: None,
        }
    }

    #[tokio::test]
    async fn failures_go_out_urgent_with_a_link_to_the_series() {
        let (url, seen) = server(ok).await;
        let ntfy = Ntfy {
            token: Some("tk_abc".into()),
            click_url: Some(Url::parse("https://kaze.lan/ui").unwrap()),
            ..ntfy(url)
        };
        ntfy.send(&Client::new(), &message()).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].path_and_query, "/");
        assert_eq!(seen[0].headers["authorization"], "Bearer tk_abc");
        let body: Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(
            body,
            json!({
                "topic": "manga",
                "title": "Berserk \"374\"",
                "message": "line one\nline two",
                "priority": 4,
                "tags": ["warning"],
                "click": "https://kaze.lan/ui/series/7",
            })
        );
    }

    #[tokio::test]
    async fn several_series_link_to_the_activity_log() {
        let (url, seen) = server(ok).await;
        let ntfy = Ntfy {
            username: Some("me".into()),
            password: Some("pw".into()),
            click_url: Some(Url::parse("https://kaze.lan").unwrap()),
            ..ntfy(url)
        };
        let message = Message {
            trigger: Trigger::Downloaded,
            series_ids: vec![1, 2],
            ..message()
        };
        ntfy.send(&Client::new(), &message).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].headers["authorization"], "Basic bWU6cHc=");
        let body: Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(body["priority"], 3);
        assert_eq!(body["tags"], json!(["books"]));
        assert_eq!(body["click"], "https://kaze.lan/activity");
    }

    #[test]
    fn validation() {
        let url = Url::parse("https://ntfy.sh").unwrap();
        assert!(ntfy(url.clone()).validate().is_ok());
        let no_topic = Ntfy {
            topic: " ".into(),
            ..ntfy(url.clone())
        };
        assert!(no_topic.validate().is_err());
        let both = Ntfy {
            token: Some("t".into()),
            username: Some("u".into()),
            ..ntfy(url)
        };
        assert!(both.validate().is_err());
    }

    #[test]
    fn token_and_password_are_secrets() {
        let stored = Ntfy {
            token: Some("t".into()),
            password: Some("p".into()),
            username: Some("u".into()),
            ..ntfy(Url::parse("https://ntfy.sh").unwrap())
        };
        let mut masked = stored.clone();
        masked.mask();
        assert_eq!(masked.token.as_deref(), Some(super::super::MASK));
        assert_eq!(masked.username.as_deref(), Some("u"));
        masked.keep_secrets(&stored);
        assert_eq!(masked, stored);
    }
}
