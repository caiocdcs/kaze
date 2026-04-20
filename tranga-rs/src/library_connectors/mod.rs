use async_trait::async_trait;

#[async_trait]
pub trait LibraryConnector: Send + Sync {
    fn connector_type(&self) -> &str;
    fn base_url(&self) -> &str;
    async fn update_library(&self, ct: tokio_util::sync::CancellationToken) -> Result<(), anyhow::Error>;
    async fn test_connection(&self) -> Result<bool, anyhow::Error>;
}

pub mod komga;
pub mod kavita;

pub use komga::KomgaConnector;
pub use kavita::KavitaConnector;
