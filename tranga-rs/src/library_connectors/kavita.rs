use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use crate::library_connectors::LibraryConnector;

pub struct KavitaConnector {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
    cached_token: tokio::sync::Mutex<Option<String>>,
}

impl KavitaConnector {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        let client = reqwest::Client::new();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            client,
            cached_token: tokio::sync::Mutex::new(None),
        }
    }

    fn get_token_url(&self) -> String {
        format!(
            "{}/api/Plugin/authenticate?apiKey={}&pluginName=Tranga",
            self.base_url, self.api_key
        )
    }
}

#[async_trait]
impl LibraryConnector for KavitaConnector {
    fn connector_type(&self) -> &str { "Kavita" }
    fn base_url(&self) -> &str { &self.base_url }

    async fn update_library(&self, _ct: CancellationToken) -> Result<(), anyhow::Error> {
        let token = self.refresh_auth().await?;
        let ids = self.get_library_ids(&token).await?;

        let url = format!("{}/api/Library/scan-multiple", self.base_url);
        let body = serde_json::json!({"ids": ids});
        self.client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&body)
            .send()
            .await?;

        Ok(())
    }

    async fn test_connection(&self) -> Result<bool, anyhow::Error> {
        let token = self.refresh_auth().await?;
        let url = format!("{}/api/Account", self.base_url);
        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }
}

impl KavitaConnector {
    async fn refresh_auth(&self) -> Result<String, anyhow::Error> {
        let resp = self.client.post(self.get_token_url()).send().await?;
        let data: serde_json::Value = resp.json().await?;
        let token = data["token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no token in Kavita response"))?
            .to_string();
        if let Ok(mut cached) = self.cached_token.try_lock() {
            *cached = Some(token.clone());
        }
        Ok(token)
    }

    async fn get_library_ids(&self, _token: &str) -> Result<Vec<i64>, anyhow::Error> {
        let url = format!("{}/api/Library/libraries", self.base_url);
        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {_token}"))
            .send()
            .await?;
        let libraries: Vec<serde_json::Value> = resp.json().await?;
        Ok(libraries.iter().filter_map(|l| l["id"].as_i64()).collect())
    }
}
