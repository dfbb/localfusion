//! 管理 REST API 模块
//!
//! 提供 admin token 鉴权的完整管理接口，覆盖：
//! - 真实模型 CRUD（含删除引用检查）
//! - 虚拟模型 + 成员 CRUD
//! - 策略列表
//! - ingress key CRUD + ACL 管理
//! - 统计数据（usage/prices/latency/requests）
//! - 日志级别热重载
pub mod api;
pub mod static_assets;

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::db::Db;
use crate::error::FusionError;
use crate::logging::LogHandle;

/// admin 路由共享状态
#[derive(Clone)]
pub struct AdminState {
    pub db: Db,
    pub log: Arc<LogHandle>,
    pub enc_key: [u8; 32],
}

/// 将 FusionError 转换为 (status, json) 响应（不泄露内部细节，设计 §5.3）
pub(crate) fn err_response(e: FusionError) -> Response {
    let code =
        StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (code, Json(serde_json::json!({"error": e.to_string()}))).into_response()
}

/// 验证请求头中的 admin token（从 settings 表取 hash 并比对）
pub(crate) async fn require_admin(
    state: &AdminState,
    headers: &HeaderMap,
) -> Result<(), Response> {
    let hash = state
        .db
        .setting_get_or("admin_token_hash", "")
        .await
        .map_err(err_response)?;
    crate::auth::verify_admin(&hash, headers).map_err(err_response)
}

/// 构建管理路由器；所有端点挂载在 /admin/api/ 前缀下。
///
/// 生产形态下前端经 rust-embed 与本服务同源,不需要 CORS;
/// 仅为前端开发态(Vite dev server 在另一 localhost 端口)放开 localhost/127.0.0.1 来源,
/// 绝不放开任意来源,避免本机其他网页跨站访问管理 API。
pub fn router(state: AdminState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _req| {
            origin
                .to_str()
                .map(|o| {
                    o.starts_with("http://127.0.0.1:")
                        || o.starts_with("http://localhost:")
                        || o == "http://127.0.0.1"
                        || o == "http://localhost"
                })
                .unwrap_or(false)
        }))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    Router::new()
        .route("/admin/api/health", get(health))
        .merge(api::models_routes())
        .merge(api::vmodels_routes())
        .merge(api::keys_routes())
        .merge(api::stats_routes())
        .merge(api::playground_routes())
        .merge(api::logging_routes())
        .fallback(get(static_assets::serve))
        .layer(cors)
        .with_state(state)
}

/// GET /admin/api/health — 鉴权探活端点
async fn health(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Err(r) = require_admin(&state, &headers).await {
        return r;
    }
    Json(serde_json::json!({"status": "ok"})).into_response()
}
