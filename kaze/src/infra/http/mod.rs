pub mod downloader;
pub mod mock;

pub use downloader::HttpDownloader;
pub use mock::MockHttpClient;

pub type HttpClient = downloader::HttpDownloader;
