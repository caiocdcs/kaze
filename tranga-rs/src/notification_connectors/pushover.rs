use async_trait::async_trait;
use crate::notification_connectors::NotificationConnector;

pub struct PushoverConnector {
    pub name: String,
    pub app_token: String,
    pub user_key: String,
}

#[async_trait]
impl NotificationConnector for PushoverConnector {
    fn name(&self) -> &str { &self.name }

    async fn send_notification(&self, title: &str, text: &str) -> Result<(), anyhow::Error> {
        let params = [
            ("token", self.app_token.as_str()),
            ("user", self.user_key.as_str()),
            ("title", title),
            ("message", text),
        ];

        let resp = reqwest::Client::new()
            .post("https://api.pushover.net/1/messages.json")
            .form(&params)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Pushover returned {}", resp.status()));
        }
        Ok(())
    }
}
