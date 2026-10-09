use std::sync::Arc;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// When an API key is configured, require it as the `X-Api-Key` header or
/// an `apikey` query parameter (for `<img>` and `EventSource`, which cannot
/// set headers).
pub async fn require_api_key(
    State(expected): State<Option<Arc<str>>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected) = expected else {
        return next.run(request).await;
    };
    let from_header = request
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok());
    let from_query = request.uri().query().and_then(|q| {
        url::form_urlencoded::parse(q.as_bytes())
            .find(|(k, _)| k == "apikey")
            .map(|(_, v)| v.into_owned())
    });
    let given = from_header.map(ToOwned::to_owned).or(from_query);
    if given.as_deref() == Some(&*expected) {
        next.run(request).await
    } else {
        (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "missing or wrong API key" })),
        )
            .into_response()
    }
}
