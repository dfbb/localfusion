// P5-T07 用 rust-embed 实现；当前占位返回提示页。
use axum::response::Html;

pub async fn serve_index() -> Html<&'static str> {
    Html("<h1>LocalFusion</h1><p>前端未构建。请用 /admin/api 管理。</p>")
}
