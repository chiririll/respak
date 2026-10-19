//! The frontend, built into the binary.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// `web/dist`, as built by `npm run build`. A debug build reads the files from disk, so the
/// frontend can be rebuilt without recompiling.
#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Dist;

/// A file of the frontend; any path that is not a file is the page itself (it routes in the browser).
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let (name, file) = match Dist::get(path) {
        Some(file) if !path.is_empty() => (path, file),
        _ => match Dist::get("index.html") {
            Some(file) => ("index.html", file),
            None => return (StatusCode::NOT_FOUND, "the frontend is not built").into_response(),
        },
    };
    let cache = if name.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (
                header::CONTENT_TYPE,
                mime_guess::from_path(name)
                    .first_or_octet_stream()
                    .to_string(),
            ),
            (header::CACHE_CONTROL, cache.to_owned()),
        ],
        file.data.into_owned(),
    )
        .into_response()
}
