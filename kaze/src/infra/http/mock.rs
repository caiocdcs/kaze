use std::collections::HashMap;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::http::{DownloadResponse, HttpClient, RequestType};

pub struct MockResponse {
    pub status: u16,
    pub body: String,
    pub is_cloudflare: bool,
}

pub struct MockImageResponse {
    pub bytes: bytes::Bytes,
}

/// Mock HTTP client for testing.
pub struct MockHttpClient {
    get_responses: HashMap<String, MockResponse>,
    image_responses: HashMap<String, MockImageResponse>,
}

impl MockHttpClient {
    pub fn builder() -> MockHttpClientBuilder {
        MockHttpClientBuilder::default()
    }
}

#[derive(Default)]
pub struct MockHttpClientBuilder {
    get_responses: HashMap<String, MockResponse>,
    image_responses: HashMap<String, MockImageResponse>,
}

impl MockHttpClientBuilder {
    pub fn expect_get(mut self, url: &str, body: impl Into<String>) -> Self {
        self.get_responses.insert(
            url.to_string(),
            MockResponse {
                status: 200,
                body: body.into(),
                is_cloudflare: false,
            },
        );
        self
    }

    pub fn expect_get_with_status(
        mut self,
        url: &str,
        status: u16,
        body: impl Into<String>,
    ) -> Self {
        self.get_responses.insert(
            url.to_string(),
            MockResponse {
                status,
                body: body.into(),
                is_cloudflare: false,
            },
        );
        self
    }

    pub fn expect_image(mut self, url: &str, data: Vec<u8>) -> Self {
        self.image_responses.insert(
            url.to_string(),
            MockImageResponse {
                bytes: bytes::Bytes::from(data),
            },
        );
        self
    }

    pub fn build(self) -> MockHttpClient {
        MockHttpClient {
            get_responses: self.get_responses,
            image_responses: self.image_responses,
        }
    }
}

impl MockHttpClient {
    fn find_response(&self, url: &str) -> Option<&MockResponse> {
        if let Some(resp) = self.get_responses.get(url) {
            return Some(resp);
        }
        for (pattern, resp) in &self.get_responses {
            if url.contains(pattern) || pattern.contains(url) {
                return Some(resp);
            }
        }
        None
    }
}

#[async_trait]
impl HttpClient for MockHttpClient {
    async fn make_request(
        &self,
        url: &str,
        _request_type: RequestType,
        _referrer: Option<&str>,
        _ct: CancellationToken,
    ) -> Result<DownloadResponse, anyhow::Error> {
        match self.find_response(url) {
            Some(resp) => Ok(DownloadResponse {
                status: resp.status,
                body: resp.body.clone(),
                is_cloudflare: resp.is_cloudflare,
            }),
            None => Err(anyhow::anyhow!("no mock response for URL: {url}")),
        }
    }

    async fn post_request(
        &self,
        url: &str,
        _body: &str,
        _content_type: &str,
        _ct: CancellationToken,
    ) -> Result<DownloadResponse, anyhow::Error> {
        match self.find_response(url) {
            Some(resp) => Ok(DownloadResponse {
                status: resp.status,
                body: resp.body.clone(),
                is_cloudflare: resp.is_cloudflare,
            }),
            None => Err(anyhow::anyhow!("no mock response for URL: {url}")),
        }
    }

    async fn download_image(
        &self,
        url: &str,
        _referrer: Option<&str>,
        _ct: CancellationToken,
    ) -> Result<Option<bytes::Bytes>, anyhow::Error> {
        match self.image_responses.get(url) {
            Some(resp) => Ok(Some(resp.bytes.clone())),
            None => Ok(None),
        }
    }
}
