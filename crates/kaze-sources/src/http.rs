//! The HTTP client every source uses.
//!
//! [`Http`] owns the connection pool and cookie jar and is shared by the
//! whole app. Each source gets a [`SourceHttp`] from it, which adds that
//! source's rate limit, `Referer` and retry policy on top.

use std::{sync::Arc, time::Duration};

use backon::{ExponentialBuilder, Retryable};
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use kaze_core::{Image, RateLimit, Result, SourceError};
use reqwest::{Client, Response, StatusCode, header};
use serde::de::DeserializeOwned;
use url::Url;

/// A desktop browser user agent: several sites reject unknown clients.
pub const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub user_agent: String,
    /// Timeout of a single attempt, including reading the body.
    pub timeout: Duration,
    /// Extra attempts after the first one, for retryable failures.
    pub max_retries: usize,
    /// Delay before the first retry; doubles on every further retry.
    pub retry_base_delay: Duration,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_base_delay: Duration::from_secs(1),
        }
    }
}

/// App-wide HTTP client. Cheap to clone: everything inside is reference counted.
#[derive(Debug, Clone)]
pub struct Http {
    client: Client,
    config: Arc<HttpConfig>,
}

impl Http {
    pub fn new(config: HttpConfig) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .user_agent(&config.user_agent)
            .timeout(config.timeout)
            .cookie_store(true)
            .build()?;
        Ok(Self {
            client,
            config: Arc::new(config),
        })
    }

    /// The shared client without any source's limits, for other services.
    #[must_use]
    pub fn client(&self) -> Client {
        self.client.clone()
    }

    /// A client for one source, honouring its rate limit.
    #[must_use]
    pub fn for_source(&self, rate_limit: RateLimit, referer: Option<&Url>) -> SourceHttp {
        // Spread requests evenly (no bursts): N per window ⇒ one every window/N.
        let period = rate_limit.per / rate_limit.requests.get();
        let quota =
            Quota::with_period(period).unwrap_or_else(|| Quota::per_second(rate_limit.requests));
        SourceHttp {
            http: self.clone(),
            limiter: Arc::new(RateLimiter::direct(quota)),
            referer: referer.and_then(|r| header::HeaderValue::from_str(r.as_str()).ok()),
            user_agent: None,
        }
    }
}

/// Per-source client: shared pool + this source's rate limiter and referer.
#[derive(Debug, Clone)]
pub struct SourceHttp {
    http: Http,
    limiter: Arc<DefaultDirectRateLimiter>,
    referer: Option<header::HeaderValue>,
    user_agent: Option<header::HeaderValue>,
}

impl SourceHttp {
    /// Send this User-Agent instead of the browser one, for APIs that ask
    /// clients to identify themselves.
    #[must_use]
    pub fn with_user_agent(mut self, user_agent: &str) -> Self {
        self.user_agent = header::HeaderValue::from_str(user_agent).ok();
        self
    }

    pub async fn get_text(&self, url: &Url) -> Result<String> {
        let response = self.get(url).await?;
        response.text().await.map_err(|e| transport(url, e))
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &Url) -> Result<T> {
        let body = self
            .get(url)
            .await?
            .bytes()
            .await
            .map_err(|e| transport(url, e))?;
        serde_json::from_slice(&body)
            .map_err(|e| SourceError::parse("JSON response", e.to_string()))
    }

    pub async fn get_image(&self, url: &Url) -> Result<Image> {
        let response = self.get(url).await?;
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let bytes = response.bytes().await.map_err(|e| transport(url, e))?;
        Ok(Image {
            bytes,
            content_type,
        })
    }

    /// GET with rate limiting and retries. Each attempt waits for the
    /// limiter, so retries count against the source's budget too.
    async fn get(&self, url: &Url) -> Result<Response> {
        let config = &self.http.config;
        let backoff = ExponentialBuilder::default()
            .with_min_delay(config.retry_base_delay)
            .with_max_times(config.max_retries)
            .with_jitter();

        (|| async {
            self.limiter.until_ready().await;
            self.send_once(url).await
        })
        .retry(backoff)
        .sleep(tokio::time::sleep)
        .when(SourceError::is_retryable)
        .notify(|err, delay| tracing::debug!(%url, ?delay, "retrying after: {err}"))
        .await
    }

    async fn send_once(&self, url: &Url) -> Result<Response> {
        let mut request = self.http.client.get(url.clone());
        if let Some(referer) = &self.referer {
            request = request.header(header::REFERER, referer.clone());
        }
        if let Some(user_agent) = &self.user_agent {
            request = request.header(header::USER_AGENT, user_agent.clone());
        }
        let response = request.send().await.map_err(|e| transport(url, e))?;
        check_status(url, response)
    }
}

fn transport(url: &Url, err: reqwest::Error) -> SourceError {
    SourceError::Transport {
        url: url.clone(),
        source: Box::new(err),
    }
}

fn check_status(url: &Url, response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if is_challenge(&response) {
        return Err(SourceError::Blocked { url: url.clone() });
    }
    if status == StatusCode::NOT_FOUND {
        return Err(SourceError::NotFound { url: url.clone() });
    }
    Err(SourceError::Status {
        url: url.clone(),
        status: status.as_u16(),
    })
}

/// Cloudflare marks challenge pages with `cf-mitigated: challenge`; older
/// setups only answer 403 from a `server: cloudflare`. A plain 503 from
/// Cloudflare is usually an origin outage, so it stays retryable.
fn is_challenge(response: &Response) -> bool {
    let headers = response.headers();
    let header_is = |name: &str, value: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case(value))
    };
    header_is("cf-mitigated", "challenge")
        || (response.status() == StatusCode::FORBIDDEN && header_is("server", "cloudflare"))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    use super::*;

    fn client(rate_limit: RateLimit, referer: Option<&Url>) -> SourceHttp {
        let config = HttpConfig {
            retry_base_delay: Duration::from_millis(1),
            ..HttpConfig::default()
        };
        Http::new(config).unwrap().for_source(rate_limit, referer)
    }

    fn fast() -> SourceHttp {
        client(RateLimit::per_second(1000), None)
    }

    fn url(server: &MockServer, p: &str) -> Url {
        format!("{}{p}", server.uri()).parse().unwrap()
    }

    #[tokio::test]
    async fn retries_server_errors_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(path("/flaky"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .with_priority(1)
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(path("/flaky"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(&server)
            .await;

        let body = fast().get_text(&url(&server, "/flaky")).await.unwrap();
        assert_eq!(body, "ok");
    }

    #[tokio::test]
    async fn gives_up_after_max_retries() {
        let server = MockServer::start().await;
        Mock::given(path("/down"))
            .respond_with(ResponseTemplate::new(500))
            .expect(4) // 1 attempt + 3 retries
            .mount(&server)
            .await;

        let err = fast().get_text(&url(&server, "/down")).await.unwrap_err();
        assert!(
            matches!(err, SourceError::Status { status: 500, .. }),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn not_found_is_not_retried() {
        let server = MockServer::start().await;
        Mock::given(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let err = fast()
            .get_text(&url(&server, "/missing"))
            .await
            .unwrap_err();
        assert!(matches!(err, SourceError::NotFound { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn detects_cloudflare_challenge() {
        let server = MockServer::start().await;
        Mock::given(path("/cf"))
            .respond_with(ResponseTemplate::new(403).insert_header("cf-mitigated", "challenge"))
            .expect(1)
            .mount(&server)
            .await;

        let err = fast().get_text(&url(&server, "/cf")).await.unwrap_err();
        assert!(matches!(err, SourceError::Blocked { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn sends_referer_and_returns_content_type() {
        let server = MockServer::start().await;
        let referer: Url = "https://example.com/".parse().unwrap();
        Mock::given(method("GET"))
            .and(path("/img"))
            .and(header("referer", "https://example.com/"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(vec![1, 2, 3], "image/jpeg"))
            .expect(1)
            .mount(&server)
            .await;

        let fetched = client(RateLimit::per_second(1000), Some(&referer))
            .get_image(&url(&server, "/img"))
            .await
            .unwrap();
        assert_eq!(fetched.bytes.as_ref(), [1, 2, 3]);
        assert_eq!(fetched.content_type.as_deref(), Some("image/jpeg"));
    }

    #[tokio::test]
    async fn bad_json_is_a_parse_error() {
        let server = MockServer::start().await;
        Mock::given(path("/json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let err = fast()
            .get_json::<serde_json::Value>(&url(&server, "/json"))
            .await
            .unwrap_err();
        assert!(matches!(err, SourceError::Parse { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn rate_limit_spaces_requests() {
        let server = MockServer::start().await;
        Mock::given(path("/r"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        // 20/s ⇒ one every 50ms; the first is immediate, so 4 requests ≥ 150ms.
        let http = client(RateLimit::per_second(20), None);
        let start = Instant::now();
        for _ in 0..4 {
            http.get_text(&url(&server, "/r")).await.unwrap();
        }
        assert!(
            start.elapsed() >= Duration::from_millis(150),
            "{:?}",
            start.elapsed()
        );
    }
}
