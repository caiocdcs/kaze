//! Requests through the full router against an in-memory app.

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;

use super::router;
use crate::{
    app::{App, Context, Settings},
    db, download,
    events::Events,
    testing::FakeSource,
};

struct Api {
    router: Router,
    app: Arc<App>,
    _library: tempfile::TempDir,
}

async fn api(api_key: Option<&str>) -> Api {
    let library = tempfile::tempdir().unwrap();
    let source = Arc::new(FakeSource::new().with_chapters(&["1", "2"]));
    let ctx = Context {
        db: db::memory().await,
        sources: kaze_sources::Registry::from_sources(vec![source]),
        events: Events::new(256),
        library: library.path().to_owned(),
        download: download::Options::default(),
    };
    let app = Arc::new(App::start(ctx, Settings::default()).await.unwrap());
    Api {
        router: router(Arc::clone(&app), api_key.map(ToOwned::to_owned)),
        app,
        _library: library,
    }
}

impl Api {
    async fn call(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(json) => {
                request = request.header("content-type", "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        // `oneshot` drives the router like a server would, without a socket.
        let response = self
            .router
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        self.call(Method::GET, uri, None).await
    }
}

#[tokio::test]
async fn health_and_sources() {
    let api = api(None).await;
    assert_eq!(api.get("/healthz").await.0, StatusCode::OK);
    let (status, sources) = api.get("/api/v1/sources").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sources[0]["id"], "fake");
    api.app.shutdown().await;
}

#[tokio::test]
async fn follow_then_inspect_series() {
    let api = api(None).await;
    let body = json!({ "source": "fake", "key": "S1" });
    let (status, created) = api
        .call(Method::POST, "/api/v1/series", Some(body.clone()))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().unwrap();

    let (status, _) = api.call(Method::POST, "/api/v1/series", Some(body)).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Wait for the refresh job to store the chapters.
    let mut detail = Value::Null;
    for _ in 0..50 {
        detail = api.get(&format!("/api/v1/series/{id}")).await.1;
        if detail["chapters"].as_array().is_some_and(|c| c.len() == 2) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(detail["title"], "Test Series");
    assert_eq!(detail["chapters"][0]["number"], "1");

    let (_, list) = api.get("/api/v1/series").await;
    assert_eq!(list[0]["chapters"], 2);

    let (status, _) = api
        .call(
            Method::PATCH,
            &format!("/api/v1/series/{id}"),
            Some(json!({ "monitor": "none" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        api.call(Method::DELETE, &format!("/api/v1/series/{id}"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api.get(&format!("/api/v1/series/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
    api.app.shutdown().await;
}

#[tokio::test]
async fn bad_requests() {
    let api = api(None).await;
    let (status, body) = api.get("/api/v1/lookup?url=https://example.com/x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("no source"));

    let (status, _) = api
        .call(
            Method::POST,
            "/api/v1/series",
            Some(json!({ "source": "nope", "key": "S1" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        api.get("/api/v1/search?q=%20").await.0,
        StatusCode::BAD_REQUEST
    );
    api.app.shutdown().await;
}

#[tokio::test]
async fn api_key_is_enforced_when_configured() {
    let api = api(Some("s3cret")).await;
    assert_eq!(api.get("/api/v1/sources").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        api.get("/api/v1/sources?apikey=wrong").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api.get("/api/v1/sources?apikey=s3cret").await.0,
        StatusCode::OK
    );

    let request = Request::get("/api/v1/sources")
        .header("x-api-key", "s3cret")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        api.router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        api.get("/healthz").await.0,
        StatusCode::OK,
        "health stays public"
    );
    api.app.shutdown().await;
}
