use axum::{Json, extract::State};
use serde_json::{Map, Value};

use super::{ApiResult, AppState, ErrorBody};
use crate::settings::SettingsView;

/// Effective settings plus which keys kaze.toml locks.
#[utoipa::path(
    operation_id = "settings_get",
    get,
    path = "/settings",
    tag = "settings",
    responses((status = 200, description = "OK", body = SettingsView))
)]
pub async fn get(State(app): State<AppState>) -> Json<SettingsView> {
    Json(app.ctx.settings.view())
}

/// Change some settings; `null` resets one to its default.
#[utoipa::path(
    operation_id = "settings_update",
    patch,
    path = "/settings",
    tag = "settings",
    request_body = Object,
    responses((status = 200, description = "OK", body = SettingsView), (status = "4XX", description = "Bad request, not found or conflict", body = ErrorBody))
)]
pub async fn update(
    State(app): State<AppState>,
    Json(changes): Json<Map<String, Value>>,
) -> ApiResult<Json<SettingsView>> {
    app.ctx.settings.update(changes).await?;
    Ok(Json(app.ctx.settings.view()))
}
