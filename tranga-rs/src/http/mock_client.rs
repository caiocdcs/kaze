use std::collections::HashMap;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::{DownloadResponse, HttpClient, RequestType};

#[derive(Debug, Clone)]
pub struct MockResponse {
    pub status: u16,
    pub body: String,
    pub is_cloudflare: bool,
}

#[derive(Debug, Clone)]
pub struct MockImageResponse {
    pub bytes: bytes::Bytes,
}

pub struct MockHttpClient {
    get_responses: HashMap<String, MockResponse>,
    image_responses: HashMap<String, MockImageResponse>,
}

impl MockHttpClient {
    pub fn builder() -> MockHttpClientBuilder {
        MockHttpClientBuilder {
            get_responses: HashMap::new(),
            image_responses: HashMap::new(),
        }
    }
}

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

    pub fn expect_get_with_status(mut self, url: &str, status: u16, body: impl Into<String>) -> Self {
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

    pub fn expect_cloudflare(mut self, url: &str, body: impl Into<String>) -> Self {
        self.get_responses.insert(
            url.to_string(),
            MockResponse {
                status: 403,
                body: body.into(),
                is_cloudflare: true,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_returns_configured_response() {
        let mock = MockHttpClient::builder()
            .expect_get("https://example.com/api", r#"{"data":[]}"#)
            .build();

        let resp = mock
            .make_request(
                "https://example.com/api",
                RequestType::Default,
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, r#"{"data":[]}"#);
    }

    #[tokio::test]
    async fn mock_returns_error_for_unknown_url() {
        let mock = MockHttpClient::builder().build();
        let result = mock
            .make_request(
                "https://unknown.com",
                RequestType::Default,
                None,
                CancellationToken::new(),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mock_returns_image_bytes() {
        let mock = MockHttpClient::builder()
            .expect_image("https://example.com/img.jpg", vec![0xFF, 0xD8, 0xFF])
            .build();

        let bytes = mock
            .download_image("https://example.com/img.jpg", None, CancellationToken::new())
            .await
            .unwrap();

        assert!(bytes.is_some());
        assert_eq!(bytes.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn mock_returns_none_for_unknown_image() {
        let mock = MockHttpClient::builder().build();
        let bytes = mock
            .download_image("https://unknown.com/img.jpg", None, CancellationToken::new())
            .await
            .unwrap();
        assert!(bytes.is_none());
    }

    #[tokio::test]
    async fn mock_partial_url_match() {
        let mock = MockHttpClient::builder()
            .expect_get("example.com/api", "matched")
            .build();

        let resp = mock
            .make_request(
                "https://example.com/api/v1/test",
                RequestType::Default,
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(resp.body, "matched");
    }
}
