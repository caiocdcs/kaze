use std::time::Duration;

use url::Url;

pub type Result<T, E = SourceError> = std::result::Result<T, E>;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Everything that can go wrong while talking to a source.
///
/// The transport error is boxed so this crate does not depend on any HTTP
/// library; sources wrap whatever their client returns.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("request to {url} failed")]
    Transport {
        url: Url,
        #[source]
        source: BoxError,
    },

    #[error("{url} returned HTTP {status}")]
    Status { url: Url, status: u16 },

    #[error("not found: {url}")]
    NotFound { url: Url },

    #[error("blocked by anti-bot protection at {url}")]
    Blocked { url: Url },

    #[error("could not parse {what}: {reason}")]
    Parse { what: &'static str, reason: String },

    #[error("timed out after {0:?}")]
    Timeout(Duration),

    #[error("{0} is not supported by this source")]
    Unsupported(&'static str),
}

impl SourceError {
    pub fn parse(what: &'static str, reason: impl Into<String>) -> Self {
        Self::Parse {
            what,
            reason: reason.into(),
        }
    }

    /// Whether trying the same request again later might succeed.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport { .. } | Self::Timeout(_) => true,
            Self::Status { status, .. } => *status == 429 || *status >= 500,
            Self::NotFound { .. }
            | Self::Blocked { .. }
            | Self::Parse { .. }
            | Self::Unsupported(_) => false,
        }
    }
}
