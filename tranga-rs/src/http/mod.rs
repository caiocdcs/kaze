pub mod reqwest_client;
pub mod mock_client;

pub use reqwest_client::ReqwestHttpClient;
pub use mock_client::MockHttpClient;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestType {
    Default,
    MangaDexFeed,
    MangaImage,
    MangaCover,
    MangaInfo,
}

#[derive(Debug, Clone)]
pub struct DownloadResponse {
    pub status: u16,
    pub body: String,
    pub is_cloudflare: bool,
}

#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn make_request(
        &self,
        url: &str,
        request_type: RequestType,
        referrer: Option<&str>,
        ct: CancellationToken,
    ) -> Result<DownloadResponse, anyhow::Error>;

    async fn download_image(
        &self,
        url: &str,
        referrer: Option<&str>,
        ct: CancellationToken,
    ) -> Result<Option<bytes::Bytes>, anyhow::Error>;
}
