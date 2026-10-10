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
    app::{App, Context},
    db,
    events::Events,
    jobs::RetryPolicy,
    settings::SettingsStore,
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
    let db = db::memory().await;
    let locked = match api_key {
        Some(key) => json!({ "api_key": key }),
        None => json!({}),
    };
    let ctx = Context {
        settings: SettingsStore::load(db.clone(), locked.as_object().unwrap().clone())
            .await
            .unwrap(),
        db,
        sources: kaze_sources::Registry::from_sources(vec![source]),
        events: Events::new(256),
        library: library.path().to_owned(),
        http: reqwest::Client::new(),
        flaresolverr: kaze_sources::http::Http::new(kaze_sources::http::HttpConfig::default())
            .unwrap()
            .flaresolverr(),
    };
    let app = Arc::new(App::start(ctx, RetryPolicy::default()).await.unwrap());
    Api {
        router: router(Arc::clone(&app)),
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
async fn unknown_paths_get_the_ui_or_a_404() {
    let api = api(None).await;
    // index.html when the UI is built, a hint page otherwise; HTML either way.
    let response = api
        .router
        .clone()
        .oneshot(Request::get("/series/1").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert_eq!(api.get("/api/v1/nope").await.0, StatusCode::NOT_FOUND);
    assert_eq!(api.get("/_app/missing.js").await.0, StatusCode::NOT_FOUND);
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

    // Job rows name what they work on, so the queue can show it.
    let (_, jobs) = api.get("/api/v1/jobs").await;
    let jobs = jobs.as_array().unwrap();
    let refresh = jobs
        .iter()
        .find(|j| j["job"]["type"] == "refresh_series")
        .unwrap();
    assert_eq!(refresh["series_title"], "Test Series");
    assert_eq!(refresh["chapter"], Value::Null);
    let download = jobs
        .iter()
        .find(|j| j["job"]["type"] == "download_chapter")
        .unwrap();
    assert_eq!(download["series_id"], id);
    assert!(download["chapter"].is_string());

    let uri = format!("/api/v1/series/{id}");
    let patch = |body: Value| api.call(Method::PATCH, &uri, Some(body));
    let (status, detail) = patch(json!({ "refresh_hours": 0 })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["refresh_hours"], 0);
    let (_, detail) = patch(json!({ "monitor": "none" })).await;
    assert_eq!(detail["monitor"], "none");
    assert_eq!(detail["refresh_hours"], 0, "untouched by a partial update");
    let (_, detail) = patch(json!({ "refresh_hours": null })).await;
    assert_eq!(
        detail["refresh_hours"],
        Value::Null,
        "null means the global interval"
    );
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
    let api = api(Some("s3cret-key")).await;
    assert_eq!(api.get("/api/v1/sources").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        api.get("/api/v1/sources?apikey=wrong").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api.get("/api/v1/sources?apikey=s3cret-key").await.0,
        StatusCode::OK
    );

    let request = Request::get("/api/v1/sources")
        .header("x-api-key", "s3cret-key")
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

#[tokio::test]
async fn settings_can_be_read_and_changed() {
    let api = api(None).await;
    let (status, view) = api.get("/api/v1/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["settings"]["concurrency"], 4);
    assert_eq!(view["locked"], json!([]));

    let (status, view) = api
        .call(
            Method::PATCH,
            "/api/v1/settings",
            Some(json!({ "concurrency": 6 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["settings"]["concurrency"], 6);

    let (status, body) = api
        .call(
            Method::PATCH,
            "/api/v1/settings",
            Some(json!({ "naming_template": "%M" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("%C"));

    // Setting a key in the UI turns authentication on immediately.
    api.call(
        Method::PATCH,
        "/api/v1/settings",
        Some(json!({ "api_key": "new-secret" })),
    )
    .await;
    assert_eq!(
        api.get("/api/v1/settings").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api.get("/api/v1/settings?apikey=new-secret").await.0,
        StatusCode::OK
    );
    api.app.shutdown().await;
}

#[tokio::test]
async fn locked_settings_cannot_be_changed() {
    let api = api(Some("s3cret-key")).await;
    let (status, _) = api
        .call(
            Method::PATCH,
            "/api/v1/settings?apikey=s3cret-key",
            Some(json!({ "api_key": null })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    api.app.shutdown().await;
}

#[tokio::test]
async fn scheduled_tasks_can_be_listed_and_run() {
    let api = api(None).await;
    let (status, tasks) = api.get("/api/v1/tasks").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tasks[0]["task"], "sync_latest");
    assert_eq!(tasks[1]["task"], "refresh_series");

    let (status, _) = api
        .call(Method::POST, "/api/v1/tasks/refresh_series/run", None)
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, _) = api.call(Method::POST, "/api/v1/tasks/nope/run", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    api.app.shutdown().await;
}

#[tokio::test]
async fn integrations_can_be_managed() {
    let api = api(None).await;
    let (status, presets) = api.get("/api/v1/integrations/presets").await;
    assert_eq!(status, StatusCode::OK);
    let ntfy = presets
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "ntfy")
        .unwrap();

    let body = json!({ "name": "Phone", "config": ntfy["config"], "events": ["downloaded"] });
    let (status, created) = api
        .call(Method::POST, "/api/v1/integrations", Some(body))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["enabled"], true);
    assert_eq!(created["config"]["kind"], "webhook");
    let uri = format!("/api/v1/integrations/{}", created["id"]);

    let bad = json!({ "name": " ", "config": ntfy["config"] });
    assert_eq!(
        api.call(Method::PUT, &uri, Some(bad)).await.0,
        StatusCode::BAD_REQUEST
    );
    let paused = json!({ "name": "Phone", "enabled": false, "config": ntfy["config"] });
    let (_, updated) = api.call(Method::PUT, &uri, Some(paused)).await;
    assert_eq!(updated["enabled"], false);
    assert_eq!(updated["events"], json!([]));

    assert_eq!(api.get("/api/v1/integrations").await.1[0]["name"], "Phone");
    assert_eq!(
        api.call(Method::DELETE, &uri, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(api.get("/api/v1/integrations").await.1, json!([]));
    api.app.shutdown().await;
}

#[tokio::test]
async fn existing_folders_can_be_imported() {
    let api = api(None).await;
    let folder = api.app.ctx.library.join("Old Downloads");
    std::fs::create_dir(&folder).unwrap();
    let file = std::fs::File::create(folder.join("Test Series Ch.1.cbz")).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("001.jpg", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.finish().unwrap();

    let (status, found) = api.get("/api/v1/import").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        found,
        json!([{ "folder": "Old Downloads", "files": 1, "chapters": ["1"], "unparsed": 0 }])
    );

    let (_, matches) = api.get("/api/v1/import/match?folder=Test%20Series").await;
    assert_eq!(matches[0]["hit"]["title"], "Test Series");
    assert_eq!(matches[0]["source"], "fake");

    let body =
        json!({ "folder": "Old Downloads", "source": "fake", "key": "S1", "monitor": "all" });
    let (status, imported) = api
        .call(Method::POST, "/api/v1/import", Some(body.clone()))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    assert_eq!(imported["matched"], 1);

    let (_, detail) = api.get(&format!("/api/v1/series/{}", imported["id"])).await;
    assert_eq!(detail["folder"], "Old Downloads");
    let chapters = detail["chapters"].as_array().unwrap();
    let one = chapters.iter().find(|c| c["number"] == "1").unwrap();
    assert!(
        one["file_path"]
            .as_str()
            .unwrap()
            .ends_with("Test Series Ch.1.cbz")
    );
    assert_eq!(one["monitored"], false, "on disk, so not wanted");
    let two = chapters.iter().find(|c| c["number"] == "2").unwrap();
    assert_eq!(two["monitored"], true, "missing, so queued");

    // Followed now: no longer a candidate, and can't be imported twice.
    assert_eq!(api.get("/api/v1/import").await.1, json!([]));
    assert_eq!(
        api.call(Method::POST, "/api/v1/import", Some(body)).await.0,
        StatusCode::CONFLICT
    );
    api.app.shutdown().await;
}
