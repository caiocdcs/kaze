//! Gotify: a self-hosted push server, posted to with an application token.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;
use url::Url;

use super::{IntegrationError, Message, Trigger, checked, endpoint, http_url, keep, mask};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Gotify {
    pub url: Url,
    /// An application token (Apps → Create application).
    pub token: String,
}

impl Gotify {
    pub(super) fn validate(&self) -> Result<(), String> {
        http_url(&self.url)?;
        if self.token.trim().is_empty() {
            return Err("give Gotify an application token".into());
        }
        Ok(())
    }

    pub(super) fn mask(&mut self) {
        mask(&mut self.token);
    }

    pub(super) fn keep_secrets(&mut self, stored: &Self) {
        keep(&mut self.token, &stored.token);
    }

    pub(super) async fn send(
        &self,
        http: &Client,
        message: &Message,
    ) -> Result<(), IntegrationError> {
        let priority = match message.trigger {
            Trigger::Failed | Trigger::SourceBroken => 8,
            Trigger::Downloaded | Trigger::Found => 5,
        };
        let body = json!({
            "title": message.title,
            // Markdown drops single line breaks; two trailing spaces keep them.
            "message": message.body.replace('\n', "  \n"),
            "priority": priority,
            "extras": { "client::display": { "contentType": "text/markdown" } },
        });
        let request = http
            .post(endpoint(&self.url, "message")?)
            .header("X-Gotify-Key", self.token.trim())
            .json(&body);
        checked("Gotify", request).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::integrations::tests::{message, ok, server};

    #[tokio::test]
    async fn posts_markdown_with_the_app_token() {
        let (url, seen) = server(ok).await;
        let gotify = Gotify {
            url: url.join("gotify").unwrap(),
            token: "AbC".into(),
        };
        gotify.send(&Client::new(), &message()).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].path_and_query, "/gotify/message");
        assert_eq!(seen[0].headers["x-gotify-key"], "AbC");
        let body: Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(body["message"], "line one  \nline two");
        assert_eq!(body["priority"], 8);
        assert_eq!(
            body["extras"]["client::display"]["contentType"],
            "text/markdown"
        );
    }

    #[test]
    fn needs_a_token() {
        let gotify = Gotify {
            url: Url::parse("https://gotify.lan").unwrap(),
            token: " ".into(),
        };
        assert!(gotify.validate().is_err());
    }
}
