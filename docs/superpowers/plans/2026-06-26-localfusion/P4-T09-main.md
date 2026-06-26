# P4-T09 冷启动引导 + 推理 handler + main 装配

**阶段:** 4 装配 · **前置:** P4-T01..T08 全部 · 见全局约束: `00-index.md`

**Goal:** 冷启动建库/生成 enc_salt/admin token/默认 bind；三协议推理 handler（含 Full/Stream 两路径统计落库）；起推理 + 管理两个 axum server。完成后 `curl` 端到端可用（Spec A 完成）。

**Files:** Create: `src/bootstrap.rs`, `src/ingress/handler.rs`；Modify: `src/lib.rs`（加 `pub mod bootstrap; pub mod ingress;` 已加）、`src/ingress/mod.rs`（加 `pub mod handler;`）、`src/main.rs`

**Produces:**
- `bootstrap::ensure_initialized(db)->Result<[u8;32],FusionError>`（首启生成 salt + admin token 直接打印 + 默认 bind；返回 enc_key）
- `ingress::handler::{chat_handler, responses_handler, anthropic_handler}`（axum handler）
- `ingress::handler::InferenceState { db, enc_key }`
- `main()`：clap 解析 `--db`；init 日志；ensure_initialized；起两 server + 探测循环

- [ ] **Step 1: 写 bootstrap 失败测试**

```rust
// src/bootstrap.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    #[tokio::test]
    async fn first_run_sets_salt_and_token_and_binds() {
        let db = Db::open_memory().await.unwrap();
        let _key = ensure_initialized(&db).await.unwrap();
        assert!(db.setting_get("enc_salt").await.unwrap().is_some());
        assert!(db.setting_get("admin_token_hash").await.unwrap().is_some());
        assert_eq!(db.setting_get_or("inference_bind", "").await.unwrap(), "127.0.0.1:8787");
        assert_eq!(db.setting_get_or("admin_bind", "").await.unwrap(), "127.0.0.1:8788");
        // 第二次调用不重置 token（幂等）
        let hash1 = db.setting_get("admin_token_hash").await.unwrap();
        let _ = ensure_initialized(&db).await.unwrap();
        assert_eq!(db.setting_get("admin_token_hash").await.unwrap(), hash1);
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib bootstrap` → FAIL。

- [ ] **Step 3: 实现 bootstrap.rs**

```rust
use base64::{engine::general_purpose::STANDARD, Engine};

use crate::crypto::{derive_key, random_salt, sha256_hex};
use crate::db::Db;
use crate::error::FusionError;

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
fn random_token() -> String {
    let mut b = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut b);
    format!("lfadm-{}", STANDARD.encode(b))
}

/// 幂等：首启生成 enc_salt + admin token（直接打印，设计 §3/§9）+ 默认 bind；返回 enc_key。
pub async fn ensure_initialized(db: &Db) -> Result<[u8; 32], FusionError> {
    // enc_salt
    let salt_b64 = match db.setting_get("enc_salt").await? {
        Some(s) => s,
        None => {
            let salt = random_salt();
            let b64 = STANDARD.encode(salt);
            db.setting_set("enc_salt", &b64).await?;
            b64
        }
    };
    let salt = STANDARD.decode(&salt_b64).map_err(|e| FusionError::Internal(format!("salt b64: {e}")))?;
    let enc_key = derive_key(&salt)?;

    // admin token（仅首次）
    if db.setting_get("admin_token_hash").await?.is_none() {
        let token = random_token();
        db.setting_set("admin_token_hash", &sha256_hex(&token)).await?;
        crate::logging::print_admin_token_once(&token); // 直接 println!，不经 tracing
    }
    // 默认 bind
    if db.setting_get("inference_bind").await?.is_none() {
        db.setting_set("inference_bind", "127.0.0.1:8787").await?;
    }
    if db.setting_get("admin_bind").await?.is_none() {
        db.setting_set("admin_bind", "127.0.0.1:8788").await?;
    }
    let _ = now_secs(); // 预留
    Ok(enc_key)
}
```

- [ ] **Step 4: 实现 ingress/handler.rs（推理 handler + 统计落库）**

核心：解析入口 body → UnifiedRequest，鉴权，Router.dispatch；`Full` 用 `finalize_full` 写库后按协议 format_response；`Stream` 用 SSE body 边转发边收集尾用量、流关闭后 `write_stats`。

```rust
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response, Sse};
use axum::response::sse::Event;
use axum::Json;
use serde_json::Value;

use crate::db::Db;
use crate::error::FusionError;
use crate::ingress::{anthropic, openai_chat, openai_responses};
use crate::pipeline::{finalize_full, write_stats};
use crate::router::Router as FusionRouter;
use crate::strategy::StrategyOutput;
use crate::unified::{CallRecorder, ModelUsage, UnifiedStreamEvent};

#[derive(Clone)]
pub struct InferenceState {
    pub db: Db,
    pub enc_key: [u8; 32],
}

#[derive(Clone, Copy)]
pub enum Proto { Chat, Responses, Anthropic }

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn parse(proto: Proto, body: &Value) -> Result<crate::unified::UnifiedRequest, FusionError> {
    match proto {
        Proto::Chat => openai_chat::parse_request(body),
        Proto::Responses => openai_responses::parse_request(body),
        Proto::Anthropic => anthropic::parse_request(body),
    }
}
fn format(proto: Proto, resp: &crate::unified::UnifiedResponse) -> Value {
    match proto {
        Proto::Chat => openai_chat::format_response(resp),
        Proto::Responses => openai_responses::format_response(resp),
        Proto::Anthropic => anthropic::format_response(resp),
    }
}
fn sse_lines(proto: Proto, ev: &UnifiedStreamEvent) -> Vec<String> {
    match proto {
        Proto::Chat => openai_chat::sse_events(ev),
        Proto::Responses => openai_responses::sse_events(ev),
        Proto::Anthropic => anthropic::sse_events(ev),
    }
}

pub async fn handle(state: InferenceState, proto: Proto, headers: HeaderMap, body: Value) -> Response {
    let model = match body.get("model").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => return err(FusionError::InvalidRequest("model required".into())),
    };
    if let Err(e) = crate::auth::authorize_ingress(&state.db, &headers, &model).await {
        let code = if matches!(e, FusionError::Unauthorized(_)) { StatusCode::UNAUTHORIZED } else { StatusCode::FORBIDDEN };
        return (code, Json(serde_json::json!({"error": e.to_string()}))).into_response();
    }
    let req = match parse(proto, &body) { Ok(r) => r, Err(e) => return err(e) };
    let want_stream = req.stream;

    // 取策略名（写 request_log 用）
    let strategy = state.db.vmodel_get(&model).await.ok().flatten().map(|v| v.strategy).unwrap_or_default();
    let router = FusionRouter::new(state.db.clone(), state.enc_key);
    let recorder = Arc::new(CallRecorder::default());

    match router.dispatch(&model, req, want_stream, &recorder, None).await {
        Ok(StrategyOutput::Full(resp)) => {
            let _ = finalize_full(&state.db, &model, &strategy, &recorder, false, now_secs()).await;
            if want_stream {
                stream_from_full(proto, resp)
            } else {
                Json(format(proto, &resp)).into_response()
            }
        }
        Ok(StrategyOutput::Stream(stream)) => {
            stream_real(state.db.clone(), model, strategy, recorder, proto, stream)
        }
        Err(e) => {
            // 错误路径也写统计（drain 已发生调用）
            let calls = recorder.drain();
            let _ = write_stats(&state.db, &model, &strategy, &calls, true, now_secs()).await;
            err(e)
        }
    }
}

fn err(e: FusionError) -> Response {
    let code = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (code, Json(serde_json::json!({"error": e.to_string()}))).into_response()
}
```

- [ ] **Step 5: 实现 handler.rs 的两个流式辅助（伪流 + 真流）**

```rust
/// panel/multimodal 的 Full → 伪流 SSE（统计已在 finalize_full 落库）。
fn stream_from_full(proto: Proto, resp: crate::unified::UnifiedResponse) -> Response {
    use futures::stream;
    let text = resp.items.iter().find_map(|i| match i {
        crate::unified::Item::Message { content, .. } => Some(content.iter().filter_map(|c| match c {
            crate::unified::ContentBlock::Text(t) => Some(t.clone()), _ => None }).collect::<String>()),
        _ => None }).unwrap_or_default();
    let mut evs: Vec<UnifiedStreamEvent> = vec![UnifiedStreamEvent::Started { model_id: resp.model_id.clone() }];
    evs.push(UnifiedStreamEvent::TextDelta { text });
    evs.push(UnifiedStreamEvent::Done { usage: resp.usage, call: None, finish_reason: Some("stop".into()) });
    let mut lines: Vec<String> = Vec::new();
    for ev in &evs { for l in sse_lines(proto, ev) { lines.push(l); } }
    let s = stream::iter(lines.into_iter().map(|l| Ok::<_, std::convert::Infallible>(Event::default().data(l))));
    Sse::new(s).into_response()
}

/// 单模型真流：边转发边收集尾用量，流关闭后 write_stats。
fn stream_real(db: Db, model: String, strategy: String, recorder: Arc<CallRecorder>,
    proto: Proto, mut stream: crate::unified::UnifiedStream) -> Response {
    use async_stream::stream as astream; // 需加依赖 async-stream
    let body = astream! {
        let mut tail: Option<ModelUsage> = None;
        let mut failed = false;
        while let Some(item) = stream.rx.recv().await {
            match item {
                Ok(ev) => {
                    if let UnifiedStreamEvent::Done { call, .. } = &ev { tail = call.clone(); }
                    if let UnifiedStreamEvent::Error { call, .. } = &ev { tail = call.clone(); failed = true; }
                    for l in sse_lines(proto, &ev) {
                        yield Ok::<_, std::convert::Infallible>(Event::default().data(l));
                    }
                }
                Err(e) => { failed = true; yield Ok(Event::default().data(format!("{{\"error\":\"{e}\"}}"))); break; }
            }
        }
        // 合并 recorder 暂存的失败尝试 + 尾用量，写统计
        let mut all = recorder.drain();
        if let Some(t) = tail { all.push(t); }
        let _ = write_stats(&db, &model, &strategy, &all, failed, now_secs()).await;
    };
    Sse::new(body).into_response()
}
```

> 依赖：`async-stream = "0.3"` 加入 Cargo.toml `[dependencies]`。

- [ ] **Step 6: 实现 main.rs**

```rust
use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use clap::Parser;
use serde_json::Value;

use localfusion::admin::{self, AdminState};
use localfusion::bootstrap::ensure_initialized;
use localfusion::db::Db;
use localfusion::ingress::handler::{handle, InferenceState, Proto};

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "./localfusion.db")]
    db: String,
}

async fn chat(State(s): State<InferenceState>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    handle(s, Proto::Chat, h, b).await
}
async fn responses(State(s): State<InferenceState>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    handle(s, Proto::Responses, h, b).await
}
async fn messages(State(s): State<InferenceState>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    handle(s, Proto::Anthropic, h, b).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let db = Db::open(&cli.db).await?;

    // 日志（先用 db 里的设置，无则默认）
    let level = db.setting_get_or("log_level", "info").await.unwrap_or_else(|_| "info".into());
    let file = db.setting_get("log_file").await.ok().flatten().filter(|s| !s.is_empty());
    let to_stdout = db.setting_get_or("log_to_stdout", "true").await.unwrap_or_else(|_| "true".into()) == "true";
    let log = Arc::new(localfusion::logging::init(&level, file.as_deref(), to_stdout));

    // 冷启动引导（admin token 直接打印）
    let enc_key = ensure_initialized(&db).await?;

    let inference_bind = db.setting_get_or("inference_bind", "127.0.0.1:8787").await?;
    let admin_bind = db.setting_get_or("admin_bind", "127.0.0.1:8788").await?;

    // 探测后台
    localfusion::probe::spawn_probe_loop(db.clone(), enc_key, 1800);

    // 推理 server
    let inf_state = InferenceState { db: db.clone(), enc_key };
    let inf_app = Router::new()
        .route("/v1/chat/completions", post(chat))
        .route("/v1/responses", post(responses))
        .route("/v1/messages", post(messages))
        .with_state(inf_state);

    // 管理 server
    let admin_app = admin::router(AdminState { db: db.clone(), log, enc_key });

    let inf_listener = tokio::net::TcpListener::bind(&inference_bind).await?;
    let admin_listener = tokio::net::TcpListener::bind(&admin_bind).await?;
    tracing::info!("inference on {inference_bind}, admin on {admin_bind}");

    let inf = axum::serve(inf_listener, inf_app);
    let adm = axum::serve(admin_listener, admin_app);
    tokio::try_join!(inf, adm)?;
    Ok(())
}
```

- [ ] **Step 7: 写端到端集成测试 `tests/e2e.rs`（wiremock 后端 + 真实 dispatch）**

```rust
use localfusion::db::Db;
use localfusion::ingress::handler::{handle, InferenceState, Proto};
use localfusion::db::{models::ModelRow, virtual_models::VirtualModelRow};
use axum::http::HeaderMap;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn chat_e2e_failover_single_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "choices":[{"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":1,"completion_tokens":1}}))).mount(&server).await;
    let db = Db::open_memory().await.unwrap();
    std::env::set_var("E2E_KEY", "k");
    db.model_upsert(&ModelRow{id:"m".into(),connector:"chat".into(),
        base_url:format!("{}/v1",server.uri()),api_key_enc:None,api_key_env:Some("E2E_KEY".into()),
        model:"gpt".into(),anthropic_version:None,extra:None}).await.unwrap();
    db.vmodel_upsert(&VirtualModelRow{name:"vf".into(),strategy:"failover".into(),params:"{}".into()},&["m".into()]).await.unwrap();
    let id = db.key_insert("sk-1", None, 0).await.unwrap();
    db.key_set_acl(id, true, &[]).await.unwrap();
    let mut h = HeaderMap::new();
    h.insert("authorization", "Bearer sk-1".parse().unwrap());
    let resp = handle(InferenceState{db:db.clone(),enc_key:[0u8;32]}, Proto::Chat, h,
        serde_json::json!({"model":"vf","messages":[{"role":"user","content":"hi"}]})).await;
    assert_eq!(resp.status(), 200);
    // 统计已落库
    let total = db.usage_query("total", Some(""), 0, i64::MAX).await.unwrap();
    assert_eq!(total.iter().map(|r| r.requests).sum::<i64>(), 1);
}
```

- [ ] **Step 8: 全量构建 + 测试 + clippy + 提交**

```bash
cargo build && cargo test && cargo clippy --all-targets
git add Cargo.toml src/bootstrap.rs src/ingress/handler.rs src/ingress/mod.rs src/main.rs tests/e2e.rs
git commit -m "feat: 冷启动引导 + 三协议推理handler(Full/Stream统计) + main装配 + e2e"
```

> **阶段 4 / Spec A 完成**：单一可执行文件，三协议入口 + 6 策略 + 鉴权 + 统计 + 管理 API + 日志 + 探测，`curl` 端到端可用。手动验收：
> ```bash
> cargo run -- --db /tmp/lf.db   # 控制台打印 admin token
> # 用 admin token 经 /admin/api 配置 model + virtual-model + key，再用 OpenAI SDK 指向 127.0.0.1:8787 测试
> ```
