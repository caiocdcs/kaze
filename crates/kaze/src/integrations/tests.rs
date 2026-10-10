//! Integrations against a local stand-in server that records requests.

use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
};
use serde_json::{Value, json};

use super::*;

#[derive(Debug, Clone)]
struct Seen {
    method: Method,
    path_and_query: String,
    headers: HeaderMap,
    body: String,
}

/// Serve `reply(path)` for every request, recording each one.
async fn server(
    reply: impl Fn(&str) -> (StatusCode, Value) + Clone + Send + Sync + 'static,
) -> (Url, Arc<Mutex<Vec<Seen>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let app = Router::new().fallback(
        move |method: Method, uri: Uri, headers: HeaderMap, body: Bytes| {
            let reply = reply.clone();
            let log = Arc::clone(&log);
            async move {
                log.lock().unwrap().push(Seen {
                    method,
                    path_and_query: uri.path_and_query().unwrap().to_string(),
                    headers,
                    body: String::from_utf8_lossy(&body).into_owned(),
                });
                let (status, json) = reply(uri.path());
                (status, axum::Json(json)).into_response()
            }
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (Url::parse(&format!("http://{addr}/")).unwrap(), seen)
}

fn ok(_: &str) -> (StatusCode, Value) {
    (StatusCode::OK, json!({}))
}

fn message() -> Message {
    Message {
        trigger: Trigger::Failed,
        title: "Berserk \"374\"".into(),
        body: "line one\nline two".into(),
        count: 1,
        series: vec!["Berserk".into()],
    }
}

#[test]
fn every_preset_renders_valid_json() {
    for preset in presets() {
        let Config::Webhook { body, .. } = &preset.config else {
            panic!("{} is not a webhook", preset.id);
        };
        let rendered = render(body, &message()).unwrap();
        let parsed: Value = serde_json::from_str(&rendered)
            .unwrap_or_else(|e| panic!("{}: {e}\n{rendered}", preset.id));
        assert!(
            rendered.contains(r#"Berserk \"374\""#),
            "{}: title escaped",
            preset.id
        );
        assert!(parsed.is_object());
        preset.config.validate().unwrap();
    }
}

#[test]
fn validation_catches_bad_templates_and_urls() {
    let webhook = |url: &str, body: &str| Config::Webhook {
        url: Url::parse(url).unwrap(),
        method: WebhookMethod::Post,
        headers: BTreeMap::new(),
        body: body.into(),
    };
    assert!(webhook("https://x.test", "{{ title").validate().is_err());
    assert!(webhook("ftp://x.test", "{}").validate().is_err());
    assert!(
        webhook("https://x.test", "{{ title | tojson }}")
            .validate()
            .is_ok()
    );
    // Unknown variables fail at render time, not silently as "".
    assert!(render("{{ nope }}", &message()).is_err());
}

#[test]
fn endpoints_keep_a_path_prefix() {
    let base = Url::parse("http://host/komga").unwrap();
    assert_eq!(
        endpoint(&base, "api/v1/libraries").unwrap().as_str(),
        "http://host/komga/api/v1/libraries"
    );
}

#[tokio::test]
async fn webhook_posts_the_rendered_template() {
    let (url, seen) = server(ok).await;
    let config = Config::Webhook {
        url: url.join("hook").unwrap(),
        method: WebhookMethod::Put,
        headers: BTreeMap::from([("X-Title".into(), "{{ title }}".into())]),
        body: r#"{"text": {{ message | tojson }}, "event": "{{ event }}"}"#.into(),
    };
    send(&Client::new(), &config, &message()).await.unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].method, Method::PUT);
    assert_eq!(seen[0].path_and_query, "/hook");
    assert_eq!(seen[0].headers["x-title"], "Berserk \"374\"");
    assert_eq!(seen[0].headers["content-type"], "application/json");
    let body: Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(
        body,
        json!({ "text": "line one\nline two", "event": "failed" })
    );
}

#[tokio::test]
async fn apprise_posts_to_the_notify_key() {
    let (url, seen) = server(ok).await;
    let config = Config::Apprise {
        url,
        key: Some("kaze".into()),
        urls: None,
        tags: Some("phone".into()),
    };
    send(&Client::new(), &config, &message()).await.unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path_and_query, "/notify/kaze");
    let body: Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["type"], "failure");
    assert_eq!(body["tag"], "phone");
}

#[tokio::test]
async fn apprise_sends_urls_statelessly() {
    let (url, seen) = server(ok).await;
    let config = Config::Apprise {
        url,
        key: None,
        urls: Some("ntfy://ntfy.sh/kaze-test".into()),
        tags: None,
    };
    send(&Client::new(), &config, &message()).await.unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path_and_query, "/notify/");
    let body: Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["urls"], "ntfy://ntfy.sh/kaze-test");
    assert_eq!(body["title"], message().title);
}

#[test]
fn apprise_needs_a_key_or_urls() {
    let config = Config::Apprise {
        url: Url::parse("http://apprise:8000").unwrap(),
        key: Some("  ".into()),
        urls: None,
        tags: None,
    };
    assert!(config.validate().is_err());
}

#[tokio::test]
async fn komga_scans_every_library_when_none_is_chosen() {
    let (url, seen) = server(|path| match path {
        "/api/v1/libraries" => (StatusCode::OK, json!([{ "id": "A" }, { "id": "B" }])),
        _ => (StatusCode::ACCEPTED, json!({})),
    })
    .await;
    let config = Config::Komga {
        url,
        api_key: "secret".into(),
        library_id: None,
    };
    scan(&Client::new(), &config).await.unwrap();
    let seen = seen.lock().unwrap();
    let paths: Vec<_> = seen.iter().map(|s| s.path_and_query.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/api/v1/libraries",
            "/api/v1/libraries/A/scan",
            "/api/v1/libraries/B/scan"
        ]
    );
    assert!(seen.iter().all(|s| s.headers["x-api-key"] == "secret"));
}

#[tokio::test]
async fn kavita_authenticates_then_scans() {
    let (url, seen) = server(|path| match path {
        "/api/Plugin/authenticate" => (StatusCode::OK, json!({ "token": "jwt" })),
        _ => (StatusCode::OK, json!({})),
    })
    .await;
    let config = Config::Kavita {
        url,
        api_key: "k".into(),
        library_id: Some(3),
    };
    scan(&Client::new(), &config).await.unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen[0].path_and_query,
        "/api/Plugin/authenticate?apiKey=k&pluginName=kaze"
    );
    assert_eq!(seen[1].path_and_query, "/api/Library/scan?libraryId=3");
    assert_eq!(seen[1].headers["authorization"], "Bearer jwt");
}

#[tokio::test]
async fn server_errors_retry_client_errors_do_not() {
    let (url, _) = server(|path| match path {
        "/busy" => (StatusCode::SERVICE_UNAVAILABLE, json!({})),
        _ => (StatusCode::UNAUTHORIZED, json!({ "error": "bad key" })),
    })
    .await;
    let hook = |path: &str| Config::Webhook {
        url: url.join(path).unwrap(),
        method: WebhookMethod::Post,
        headers: BTreeMap::new(),
        body: "{}".into(),
    };
    let client = Client::new();
    let busy = send(&client, &hook("busy"), &message()).await.unwrap_err();
    assert!(busy.is_retryable());
    let denied = send(&client, &hook("denied"), &message())
        .await
        .unwrap_err();
    assert!(!denied.is_retryable());
    assert!(denied.to_string().contains("401"), "{denied}");
}
