use axum::{
    body::Body,
    extract::Path,
    http::{Response, StatusCode, header},
    response::IntoResponse,
};
use rust_embed::RustEmbed;

/// Static assets embedded into the WasmBridge binary (e.g., UI CSS, JS, images).
#[derive(RustEmbed)]
#[folder = "assets/"]
pub struct Assets;

/// Handler for serving static assets via Axum.
pub async fn static_handler(Path(path): Path<String>) -> impl IntoResponse {
    let path = path.trim_start_matches('/');

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Asset Not Found"))
            .unwrap(),
    }
}
