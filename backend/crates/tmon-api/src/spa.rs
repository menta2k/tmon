//! Serves the embedded Vue SPA (feature `embed-ui`) with history fallback.

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

#[cfg(feature = "embed-ui")]
#[derive(rust_embed::RustEmbed)]
#[folder = "../../../frontend/dist"]
struct Assets;

#[cfg(feature = "embed-ui")]
fn asset(path: &str) -> Option<(Vec<u8>, String)> {
    Assets::get(path).map(|f| {
        (
            f.data.into_owned(),
            mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string(),
        )
    })
}

#[cfg(not(feature = "embed-ui"))]
fn asset(path: &str) -> Option<(Vec<u8>, String)> {
    (path == "index.html").then(|| {
        let body = b"<!doctype html><title>tmon</title><p>UI not embedded. Build with <code>--features embed-ui</code> or run the Vite dev server.</p>".to_vec();
        (body, "text/html; charset=utf-8".to_string())
    })
}

/// Hashed Vite assets live under `assets/` and are immutable.
fn cache_control(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let (path, found) = match asset(path) {
        Some(a) => (path.to_string(), Some(a)),
        None => ("index.html".to_string(), asset("index.html")),
    };
    match found {
        Some((body, mime)) => (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, cache_control(&path).to_string()),
            ],
            body,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
