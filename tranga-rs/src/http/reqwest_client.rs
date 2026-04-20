use reqwest::StatusCode;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

use crate::config::Config;
use super::{DownloadResponse, HttpClient, RequestType};

pub struct ReqwestHttpClient {
    client: reqwest::Client,
    flare_solverr_url: Arc<Mutex<String>>,
    rate_limiter: governor::DefaultDirectRateLimiter,
    semaphore: Arc<Semaphore>,
}

impl ReqwestHttpClient {
    pub async fn new(config: &Config) -> Self {
        let settings = config.settings.read().await;
        let ua = settings.user_agent.clone();
        let fs_url = settings.flare_solverr_url.clone();
        drop(settings);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.http_request_timeout_secs))
            .user_agent(&ua)
            .cookie_provider(Arc::new(reqwest::cookie::Jar::default()))
            .build()
            .expect("failed to build reqwest client");

        let rate_limiter = governor::RateLimiter::direct(governor::Quota::per_second(
            std::num::NonZeroU32::new(config.requests_per_minute / 60)
                .unwrap_or_else(|| std::num::NonZeroU32::new(1).unwrap()),
        ));

        Self {
            client,
            flare_solverr_url: Arc::new(Mutex::new(fs_url)),
            rate_limiter,
            semaphore: Arc::new(Semaphore::new(10)),
        }
    }
}

#[async_trait::async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn make_request(
        &self,
        url: &str,
        request_type: RequestType,
        referrer: Option<&str>,
        ct: CancellationToken,
    ) -> Result<DownloadResponse, anyhow::Error> {
        if ct.is_cancelled() {
            return Err(anyhow::anyhow!("cancelled"));
        }

        let _permit = self.semaphore.acquire().await;

        if self.rate_limiter.check().is_err() {
            debug!("Rate limited, waiting for permit");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let mut builder = self.client.get(url);
        if let Some(ref_url) = referrer {
            builder = builder.header("Referer", ref_url);
        }

        match request_type {
            RequestType::MangaDexFeed => {
                builder = builder.header("Accept", "application/json");
            }
            RequestType::MangaImage | RequestType::MangaCover => {
                builder = builder.header("Accept", "image/webp,image/apng,image/*,*/*;q=0.8");
            }
            _ => {}
        }

        let response = builder.send().await;

        match response {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let is_cloudflare = resp
                    .headers()
                    .get("server")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_lowercase().contains("cloudflare"))
                    .unwrap_or(false);

                let body = resp.text().await?;

                if !StatusCode::from_u16(status).map(|s| s.is_success()).unwrap_or(false)
                    && is_cloudflare
                {
                    warn!("Cloudflare detected for {url}, trying FlareSolverr");
                    return self.try_flaresolverr(url, ct).await;
                }

                Ok(DownloadResponse {
                    status,
                    body,
                    is_cloudflare: false,
                })
            }
            Err(e) => {
                error!("HTTP request failed for {url}: {e}");
                Err(anyhow::anyhow!("HTTP request failed: {e}"))
            }
        }
    }

    async fn download_image(
        &self,
        url: &str,
        referrer: Option<&str>,
        ct: CancellationToken,
    ) -> Result<Option<bytes::Bytes>, anyhow::Error> {
        if ct.is_cancelled() {
            return Ok(None);
        }

        let _permit = self.semaphore.acquire().await;

        let mut builder = self.client.get(url);
        if let Some(ref_url) = referrer {
            builder = builder.header("Referer", ref_url);
        }
        builder = builder.header("Accept", "image/webp,image/apng,image/*,*/*;q=0.8");

        match builder.send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    let bytes = resp.bytes().await?;
                    Ok(Some(bytes))
                } else {
                    warn!("Image download failed: {} status={}", url, resp.status());
                    Ok(None)
                }
            }
            Err(e) => {
                error!("Image download error for {url}: {e}");
                Ok(None)
            }
        }
    }
}

impl ReqwestHttpClient {
    async fn try_flaresolverr(
        &self,
        url: &str,
        _ct: CancellationToken,
    ) -> Result<DownloadResponse, anyhow::Error> {
        let fs_url = self.flare_solverr_url.lock().await;
        if fs_url.is_empty() {
            return Err(anyhow::anyhow!("FlareSolverr URL not configured"));
        }
        drop(fs_url);

        let fs_url = self.flare_solverr_url.lock().await.clone();
        let body = serde_json::json!({
            "cmd": "request.get",
            "url": url
        });

        let resp = self
            .client
            .post(format!("{}/v1", fs_url.trim_end_matches('/')))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "FlareSolverr returned status: {}",
                resp.status()
            ));
        }

        let data: serde_json::Value = resp.json().await?;

        let status = data["solution"]["status"].as_u64().unwrap_or(200) as u16;

        let response_body = data["solution"]["response"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let response_body = extract_json_from_pre(&response_body);

        Ok(DownloadResponse {
            status,
            body: response_body,
            is_cloudflare: false,
        })
    }
}

fn extract_json_from_pre(html: &str) -> String {
    if let Some(start) = html.find("<pre>")
        && let Some(end) = html.find("</pre>")
    {
        let content = &html[start + 5..end];
        if serde_json::from_str::<serde_json::Value>(content).is_ok() {
            return content.to_string();
        }
    }
    html.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_from_pre_returns_json() {
        let html = r#"<html><body><pre>{"key": "value"}</pre></body></html>"#;
        let result = extract_json_from_pre(html);
        assert_eq!(result, r#"{"key": "value"}"#);
    }

    #[test]
    fn extract_json_from_pre_returns_original_if_no_pre() {
        let html = r#"{"direct": "json"}"#;
        let result = extract_json_from_pre(html);
        assert_eq!(result, html);
    }

    #[test]
    fn extract_json_from_pre_returns_original_if_invalid_json() {
        let html = "<html><body><pre>not json</pre></body></html>";
        let result = extract_json_from_pre(html);
        assert_eq!(result, html);
    }

    #[test]
    fn extract_json_from_pre_handles_empty_pre() {
        let html = "<html><body><pre></pre></body></html>";
        let result = extract_json_from_pre(html);
        assert_eq!(result, html);
    }
}
