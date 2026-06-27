//! Admin REST API module
//!
//! Provides a complete management interface with admin token authentication, covering:
//! - Real model CRUD (with deletion reference checks)
//! - Virtual model + member CRUD
//! - Policy listing
//! - ingress key CRUD + ACL management
//! - Statistics data (usage/prices/latency/requests)
//! - Log level hot reload
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

/// Shared state for admin routes
#[derive(Clone)]
pub struct AdminState {
    pub db: Db,
    pub log: Arc<LogHandle>,
    pub enc_key: [u8; 32],
}

/// Converts FusionError into a (status, json) response (without leaking internal details, design §5.3)
pub(crate) fn err_response(e: FusionError) -> Response {
    let code =
        StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (code, Json(serde_json::json!({"error": e.to_string()}))).into_response()
}

/// Validates the admin token in the request headers (fetches hash from settings table and compares)
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

/// Builds the admin router; all endpoints are mounted under the /admin/api/ prefix.
///
/// In production, the frontend is served by rust-embed from the same origin, so CORS is not needed;
/// only localhost/127.0.0.1 origins are allowed for frontend development (Vite dev server on a different localhost port),
/// arbitrary origins are never allowed, to prevent cross-site access to the admin API from other local web pages.
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

/// GET /admin/api/health — authenticated liveness probe endpoint
async fn health(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Err(r) = require_admin(&state, &headers).await {
        return r;
    }
    Json(serde_json::json!({"status": "ok"})).into_response()
}
