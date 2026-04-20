use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json,
};
use std::str::FromStr;

use crate::app_state::AppState;
use crate::error::{AppError, AppResult};
use crate::id::WorkerId;
use crate::api::dto::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workers", get(list))
        .route("/workers/state/{state}", get(list_by_state))
        .route("/workers/{id}", get(get_one).delete(cancel))
        .route("/workers/{id}/stop", post(stop_worker))
}

async fn list(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let scheduler = state.scheduler.lock().await;
    let workers: Vec<WorkerInfo> = scheduler
        .list()
        .into_iter()
        .map(|e| e.to_info())
        .collect();
    Ok(Json(workers))
}

async fn list_by_state(
    State(state): State<AppState>,
    Path(state_filter): Path<i32>,
) -> AppResult<impl IntoResponse> {
    let scheduler = state.scheduler.lock().await;
    let workers: Vec<WorkerInfo> = scheduler
        .list()
        .into_iter()
        .filter(|e| e.state as i32 == state_filter)
        .map(|e| e.to_info())
        .collect();
    Ok(Json(workers))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let wid = WorkerId::from_str(&id).map_err(|_| AppError::BadRequest {
        detail: format!("invalid worker id: {id}"),
    })?;
    let scheduler = state.scheduler.lock().await;
    let entry = scheduler.get(&wid);
    match entry {
        Some(e) => Ok(Json(e.to_info())),
        None => Err(AppError::NotFound {
            entity: "Worker",
            id,
        }),
    }
}

async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let wid = WorkerId::from_str(&id).map_err(|_| AppError::BadRequest {
        detail: format!("invalid worker id: {id}"),
    })?;
    let mut scheduler = state.scheduler.lock().await;
    if scheduler.cancel(&wid) {
        Ok(StatusCode::OK)
    } else {
        Err(AppError::NotFound {
            entity: "Worker",
            id,
        })
    }
}

async fn stop_worker(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let wid = WorkerId::from_str(&id).map_err(|_| AppError::BadRequest {
        detail: format!("invalid worker id: {id}"),
    })?;
    let mut scheduler = state.scheduler.lock().await;
    if scheduler.cancel(&wid) {
        Ok(StatusCode::OK)
    } else {
        Err(AppError::NotFound {
            entity: "Worker",
            id,
        })
    }
}
