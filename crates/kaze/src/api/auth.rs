use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::AppState;

/// When an API key is set, require it as the `X-Api-Key` header or an
/// `apikey` query parameter (for `<img>` and `EventSource`, which cannot
/// set headers). Read per request, so a key set in the UI applies at once.
pub async fn require_api_key(
    State(app): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected) = app.ctx.settings.current().api_key.clone() else {
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
    if given.as_deref() == Some(expected.as_str()) {
        next.run(request).await
    } else {
        (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "missing or wrong API key" })),
        )
            .into_response()
    }
}
