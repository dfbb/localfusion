use std::future::IntoFuture;
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
    let level = db
        .setting_get_or("log_level", "info")
        .await
        .unwrap_or_else(|_| "info".into());
    let file = db
        .setting_get("log_file")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    let to_stdout = db
        .setting_get_or("log_to_stdout", "true")
        .await
        .unwrap_or_else(|_| "true".into())
        == "true";
    let log = Arc::new(localfusion::logging::init(&level, file.as_deref(), to_stdout));

    // 冷启动引导（admin token 直接打印）
    let enc_key = ensure_initialized(&db).await?;

    let inference_bind = db.setting_get_or("inference_bind", "127.0.0.1:8787").await?;
    let admin_bind = db.setting_get_or("admin_bind", "127.0.0.1:8788").await?;

    // 探测后台
    localfusion::probe::spawn_probe_loop(db.clone(), enc_key, 1800);

    // 推理 server
    let inf_state = InferenceState {
        db: db.clone(),
        enc_key,
    };
    let inf_app = Router::new()
        .route("/v1/chat/completions", post(chat))
        .route("/v1/responses", post(responses))
        .route("/v1/messages", post(messages))
        .with_state(inf_state);

    // 管理 server
    let admin_app = admin::router(AdminState {
        db: db.clone(),
        log,
        enc_key,
    });

    let inf_listener = tokio::net::TcpListener::bind(&inference_bind).await?;
    let admin_listener = tokio::net::TcpListener::bind(&admin_bind).await?;
    tracing::info!("inference on {inference_bind}, admin on {admin_bind}");

    let inf = axum::serve(inf_listener, inf_app);
    let adm = axum::serve(admin_listener, admin_app);
    tokio::try_join!(inf.into_future(), adm.into_future())?;
    Ok(())
}
