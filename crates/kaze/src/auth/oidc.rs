//! Signing in with an OpenID Connect provider: authorization code flow
//! with PKCE, a state and a nonce.

use std::{
    collections::HashMap,
    fmt,
    sync::Mutex,
    time::{Duration, Instant},
};

use futures_util::future::BoxFuture;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, HttpRequest, HttpResponse, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, TokenResponse as _,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::UrlBase;

/// `[auth.oidc]` in kaze.toml.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcConfig {
    /// Exactly as the provider names itself, trailing slash included.
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    /// Where browsers reach kaze, for the callback URL.
    pub public_url: Url,
    /// The provider's name on the sign-in button.
    #[serde(default)]
    pub name: Option<String>,
    /// Emails or subject ids allowed in; empty lets in anyone the provider
    /// signs in for this client.
    #[serde(default)]
    pub allowed: Vec<String>,
}

impl fmt::Debug for OidcConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OidcConfig")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &"***")
            .field("public_url", &self.public_url.as_str())
            .field("name", &self.name)
            .field("allowed", &self.allowed)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    #[error("invalid [auth.oidc] {0}")]
    Config(String),
    #[error("couldn't read the provider's configuration: {0}")]
    Discovery(String),
    #[error("this sign-in expired or was started elsewhere; try again")]
    UnknownLogin,
    #[error("the provider refused: {0}")]
    Refused(String),
    #[error("couldn't redeem the sign-in with the provider: {0}")]
    Exchange(String),
    #[error("the provider's answer didn't check out: {0}")]
    Token(String),
    #[error("{0} may not sign in to kaze")]
    NotAllowed(String),
}

/// Who signed in.
#[derive(Debug)]
pub struct Identity {
    pub subject: String,
    pub email: Option<String>,
}

type Client = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// How long a started sign-in may take.
const LOGIN_TTL: Duration = Duration::from_secs(10 * 60);
/// Sign-ins in flight; the oldest go first, so spamming the login link
/// costs memory only up to here.
const MAX_PENDING: usize = 64;
/// Re-read the provider's configuration (and signing keys) this often, so
/// rotated keys are picked up.
const DISCOVERY_TTL: Duration = Duration::from_secs(60 * 60);

struct Pending {
    verifier: PkceCodeVerifier,
    nonce: Nonce,
    started: Instant,
}

pub struct Oidc {
    config: OidcConfig,
    issuer: IssuerUrl,
    redirect: RedirectUrl,
    /// Not the sources' client: no redirects, as the OAuth2 spec asks.
    http: reqwest::Client,
    discovered: tokio::sync::Mutex<Option<(Instant, CoreProviderMetadata)>>,
    /// By the state sent to the provider.
    pending: Mutex<HashMap<String, Pending>>,
}

impl fmt::Debug for Oidc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Oidc")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Oidc {
    pub fn new(config: OidcConfig, base: &UrlBase) -> Result<Self, OidcError> {
        let issuer = IssuerUrl::new(config.issuer.clone())
            .map_err(|e| OidcError::Config(format!("issuer: {e}")))?;
        let callback = format!(
            "{}{}/api/v1/auth/oidc/callback",
            config.public_url.as_str().trim_end_matches('/'),
            base.as_str()
        );
        let redirect = RedirectUrl::new(callback)
            .map_err(|e| OidcError::Config(format!("public_url: {e}")))?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| OidcError::Config(format!("HTTP client: {e}")))?;
        Ok(Self {
            config,
            issuer,
            redirect,
            http,
            discovered: tokio::sync::Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
        })
    }

    /// The name for the sign-in button.
    #[must_use]
    pub fn name(&self) -> &str {
        self.config.name.as_deref().unwrap_or("single sign-on")
    }

    /// The callback URL to register with the provider.
    #[must_use]
    pub fn redirect_url(&self) -> &str {
        self.redirect.as_str()
    }

    fn http_client(
        &self,
    ) -> impl Fn(HttpRequest) -> BoxFuture<'static, Result<HttpResponse, reqwest::Error>> {
        let client = self.http.clone();
        move |request| Box::pin(send(client.clone(), request))
    }

    /// Discovered on first use rather than at startup, so a provider that
    /// is down doesn't keep kaze from starting.
    async fn client(&self) -> Result<Client, OidcError> {
        let mut discovered = self.discovered.lock().await;
        let metadata = match &*discovered {
            Some((at, metadata)) if at.elapsed() < DISCOVERY_TTL => metadata.clone(),
            _ => {
                let http = self.http_client();
                let metadata = CoreProviderMetadata::discover_async(self.issuer.clone(), &http)
                    .await
                    .map_err(|e| OidcError::Discovery(error_chain(&e)))?;
                *discovered = Some((Instant::now(), metadata.clone()));
                metadata
            }
        };
        Ok(CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.config.client_id.clone()),
            Some(ClientSecret::new(self.config.client_secret.clone())),
        )
        .set_redirect_uri(self.redirect.clone()))
    }

    /// Where to send the browser to sign in.
    pub async fn begin(&self) -> Result<Url, OidcError> {
        let client = self.client().await?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("email".into()))
            .add_scope(Scope::new("profile".into()))
            .set_pkce_challenge(challenge)
            .url();
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.retain(|_, p| p.started.elapsed() < LOGIN_TTL);
        if pending.len() >= MAX_PENDING
            && let Some(oldest) = pending
                .iter()
                .min_by_key(|(_, p)| p.started)
                .map(|(state, _)| state.clone())
        {
            pending.remove(&oldest);
        }
        pending.insert(
            state.secret().clone(),
            Pending {
                verifier,
                nonce,
                started: Instant::now(),
            },
        );
        Ok(url)
    }

    /// Redeem the code the provider sent the browser back with.
    pub async fn finish(&self, code: String, state: &str) -> Result<Identity, OidcError> {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(state)
            .filter(|p| p.started.elapsed() < LOGIN_TTL)
            .ok_or(OidcError::UnknownLogin)?;
        let client = self.client().await?;
        let http = self.http_client();
        let token = client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(|e| OidcError::Exchange(e.to_string()))?
            .set_pkce_verifier(pending.verifier)
            .request_async(&http)
            .await
            .map_err(|e| OidcError::Exchange(error_chain(&e)))?;
        let id_token = token
            .id_token()
            .ok_or_else(|| OidcError::Token("no ID token".into()))?;
        let claims = id_token
            .claims(&client.id_token_verifier(), &pending.nonce)
            .map_err(|e| OidcError::Token(e.to_string()))?;
        // An address the provider says it hasn't checked can't name anyone.
        let email = claims
            .email()
            .filter(|_| claims.email_verified() != Some(false))
            .map(|e| e.as_str().to_owned());
        let identity = Identity {
            subject: claims.subject().as_str().to_owned(),
            email,
        };
        let allowed = &self.config.allowed;
        if !allowed.is_empty()
            && !allowed
                .iter()
                .any(|a| *a == identity.subject || Some(a) == identity.email.as_ref())
        {
            return Err(OidcError::NotAllowed(
                identity.email.unwrap_or(identity.subject),
            ));
        }
        Ok(identity)
    }
}

/// openidconnect's HTTP, over our reqwest (it was built for an older one).
/// The client is owned, not borrowed: a borrowing future isn't `Send` for
/// every lifetime, which axum handlers need.
async fn send(
    client: reqwest::Client,
    request: HttpRequest,
) -> Result<HttpResponse, reqwest::Error> {
    let response = client.execute(request.try_into()?).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let mut out = HttpResponse::new(response.bytes().await?.to_vec());
    *out.status_mut() = status;
    *out.headers_mut() = headers;
    Ok(out)
}

/// An error with its causes, which openidconnect's errors keep apart.
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
