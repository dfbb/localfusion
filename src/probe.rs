use std::time::Instant;

use crate::db::Db;
use crate::router::ModelResolver;
use crate::unified::{ContentBlock, Item, Role, UnifiedRequest};

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn probe_request() -> UnifiedRequest {
    UnifiedRequest {
        items: vec![Item::Message {
            role: Role::User,
            content: vec![ContentBlock::Text("ping".into())],
        }],
        tools: vec![],
        max_tokens: Some(8),
        temperature: None,
        stream: false,
        raw_extra: serde_json::Value::Null,
    }
}

/// Sends one minimal request to each model with no samples in the last stale_window_secs, recording is_probe=1 samples.
pub async fn probe_once(
    db: &Db,
    resolver: &ModelResolver,
    now_ts: i64,
    stale_window_secs: i64,
) {
    let since = now_ts - stale_window_secs;
    let stale = match db.latency_models_without_recent(since).await {
        Ok(v) => v,
        Err(_) => return,
    };
    for model_id in stale {
        let member = match resolver.resolve(&model_id).await {
            Ok(m) => m,
            Err(_) => continue,
        };
        let start = Instant::now();
        if let Ok(resp) = member.connector.complete(&probe_request(), &member.egress).await {
            let secs = start.elapsed().as_secs_f64();
            let out = resp.usage.output_tokens as i64;
            let _ = db
                .latency_insert(&model_id, out.max(1), secs.max(0.001), true, now_ts)
                .await;
        }
    }
}

/// Background loop (spawned during main assembly). Exits when the shutdown signal is received (watch becomes true).
pub fn spawn_probe_loop(
    db: Db,
    enc_key: [u8; 32],
    interval_secs: u64,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let resolver = ModelResolver::new(db.clone(), enc_key);
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    probe_once(&db, &resolver, now_secs(), interval_secs as i64 * 2).await;
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::info!("Probe background task received shutdown signal, exiting");
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{models::ModelRow, Db};
    use crate::router::ModelResolver;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn probe_records_sample_for_stale_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":1,"completion_tokens":3}
                })),
            )
            .mount(&server)
            .await;
        let db = Db::open_memory().await.unwrap();
        db.model_upsert(&ModelRow {
            id: "m".into(),
            connector: "chat".into(),
            base_url: format!("{}/v1", server.uri()),
            api_key_enc: None,
            api_key_env: Some("PROBE_KEY".into()),
            model: "gpt".into(),
            anthropic_version: None,
            extra: None,
        })
        .await
        .unwrap();
        std::env::set_var("PROBE_KEY", "k");
        // old sample puts m into the stale list
        db.latency_insert("m", 1, 1.0, false, 1).await.unwrap();
        let resolver = ModelResolver::new(db.clone(), [0u8; 32]);
        probe_once(&db, &resolver, 100_000, 3600).await;
        // there should now be one is_probe=1 sample
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM latency_samples WHERE is_probe=1")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert!(n >= 1);
    }
}
