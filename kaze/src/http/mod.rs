// Re-exports from infra for backward compatibility during migration.
pub use crate::infra::http::downloader::HttpDownloader;
pub use crate::infra::http::downloader::extract_json_from_pre;
pub use crate::infra::http::mock::MockHttpClient;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestType {
    Default,
    MangaDexFeed,
    MangaImage,
    MangaCover,
    MangaInfo,
    RequiresBrowser,
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

    async fn post_request(
        &self,
        url: &str,
        body: &str,
        content_type: &str,
        ct: CancellationToken,
    ) -> Result<DownloadResponse, anyhow::Error>;

    async fn download_image(
        &self,
        url: &str,
        referrer: Option<&str>,
        ct: CancellationToken,
    ) -> Result<Option<bytes::Bytes>, anyhow::Error>;
}
