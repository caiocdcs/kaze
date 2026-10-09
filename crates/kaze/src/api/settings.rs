use axum::{Json, extract::State};
use serde_json::{Map, Value};

use super::{ApiResult, AppState};
use crate::settings::SettingsView;

/// Effective settings plus which keys kaze.toml locks.
pub async fn get(State(app): State<AppState>) -> Json<SettingsView> {
    Json(app.ctx.settings.view())
}

/// Change some settings; `null` resets one to its default.
pub async fn update(
    State(app): State<AppState>,
    Json(changes): Json<Map<String, Value>>,
) -> ApiResult<Json<SettingsView>> {
    app.ctx.settings.update(changes).await?;
    Ok(Json(app.ctx.settings.view()))
}
