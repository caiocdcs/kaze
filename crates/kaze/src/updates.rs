//! Whether a newer kaze is out, from GitHub's latest release. Checked
//! daily unless `check_updates` is off; the request carries nothing but
//! kaze's version in its user agent.

use std::sync::{Arc, Mutex, PoisonError};

use serde::Deserialize;
use url::Url;

pub const RELEASES: &str = "https://api.github.com/repos/caiocdcs/kaze/releases/latest";
const USER_AGENT: &str = concat!("kaze/", env!("CARGO_PKG_VERSION"));

/// The latest release as GitHub lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    #[serde(rename = "tag_name")]
    pub tag: String,
    #[serde(rename = "html_url")]
    pub url: Url,
}

impl Release {
    /// Whether this release is newer than `version` (both `x.y.z`, with an
    /// optional `v` and pre-release suffix).
    #[must_use]
    pub fn is_newer_than(&self, version: &str) -> bool {
        match (parse(&self.tag), parse(version)) {
            (Some(latest), Some(current)) => latest > current,
            _ => false,
        }
    }
}

/// `(major, minor, patch, is_release)`: a release sorts after its own
/// pre-releases (`0.7.0-rc.1` < `0.7.0`).
fn parse(version: &str) -> Option<(u64, u64, u64, bool)> {
    let version = version.trim().trim_start_matches('v');
    let (core, pre) = version
        .split_once('-')
        .map_or((version, None), |(c, p)| (c, Some(p)));
    let mut parts = core.split('.').map(str::parse::<u64>);
    let (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    Some((major, minor, patch, pre.is_none()))
}

/// The last check's result, shared with the health panel.
#[derive(Debug, Clone)]
pub struct Updates {
    releases: Url,
    latest: Arc<Mutex<Option<Release>>>,
}

impl Updates {
    #[must_use]
    pub fn new(releases: Url) -> Self {
        Self {
            releases,
            latest: Arc::default(),
        }
    }

    /// The latest release seen; `None` before the first check.
    #[must_use]
    pub fn latest(&self) -> Option<Release> {
        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub async fn check(&self, http: &reqwest::Client) -> Result<Release, reqwest::Error> {
        let release: Release = http
            .get(self.releases.clone())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(release.clone());
        Ok(release)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.into(),
            url: Url::parse("https://example.com/r").unwrap(),
        }
    }

    #[test]
    fn compares_versions() {
        assert!(release("v0.7.0").is_newer_than("0.6.0"));
        assert!(release("v0.6.1").is_newer_than("0.6.0"));
        assert!(release("v1.0.0").is_newer_than("0.12.3"));
        assert!(
            release("0.10.0").is_newer_than("0.9.0"),
            "numbers, not text"
        );
        assert!(!release("v0.6.0").is_newer_than("0.6.0"));
        assert!(!release("v0.5.9").is_newer_than("0.6.0"));
        assert!(release("v0.7.0").is_newer_than("0.7.0-rc.1"));
        assert!(!release("v0.7.0-rc.1").is_newer_than("0.7.0"));
        assert!(
            !release("nightly").is_newer_than("0.6.0"),
            "unreadable tags never nag"
        );
    }

    #[tokio::test]
    async fn remembers_the_latest_release() {
        use axum::{Json, Router, http::HeaderMap, routing::get};

        let app = Router::new().route(
            "/latest",
            get(|headers: HeaderMap| async move {
                assert_eq!(
                    headers["user-agent"], USER_AGENT,
                    "only the version is sent"
                );
                Json(serde_json::json!({
                    "tag_name": "v9.0.0",
                    "html_url": "https://github.com/caiocdcs/kaze/releases/tag/v9.0.0",
                    "body": "ignored"
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let updates = Updates::new(format!("http://{addr}/latest").parse().unwrap());
        assert_eq!(updates.latest(), None);
        let latest = updates.check(&reqwest::Client::new()).await.unwrap();
        assert_eq!(latest.tag, "v9.0.0");
        assert_eq!(updates.latest(), Some(latest.clone()));
        assert!(latest.is_newer_than(env!("CARGO_PKG_VERSION")));
    }
}
