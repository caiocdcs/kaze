use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use ts_rs::TS;

use super::{ApiError, ApiResult, AppState};
use crate::{
    db::{
        self,
        integrations::{Integration, NewIntegration},
    },
    integrations::{self, Config, Preset, Trigger},
};

/// What the UI sends to create or replace an integration.
#[derive(Deserialize, TS)]
#[ts(export)]
pub struct IntegrationInput {
    name: String,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
    config: Config,
    #[serde(default)]
    events: Vec<Trigger>,
}

const fn enabled_by_default() -> bool {
    true
}

impl IntegrationInput {
    fn validated(&self) -> ApiResult<NewIntegration<'_>> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(ApiError::BadRequest("give the connection a name".into()));
        }
        self.config.validate().map_err(ApiError::BadRequest)?;
        Ok(NewIntegration {
            name,
            enabled: self.enabled,
            config: &self.config,
            events: &self.events,
        })
    }
}

pub async fn list(State(app): State<AppState>) -> ApiResult<Json<Vec<Integration>>> {
    Ok(Json(db::integrations::list(&app.ctx.db).await?))
}

pub async fn presets() -> Json<Vec<Preset>> {
    Json(integrations::presets())
}

async fn load(app: &AppState, id: i64) -> ApiResult<Integration> {
    db::integrations::get(&app.ctx.db, id)
        .await?
        .ok_or(ApiError::NotFound("integration"))
}

pub async fn create(
    State(app): State<AppState>,
    Json(input): Json<IntegrationInput>,
) -> ApiResult<(StatusCode, Json<Integration>)> {
    let id = db::integrations::insert(&app.ctx.db, &input.validated()?).await?;
    Ok((StatusCode::CREATED, Json(load(&app, id).await?)))
}

pub async fn replace(
    State(app): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<IntegrationInput>,
) -> ApiResult<Json<Integration>> {
    if !db::integrations::update(&app.ctx.db, id, &input.validated()?).await? {
        return Err(ApiError::NotFound("integration"));
    }
    Ok(Json(load(&app, id).await?))
}

pub async fn remove(State(app): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if db::integrations::delete(&app.ctx.db, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("integration"))
    }
}

/// Test a saved integration and remember the outcome.
pub async fn test_saved(
    State(app): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Integration>> {
    let integration = load(&app, id).await?;
    let result = integrations::test(&app.ctx.http, &integration.config).await;
    let status = match &result {
        Ok(()) => "Test passed".to_owned(),
        Err(err) => err.to_string(),
    };
    db::integrations::record(&app.ctx.db, id, result.is_ok(), &status).await?;
    result.map_err(|e| ApiError::Integration(e.to_string()))?;
    Ok(Json(load(&app, id).await?))
}

/// Test settings before saving them.
pub async fn test_unsaved(
    State(app): State<AppState>,
    Json(config): Json<Config>,
) -> ApiResult<StatusCode> {
    config.validate().map_err(ApiError::BadRequest)?;
    integrations::test(&app.ctx.http, &config)
        .await
        .map_err(|e| ApiError::Integration(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}
