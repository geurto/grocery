//! The web app (`web/`, built to `web/dist`), served from the binary.
//!
//! Release builds embed the files; debug builds read them from disk, so a
//! fresh `npm run build` shows up without recompiling. If the app hasn't
//! been built, `/` explains how instead of failing to compile.

use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;

#[derive(rust_embed::Embed)]
// Relative to this crate.
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Assets;

const NOT_BUILT: &str = "<!doctype html><meta charset=utf-8><title>grocery</title>\
<p>The web app hasn't been built. Run <code>npm ci &amp;&amp; npm run build</code> \
in <code>web/</code>, then rebuild the server. The API is at <code>/api/</code>.";

/// Fallback route: files from `web/dist`, `index.html` for `/`.
pub async fn static_file(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "no such API route" })),
        )
            .into_response();
    }
    let path = if path.is_empty() { "index.html" } else { path };
    let Some(file) = Assets::get(path) else {
        if path == "index.html" {
            return (StatusCode::SERVICE_UNAVAILABLE, Html(NOT_BUILT)).into_response();
        }
        return StatusCode::NOT_FOUND.into_response();
    };

    let etag = format!(
        "\"{}\"",
        file.metadata
            .sha256_hash()
            .iter()
            .take(16)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    // Vite puts content-hashed files under assets/; those never change.
    // Everything else (index.html, sw.js, the manifest) must be revalidated
    // so an update reaches the phones.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    if headers
        .get(header::IF_NONE_MATCH)
        .is_some_and(|v| v.as_bytes() == etag.as_bytes())
    {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (header::CACHE_CONTROL, cache.to_owned()),
            ],
        )
            .into_response();
    }
    (
        [
            (header::CONTENT_TYPE, file.metadata.mimetype().to_owned()),
            (header::ETAG, etag),
            (header::CACHE_CONTROL, cache.to_owned()),
        ],
        file.data,
    )
        .into_response()
}
