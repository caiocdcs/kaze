//! A local stand-in server that records requests, shared by every kind's
//! tests, and the checks that span kinds.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
};
use serde_json::{Value, json};

use super::*;

#[derive(Debug, Clone)]
pub(super) struct Seen {
    pub method: Method,
    pub path_and_query: String,
    pub headers: HeaderMap,
    pub body: String,
}

pub(super) type Log = Arc<Mutex<Vec<Seen>>>;

/// Serve `reply(path)` for every request, recording each one.
pub(super) async fn server(
    reply: impl Fn(&str) -> (StatusCode, Value) + Clone + Send + Sync + 'static,
) -> (Url, Log) {
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

pub(super) fn ok(_: &str) -> (StatusCode, Value) {
    (StatusCode::OK, json!({}))
}

pub(super) fn message() -> Message {
    Message {
        trigger: Trigger::Failed,
        title: "Berserk \"374\"".into(),
        body: "line one\nline two".into(),
        count: 1,
        series: vec!["Berserk".into()],
    }
}

#[test]
fn endpoints_keep_a_path_prefix() {
    let base = Url::parse("http://host/komga").unwrap();
    assert_eq!(
        endpoint(&base, "api/v1/libraries").unwrap().as_str(),
        "http://host/komga/api/v1/libraries"
    );
}

#[test]
fn configs_keep_their_flat_json_shape() {
    let json = json!({ "kind": "komga", "url": "http://komga:25600/", "api_key": "k" });
    let config: Config = serde_json::from_value(json.clone()).unwrap();
    assert!(matches!(&config, Config::Komga(k) if k.api_key == "k" && k.library_id.is_none()));
    let mut back = serde_json::to_value(&config).unwrap();
    back.as_object_mut().unwrap().remove("library_id");
    assert_eq!(back, json);
}

#[tokio::test]
async fn library_servers_get_no_messages() {
    let config = Config::Komga(komga::Komga {
        url: Url::parse("http://komga:25600").unwrap(),
        api_key: "k".into(),
        library_id: None,
    });
    let err = send(&reqwest::Client::new(), &config, &message())
        .await
        .unwrap_err();
    assert!(!err.is_retryable());
}

#[test]
fn secrets_are_masked_and_kept_when_sent_back() {
    let stored = Config::Webhook(webhook::Webhook {
        url: Url::parse("https://x.test").unwrap(),
        method: WebhookMethod::Post,
        headers: BTreeMap::from([
            ("Authorization".into(), "Bearer s3cret".into()),
            ("Content-Type".into(), "application/json".into()),
        ]),
        body: "{}".into(),
    });
    let mut masked = stored.masked();
    let Config::Webhook(hook) = &masked else {
        unreachable!()
    };
    assert_eq!(hook.headers["Authorization"], MASK);
    assert_eq!(hook.headers["Content-Type"], "application/json");
    masked.keep_secrets(&stored);
    assert_eq!(masked, stored);
}

#[test]
fn a_new_secret_replaces_the_stored_one() {
    let komga = |key: &str| {
        Config::Komga(komga::Komga {
            url: Url::parse("http://komga:25600").unwrap(),
            api_key: key.into(),
            library_id: None,
        })
    };
    let mut changed = komga("new");
    changed.keep_secrets(&komga("old"));
    assert_eq!(changed, komga("new"));
    // An empty secret stays empty rather than turning into a mask.
    assert_eq!(komga("").masked(), komga(""));
}
