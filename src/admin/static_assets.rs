use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

// dist 缺失时该目录可能为空——编译仍通过（RustEmbed 允许空目录）。
#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Assets;

const PLACEHOLDER: &str = "<h1>LocalFusion</h1><p>前端未构建（web/dist 为空）。请用 /admin/api 管理，或运行 pnpm build 后重新 cargo build。</p>";

/// 静态资源 + SPA fallback：命中文件返回文件；否则返回 index.html（前端路由）；index 也缺失则占位页。
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
