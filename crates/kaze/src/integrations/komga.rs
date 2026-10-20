//! Komga: scan a library (or all of them) after downloads.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use url::Url;

use std::collections::BTreeMap;

use super::{IntegrationError, checked, endpoint, http_url, library_languages};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Komga {
    pub url: Url,
    pub api_key: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub library_id: Option<String>,
    /// Language → library id, for languages with their own folder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[ts(as = "Option<BTreeMap<String, String>>", optional)]
    pub libraries: BTreeMap<String, String>,
}

impl Komga {
    pub(super) fn validate(&self) -> Result<(), String> {
        http_url(&self.url)?;
        library_languages(&self.libraries)
    }

    /// Scan `language`'s library if it has one, else `library_id`, else all.
    pub(super) async fn scan(
        &self,
        http: &Client,
        language: Option<&str>,
    ) -> Result<(), IntegrationError> {
        let chosen = language
            .and_then(|l| self.libraries.get(l))
            .or(self.library_id.as_ref())
            .filter(|id| !id.is_empty());
        let ids = if let Some(id) = chosen {
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
            libraries: BTreeMap::new(),
        };
        komga.scan(&Client::new(), None).await.unwrap();
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

    #[tokio::test]
    async fn scans_the_library_of_the_language() {
        let (url, seen) = server(|_| (StatusCode::ACCEPTED, json!({}))).await;
        let komga = Komga {
            url,
            api_key: "secret".into(),
            library_id: Some("EN".into()),
            libraries: [("pt-BR".to_owned(), "PT".to_owned())].into(),
        };
        komga.scan(&Client::new(), Some("pt-BR")).await.unwrap();
        komga.scan(&Client::new(), Some("it")).await.unwrap();
        let seen = seen.lock().unwrap();
        let paths: Vec<_> = seen.iter().map(|s| s.path_and_query.as_str()).collect();
        assert_eq!(
            paths,
            ["/api/v1/libraries/PT/scan", "/api/v1/libraries/EN/scan"]
        );
    }
}
