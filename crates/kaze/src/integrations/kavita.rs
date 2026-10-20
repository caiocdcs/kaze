//! Kavita: scan a library (or all of them) after downloads.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use url::Url;

use std::collections::BTreeMap;

use super::{IntegrationError, checked, endpoint, http_url, library_languages};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Kavita {
    pub url: Url,
    pub api_key: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub library_id: Option<i64>,
    /// Language → library id, for languages with their own folder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[ts(as = "Option<BTreeMap<String, i64>>", optional)]
    pub libraries: BTreeMap<String, i64>,
}

impl Kavita {
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
        let token = self.token(http).await?;
        let chosen = language
            .and_then(|l| self.libraries.get(l).copied())
            .or(self.library_id);
        let request = match chosen {
            Some(id) => http
                .post(endpoint(&self.url, "api/Library/scan")?)
                .query(&[("libraryId", id.to_string())]),
            None => http.post(endpoint(&self.url, "api/Library/scan-all")?),
        };
        checked("Kavita", request.bearer_auth(token)).await?;
        Ok(())
    }

    /// Log in without scanning.
    pub(super) async fn test(&self, http: &Client) -> Result<(), IntegrationError> {
        self.token(http).await.map(drop)
    }

    /// Kavita trades the user's API key for a short-lived JWT.
    async fn token(&self, http: &Client) -> Result<String, IntegrationError> {
        #[derive(Deserialize)]
        struct Auth {
            token: String,
        }
        let request = http
            .post(endpoint(&self.url, "api/Plugin/authenticate")?)
            .query(&[("apiKey", self.api_key.as_str()), ("pluginName", "kaze")]);
        let auth: Auth = checked("Kavita", request).await?.json().await?;
        Ok(auth.token)
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use serde_json::json;

    use super::*;
    use crate::integrations::tests::server;

    #[tokio::test]
    async fn authenticates_then_scans() {
        let (url, seen) = server(|path| match path {
            "/api/Plugin/authenticate" => (StatusCode::OK, json!({ "token": "jwt" })),
            _ => (StatusCode::OK, json!({})),
        })
        .await;
        let kavita = Kavita {
            url,
            api_key: "k".into(),
            library_id: Some(3),
            libraries: [("pt-BR".to_owned(), 7)].into(),
        };
        kavita.scan(&Client::new(), None).await.unwrap();
        kavita.scan(&Client::new(), Some("pt-BR")).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(
            seen[0].path_and_query,
            "/api/Plugin/authenticate?apiKey=k&pluginName=kaze"
        );
        assert_eq!(seen[1].path_and_query, "/api/Library/scan?libraryId=3");
        assert_eq!(seen[1].headers["authorization"], "Bearer jwt");
        assert_eq!(seen[3].path_and_query, "/api/Library/scan?libraryId=7");
    }
}
