use async_trait::async_trait;

#[async_trait]
pub trait NotificationConnector: Send + Sync {
    fn name(&self) -> &str;
    async fn send_notification(&self, title: &str, text: &str) -> Result<(), anyhow::Error>;
}

pub mod gotify;
pub mod ntfy;
pub mod pushover;
pub mod webhook;

pub use gotify::GotifyConnector;
pub use ntfy::NtfyConnector;
pub use pushover::PushoverConnector;
pub use webhook::WebhookConnector;
