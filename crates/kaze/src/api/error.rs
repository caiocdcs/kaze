use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use kaze_core::SourceError;
use serde::Serialize;
use utoipa::ToSchema;

use crate::{
    app::{FollowError, MetadataError, SeriesSourceError},
    backup::RestoreError,
    jobs::QueueError,
    settings::SettingsError,
};

pub type ApiResult<T> = Result<T, ApiError>;

/// What every error response holds.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: String,
}

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
        if matches!(self, Self::Upstream(_) | Self::Integration(_)) {
            tracing::warn!("{self}");
        }
        let body = ErrorBody {
            error: self.to_string(),
        };
        (status, Json(body)).into_response()
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
            FollowError::UnknownSource(_) | FollowError::Language { .. } => {
                Self::BadRequest(err.to_string())
            }
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

impl From<SeriesSourceError> for ApiError {
    fn from(err: SeriesSourceError) -> Self {
        match err {
            SeriesSourceError::SeriesNotFound => Self::NotFound("series"),
            SeriesSourceError::NotLinked(_) => Self::NotFound("source of the series"),
            SeriesSourceError::BadOrder => Self::BadRequest(err.to_string()),
            SeriesSourceError::AlreadyLinked(_) | SeriesSourceError::LastSource => {
                Self::Conflict(err.to_string())
            }
            SeriesSourceError::Follow(e) => e.into(),
            SeriesSourceError::Db(e) => e.into(),
            SeriesSourceError::Queue(e) => e.into(),
        }
    }
}

impl From<MetadataError> for ApiError {
    fn from(err: MetadataError) -> Self {
        match err {
            MetadataError::SeriesNotFound => Self::NotFound("series"),
            MetadataError::NotOnAniList(_) => Self::NotFound("AniList entry"),
            MetadataError::AniList(e) => Self::Upstream(e.to_string()),
            MetadataError::Cover(e) => {
                tracing::error!("removing the cover: {e}");
                Self::Internal
            }
            MetadataError::Db(e) => e.into(),
            MetadataError::Queue(e) => e.into(),
        }
    }
}

impl From<RestoreError> for ApiError {
    fn from(err: RestoreError) -> Self {
        match err {
            RestoreError::NotSqlite | RestoreError::NotKaze | RestoreError::Newer { .. } => {
                Self::BadRequest(format!("can't restore it: {err}"))
            }
            RestoreError::Backup(e) => {
                tracing::error!("staging a restore: {e}");
                Self::Internal
            }
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
