//! Komga: scan a library (or all of them) after downloads.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use url::Url;

use super::{IntegrationError, checked, endpoint, http_url};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Komga {
    pub url: Url,
    pub api_key: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub library_id: Option<String>,
}

impl Komga {
    pub(super) fn validate(&self) -> Result<(), String> {
        http_url(&self.url)
    }

    pub(super) async fn scan(&self, http: &Client) -> Result<(), IntegrationError> {
        let ids = if let Some(id) = self.library_id.as_deref().filter(|id| !id.is_empty()) {
            vec![id.to_owned()]
        } else {
            #[derive(Deserialize)]
            struct Library {
                id: String,
            }
            let request = http
                .get(endpoint(&self.url, "api/v1/libraries")?)
                .header("X-API-Key", &self.api_key);
            let libraries: Vec<Library> = checked("Komga", request).await?.json().await?;
            libraries.into_iter().map(|l| l.id).collect()
        };
        for id in ids {
            let request = http
                .post(endpoint(&self.url, &format!("api/v1/libraries/{id}/scan"))?)
                .header("X-API-Key", &self.api_key);
            checked("Komga", request).await?;
        }
        Ok(())
    }

    /// Log in without scanning.
    pub(super) async fn test(&self, http: &Client) -> Result<(), IntegrationError> {
        let request = http
            .get(endpoint(&self.url, "api/v2/users/me")?)
            .header("X-API-Key", &self.api_key);
        checked("Komga", request).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use serde_json::json;

    use super::*;
    use crate::integrations::tests::server;

    #[tokio::test]
    async fn scans_every_library_when_none_is_chosen() {
        let (url, seen) = server(|path| match path {
            "/api/v1/libraries" => (StatusCode::OK, json!([{ "id": "A" }, { "id": "B" }])),
            _ => (StatusCode::ACCEPTED, json!({})),
        })
        .await;
        let komga = Komga {
            url,
            api_key: "secret".into(),
            library_id: None,
        };
        komga.scan(&Client::new()).await.unwrap();
        let seen = seen.lock().unwrap();
        let paths: Vec<_> = seen.iter().map(|s| s.path_and_query.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/api/v1/libraries",
                "/api/v1/libraries/A/scan",
                "/api/v1/libraries/B/scan"
            ]
        );
        assert!(seen.iter().all(|s| s.headers["x-api-key"] == "secret"));
    }
}
