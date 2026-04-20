use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json,
};

use crate::app_state::AppState;
use crate::db::connector_repo::ConnectorRepo;
use crate::error::{AppError, AppResult};
use crate::id::{EntityId, LibraryConnectorId};
use crate::api::dto::*;
use crate::library_connectors::LibraryConnector;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library-connectors", get(list).post(create))
        .route(
            "/library-connectors/{id}",
            get(get_one).delete(delete_conn).post(refresh),
        )
}

async fn list(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let connectors = ConnectorRepo::list_library_connectors(&state.db).await?;
    Ok(Json(connectors))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let connector = ConnectorRepo::get_library_connector(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "LibraryConnector",
            id: id.clone(),
        })?;
    Ok(Json(connector))
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateLibraryConnector>,
) -> AppResult<impl IntoResponse> {
    let key = LibraryConnectorId::new();
    let connector = ConnectorRepo::create_library_connector(
        &state.db,
        &key.to_string(),
        &body.library_type,
        &body.url,
        &body.api_key,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(connector)))
}

async fn delete_conn(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    if !ConnectorRepo::delete_library_connector(&state.db, &id).await? {
        return Err(AppError::NotFound {
            entity: "LibraryConnector",
            id,
        });
    }
    Ok(StatusCode::OK)
}

async fn refresh(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let connector = ConnectorRepo::get_library_connector(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "LibraryConnector",
            id: id.clone(),
        })?;

    let result = match connector.library_type.as_str() {
        "Komga" => {
            let conn =
                crate::library_connectors::KomgaConnector::new(&connector.base_url, &connector.auth);
            conn.update_library(state.shutdown_token.clone()).await
        }
        "Kavita" => {
            let conn =
                crate::library_connectors::KavitaConnector::new(&connector.base_url, &connector.auth);
            conn.update_library(state.shutdown_token.clone()).await
        }
        _ => Err(anyhow::anyhow!(
            "unknown library type: {}",
            connector.library_type
        )),
    };

    result.map_err(crate::error::AppError::Internal)?;
    Ok(StatusCode::OK)
}
