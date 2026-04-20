use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch},
    Json,
};

use crate::app_state::AppState;
use crate::db::connector_repo::ConnectorRepo;
use crate::error::{AppError, AppResult};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/connectors", get(list))
        .route("/connectors/{connector_name}", get(get_connector))
        .route("/connectors/enabled", get(list_enabled))
        .route("/connectors/enabled/{enabled}", get(list_by_enabled))
        .route("/connectors/{connector_name}/enabled", patch(toggle_enabled))
}

async fn list(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let connectors = state.connectors.read().await;
    Ok(Json(connectors.all_info()))
}

async fn get_connector(
    State(state): State<AppState>,
    Path(connector_name): Path<String>,
) -> AppResult<impl IntoResponse> {
    let connectors = state.connectors.read().await;
    let info = connectors.get(&connector_name)
        .ok_or_else(|| AppError::ConnectorNotFound { name: connector_name.clone() })?;
    Ok(Json(info.to_info()))
}

async fn list_enabled(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let connectors = state.connectors.read().await;
    Ok(Json(connectors.enabled_info()))
}

async fn list_by_enabled(
    State(state): State<AppState>,
    Path(enabled): Path<bool>,
) -> AppResult<impl IntoResponse> {
    let connectors = state.connectors.read().await;
    let filtered: Vec<_> = connectors.all_info().into_iter()
        .filter(|c| c.enabled == enabled)
        .collect();
    Ok(Json(filtered))
}

async fn toggle_enabled(
    State(state): State<AppState>,
    Path(connector_name): Path<String>,
    body: axum::body::Bytes,
) -> AppResult<impl IntoResponse> {
    let val: bool = serde_json::from_slice(&body)
        .map_err(|_| AppError::BadRequest {
            detail: "invalid boolean value".into(),
        })?;

    {
        let mut connectors = state.connectors.write().await;
        let connector = connectors
            .get_mut(&connector_name)
            .ok_or_else(|| AppError::ConnectorNotFound {
                name: connector_name.clone(),
            })?;

        connector.set_enabled(val);
    }

    ConnectorRepo::set_manga_connector_enabled(&state.db, &connector_name, val).await?;

    Ok(StatusCode::OK)
}
