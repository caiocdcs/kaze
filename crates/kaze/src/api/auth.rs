use axum::{
    Json,
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::{ApiError, ApiResult, AppState};

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
    if given.is_some_and(|given| keys_match(&given, &expected)) {
        next.run(request).await
    } else {
        (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(super::ErrorBody {
                error: "missing or wrong API key".into(),
            }),
        )
            .into_response()
    }
}

/// Compares digests in constant time, so neither the key's contents nor its
/// length show in the response time.
fn keys_match(given: &str, expected: &str) -> bool {
    Sha256::digest(given)
        .ct_eq(&Sha256::digest(expected))
        .into()
}

#[derive(Debug, serde::Serialize)]
pub struct ClaimedKey {
    api_key: String,
}

/// Outside the API key: hands the key kaze generated on its first start to
/// the first browser that asks, like Radarr's first-run setup. Later calls
/// get 404.
pub async fn claim(State(app): State<AppState>) -> ApiResult<Json<ClaimedKey>> {
    let api_key = app
        .ctx
        .settings
        .claim_api_key()
        .await?
        .ok_or(ApiError::NotFound("unclaimed API key"))?;
    tracing::info!("a browser claimed the generated API key");
    Ok(Json(ClaimedKey { api_key }))
}
