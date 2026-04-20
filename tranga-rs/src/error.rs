use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{entity} {id} not found")]
    NotFound { entity: &'static str, id: String },

    #[error("connector {name} is disabled")]
    ConnectorDisabled { name: String },

    #[error("connector {name} not found")]
    ConnectorNotFound { name: String },

    #[error("precondition required: {detail}")]
    PreconditionRequired { detail: String },

    #[error("precondition failed: {detail}")]
    PreconditionFailed { detail: String },

    #[error("bad request: {detail}")]
    BadRequest { detail: String },

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("{0}")]
    Internal(anyhow::Error),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDetail {
    #[serde(rename = "type")]
    pub problem_type: &'static str,
    pub title: &'static str,
    pub status: u16,
    pub detail: Option<String>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, problem) = match &self {
            AppError::NotFound { entity, id } => (
                StatusCode::NOT_FOUND,
                ProblemDetail {
                    problem_type: "/errors/not-found",
                    title: "Not Found",
                    status: 404,
                    detail: Some(format!("{entity} {id} not found")),
                },
            ),
            AppError::ConnectorDisabled { name } => (
                StatusCode::PRECONDITION_FAILED,
                ProblemDetail {
                    problem_type: "/errors/connector-disabled",
                    title: "Connector Disabled",
                    status: 412,
                    detail: Some(format!("Connector {name} is disabled")),
                },
            ),
            AppError::ConnectorNotFound { name } => (
                StatusCode::NOT_FOUND,
                ProblemDetail {
                    problem_type: "/errors/not-found",
                    title: "Not Found",
                    status: 404,
                    detail: Some(format!("Connector {name} not found")),
                },
            ),
            AppError::PreconditionRequired { detail } => (
                StatusCode::PRECONDITION_REQUIRED,
                ProblemDetail {
                    problem_type: "/errors/precondition-required",
                    title: "Precondition Required",
                    status: 428,
                    detail: Some(detail.clone()),
                },
            ),
            AppError::PreconditionFailed { detail } => (
                StatusCode::PRECONDITION_FAILED,
                ProblemDetail {
                    problem_type: "/errors/precondition-failed",
                    title: "Precondition Failed",
                    status: 412,
                    detail: Some(detail.clone()),
                },
            ),
            AppError::BadRequest { detail } => (
                StatusCode::BAD_REQUEST,
                ProblemDetail {
                    problem_type: "/errors/bad-request",
                    title: "Bad Request",
                    status: 400,
                    detail: Some(detail.clone()),
                },
            ),
            AppError::Database(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ProblemDetail {
                    problem_type: "/errors/database",
                    title: "Database Error",
                    status: 500,
                    detail: Some(e.to_string()),
                },
            ),
            AppError::Internal(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ProblemDetail {
                    problem_type: "/errors/internal",
                    title: "Internal Server Error",
                    status: 500,
                    detail: Some(e.to_string()),
                },
            ),
        };

        (status, Json(problem)).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e)
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;

    fn response_status_and_body(response: Response) -> (StatusCode, String) {
        let status = response.status();
        let body = tokio_test::block_on(async {
            let collected = response.into_body()
                .collect()
                .await
                .expect("collect body");
            String::from_utf8_lossy(&collected.to_bytes()).to_string()
        });
        (status, body)
    }

    #[test]
    fn not_found_returns_404() {
        let err = AppError::NotFound {
            entity: "Manga",
            id: "manga_123".into(),
        };
        let (status, body) = response_status_and_body(err.into_response());
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.contains("not-found"));
        assert!(body.contains("Manga manga_123"));
    }

    #[test]
    fn connector_disabled_returns_412() {
        let err = AppError::ConnectorDisabled {
            name: "MangaDex".into(),
        };
        let (status, body) = response_status_and_body(err.into_response());
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert!(body.contains("connector-disabled"));
        assert!(body.contains("MangaDex"));
    }

    #[test]
    fn precondition_required_returns_428() {
        let err = AppError::PreconditionRequired {
            detail: "must link first".into(),
        };
        let (status, body) = response_status_and_body(err.into_response());
        assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
        assert!(body.contains("precondition-required"));
    }

    #[test]
    fn bad_request_returns_400() {
        let err = AppError::BadRequest {
            detail: "page < 1".into(),
        };
        let (status, body) = response_status_and_body(err.into_response());
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("bad-request"));
    }

    #[test]
    fn internal_error_returns_500() {
        let err = AppError::Internal(anyhow::anyhow!("something broke"));
        let (status, body) = response_status_and_body(err.into_response());
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.contains("internal"));
    }

    #[test]
    fn problem_detail_serializes_correctly() {
        let pd = ProblemDetail {
            problem_type: "/errors/not-found",
            title: "Not Found",
            status: 404,
            detail: Some("Manga xyz not found".into()),
        };
        let json = serde_json::to_string(&pd).expect("serialize");
        assert!(json.contains(r#""type":"/errors/not-found""#), "type field: {json}");
        assert!(json.contains(r#""title":"Not Found""#), "title field: {json}");
        assert!(json.contains(r#""status":404"#), "status field: {json}");
        assert!(json.contains("Manga xyz"), "detail field: {json}");
    }

    #[test]
    fn anyhow_error_converts_to_internal() {
        let err: AppError = anyhow::anyhow!("oops").into();
        match err {
            AppError::Internal(_) => {}
            other => panic!("expected Internal, got: {other}"),
        }
    }
}
