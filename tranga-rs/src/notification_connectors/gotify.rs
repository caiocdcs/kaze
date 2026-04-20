use async_trait::async_trait;
use crate::notification_connectors::NotificationConnector;

pub struct GotifyConnector {
    pub name: String,
    pub url: String,
    pub app_token: String,
    pub priority: i32,
}

#[async_trait]
impl NotificationConnector for GotifyConnector {
    fn name(&self) -> &str { &self.name }

    async fn send_notification(&self, title: &str, text: &str) -> Result<(), anyhow::Error> {
        let url = format!("{}/message", self.url.trim_end_matches('/'));
        let body = serde_json::json!({
            "title": title,
            "message": text,
            "priority": self.priority
        });

        let resp = reqwest::Client::new()
            .post(&url)
            .header("X-Gotify-Key", &self.app_token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Gotify returned {}", resp.status()));
        }
        Ok(())
    }
}
