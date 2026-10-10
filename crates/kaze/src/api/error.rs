use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use kaze_core::SourceError;
use serde_json::json;

use crate::{app::FollowError, jobs::QueueError, settings::SettingsError};

pub type ApiResult<T> = Result<T, ApiError>;

/// Errors as the API reports them: a status code and `{"error": "..."}`.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Conflict(String),
    /// The manga site failed or answered something unexpected.
    #[error("source error: {0}")]
    Upstream(String),
    /// A library server or notification service failed.
    #[error("{0}")]
    Integration(String),
    #[error("internal error")]
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Upstream(_) | Self::Integration(_) => StatusCode::BAD_GATEWAY,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

// `?` in handlers converts library errors through these impls. Internal
// details are logged, not sent to the client.

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!("database error: {err}");
        Self::Internal
    }
}

impl From<QueueError> for ApiError {
    fn from(err: QueueError) -> Self {
        tracing::error!("job queue error: {err}");
        Self::Internal
    }
}

impl From<SourceError> for ApiError {
    fn from(err: SourceError) -> Self {
        match err {
            SourceError::NotFound { .. } => Self::NotFound("series on source"),
            err => Self::Upstream(err.to_string()),
        }
    }
}

impl From<FollowError> for ApiError {
    fn from(err: FollowError) -> Self {
        match err {
            FollowError::UnknownSource(_) => Self::BadRequest(err.to_string()),
            FollowError::AlreadyFollowing(_) | FollowError::FolderTaken(_) => {
                Self::Conflict(err.to_string())
            }
            FollowError::Io(e) => Self::BadRequest(format!("reading the folder: {e}")),
            FollowError::Source(e) => e.into(),
            FollowError::Db(e) => e.into(),
            FollowError::Queue(e) => e.into(),
        }
    }
}

impl From<SettingsError> for ApiError {
    fn from(err: SettingsError) -> Self {
        match err {
            SettingsError::Locked(_) => Self::Conflict(err.to_string()),
            SettingsError::Invalid(_) => Self::BadRequest(err.to_string()),
            SettingsError::Db(e) => e.into(),
        }
    }
}
