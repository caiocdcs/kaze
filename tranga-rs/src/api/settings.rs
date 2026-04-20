use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post},
    Json,
};

use crate::app_state::AppState;
use crate::config::Settings;
use crate::error::{AppError, AppResult};
use crate::api::dto::LibraryRefreshConfig;

const DEFAULT_USER_AGENT: &str = "Tranga/2.0";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings", get(get_settings).put(update_settings))
        .route("/settings/user-agent", get(get_user_agent).patch(set_user_agent).delete(reset_user_agent))
        .route("/settings/image-compression-level", get(get_image_compression))
        .route("/settings/image-compression-level/{level}", patch(set_image_compression))
        .route("/settings/bw-images", get(get_bw_images))
        .route("/settings/bw-images/{enabled}", patch(set_bw_images))
        .route("/settings/chapter-naming-scheme", get(get_chapter_naming_scheme).patch(set_chapter_naming_scheme))
        .route("/settings/flare-solverr/url", patch(set_flaresolverr_url).delete(clear_flaresolverr_url))
        .route("/settings/flare-solverr/test", post(test_flaresolverr))
        .route("/settings/download-language", get(get_download_language))
        .route("/settings/download-language/{language}", patch(set_download_language))
        .route("/settings/library-refresh", patch(update_library_refresh))
}

async fn get_settings(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let settings = state.config.settings.read().await.clone();
    Ok(Json(settings))
}

async fn update_settings(
    State(state): State<AppState>,
    Json(new_settings): Json<Settings>,
) -> AppResult<impl IntoResponse> {
    {
        let mut settings = state.config.settings.write().await;
        *settings = new_settings;
    }
    state.config.persist_settings().await;
    Ok(StatusCode::OK)
}

async fn get_user_agent(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let settings = state.config.settings.read().await;
    Ok(settings.user_agent.clone())
}

async fn set_user_agent(
    State(state): State<AppState>,
    body: String,
) -> AppResult<impl IntoResponse> {
    {
        let mut settings = state.config.settings.write().await;
        settings.user_agent = body;
    }
    state.config.persist_settings().await;
    Ok(StatusCode::OK)
}

async fn reset_user_agent(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    {
        let mut settings = state.config.settings.write().await;
        settings.user_agent = DEFAULT_USER_AGENT.to_string();
    }
    state.config.persist_settings().await;
    Ok(StatusCode::OK)
}

async fn get_image_compression(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let settings = state.config.settings.read().await;
    Ok(settings.image_compression.to_string())
}

async fn set_image_compression(
    State(state): State<AppState>,
    Path(level): Path<i32>,
) -> AppResult<impl IntoResponse> {
    if !(1..=100).contains(&level) {
        return Err(AppError::BadRequest {
            detail: "Image compression level must be between 1 and 100".into(),
        });
    }
    {
        let mut settings = state.config.settings.write().await;
        settings.image_compression = level;
    }
    state.config.persist_settings().await;
    Ok(StatusCode::OK)
}

async fn get_bw_images(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let settings = state.config.settings.read().await;
    Ok(settings.black_white_images.to_string())
}

async fn set_bw_images(
    State(state): State<AppState>,
    Path(enabled): Path<bool>,
) -> AppResult<impl IntoResponse> {
    {
        let mut settings = state.config.settings.write().await;
        settings.black_white_images = enabled;
    }
    state.config.persist_settings().await;
    Ok(StatusCode::OK)
}

async fn get_chapter_naming_scheme(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let settings = state.config.settings.read().await;
    Ok(settings.chapter_naming_scheme.clone())
}

async fn set_chapter_naming_scheme(
    State(state): State<AppState>,
    body: String,
) -> AppResult<impl IntoResponse> {
    {
        let mut settings = state.config.settings.write().await;
        settings.chapter_naming_scheme = body;
    }
    state.config.persist_settings().await;
    Ok(StatusCode::OK)
}

async fn set_flaresolverr_url(
    State(state): State<AppState>,
    body: String,
) -> AppResult<impl IntoResponse> {
    {
        let mut settings = state.config.settings.write().await;
        settings.flare_solverr_url = body;
    }
    state.config.persist_settings().await;
    Ok(StatusCode::OK)
}

async fn clear_flaresolverr_url(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    {
        let mut settings = state.config.settings.write().await;
        settings.flare_solverr_url = String::new();
    }
    state.config.persist_settings().await;
    Ok(StatusCode::OK)
}

async fn test_flaresolverr(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let fs_url = {
        let settings = state.config.settings.read().await;
        settings.flare_solverr_url.clone()
    };

    if fs_url.is_empty() {
        return Err(AppError::BadRequest {
            detail: "FlareSolverr URL not configured".into(),
        });
    }

    let client = reqwest::Client::new();
    let result = client.get(&fs_url).send().await;
    match result {
        Ok(resp) if resp.status().is_success() => Ok(StatusCode::OK),
        _ => Err(AppError::Internal(anyhow::anyhow!("FlareSolverr is not reachable"))),
    }
}

async fn get_download_language(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let settings = state.config.settings.read().await;
    Ok(settings.download_language.clone())
}

async fn set_download_language(
    State(state): State<AppState>,
    Path(language): Path<String>,
) -> AppResult<impl IntoResponse> {
    {
        let mut settings = state.config.settings.write().await;
        settings.download_language = language;
    }
    state.config.persist_settings().await;
    Ok(StatusCode::OK)
}

async fn update_library_refresh(
    State(state): State<AppState>,
    Json(body): Json<LibraryRefreshConfig>,
) -> AppResult<impl IntoResponse> {
    {
        let mut settings = state.config.settings.write().await;
        settings.library_refresh_setting = body.setting;
        if let Some(mins) = body.refresh_while_downloading_every_minutes {
            settings.refresh_library_while_downloading_every_minutes = mins;
        }
    }
    state.config.persist_settings().await;
    Ok(StatusCode::OK)
}
