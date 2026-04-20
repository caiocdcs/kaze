use axum::{extract::{Path, Query, State}, routing::get, Json, Router, response::IntoResponse};
use serde::Deserialize;
use tracing::{info, warn};

use crate::app_state::AppState;
use crate::api::dto::*;
use crate::db::manga_repo::MangaRepo;
use crate::error::AppResult;
use crate::id::{MangaId, ConnectorMappingId, AuthorId, LinkId, AltTitleId, EntityId};
use crate::workers::download_chapter::DownloadCoverWorker;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search/{connector_name}/{query}", get(search))
        .route("/search", get(search_by_url))
}

#[derive(Deserialize)]
pub struct SearchUrlQuery { pub url: String }

async fn search(
    State(state): State<AppState>,
    Path((connector_name, query)): Path<(String, String)>,
) -> AppResult<impl IntoResponse> {
    let connectors = state.connectors.read().await;
    let connector = connectors.get(&connector_name)
        .ok_or_else(|| crate::error::AppError::ConnectorNotFound { name: connector_name.clone() })?;

    if !connector.enabled() {
        return Err(crate::error::AppError::ConnectorDisabled { name: connector_name });
    }

    let results = match connector.search_manga(&query, &*state.http_client).await {
        Ok(r) => r,
        Err(e) => {
            warn!("Search failed for {connector_name}: {e}");
            vec![]
        }
    };
    drop(connectors);

    let mut added = Vec::new();
    for result in results {
        match add_manga_to_context(&state, &result, &connector_name).await {
            Ok(manga) => added.push(manga),
            Err(e) => warn!("Failed to add manga '{}': {e}", result.name),
        }
    }

    Ok(Json(added))
}

async fn search_by_url(
    State(state): State<AppState>,
    Query(params): Query<SearchUrlQuery>,
) -> AppResult<impl IntoResponse> {
    let url = params.url.trim_matches(|c| c == '"' || c == '\'' || c == ' ');
    let connectors = state.connectors.read().await;

    for connector in connectors.enabled() {
        if connector.url_matches_connector(url) {
            let connector_name = connector.name().to_string();
            if let Ok(Some(result)) = connector.get_manga_from_url(url, &*state.http_client).await {
                drop(connectors);
                let manga = add_manga_to_context(&state, &result, &connector_name).await?;
                return Ok(Json(manga).into_response());
            }
        }
    }

    Err(crate::error::AppError::NotFound { entity: "Manga", id: url.to_string() })
}

async fn add_manga_to_context(
    state: &AppState,
    result: &crate::connectors::NewMangaResult,
    connector_name: &str,
) -> AppResult<MinimalManga> {
    if let Some(existing_id) = MangaRepo::find_manga_like(&state.db, &result.name).await? {
        let existing = MangaRepo::get_by_id(&state.db, &existing_id).await?;
        if let Some(manga) = existing {
            let already_has = manga.connectors.iter()
                .any(|id| id.manga_connector_name.eq_ignore_ascii_case(connector_name)
                    && id.id_on_connector_site == result.connector_site_id);

            if !already_has {
                let mapping_key = ConnectorMappingId::new();
                MangaRepo::add_connector_mapping(
                    &state.db,
                    &mapping_key.to_string(),
                    &existing_id,
                    connector_name,
                    &result.connector_site_id,
                    result.website_url.as_deref(),
                    false,
                ).await?;

                info!("Added connector mapping {} -> {connector_name}:{}", existing_id, result.connector_site_id);
            }

            return Ok(MinimalManga {
                id: manga.id,
                name: manga.name,
                description: manga.description,
                release_status: manga.release_status,
                connectors: manga.connectors,
            });
        }
    }

    let manga_id = MangaId::new();
    let dir_name = crate::utils::clean_name_for_windows(&result.name);

    MangaRepo::insert(
        &state.db,
        &manga_id.to_string(),
        &result.name,
        &result.description,
        &result.cover_url,
        &result.release_status,
        None,
        &dir_name,
        result.year.map(|y| y as i32),
        result.original_language.as_deref(),
    ).await?;

    for author_name in &result.authors {
        let author_key = AuthorId::new();
        MangaRepo::insert_author_if_new(&state.db, &author_key.to_string(), author_name).await?;
        MangaRepo::link_author(&state.db, &manga_id.to_string(), &author_key.to_string()).await?;
    }

    for tag in &result.tags {
        MangaRepo::insert_tag_if_new(&state.db, tag).await?;
        MangaRepo::link_tag(&state.db, &manga_id.to_string(), tag).await?;
    }

    for (provider, url) in &result.links {
        let link_key = LinkId::new();
        MangaRepo::insert_link(&state.db, &link_key.to_string(), &manga_id.to_string(), provider, url).await?;
    }

    for (language, title) in &result.alt_titles {
        let alt_key = AltTitleId::new();
        MangaRepo::insert_alt_title(&state.db, &alt_key.to_string(), &manga_id.to_string(), language, title).await?;
    }

    let mapping_key = ConnectorMappingId::new();
    let mapping = MangaRepo::add_connector_mapping(
        &state.db,
        &mapping_key.to_string(),
        &manga_id.to_string(),
        connector_name,
        &result.connector_site_id,
        result.website_url.as_deref(),
        false,
    ).await?;

    {
        let mut scheduler = state.scheduler.lock().await;
        scheduler.submit(DownloadCoverWorker::create(mapping.id.clone()));
    }

    info!("Added new manga {} ({})", result.name, manga_id);

    Ok(MinimalManga {
        id: manga_id.to_string(),
        name: result.name.clone(),
        description: result.description.clone(),
        release_status: result.release_status.clone(),
        connectors: vec![mapping],
    })
}
