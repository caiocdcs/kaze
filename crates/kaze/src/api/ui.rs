//! The web UI, compiled into the binary from `web/build`.
//!
//! Release builds embed the files; debug builds read them from disk, so
//! `just web-build` shows up without recompiling kaze. Paths that aren't
//! files get `index.html`, and the SPA's router takes it from there.

use axum::{
    http::{HeaderValue, StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/build"]
// Lets kaze build (and its tests and clippy run) without Node.
#[allow_missing = true]
struct Assets;

const NOT_BUILT: &str = "<!doctype html><title>kaze</title>\
<p>The web UI isn't built into this binary. Run <code>just web-build</code>, \
or use the API under <code>/api/v1</code>.</p>";

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(response) = asset(path) {
        return response;
    }
    // A missing API route or file with an extension is a real 404, not a page.
    if path.starts_with("api/") || path.rsplit('/').next().is_some_and(|f| f.contains('.')) {
        return StatusCode::NOT_FOUND.into_response();
    }
    asset("index.html").unwrap_or_else(|| Html(NOT_BUILT).into_response())
}

fn asset(path: &str) -> Option<Response> {
    let file = Assets::get(path)?;
    // Vite fingerprints everything under _app/immutable; the rest (index.html)
    // must be revalidated so a new version is picked up.
    let cache = if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mime = HeaderValue::from_str(file.metadata.mimetype())
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    Some(
        (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
            ],
            file.data,
        )
            .into_response(),
    )
}
