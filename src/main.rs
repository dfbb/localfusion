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

fn now_secs_main() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "./localfusion.db")]
    db: String,
    /// Print all debug-level log output to stdout, overriding the DB log_level setting.
    /// WARNING: DEBUG logs full upstream request/response bodies (prompts and completions)
    /// in plaintext. Use only for local troubleshooting, not against a shared log sink.
    #[arg(long, default_value_t = false)]
    debug: bool,
    /// Allow binding the inference/admin servers to a non-loopback address.
    /// Without this flag, a configured non-loopback bind is refused: traffic is plaintext
    /// HTTP (admin token, ingress keys, and prompts would cross the network in the clear).
    /// When set, put a TLS-terminating reverse proxy in front.
    #[arg(long, default_value_t = false)]
    allow_remote: bool,
}

/// Whether a `host:port` bind string targets a loopback interface.
///
/// Resolves the bind via the standard library so both `127.0.0.1:8787` and `localhost:8787`
/// (and `[::1]:8787`) are recognized. A bind that fails to resolve is treated as non-loopback
/// (fail-closed), so an unparseable or externally-resolving host requires --allow-remote.
fn is_loopback_bind(bind: &str) -> bool {
    use std::net::ToSocketAddrs;
    match bind.to_socket_addrs() {
        Ok(addrs) => {
            let mut any = false;
            for a in addrs {
                any = true;
                if !a.ip().is_loopback() {
                    return false;
                }
            }
            any // all resolved addrs are loopback (and there was at least one)
        }
        Err(_) => false,
    }
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

    println!("LocalFusion v{}", env!("APP_VERSION"));

    let db = Db::open(&cli.db).await?;

    // Logging (use settings from db first, fall back to defaults)
    // --debug overrides both log_level and to_stdout for the current run only (no DB write)
    let level = if cli.debug {
        "debug".to_string()
    } else {
        db.setting_get_or("log_level", "info")
            .await
            .unwrap_or_else(|_| "info".into())
    };
    let file = db
        .setting_get("log_file")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    let to_stdout = cli.debug || db
        .setting_get_or("log_to_stdout", "true")
        .await
        .unwrap_or_else(|_| "true".into())
        == "true";
    let log = Arc::new(localfusion::logging::init(&level, file.as_deref(), to_stdout));

    // Cold-start bootstrap (admin token printed directly)
    let enc_key = ensure_initialized(&db).await?;

    let inference_bind = db.setting_get_or("inference_bind", "127.0.0.1:8787").await?;
    let admin_bind = db.setting_get_or("admin_bind", "127.0.0.1:8788").await?;

    // Refuse non-loopback binds unless explicitly opted in: traffic is plaintext HTTP, so
    // remote exposure would put the admin token, ingress keys, and prompts on the wire.
    if !cli.allow_remote {
        for (label, bind) in [("inference", &inference_bind), ("admin", &admin_bind)] {
            if !is_loopback_bind(bind) {
                return Err(format!(
                    "{label} bind '{bind}' is not loopback; refusing to start. \
                     Traffic is plaintext HTTP — pass --allow-remote (behind a TLS proxy) to override."
                )
                .into());
            }
        }
    }

    // Probe background task (exits when shutdown signal received)
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    localfusion::probe::spawn_probe_loop(db.clone(), enc_key, 1800, shutdown_rx.clone());

    // Seed price defaults from the embedded snapshot if the table is empty, then refresh daily.
    if let Err(e) = localfusion::prices_litellm::seed_defaults_if_empty(&db, now_secs_main()).await {
        tracing::warn!("price_defaults seed failed: {e}");
    }
    localfusion::price_refresh::spawn_price_refresh_loop(db.clone(), shutdown_rx);

    // Inference server
    let inf_state = InferenceState {
        db: db.clone(),
        enc_key,
    };
    let inf_app = Router::new()
        .route("/v1/chat/completions", post(chat))
        .route("/v1/responses", post(responses))
        .route("/v1/messages", post(messages))
        .with_state(inf_state);

    // Admin server
    let admin_app = admin::router(AdminState {
        db: db.clone(),
        log,
        enc_key,
    });

    let inf_listener = tokio::net::TcpListener::bind(&inference_bind).await?;
    let admin_listener = tokio::net::TcpListener::bind(&admin_bind).await?;
    tracing::info!("inference on {inference_bind}, admin on {admin_bind}");

    // Graceful shutdown: triggered by SIGINT/SIGTERM; both servers stop accepting new requests,
    // drain in-flight requests, and the probe task exits
    let inf = axum::serve(inf_listener, inf_app).with_graceful_shutdown(shutdown_signal());
    let adm = axum::serve(admin_listener, admin_app).with_graceful_shutdown(shutdown_signal());
    let result = tokio::try_join!(inf.into_future(), adm.into_future());
    let _ = shutdown_tx.send(true); // Notify probe task to exit
    result?;
    Ok(())
}

/// Waits for SIGINT (Ctrl-C) or SIGTERM; returns when either arrives, triggering graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("shutdown signal received, starting graceful shutdown");
}

#[cfg(test)]
mod tests {
    use super::is_loopback_bind;

    #[test]
    fn loopback_binds_are_accepted() {
        assert!(is_loopback_bind("127.0.0.1:8787"));
        assert!(is_loopback_bind("[::1]:8788"));
    }

    #[test]
    fn non_loopback_and_wildcard_binds_are_rejected() {
        assert!(!is_loopback_bind("0.0.0.0:8787"));
        assert!(!is_loopback_bind("[::]:8787"));
        // Unparseable bind fails closed.
        assert!(!is_loopback_bind("not a bind"));
    }
}
