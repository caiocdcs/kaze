//! Who may use the API: a signed-in browser (session cookie) or a script
//! with the API key.

use std::net::SocketAddr;

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use ts_rs::TS;
use utoipa::ToSchema;

use super::{ApiError, ApiResult, AppState};
use crate::{
    app::App,
    auth::{self, AuthError, NewPassword, SESSION_DAYS, SessionToken},
    config::UrlBase,
    db,
};

const COOKIE: &str = "kaze_session";

/// Let a request through with a valid session cookie or API key. Read per
/// request, so a key set in the UI applies at once.
pub async fn require_auth(State(app): State<AppState>, request: Request, next: Next) -> Response {
    match allowed(&app, request.headers(), request.uri()).await {
        Ok(true) => next.run(request).await,
        Ok(false) => ApiError::Unauthorized("sign in or send the API key").into_response(),
        Err(err) => ApiError::from(err).into_response(),
    }
}

async fn allowed(app: &App, headers: &HeaderMap, uri: &Uri) -> sqlx::Result<bool> {
    if let Some(token) = session(headers)
        && auth::session_valid(&app.ctx.db, &token).await?
    {
        return Ok(true);
    }
    Ok(match app.ctx.settings.current().api_key.as_deref() {
        Some(expected) => sent_key(headers, uri).is_some_and(|given| keys_match(&given, expected)),
        // Without a key kaze is open, as before logins, until the account
        // is made.
        None => !db::auth::has_account(&app.ctx.db).await?,
    })
}

/// The `X-Api-Key` header, or an `apikey` query parameter for clients
/// that cannot set headers.
fn sent_key(headers: &HeaderMap, uri: &Uri) -> Option<String> {
    let from_header = headers.get("x-api-key").and_then(|v| v.to_str().ok());
    let from_query = || {
        uri.query().and_then(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .find(|(k, _)| k == "apikey")
                .map(|(_, v)| v.into_owned())
        })
    };
    from_header.map(ToOwned::to_owned).or_else(from_query)
}

/// Compares digests in constant time, so neither the key's contents nor its
/// length show in the response time.
fn keys_match(given: &str, expected: &str) -> bool {
    Sha256::digest(given)
        .ct_eq(&Sha256::digest(expected))
        .into()
}

fn session(headers: &HeaderMap) -> Option<SessionToken> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(COOKIE)?.strip_prefix('='))
        .map(SessionToken::from_cookie)
}

/// `HttpOnly` keeps the token from scripts; `SameSite=Lax` keeps other
/// sites from sending it with their requests. Not `Secure`: kaze is often
/// reached over plain HTTP at home.
fn cookie(base: &UrlBase, value: &str, max_age: i64) -> Response {
    let path = if base.as_str().is_empty() {
        "/"
    } else {
        base.as_str()
    };
    let cookie =
        format!("{COOKIE}={value}; Path={path}; Max-Age={max_age}; HttpOnly; SameSite=Lax");
    match HeaderValue::from_str(&cookie) {
        Ok(cookie) => (StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response(),
        Err(_) => ApiError::Internal.into_response(),
    }
}

/// Sign the browser in: a new session in its cookie.
async fn signed_in(app: &App, base: &UrlBase) -> ApiResult<Response> {
    let token = auth::start_session(&app.ctx.db).await?;
    Ok(cookie(base, token.as_str(), SESSION_DAYS * 24 * 60 * 60))
}

#[derive(Debug, Serialize, TS, ToSchema)]
#[ts(export)]
pub struct AuthStatus {
    /// Whether the account exists; until then the UI offers to create it.
    pub account: bool,
    /// Whether this browser's session cookie is valid.
    pub signed_in: bool,
}

#[utoipa::path(
    operation_id = "auth_status",
    summary = "Whether the account exists and this browser is signed in",
    get,
    path = "/auth/status",
    tag = "auth",
    security(()),
    responses((status = 200, description = "OK", body = AuthStatus))
)]
pub async fn status(
    State(app): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<AuthStatus>> {
    let signed_in = match session(&headers) {
        Some(token) => auth::session_valid(&app.ctx.db, &token).await?,
        None => false,
    };
    Ok(Json(AuthStatus {
        account: db::auth::has_account(&app.ctx.db).await?,
        signed_in,
    }))
}

#[derive(Debug, Deserialize, TS, ToSchema)]
#[ts(export)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[utoipa::path(
    operation_id = "auth_setup",
    summary = "Create the account and sign in",
    description = "Only while there is no account, and only for the first browser after \
                   kaze generated its API key, or with the API key.",
    post,
    path = "/auth/setup",
    tag = "auth",
    security(()),
    request_body = Credentials,
    responses(
        (status = 204, description = "Created and signed in; the session is in a cookie"),
        (status = 400, description = "Empty username or short password", body = super::ErrorBody),
        (status = 401, description = "Needs the API key", body = super::ErrorBody),
        (status = 409, description = "The account exists", body = super::ErrorBody),
    )
)]
pub async fn setup(
    State(app): State<AppState>,
    Extension(base): Extension<UrlBase>,
    headers: HeaderMap,
    uri: Uri,
    Json(credentials): Json<Credentials>,
) -> ApiResult<Response> {
    let username = credentials.username.trim();
    if username.is_empty() {
        return Err(ApiError::BadRequest("the username is empty".into()));
    }
    let password = NewPassword::new(credentials.password).ok_or_else(short_password)?;
    let hash = password.hash().await?;
    let allowed = allowed(&app, &headers, &uri).await?;
    let mut tx = app.ctx.db.begin().await?;
    // The generated key's claim is taken only if the account is made too.
    if !allowed && !db::settings::claim_key(&mut *tx).await? {
        return Err(ApiError::Unauthorized(
            "send the API key to create the account",
        ));
    }
    if !db::auth::create_account(&mut *tx, username, &hash).await? {
        return Err(ApiError::Conflict("the account exists already".into()));
    }
    tx.commit().await?;
    tracing::info!(username, "account created");
    signed_in(&app, &base).await
}

#[utoipa::path(
    operation_id = "auth_login",
    summary = "Sign in",
    post,
    path = "/auth/login",
    tag = "auth",
    security(()),
    request_body = Credentials,
    responses(
        (status = 204, description = "Signed in; the session is in a cookie"),
        (status = 401, description = "Wrong username or password", body = super::ErrorBody),
    )
)]
pub async fn login(
    State(app): State<AppState>,
    Extension(base): Extension<UrlBase>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(credentials): Json<Credentials>,
) -> ApiResult<Response> {
    let ok = match db::auth::account(&app.ctx.db).await? {
        // Both checked always, so a wrong name takes as long as a wrong password.
        Some(account) => {
            let password = auth::verify(credentials.password, account.password_hash).await;
            password && credentials.username == account.username
        }
        None => false,
    };
    if !ok {
        // The client address, for fail2ban; behind a proxy, what it forwarded.
        let forwarded = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
        let peer = peer.map(|Extension(ConnectInfo(peer))| peer.ip().to_string());
        tracing::warn!(
            client = forwarded.map(ToOwned::to_owned).or(peer),
            "failed sign-in"
        );
        return Err(ApiError::Unauthorized("wrong username or password"));
    }
    signed_in(&app, &base).await
}

#[utoipa::path(
    operation_id = "auth_logout",
    summary = "Sign out this browser",
    post,
    path = "/auth/logout",
    tag = "auth",
    security(()),
    responses((status = 204, description = "Signed out"))
)]
pub async fn logout(
    State(app): State<AppState>,
    Extension(base): Extension<UrlBase>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    if let Some(token) = session(&headers) {
        db::auth::delete_session(&app.ctx.db, &token.hash()).await?;
    }
    Ok(cookie(&base, "", 0))
}

#[derive(Debug, Deserialize, TS, ToSchema)]
#[ts(export)]
pub struct PasswordChange {
    pub current: String,
    pub new: String,
}

#[utoipa::path(
    operation_id = "auth_change_password",
    summary = "Change the password",
    description = "Signs out every other browser.",
    put,
    path = "/auth/password",
    tag = "auth",
    request_body = PasswordChange,
    responses(
        (status = 204, description = "Changed"),
        (status = 400, description = "Wrong current password or short new one", body = super::ErrorBody),
    )
)]
pub async fn change_password(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(change): Json<PasswordChange>,
) -> ApiResult<StatusCode> {
    let db = &app.ctx.db;
    let account = db::auth::account(db)
        .await?
        .ok_or(ApiError::NotFound("account"))?;
    if !auth::verify(change.current, account.password_hash).await {
        return Err(ApiError::BadRequest("the current password is wrong".into()));
    }
    let hash = NewPassword::new(change.new)
        .ok_or_else(short_password)?
        .hash()
        .await?;
    db::auth::set_password(db, &hash).await?;
    let keep = session(&headers).map(|token| token.hash());
    db::auth::delete_sessions(db, keep.as_deref()).await?;
    tracing::info!("password changed; other browsers signed out");
    Ok(StatusCode::NO_CONTENT)
}

fn short_password() -> ApiError {
    ApiError::BadRequest(format!(
        "the password needs at least {} characters",
        NewPassword::MIN_LEN
    ))
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        tracing::error!("{err}");
        Self::Internal
    }
}
