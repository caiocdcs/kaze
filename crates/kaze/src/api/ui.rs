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

pub fn serve(uri: &Uri, base: &str) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path != "index.html"
        && let Some(response) = asset(path)
    {
        return response;
    }
    // A missing API route or file with an extension is a real 404, not a page.
    if path.starts_with("api/") || path.rsplit('/').next().is_some_and(|f| f.contains('.')) {
        return StatusCode::NOT_FOUND.into_response();
    }
    page(base).unwrap_or_else(|| Html(NOT_BUILT).into_response())
}

/// `index.html`, pointed at `base`. Everything else in the build loads
/// relative to its own URL, so only this page needs the prefix.
fn page(base: &str) -> Option<Response> {
    if base.is_empty() {
        return asset("index.html");
    }
    let file = Assets::get("index.html")?;
    let html = with_base(&String::from_utf8_lossy(&file.data), base);
    Some(
        (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            html,
        )
            .into_response(),
    )
}

fn with_base(html: &str, base: &str) -> String {
    html.replace("base: \"\"", &format!("base: \"{base}\""))
        .replace("\"/_app/", &format!("\"{base}/_app/"))
        .replace(
            "href=\"/favicon.svg\"",
            &format!("href=\"{base}/favicon.svg\""),
        )
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

#[cfg(test)]
mod tests {
    #[test]
    fn points_the_page_at_the_base() {
        let html = r#"<link rel="icon" href="/favicon.svg" /><link href="/_app/a.js">
__sveltekit_x = { base: "", version: "1" }; import("/_app/b.js")"#;
        let out = super::with_base(html, "/kaze");
        assert!(out.contains(r#"href="/kaze/favicon.svg""#));
        assert!(out.contains(r#"href="/kaze/_app/a.js""#));
        assert!(out.contains(r#"base: "/kaze""#));
        assert!(out.contains(r#"import("/kaze/_app/b.js")"#));
    }

    /// Catches a SvelteKit upgrade changing what the rewrite looks for.
    /// Skipped when the UI isn't built.
    #[test]
    fn the_built_page_takes_the_base() {
        let Some(file) = super::Assets::get("index.html") else {
            return;
        };
        let html = String::from_utf8_lossy(&file.data);
        let out = super::with_base(&html, "/kaze");
        assert!(out.contains(r#"base: "/kaze""#), "SvelteKit's base");
        assert!(!out.contains(r#""/_app/"#), "every asset path is prefixed");
    }
}
