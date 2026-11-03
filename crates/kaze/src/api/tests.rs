//! Requests through the full router against an in-memory app.

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
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
    api_with(api_key, db::memory().await).await
}

async fn api_with(api_key: Option<&str>, db: sqlx::SqlitePool) -> Api {
    api_full(api_key, db, crate::testing::empty_anilist().await).await
}

async fn api_full(
    api_key: Option<&str>,
    db: sqlx::SqlitePool,
    anilist: kaze_sources::anilist::AniList,
) -> Api {
    let library = tempfile::tempdir().unwrap();
    let source = Arc::new(FakeSource::new().with_chapters(&["1", "2"]));
    let other = Arc::new(
        FakeSource::new()
            .with_id("other", "Other")
            .with_chapters(&["1", "2", "3"]),
    );
    let locked = match api_key {
        Some(key) => json!({ "api_key": key }),
        None => json!({}),
    };
    let ctx = Context {
        settings: SettingsStore::load(db.clone(), locked.as_object().unwrap().clone())
            .await
            .unwrap(),
        db,
        sources: kaze_sources::Registry::from_sources(vec![source, other]),
        events: Events::new(256),
        libraries: crate::library::Libraries::single(library.path().to_owned()),
        http: reqwest::Client::new(),
        flaresolverr: kaze_sources::http::Http::new(kaze_sources::http::HttpConfig::default())
            .unwrap()
            .flaresolverr(),
        anilist,
    };
    let app = Arc::new(App::start(ctx, RetryPolicy::default()).await.unwrap());
    Api {
        router: router(Arc::clone(&app), &crate::config::UrlBase::default()),
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
async fn backup_is_a_database_file() {
    // `VACUUM INTO` needs a file database.
    let dir = tempfile::tempdir().unwrap();
    let db = db::connect(&dir.path().join("kaze.db")).await.unwrap();
    let api = api_with(None, db).await;
    let response = api
        .router
        .clone()
        .oneshot(Request::get("/api/v1/backup").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let disposition = response.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(disposition.starts_with("attachment; filename=\"kaze-"));
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.starts_with(b"SQLite format 3\0"));
}

#[tokio::test]
async fn events_start_right_away() {
    let api = api(None).await;
    let response = api
        .router
        .clone()
        .oneshot(Request::get("/api/v1/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let mut body = response.into_body();
    let first = tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .expect("no bytes before the keep-alive")
        .unwrap()
        .unwrap();
    assert!(first.into_data().unwrap().starts_with(b":"));
}

#[tokio::test]
async fn health_and_sources() {
    let api = api(None).await;
    assert_eq!(api.get("/healthz").await.0, StatusCode::OK);
    let (status, sources) = api.get("/api/v1/sources").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sources[0]["id"], "fake");
    assert_eq!(sources[0]["failing"], false);
    assert_eq!(sources[0]["health"]["failures_in_row"], 0);
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
    let discord = presets
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "discord")
        .unwrap();

    let body = json!({ "name": "Phone", "config": discord["config"], "events": ["downloaded"] });
    let (status, created) = api
        .call(Method::POST, "/api/v1/integrations", Some(body))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["enabled"], true);
    assert_eq!(created["config"]["kind"], "webhook");
    let uri = format!("/api/v1/integrations/{}", created["id"]);

    let bad = json!({ "name": " ", "config": discord["config"] });
    assert_eq!(
        api.call(Method::PUT, &uri, Some(bad)).await.0,
        StatusCode::BAD_REQUEST
    );
    let paused = json!({ "name": "Phone", "enabled": false, "config": discord["config"] });
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
async fn integration_secrets_stay_on_the_server() {
    let api = api(None).await;
    let config =
        |key: &str| json!({ "kind": "komga", "url": "http://komga:25600", "api_key": key });
    let body = json!({ "name": "Komga", "config": config("s3cret") });
    let (_, created) = api
        .call(Method::POST, "/api/v1/integrations", Some(body))
        .await;
    assert_eq!(created["config"]["api_key"], crate::integrations::MASK);
    let id = created["id"].as_i64().unwrap();
    let uri = format!("/api/v1/integrations/{id}");
    let stored = || async {
        let integration = db::integrations::get(&api.app.ctx.db, id)
            .await
            .unwrap()
            .unwrap();
        let crate::integrations::Config::Komga(komga) = integration.config.0 else {
            unreachable!()
        };
        komga.api_key
    };

    // Toggling it off sends the masked config back: the key survives.
    let paused = json!({ "name": "Komga", "enabled": false, "config": created["config"] });
    let (status, updated) = api.call(Method::PUT, &uri, Some(paused)).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["config"]["api_key"], crate::integrations::MASK);
    assert_eq!(stored().await, "s3cret");

    let rekeyed = json!({ "name": "Komga", "config": config("fresh") });
    api.call(Method::PUT, &uri, Some(rekeyed)).await;
    assert_eq!(stored().await, "fresh");
    api.app.shutdown().await;
}

#[tokio::test]
async fn existing_folders_can_be_imported() {
    let api = api(None).await;
    let folder = api.app.ctx.libraries.root("en").join("Old Downloads");
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
        json!([{
            "folder": "Old Downloads", "files": 1, "chapters": ["1"], "unparsed": 0, "language": null
        }])
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

#[tokio::test]
async fn series_sources_can_be_added_reordered_and_removed() {
    let api = api(None).await;
    let (_, created) = api
        .call(
            Method::POST,
            "/api/v1/series",
            Some(json!({ "source": "fake", "key": "S1" })),
        )
        .await;
    let id = created["id"].as_i64().unwrap();
    let sources = format!("/api/v1/series/{id}/sources");
    let order = |detail: &Value| -> Vec<String> {
        detail["sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["source"].as_str().unwrap().to_owned())
            .collect()
    };

    let add = json!({ "source": "other", "key": "S1" });
    let (status, detail) = api.call(Method::POST, &sources, Some(add.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(order(&detail), ["fake", "other"]);
    assert_eq!(detail["sources"][1]["priority"], 1);
    assert_eq!(detail["sources"][1]["failing"], false);
    let (status, _) = api.call(Method::POST, &sources, Some(add)).await;
    assert_eq!(status, StatusCode::CONFLICT, "already a source");

    // Not a permutation of the series' sources.
    let (status, _) = api
        .call(Method::PATCH, &sources, Some(json!({ "order": ["other"] })))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, detail) = api
        .call(
            Method::PATCH,
            &sources,
            Some(json!({ "order": ["other", "fake"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(order(&detail), ["other", "fake"]);
    assert_eq!(detail["source"], "other", "the new primary");

    let (status, detail) = api
        .call(Method::DELETE, &format!("{sources}/other"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(order(&detail), ["fake"]);
    assert_eq!(detail["sources"][0]["priority"], 0);
    let (status, _) = api
        .call(Method::DELETE, &format!("{sources}/fake"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "the last source stays");
    let (status, _) = api
        .call(Method::DELETE, &format!("{sources}/nope"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    api.app.shutdown().await;
}

#[tokio::test]
async fn chapter_copies_across_sources() {
    let api = api(None).await;
    let (_, created) = api
        .call(
            Method::POST,
            "/api/v1/series",
            Some(json!({ "source": "fake", "key": "S1" })),
        )
        .await;
    let id = created["id"].as_i64().unwrap();
    api.call(
        Method::POST,
        &format!("/api/v1/series/{id}/sources"),
        Some(json!({ "source": "other", "key": "S1" })),
    )
    .await;

    // Wait for both chapter lists and chapter 1's download from "fake".
    let mut copies = Value::Null;
    for _ in 0..100 {
        let detail = api.get(&format!("/api/v1/series/{id}")).await.1;
        if let Some(first) = detail["chapters"]
            .as_array()
            .and_then(|c| c.iter().find(|c| c["number"] == "1"))
        {
            copies = api
                .get(&format!("/api/v1/chapters/{}/copies", first["id"]))
                .await
                .1;
            if copies.as_array().is_some_and(|c| c.len() == 2) && copies[0]["on_disk"] == true {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(copies[0]["source"], "fake");
    assert_eq!(copies[0]["picked"], true);
    assert_eq!(copies[0]["on_disk"], true);
    assert_eq!(copies[1]["source"], "other");
    assert_eq!(copies[1]["picked"], false);

    // The number is on disk: another copy isn't downloaded over it.
    let (status, _) = api
        .call(
            Method::POST,
            &format!("/api/v1/chapters/{}/download", copies[1]["id"]),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        api.get("/api/v1/chapters/999999/copies").await.0,
        StatusCode::NOT_FOUND
    );
    api.app.shutdown().await;
}

#[tokio::test]
async fn scanlator_preferences_are_saved_and_checked() {
    let api = api(None).await;
    let (_, created) = api
        .call(
            Method::POST,
            "/api/v1/series",
            Some(json!({ "source": "fake", "key": "S1" })),
        )
        .await;
    let uri = format!("/api/v1/series/{}", created["id"]);

    let (status, detail) = api
        .call(
            Method::PATCH,
            &uri,
            Some(json!({ "preferred_scanlators": [" Knox ", "knox"], "blocked_scanlators": ["Asura"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["preferred_scanlators"], json!(["Knox"]));
    assert_eq!(detail["blocked_scanlators"], json!(["Asura"]));
    assert_eq!(
        detail["scanlators"],
        json!([]),
        "the fake source names no groups"
    );

    // Only one list given: the other is kept, and checked against.
    let (status, body) = api
        .call(
            Method::PATCH,
            &uri,
            Some(json!({ "blocked_scanlators": ["KNOX"] })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("both"));
    let (_, detail) = api.get(&uri).await;
    assert_eq!(detail["blocked_scanlators"], json!(["Asura"]));
    api.app.shutdown().await;
}

#[tokio::test]
async fn upcoming_guesses_the_next_chapter() {
    let api = api(None).await;
    let (_, created) = api
        .call(
            Method::POST,
            "/api/v1/series",
            Some(json!({ "source": "fake", "key": "S1" })),
        )
        .await;
    let id = created["id"].as_i64().unwrap();
    for _ in 0..50 {
        let detail = api.get(&format!("/api/v1/series/{id}")).await.1;
        if detail["chapters"].as_array().is_some_and(|c| c.len() == 2) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        api.get("/api/v1/upcoming").await.1,
        json!([]),
        "no dates yet"
    );

    // Chapters 1 and 2 came out two weeks and one week ago.
    sqlx::query(
        "UPDATE chapters SET published_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', (CASE number WHEN '1' THEN -14 ELSE -7 END) || ' days')",
    )
    .execute(&api.app.ctx.db)
    .await
    .unwrap();
    let (status, list) = api.get("/api/v1/upcoming").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["series_id"], id);
    assert_eq!(list[0]["cadence_days"], 7);
    assert_eq!(list[0]["state"], "expected");
    api.app.shutdown().await;
}

#[tokio::test]
async fn a_source_followed_as_its_own_series_is_not_merged() {
    let api = api(None).await;
    let mut ids = Vec::new();
    for source in ["fake", "other"] {
        let (_, created) = api
            .call(
                Method::POST,
                "/api/v1/series",
                Some(json!({ "source": source, "key": "S1" })),
            )
            .await;
        ids.push(created["id"].as_i64().unwrap());
    }
    let (status, body) = api
        .call(
            Method::POST,
            &format!("/api/v1/series/{}/sources", ids[0]),
            Some(json!({ "source": "other", "key": "S1" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains(&ids[1].to_string())
    );
    api.app.shutdown().await;
}

#[tokio::test]
async fn one_series_in_two_languages() {
    let api = api(None).await;
    let follow = |language: &str| json!({ "source": "fake", "key": "S1", "language": language });
    let (status, en) = api
        .call(Method::POST, "/api/v1/series", Some(follow("en")))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{en}");
    let (status, pt) = api
        .call(Method::POST, "/api/v1/series", Some(follow("pt-br")))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{pt}");
    assert_ne!(en["id"], pt["id"]);

    let (_, detail) = api.get(&format!("/api/v1/series/{}", pt["id"])).await;
    assert_eq!(detail["language"], "pt-BR");
    assert_eq!(detail["folder"], "Test Series (Fake)");

    let (status, _) = api
        .call(Method::POST, "/api/v1/series", Some(follow("pt-BR")))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, error) = api
        .call(Method::POST, "/api/v1/series", Some(follow("fr")))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "Fake has no fr; it offers en, pt-BR");

    let (_, lookup) = api
        .get("/api/v1/lookup?url=https://fake.test/series/S1&language=pt-BR")
        .await;
    assert_eq!(lookup["followed_id"], pt["id"]);
    api.app.shutdown().await;
}

async fn imported_series(api: &Api, folder: &str) -> (i64, std::path::PathBuf) {
    let dir = api.app.ctx.libraries.root("en").join(folder);
    std::fs::create_dir(&dir).unwrap();
    let file = std::fs::File::create(dir.join("Test Series Ch.1.cbz")).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("001.jpg", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.finish().unwrap();
    let body = json!({ "folder": folder, "source": "fake", "key": "S1", "monitor": "none" });
    let (status, imported) = api.call(Method::POST, "/api/v1/import", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    (imported["id"].as_i64().unwrap(), dir)
}

#[tokio::test]
async fn chapter_files_can_be_deleted() {
    let api = api(None).await;
    let (id, dir) = imported_series(&api, "Test Series").await;
    let (_, detail) = api.get(&format!("/api/v1/series/{id}")).await;
    let one = detail["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["number"] == "1")
        .unwrap()["id"]
        .clone();

    let body = json!({ "ids": [one], "unmonitor": true });
    let (status, result) = api
        .call(Method::POST, "/api/v1/chapters/delete-files", Some(body))
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["deleted"], 1);
    assert!(!dir.join("Test Series Ch.1.cbz").exists());

    let (_, detail) = api.get(&format!("/api/v1/series/{id}")).await;
    let chapter = detail["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == one)
        .unwrap()
        .clone();
    assert_eq!(chapter["file_path"], Value::Null);
    assert_eq!(chapter["monitored"], false);
    api.app.shutdown().await;
}

#[tokio::test]
async fn unfollowing_deletes_the_folder_only_when_asked() {
    let api = api(None).await;
    let (id, dir) = imported_series(&api, "Kept").await;
    let (status, _) = api
        .call(Method::DELETE, &format!("/api/v1/series/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(dir.exists());

    let (id, dir) = imported_series(&api, "Removed").await;
    let (status, _) = api
        .call(
            Method::DELETE,
            &format!("/api/v1/series/{id}?delete_files=true"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!dir.exists());
    assert!(api.app.ctx.libraries.root("en").exists());
    api.app.shutdown().await;
}

/// Writes `docs/openapi.json` (`just types`); `just check` fails when it
/// changed without being committed.
#[test]
fn export_openapi() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/openapi.json");
    let json = super::openapi().to_pretty_json().unwrap();
    // The Nix build's source has no docs/.
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, json + "\n").unwrap();
}

#[tokio::test]
async fn the_spec_is_served_without_the_api_key() {
    let api = api(Some("a-long-secret")).await;
    assert_eq!(api.get("/api/v1/series").await.0, StatusCode::UNAUTHORIZED);
    let (status, spec) = api.get("/api/v1/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    assert!(spec["openapi"].as_str().unwrap().starts_with("3."));
    let series = &spec["paths"]["/api/v1/series/{id}"];
    assert!(series["get"].is_object() && series["patch"].is_object());
    assert!(spec["paths"]["/api/v1/events"]["get"].is_object());
    let schemes = &spec["components"]["securitySchemes"];
    assert_eq!(schemes["api_key_header"]["name"], "X-Api-Key");
    assert_eq!(schemes["api_key_query"]["in"], "query");
    api.app.shutdown().await;
}

/// Whether the series' title becomes `expected` within 5 seconds.
async fn shows_title(api: &Api, id: i64, expected: &str) -> bool {
    let uri = format!("/api/v1/series/{id}");
    tokio::time::timeout(Duration::from_secs(5), async {
        while api.get(&uri).await.1["title"] != expected {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn metadata_is_searched_and_linked_by_hand() {
    let anilist = crate::testing::anilist_with("From AniList").await;
    let api = api_full(None, db::memory().await, anilist).await;

    let (status, hits) = api.get("/api/v1/metadata/search?q=anything").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hits[0]["anilist_id"], 42);
    assert_eq!(hits[0]["title"], "From AniList");
    assert_eq!(hits[0]["romaji"], Value::Null, "the same as the title");
    assert_eq!(hits[0]["one_shot"], false);
    assert_eq!(hits[0]["url"], "https://anilist.co/manga/42");
    let (status, _) = api.get("/api/v1/metadata/search?q=%20").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, created) = api
        .call(
            Method::POST,
            "/api/v1/series",
            Some(json!({ "source": "fake", "key": "S1" })),
        )
        .await;
    let id = created["id"].as_i64().unwrap();
    let metadata = format!("/api/v1/series/{id}/metadata");
    let (status, _) = api
        .call(Method::PUT, &metadata, Some(json!({ "anilist_id": 7 })))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "not on AniList");

    let (status, detail) = api
        .call(Method::PUT, &metadata, Some(json!({ "anilist_id": 42 })))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["anilist_id"], 42);
    assert_eq!(detail["metadata_link"], "manual");
    assert!(shows_title(&api, id, "From AniList").await);

    let (status, detail) = api
        .call(Method::PUT, &metadata, Some(json!({ "anilist_id": null })))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["anilist_id"], Value::Null);
    assert_eq!(detail["metadata_link"], "manual");
    assert!(
        shows_title(&api, id, "Test Series").await,
        "unlinking brings the source's metadata back"
    );

    let (status, _) = api
        .call(
            Method::PUT,
            "/api/v1/series/999/metadata",
            Some(json!({ "anilist_id": null })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    api.app.shutdown().await;
}

#[tokio::test]
async fn everything_moves_under_the_url_base() {
    let api = api(None).await;
    let base = crate::config::UrlBase::try_from("/kaze".to_owned()).unwrap();
    let routed = router(Arc::clone(&api.app), &base);
    let get = |uri: &'static str| {
        let routed = routed.clone();
        async move {
            routed
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap()
        }
    };
    assert_eq!(get("/kaze/api/v1/sources").await.status(), StatusCode::OK);
    assert_eq!(get("/api/v1/sources").await.status(), StatusCode::NOT_FOUND);
    let home = get("/").await;
    assert_eq!(home.status(), StatusCode::SEE_OTHER);
    assert_eq!(home.headers()[header::LOCATION], "/kaze");
    assert_eq!(get("/kaze/").await.headers()[header::LOCATION], "/kaze");
    assert_eq!(get("/kaze").await.status(), StatusCode::OK, "the UI");
    assert_eq!(get("/kaze/series/1").await.status(), StatusCode::OK);
    assert_eq!(get("/healthz").await.status(), StatusCode::OK, "for probes");
    assert_eq!(get("/kaze/healthz").await.status(), StatusCode::OK);
    api.app.shutdown().await;
}
