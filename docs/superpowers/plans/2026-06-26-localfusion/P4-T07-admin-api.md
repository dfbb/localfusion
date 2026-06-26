# P4-T07 管理 REST API

**阶段:** 4 装配 · **前置:** P1 全部, P3-T01, P4-T05, P4-T06 · 见全局约束: `00-index.md`

**Goal:** admin token 鉴权的管理 REST 路由（设计 §13.1），用 curl 即可全量管理。

**Files:** Modify: `src/lib.rs`（加 `pub mod admin;`）；Create: `src/admin/mod.rs`, `src/admin/api.rs`；占位 `src/admin/static_assets.rs`（P5-T07 实现）

**Produces:**
- `pub struct AdminState { db:Db, log: Arc<LogHandle>, enc_key:[u8;32] }`
- `pub fn router(state:AdminState) -> axum::Router`：挂载下列端点 + admin token 中间件
- 端点（全部 JSON）：
  - `GET /admin/api/health`
  - models: `GET /admin/api/models`、`POST /admin/api/models`、`PUT /admin/api/models/:id`、`DELETE /admin/api/models/:id`（删除前 `model_references` 检查，有引用返 409）
  - virtual-models: `GET`、`POST`、`PUT /:name`、`DELETE /:name`
  - `GET /admin/api/strategies`（name + params_schema 列表）
  - keys: `GET`、`POST`（返回明文一次）、`PATCH /:id`、`DELETE /:id`、`PUT /:id/acl`
  - stats: `GET /admin/api/stats/usage`、`/usage/summary`、`/prices`、`/latency`、`/requests`
  - logging: `GET /admin/api/settings/logging`、`PUT /admin/api/settings/logging`（level 热重载）

> 因端点较多，分多步：先骨架 + 中间件 + health，再 models，再 virtual-models/strategies，再 keys，再 stats/logging。每步可独立 `cargo build`，最后统一集成测试。

- [ ] **Step 1: 写集成测试骨架 `tests/admin_api.rs`（先只测 health 鉴权）**

```rust
use localfusion::admin::{router, AdminState};
use localfusion::db::Db;
use std::sync::Arc;

async fn app() -> axum::Router {
    let db = Db::open_memory().await.unwrap();
    db.setting_set("admin_token_hash", &localfusion::crypto::sha256_hex("adm")).await.unwrap();
    let log = Arc::new(localfusion::logging::init("info", None, false));
    router(AdminState { db, log, enc_key: [0u8;32] })
}

#[tokio::test]
async fn health_requires_admin_token() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    let app = app().await;
    // 无 token → 401
    let r = app.clone().oneshot(Request::builder().uri("/admin/api/health").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    // 带 token → 200
    let r = app.oneshot(Request::builder().uri("/admin/api/health")
        .header("authorization", "Bearer adm").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}
```

注：`logging::init` 在一个进程内只能调用一次（全局 subscriber）。测试用 `tracing::subscriber::set_default` 模式可能冲突——本测试 `init` 改为容忍重复（见 P4-T01 可加 `try_init`）。**调整 P4-T01**：把 `registry...init()` 改为 `let _ = registry...try_init();` 以便测试多次调用不 panic。

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --test admin_api` → FAIL。

- [ ] **Step 3: 实现 admin/mod.rs + 中间件 + health**

```rust
// src/admin/mod.rs
pub mod api;
pub mod static_assets;

use std::sync::Arc;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};

use crate::db::Db;
use crate::error::FusionError;
use crate::logging::LogHandle;

#[derive(Clone)]
pub struct AdminState {
    pub db: Db,
    pub log: Arc<LogHandle>,
    pub enc_key: [u8; 32],
}

/// 把 FusionError 转成 (status, json) 响应（不泄密，设计 §5.3）。
pub(crate) fn err_response(e: FusionError) -> Response {
    let code = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (code, Json(serde_json::json!({"error": e.to_string()}))).into_response()
}

async fn require_admin(state: &AdminState, headers: &HeaderMap) -> Result<(), Response> {
    let hash = state.db.setting_get_or("admin_token_hash", "").await
        .map_err(|e| err_response(e))?;
    crate::auth::verify_admin(&hash, headers).map_err(err_response)
}

pub fn router(state: AdminState) -> Router {
    Router::new()
        .route("/admin/api/health", get(health))
        .merge(api::models_routes())
        .merge(api::vmodels_routes())
        .merge(api::keys_routes())
        .merge(api::stats_routes())
        .merge(api::logging_routes())
        .with_state(state)
}

async fn health(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Err(r) = require_admin(&state, &headers).await { return r; }
    Json(serde_json::json!({"status": "ok"})).into_response()
}
```

- [ ] **Step 4: 实现 api.rs models 路由**

```rust
// src/admin/api.rs
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde_json::Value;

use super::{err_response, require_admin, AdminState};
use crate::db::models::ModelRow;

pub fn models_routes() -> Router<AdminState> {
    Router::new()
        .route("/admin/api/models", get(list_models).post(create_model))
        .route("/admin/api/models/:id", put(update_model).delete(delete_model))
}

async fn list_models(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match s.db.model_list().await {
        Ok(mut rows) => { for m in &mut rows { m.api_key_enc = m.api_key_enc.as_ref().map(|_| "***".into()); }
            Json(rows).into_response() }
        Err(e) => err_response(e),
    }
}

/// body: { id, connector, base_url, api_key?(明文), api_key_env?, model, anthropic_version?, extra? }
async fn create_model(State(s): State<AdminState>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match upsert_from_body(&s, &body, None).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}

async fn update_model(State(s): State<AdminState>, h: HeaderMap, Path(id): Path<String>, Json(body): Json<Value>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match upsert_from_body(&s, &body, Some(&id)).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}

async fn upsert_from_body(s: &AdminState, body: &Value, id_override: Option<&str>)
    -> Result<(), crate::error::FusionError> {
    use crate::error::FusionError;
    let id = id_override.map(String::from).or_else(|| body.get("id").and_then(|v| v.as_str()).map(String::from))
        .ok_or_else(|| FusionError::InvalidRequest("id required".into()))?;
    let connector = body.get("connector").and_then(|v| v.as_str())
        .ok_or_else(|| FusionError::InvalidRequest("connector required".into()))?.to_string();
    let base_url = body.get("base_url").and_then(|v| v.as_str())
        .ok_or_else(|| FusionError::InvalidRequest("base_url required".into()))?.to_string();
    let model = body.get("model").and_then(|v| v.as_str())
        .ok_or_else(|| FusionError::InvalidRequest("model required".into()))?.to_string();
    // api_key 明文 → 加密；编辑时未提供则保留原值
    let api_key_enc = match body.get("api_key").and_then(|v| v.as_str()) {
        Some(pt) if !pt.is_empty() => Some(crate::crypto::encrypt(&s.enc_key, pt)?),
        _ => s.db.model_get(&id).await?.and_then(|m| m.api_key_enc),
    };
    let row = ModelRow {
        id, connector, base_url, api_key_enc,
        api_key_env: body.get("api_key_env").and_then(|v| v.as_str()).map(String::from),
        model,
        anthropic_version: body.get("anthropic_version").and_then(|v| v.as_str()).map(String::from),
        extra: body.get("extra").map(|v| v.to_string()),
    };
    s.db.model_upsert(&row).await
}

async fn delete_model(State(s): State<AdminState>, h: HeaderMap, Path(id): Path<String>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match s.db.model_references(&id).await {
        Ok(refs) if !refs.is_empty() =>
            (axum::http::StatusCode::CONFLICT, Json(serde_json::json!({"error":"model in use","references":refs}))).into_response(),
        Ok(_) => match s.db.model_delete(&id).await {
            Ok(()) => Json(serde_json::json!({"ok": true})).into_response(), Err(e) => err_response(e) },
        Err(e) => err_response(e),
    }
}
```

- [ ] **Step 5: 实现 api.rs virtual-models + strategies 路由**

```rust
pub fn vmodels_routes() -> Router<AdminState> {
    Router::new()
        .route("/admin/api/virtual-models", get(list_vmodels).post(create_vmodel))
        .route("/admin/api/virtual-models/:name", put(update_vmodel).delete(delete_vmodel))
        .route("/admin/api/strategies", get(list_strategies))
}

async fn list_vmodels(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match s.db.vmodel_list().await {
        Ok(rows) => {
            // 附带 members
            let mut out = Vec::new();
            for vm in rows {
                let members = s.db.vmodel_members(&vm.name).await.unwrap_or_default();
                out.push(serde_json::json!({"name":vm.name,"strategy":vm.strategy,
                    "params": serde_json::from_str::<Value>(&vm.params).unwrap_or(Value::Null), "members": members}));
            }
            Json(out).into_response()
        }
        Err(e) => err_response(e),
    }
}

/// body: { name, strategy, params(object), members:[id...] }
async fn upsert_vmodel(s: &AdminState, body: &Value, name_override: Option<&str>)
    -> Result<(), crate::error::FusionError> {
    use crate::db::virtual_models::VirtualModelRow;
    use crate::error::FusionError;
    let name = name_override.map(String::from).or_else(|| body.get("name").and_then(|v| v.as_str()).map(String::from))
        .ok_or_else(|| FusionError::InvalidRequest("name required".into()))?;
    let strategy = body.get("strategy").and_then(|v| v.as_str())
        .ok_or_else(|| FusionError::InvalidRequest("strategy required".into()))?.to_string();
    if crate::strategy::make_strategy(&strategy).is_none() {
        return Err(FusionError::InvalidRequest(format!("unknown strategy '{strategy}'")));
    }
    let params = body.get("params").cloned().unwrap_or(serde_json::json!({})).to_string();
    let members: Vec<String> = body.get("members").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    // 校验 members + judge/能力路由引用的模型存在（设计 §6.3 fail-fast）
    let params_val: Value = serde_json::from_str(&params).unwrap_or(Value::Null);
    for id in members.iter() {
        if s.db.model_get(id).await?.is_none() {
            return Err(FusionError::InvalidRequest(format!("member model '{id}' not found")));
        }
    }
    for key in ["judge","web_search","image_generation","tool_search","image_query"] {
        if let Some(mid) = params_val.get(key).and_then(|v| v.as_str()) {
            if s.db.model_get(mid).await?.is_none() {
                return Err(FusionError::InvalidRequest(format!("{key} model '{mid}' not found")));
            }
        }
    }
    s.db.vmodel_upsert(&VirtualModelRow { name, strategy, params }, &members).await
}

async fn create_vmodel(State(s): State<AdminState>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match upsert_vmodel(&s, &body, None).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(), Err(e) => err_response(e) }
}
async fn update_vmodel(State(s): State<AdminState>, h: HeaderMap, Path(name): Path<String>, Json(body): Json<Value>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match upsert_vmodel(&s, &body, Some(&name)).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(), Err(e) => err_response(e) }
}
async fn delete_vmodel(State(s): State<AdminState>, h: HeaderMap, Path(name): Path<String>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match s.db.vmodel_delete(&name).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(), Err(e) => err_response(e) }
}
async fn list_strategies(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    let names = ["failover","speed","cheapest","synthesize","best-of-n","multimodal"];
    let out: Vec<Value> = names.iter().map(|n| serde_json::json!({
        "name": n, "params_schema": crate::strategy::params_schema(n)})).collect();
    Json(out).into_response()
}
```

- [ ] **Step 6: 实现 api.rs keys 路由**

```rust
pub fn keys_routes() -> Router<AdminState> {
    Router::new()
        .route("/admin/api/keys", get(list_keys).post(create_key))
        .route("/admin/api/keys/:id", patch(patch_key).delete(delete_key))
        .route("/admin/api/keys/:id/acl", put(set_acl))
}

async fn list_keys(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match s.db.key_list().await { Ok(rows) => Json(rows).into_response(), Err(e) => err_response(e) }
}
/// body: { label? }；返回明文一次（设计 §5.3）
async fn create_key(State(s): State<AdminState>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    let label = body.get("label").and_then(|v| v.as_str());
    let plaintext = format!("sk-lf-{}", uuid_like());
    let now = now_secs();
    match s.db.key_insert(&plaintext, label, now).await {
        Ok(id) => Json(serde_json::json!({"id": id, "key": plaintext})).into_response(),
        Err(e) => err_response(e),
    }
}
async fn patch_key(State(s): State<AdminState>, h: HeaderMap, Path(id): Path<i64>, Json(body): Json<Value>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    let enabled = body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    let label = body.get("label").and_then(|v| v.as_str());
    match s.db.key_set_enabled_label(id, enabled, label).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(), Err(e) => err_response(e) }
}
async fn delete_key(State(s): State<AdminState>, h: HeaderMap, Path(id): Path<i64>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match s.db.key_delete(id).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(), Err(e) => err_response(e) }
}
/// body: { acl_all:bool, names:[String] }
async fn set_acl(State(s): State<AdminState>, h: HeaderMap, Path(id): Path<i64>, Json(body): Json<Value>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    let acl_all = body.get("acl_all").and_then(|v| v.as_bool()).unwrap_or(false);
    let names: Vec<String> = body.get("names").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    match s.db.key_set_acl(id, acl_all, &names).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(), Err(e) => err_response(e) }
}

// 简易随机串与时间（不引第三方）：基于 std。
fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:x}")
}
```

- [ ] **Step 7: 实现 api.rs stats + logging 路由**

```rust
pub fn stats_routes() -> Router<AdminState> {
    Router::new()
        .route("/admin/api/stats/usage", get(stats_usage))
        .route("/admin/api/stats/usage/summary", get(stats_usage_summary))
        .route("/admin/api/stats/prices", get(stats_prices))
        .route("/admin/api/stats/latency", get(stats_latency))
        .route("/admin/api/stats/requests", get(stats_requests))
}

use axum::extract::Query;
use std::collections::HashMap;

async fn stats_usage(State(s): State<AdminState>, h: HeaderMap, Query(q): Query<HashMap<String,String>>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    let scope = q.get("scope").map(String::as_str).unwrap_or("total");
    let name = q.get("name").map(String::as_str);
    let from = q.get("from").and_then(|v| v.parse().ok()).unwrap_or(0);
    let to = q.get("to").and_then(|v| v.parse().ok()).unwrap_or(i64::MAX);
    match s.db.usage_query(scope, name, from, to).await {
        Ok(rows) => Json(rows).into_response(), Err(e) => err_response(e) }
}
async fn stats_usage_summary(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match s.db.usage_query("total", Some(""), 0, i64::MAX).await {
        Ok(rows) => {
            let req: i64 = rows.iter().map(|r| r.requests).sum();
            let inp: i64 = rows.iter().map(|r| r.input_tokens).sum();
            let out: i64 = rows.iter().map(|r| r.output_tokens).sum();
            let cost: f64 = rows.iter().map(|r| r.cost).sum();
            Json(serde_json::json!({"requests":req,"input_tokens":inp,"output_tokens":out,
                "total_tokens":inp+out,"cost":cost})).into_response()
        }
        Err(e) => err_response(e),
    }
}
async fn stats_prices(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    match s.db.price_list().await { Ok(rows) => Json(rows).into_response(), Err(e) => err_response(e) }
}
async fn stats_latency(State(s): State<AdminState>, h: HeaderMap, Query(q): Query<HashMap<String,String>>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    // 简单返回每模型最近平均（前端可再画趋势）；详细样本查询 v1 用聚合即可。
    let model = q.get("model").cloned().unwrap_or_default();
    match s.db.latency_avg_recent(&model, 10).await {
        Ok(avg) => Json(serde_json::json!({"model":model,"avg_throughput":avg})).into_response(),
        Err(e) => err_response(e),
    }
}
async fn stats_requests(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    // request_log 简单全量（v1）；分页可后续加。
    match sqlx::query_as::<_, (i64, Option<String>, Option<String>, Option<String>, Option<i64>, Option<f64>, i64)>(
        "SELECT id, virtual_name, strategy, status, total_tokens, cost, created_at FROM request_log ORDER BY id DESC LIMIT 200")
        .fetch_all(&s.db.pool).await {
        Ok(rows) => {
            let out: Vec<Value> = rows.into_iter().map(|(id,vn,st,status,tok,cost,ts)| serde_json::json!({
                "id":id,"virtual_name":vn,"strategy":st,"status":status,"total_tokens":tok,"cost":cost,"created_at":ts})).collect();
            Json(out).into_response()
        }
        Err(e) => err_response(e.into()),
    }
}

pub fn logging_routes() -> Router<AdminState> {
    Router::new().route("/admin/api/settings/logging", get(get_logging).put(put_logging))
}
async fn get_logging(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    let level = s.db.setting_get_or("log_level", "info").await.unwrap_or_else(|_| "info".into());
    let file = s.db.setting_get_or("log_file", "").await.unwrap_or_default();
    let stdout = s.db.setting_get_or("log_to_stdout", "true").await.unwrap_or_else(|_| "true".into());
    Json(serde_json::json!({"log_level":level,"log_file":file,"log_to_stdout":stdout=="true"})).into_response()
}
/// body: { log_level?, log_file?, log_to_stdout? }；level 热重载，file/stdout 写库提示重启。
async fn put_logging(State(s): State<AdminState>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    if let Some(level) = body.get("log_level").and_then(|v| v.as_str()) {
        if let Err(e) = s.log.set_level(level) { return err_response(e); }
        if let Err(e) = s.db.setting_set("log_level", level).await { return err_response(e); }
    }
    if let Some(f) = body.get("log_file").and_then(|v| v.as_str()) {
        if let Err(e) = s.db.setting_set("log_file", f).await { return err_response(e); }
    }
    if let Some(b) = body.get("log_to_stdout").and_then(|v| v.as_bool()) {
        if let Err(e) = s.db.setting_set("log_to_stdout", if b {"true"} else {"false"}).await { return err_response(e); }
    }
    Json(serde_json::json!({"ok": true, "note": "log_file/log_to_stdout 需重启生效"})).into_response()
}
```

- [ ] **Step 8: static_assets.rs 占位**

```rust
// P5-T07 用 rust-embed 实现；当前占位返回提示页。
use axum::response::Html;
pub async fn serve_index() -> Html<&'static str> {
    Html("<h1>LocalFusion</h1><p>前端未构建。请用 /admin/api 管理。</p>")
}
```

并在 `admin/mod.rs` 的 router 末尾加 `.route("/", get(static_assets::serve_index))`（可选）。

- [ ] **Step 9: 运行集成测试 + clippy + 提交**

```bash
cargo test --test admin_api && cargo clippy --all-targets
git add src/lib.rs src/admin/ tests/admin_api.rs
git commit -m "feat: 管理 REST API(models/vmodels/strategies/keys/stats/logging + admin鉴权)"
```
