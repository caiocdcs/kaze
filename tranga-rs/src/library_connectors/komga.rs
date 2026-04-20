use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use crate::library_connectors::LibraryConnector;

pub struct KomgaConnector {
    base_url: String,
    #[allow(dead_code)]
    api_key: String,
    client: reqwest::Client,
}

impl KomgaConnector {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        let client = reqwest::Client::builder()
            .default_headers({
                let mut h = reqwest::header::HeaderMap::new();
                h.insert("X-API-Key", api_key.parse().expect("valid header"));
                h
            })
            .build()
            .expect("build client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            client,
        }
    }
}

#[async_trait]
impl LibraryConnector for KomgaConnector {
    fn connector_type(&self) -> &str { "Komga" }
    fn base_url(&self) -> &str { &self.base_url }

    async fn update_library(&self, _ct: CancellationToken) -> Result<(), anyhow::Error> {
        let library_ids = self.get_library_ids().await?;
        for id in library_ids {
            let url = format!("{}/api/v1/libraries/{id}/scan", self.base_url);
            match self.client.post(&url).send().await {
                Ok(resp) if resp.status().is_success() => info!("Scanned Komga library {id}"),
                Ok(resp) => error!("Failed to scan Komga library {id}: {}", resp.status()),
                Err(e) => error!("Error scanning Komga library {id}: {e}"),
            }
        }
        Ok(())
    }

    async fn test_connection(&self) -> Result<bool, anyhow::Error> {
        let url = format!("{}/api/v2/users/me", self.base_url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp.status().is_success())
    }
}

impl KomgaConnector {
    async fn get_library_ids(&self) -> Result<Vec<String>, anyhow::Error> {
        let url = format!("{}/api/v1/libraries", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let libraries: Vec<Value> = resp.json().await?;
        Ok(libraries.iter().filter_map(|l| l["id"].as_str().map(String::from)).collect())
    }
}
