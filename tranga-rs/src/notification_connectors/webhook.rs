use async_trait::async_trait;
use crate::notification_connectors::NotificationConnector;

pub struct WebhookConnector {
    pub name: String,
    pub url: String,
    pub headers: std::collections::HashMap<String, String>,
    pub http_method: String,
    pub body: String,
}

impl WebhookConnector {
    pub fn format_str(s: &str, title: &str, text: &str) -> String {
        s.replace("%title", title).replace("%text", text)
    }
}

#[async_trait]
impl NotificationConnector for WebhookConnector {
    fn name(&self) -> &str { &self.name }

    async fn send_notification(&self, title: &str, text: &str) -> Result<(), anyhow::Error> {
        let url = Self::format_str(&self.url, title, text);
        let body = Self::format_str(&self.body, title, text);
        let headers: std::collections::HashMap<String, String> = self.headers
            .iter()
            .map(|(k, v)| (k.clone(), Self::format_str(v, title, text)))
            .collect();

        let client = reqwest::Client::new();
        let method = match self.http_method.to_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "PUT" => reqwest::Method::PUT,
            "PATCH" => reqwest::Method::PATCH,
            "DELETE" => reqwest::Method::DELETE,
            _ => reqwest::Method::POST,
        };

        let mut req = client.request(method, &url);
        for (k, v) in &headers {
            req = req.header(k.as_str(), v.as_str());
        }
        req = req.header("Content-Type", "application/json").body(body);

        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("webhook returned {}", resp.status()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_str_replaces_placeholders() {
        assert_eq!(WebhookConnector::format_str("hello %title", "world", "x"), "hello world");
        assert_eq!(WebhookConnector::format_str("%title: %text", "Alert", "Something happened"), "Alert: Something happened");
    }

    #[test]
    fn format_str_no_placeholders() {
        assert_eq!(WebhookConnector::format_str("plain text", "title", "text"), "plain text");
    }
}
