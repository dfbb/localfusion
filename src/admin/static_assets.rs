use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

// The dist directory may be empty when missing — compilation still succeeds (RustEmbed allows empty directories).
#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Assets;

const PLACEHOLDER: &str = "<h1>LocalFusion</h1><p>Frontend not built (web/dist is empty). Use /admin/api to manage, or run pnpm build then cargo build.</p>";

/// Static assets + SPA fallback: serves the file if found; otherwise returns index.html (for frontend routing); shows a placeholder page if index is also missing.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response();
    }
    // SPA fallback → index.html
    if let Some(index) = Assets::get("index.html") {
        return ([(header::CONTENT_TYPE, "text/html")], index.data).into_response();
    }
    (StatusCode::OK, Html(PLACEHOLDER)).into_response()
}
