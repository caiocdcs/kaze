use async_trait::async_trait;
use base64::Engine;
use crate::notification_connectors::NotificationConnector;

pub struct NtfyConnector {
    pub name: String,
    pub url: String,
    pub priority: i32,
    pub username: String,
    pub password: String,
    pub topic: String,
}

#[async_trait]
impl NotificationConnector for NtfyConnector {
    fn name(&self) -> &str { &self.name }

    async fn send_notification(&self, title: &str, text: &str) -> Result<(), anyhow::Error> {
        let url = format!("{}/{}", self.url.trim_end_matches('/'), self.topic);

        let mut req = reqwest::Client::new()
            .post(&url)
            .header("Priority", self.priority.to_string())
            .header("Title", title)
            .body(text.to_string());

        if !self.username.is_empty() || !self.password.is_empty() {
            let auth = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", self.username, self.password));
            req = req.header("Authorization", format!("Basic {auth}"));
        }

        let resp = req.send().await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Ntfy returned {}", resp.status()));
        }
        Ok(())
    }
}
