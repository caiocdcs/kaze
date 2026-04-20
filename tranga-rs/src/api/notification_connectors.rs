use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json,
};
use base64::Engine;
use std::collections::HashMap;

use crate::app_state::AppState;
use crate::db::connector_repo::ConnectorRepo;
use crate::error::{AppError, AppResult};
use crate::api::dto::*;
use crate::notification_connectors::NotificationConnector;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/notification-connectors", get(list).put(upsert))
        .route(
            "/notification-connectors/{name}",
            get(get_one).put(upsert_by_name).delete(delete_conn),
        )
        .route("/notification-connectors/{name}/test", axum::routing::post(test_conn))
        .route("/notification-connectors/Gotify", axum::routing::put(create_gotify))
        .route("/notification-connectors/Ntfy", axum::routing::put(create_ntfy))
        .route("/notification-connectors/Pushover", axum::routing::put(create_pushover))
}

async fn list(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let connectors = ConnectorRepo::list_notification_connectors(&state.db).await?;
    Ok(Json(connectors))
}

async fn get_one(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<impl IntoResponse> {
    let connector = ConnectorRepo::get_notification_connector(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "NotificationConnector",
            id: name.clone(),
        })?;
    Ok(Json(connector))
}

async fn upsert(
    State(state): State<AppState>,
    Json(body): Json<CreateNotificationConnector>,
) -> AppResult<Json<NotificationConnectorDto>> {
    let (name, url, http_method, notif_body, headers) = resolve_connector_fields(&body);
    persist_connector(&state, name, url, http_method, notif_body, headers).await
}

async fn upsert_by_name(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<CreateNotificationConnector>,
) -> AppResult<Json<NotificationConnectorDto>> {
    let (_, url, http_method, notif_body, headers) = resolve_connector_fields(&body);
    persist_connector(&state, name, url, http_method, notif_body, headers).await
}

async fn delete_conn(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<impl IntoResponse> {
    if !ConnectorRepo::delete_notification_connector(&state.db, &name).await? {
        return Err(AppError::NotFound {
            entity: "NotificationConnector",
            id: name,
        });
    }
    Ok(StatusCode::OK)
}

async fn test_conn(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<impl IntoResponse> {
    let dto = ConnectorRepo::get_notification_connector(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "NotificationConnector",
            id: name.clone(),
        })?;

    let conn: Box<dyn NotificationConnector> = Box::new(
        crate::notification_connectors::WebhookConnector {
            name: dto.name,
            url: dto.url,
            headers: dto.headers,
            http_method: dto.http_method,
            body: dto.body,
        }
    );
    conn.send_notification("Tranga Test", "Test notification from Tranga")
        .await
        .map_err(crate::error::AppError::Internal)?;
    Ok(StatusCode::OK)
}

async fn create_gotify(
    State(state): State<AppState>,
    Json(body): Json<CreateNotificationConnector>,
) -> AppResult<Json<NotificationConnectorDto>> {
    let name = body.name.clone();
    let url = body.url.clone().unwrap_or_default();
    let app_token = body.app_token.clone().unwrap_or_default();
    let priority = body.priority.unwrap_or(5);

    let gotify_url = {
        let mut parts = url.trim_end_matches('/').to_string();
        if !parts.ends_with("/message") {
            parts.push_str("/message");
        }
        parts
    };

    let notif_body = format!(
        r#"{{"message": "%text", "title": "%title", "priority": {priority}}}"#
    );
    let mut headers = HashMap::new();
    headers.insert("X-Gotify-Key".into(), app_token);

    persist_connector(&state, name, gotify_url, "POST".into(), notif_body, headers).await
}

async fn create_ntfy(
    State(state): State<AppState>,
    Json(body): Json<CreateNotificationConnector>,
) -> AppResult<Json<NotificationConnectorDto>> {
    let name = body.name.clone();
    let url = body.url.clone().unwrap_or_default();
    let priority = body.priority.unwrap_or(3);
    let username = body.username.clone().unwrap_or_default();
    let password = body.password.clone().unwrap_or_default();
    let topic = body.topic.clone().unwrap_or_default();

    let auth_header = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
    );
    let auth = base64::engine::general_purpose::STANDARD.encode(&auth_header)
        .replace("=", "");

    let ntfy_url = if url.contains('?') {
        format!("{url}&auth={auth}")
    } else {
        format!("{url}?auth={auth}")
    };

    let notif_body = format!(
        r#"{{"message": "%text", "title": "%title", "priority": {priority}, "topic": "{topic}"}}"#
    );
    let mut headers = HashMap::new();
    headers.insert("Authorization".into(), auth_header);

    persist_connector(&state, name, ntfy_url, "POST".into(), notif_body, headers).await
}

async fn create_pushover(
    State(state): State<AppState>,
    Json(body): Json<CreateNotificationConnector>,
) -> AppResult<Json<NotificationConnectorDto>> {
    let name = body.name.clone();
    let app_token = body.app_token.clone().unwrap_or_default();
    let user_key = body.username.clone().unwrap_or_default();

    let pushover_url = "https://api.pushover.net/1/messages.json".to_string();
    let notif_body = format!(
        r#"{{"token": "{app_token}", "user": "{user_key}", "message": "%text", "title": "%title"}}"#
    );
    let headers = HashMap::new();

    persist_connector(&state, name, pushover_url, "POST".into(), notif_body, headers).await
}

fn resolve_connector_fields(body: &CreateNotificationConnector) -> (String, String, String, String, HashMap<String, String>) {
    match body.connector_type.to_lowercase().as_str() {
        "gotify" => {
            let url = body.url.clone().unwrap_or_default();
            let app_token = body.app_token.clone().unwrap_or_default();
            let priority = body.priority.unwrap_or(5);
            let gotify_url = {
                let mut parts = url.trim_end_matches('/').to_string();
                if !parts.ends_with("/message") {
                    parts.push_str("/message");
                }
                parts
            };
            let notif_body = format!(
                r#"{{"message": "%text", "title": "%title", "priority": {priority}}}"#
            );
            let mut headers = HashMap::new();
            headers.insert("X-Gotify-Key".into(), app_token);
            (body.name.clone(), gotify_url, "POST".into(), notif_body, headers)
        }
        "ntfy" => {
            let url = body.url.clone().unwrap_or_default();
            let priority = body.priority.unwrap_or(3);
            let username = body.username.clone().unwrap_or_default();
            let password = body.password.clone().unwrap_or_default();
            let topic = body.topic.clone().unwrap_or_default();
            let auth_header = format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
            );
            let auth = base64::engine::general_purpose::STANDARD.encode(&auth_header)
                .replace("=", "");
            let ntfy_url = if url.contains('?') {
                format!("{url}&auth={auth}")
            } else {
                format!("{url}?auth={auth}")
            };
            let notif_body = format!(
                r#"{{"message": "%text", "title": "%title", "priority": {priority}, "topic": "{topic}"}}"#
            );
            let mut headers = HashMap::new();
            headers.insert("Authorization".into(), auth_header);
            (body.name.clone(), ntfy_url, "POST".into(), notif_body, headers)
        }
        "pushover" => {
            let app_token = body.app_token.clone().unwrap_or_default();
            let user_key = body.username.clone().unwrap_or_default();
            let pushover_url = "https://api.pushover.net/1/messages.json".to_string();
            let notif_body = format!(
                r#"{{"token": "{app_token}", "user": "{user_key}", "message": "%text", "title": "%title"}}"#
            );
            (body.name.clone(), pushover_url, "POST".into(), notif_body, HashMap::new())
        }
        _ => {
            let url = body.url.clone().unwrap_or_default();
            let headers = body.headers.clone().unwrap_or_default();
            let http_method = body.http_method.clone().unwrap_or_else(|| "POST".into());
            let notif_body = body.body.clone().unwrap_or_default();
            (body.name.clone(), url, http_method, notif_body, headers)
        }
    }
}

async fn persist_connector(
    state: &AppState,
    name: String,
    url: String,
    http_method: String,
    body: String,
    headers: HashMap<String, String>,
) -> AppResult<Json<NotificationConnectorDto>> {
    let headers_json = serde_json::to_string(&headers).unwrap_or_default();
    let connector = ConnectorRepo::upsert_notification_connector(
        &state.db,
        &name,
        &url,
        &headers_json,
        &http_method,
        &body,
    )
    .await?;
    Ok(Json(connector))
}
