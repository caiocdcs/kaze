//! [FlareSolverr](https://github.com/FlareSolverr/FlareSolverr): a headless
//! browser service that gets through Cloudflare challenges.
//!
//! When a site answers with a challenge, kaze asks FlareSolverr to load the
//! page, then keeps the `cf_clearance` cookie and the browser's User-Agent
//! (Cloudflare ties the cookie to it). Later requests go straight to the
//! site with both, so FlareSolverr only runs again when the cookie expires.
//! Each site gets its own FlareSolverr session, so one browser is reused.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use kaze_core::{Result, SourceError};
use reqwest::{Client, cookie::Jar, header::HeaderValue};
use serde::Deserialize;
use serde_json::json;
use url::Url;

/// How long FlareSolverr may spend on one challenge.
const MAX_TIMEOUT: Duration = Duration::from_secs(60);

/// Handle to the (optional) FlareSolverr service. Cheap to clone; the URL
/// can change while kaze runs.
#[derive(Debug, Clone)]
pub struct FlareSolverr {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    url: RwLock<Option<Url>>,
    client: Client,
    /// The same jar the sources' client uses, so solved cookies apply.
    jar: Arc<Jar>,
    /// Host → the User-Agent its clearance cookie belongs to.
    user_agents: Mutex<HashMap<String, HeaderValue>>,
    sessions: Mutex<HashSet<String>>,
}

/// A page as FlareSolverr's browser saw it.
#[derive(Debug)]
pub struct Solution {
    pub status: u16,
    pub html: String,
}

#[derive(Deserialize)]
struct Reply {
    status: String,
    #[serde(default)]
    message: String,
    solution: Option<ReplySolution>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplySolution {
    status: u16,
    response: String,
    #[serde(default)]
    cookies: Vec<Cookie>,
    user_agent: Option<String>,
}

#[derive(Deserialize)]
struct Cookie {
    name: String,
    value: String,
    domain: Option<String>,
    path: Option<String>,
}

impl FlareSolverr {
    pub(crate) fn new(jar: Arc<Jar>) -> Self {
        Self {
            inner: Arc::new(Inner {
                url: RwLock::new(None),
                // Solving takes a while: a generous timeout of its own.
                client: Client::builder()
                    .timeout(MAX_TIMEOUT + Duration::from_secs(30))
                    .build()
                    .unwrap_or_default(),
                jar,
                user_agents: Mutex::new(HashMap::new()),
                sessions: Mutex::new(HashSet::new()),
            }),
        }
    }

    /// Point at a FlareSolverr instance, or `None` to stop using one.
    pub fn set_url(&self, url: Option<Url>) {
        let mut current = self.inner.url.write().expect("flaresolverr url lock");
        if *current != url {
            // Sessions live in the old instance.
            self.inner.sessions.lock().expect("sessions lock").clear();
            *current = url;
        }
    }

    #[must_use]
    pub fn url(&self) -> Option<Url> {
        self.inner
            .url
            .read()
            .expect("flaresolverr url lock")
            .clone()
    }

    /// The User-Agent to send to `host`, if a clearance cookie needs one.
    pub(crate) fn user_agent_for(&self, host: &str) -> Option<HeaderValue> {
        self.inner
            .user_agents
            .lock()
            .expect("user agents lock")
            .get(host)
            .cloned()
    }

    /// Is FlareSolverr reachable? Its root answers `{"msg": "FlareSolverr is ready!"}`.
    pub async fn check(&self) -> std::result::Result<String, String> {
        #[derive(Deserialize)]
        struct Ready {
            #[serde(default)]
            version: String,
        }
        let url = self.url().ok_or("not set")?;
        let ready: Ready = self
            .inner
            .client
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        Ok(format!("v{}", ready.version))
    }

    /// Load `page` in FlareSolverr's browser and keep the clearance it got.
    /// `None` when no FlareSolverr is configured.
    pub(crate) async fn solve(&self, page: &Url) -> Option<Result<Solution>> {
        let base = self.url()?;
        Some(self.solve_with(&base, page).await)
    }

    async fn solve_with(&self, base: &Url, page: &Url) -> Result<Solution> {
        let host = page.host_str().unwrap_or_default().to_owned();
        let session = format!("kaze-{host}");
        let endpoint = base
            .join("v1")
            .map_err(|e| SourceError::parse("FlareSolverr URL", e.to_string()))?;
        let fail = |message: String| {
            tracing::warn!(%page, "FlareSolverr failed: {message}");
            SourceError::Blocked { url: page.clone() }
        };

        let created = self
            .inner
            .sessions
            .lock()
            .expect("sessions lock")
            .contains(&session);
        if !created {
            // An "already exists" error is fine: the session is there either way.
            let _ = self
                .call(
                    &endpoint,
                    &json!({ "cmd": "sessions.create", "session": session }),
                )
                .await;
            self.inner
                .sessions
                .lock()
                .expect("sessions lock")
                .insert(session.clone());
        }

        let request = json!({
            "cmd": "request.get",
            "url": page.as_str(),
            "session": session,
            "maxTimeout": MAX_TIMEOUT.as_millis(),
        });
        let reply = self.call(&endpoint, &request).await.map_err(fail)?;
        if reply.status != "ok" {
            return Err(fail(reply.message));
        }
        let solution = reply.solution.ok_or_else(|| fail("no solution".into()))?;

        for cookie in &solution.cookies {
            let path = cookie.path.as_deref().unwrap_or("/");
            // Host-only when it's the page's own host (also the only form
            // an IP address accepts); otherwise keep the wider domain.
            let domain = cookie
                .domain
                .as_deref()
                .filter(|d| d.trim_start_matches('.') != host)
                .map(|d| format!("; Domain={d}"))
                .unwrap_or_default();
            let header = format!("{}={}; Path={path}{domain}", cookie.name, cookie.value);
            self.inner.jar.add_cookie_str(&header, page);
        }
        if let Some(agent) = solution
            .user_agent
            .as_deref()
            .and_then(|a| HeaderValue::from_str(a).ok())
        {
            self.inner
                .user_agents
                .lock()
                .expect("user agents lock")
                .insert(host, agent);
        }
        Ok(Solution {
            status: solution.status,
            html: solution.response,
        })
    }

    async fn call(
        &self,
        endpoint: &Url,
        body: &serde_json::Value,
    ) -> std::result::Result<Reply, String> {
        self.inner
            .client
            .post(endpoint.clone())
            .json(body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }
}
