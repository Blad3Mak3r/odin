//! Serves the built frontend (`web/dist/`), embedded into the binary at
//! compile time. Any path that isn't a known asset falls back to
//! `index.html` so client-side routing (React Router) works on a hard
//! refresh of a deep link like `/instances/my-server`.

use axum::body::Body;
use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

pub async fn serve_index() -> Response {
    serve_embedded("index.html")
}

pub async fn serve_asset(Path(path): Path<String>) -> Response {
    let path = path.trim_start_matches('/');
    if path.is_empty() || Assets::get(path).is_none() {
        return serve_embedded("index.html");
    }
    serve_embedded(path)
}

fn serve_embedded(path: &str) -> Response {
    match Assets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                Body::from(file.data.into_owned()),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            "dashboard frontend not built; run `make web-build`",
        )
            .into_response(),
    }
}
