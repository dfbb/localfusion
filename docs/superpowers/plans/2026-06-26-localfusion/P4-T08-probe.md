# P4-T08 speed 探测后台任务

**阶段:** 4 装配 · **前置:** P1-T11, P3-T08 · 见全局约束: `00-index.md`

**Goal:** 定期对最近无样本的模型发小探测请求，写 latency_sample(is_probe=1)，保持 speed 数据新鲜（设计 §7.2）。

**Files:** Modify: `src/lib.rs`（加 `pub mod probe;`）；Create: `src/probe.rs`

**Produces:** `pub async fn probe_once(db:&Db, resolver:&ModelResolver, now_ts:i64, stale_window_secs:i64)`（对 `latency_models_without_recent` 的模型各发一个最小请求并记样本）；`pub fn spawn_probe_loop(db:Db, enc_key:[u8;32], interval_secs:u64)`（tokio interval 后台循环）。

- [ ] **Step 1: 写失败测试（probe_once 对已知模型记一条样本）**

```rust
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
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":1,"completion_tokens":3}}))).mount(&server).await;
        let db = Db::open_memory().await.unwrap();
        db.model_upsert(&ModelRow { id:"m".into(), connector:"chat".into(),
            base_url: format!("{}/v1", server.uri()), api_key_enc:None, api_key_env:Some("PROBE_KEY".into()),
            model:"gpt".into(), anthropic_version:None, extra:None }).await.unwrap();
        std::env::set_var("PROBE_KEY", "k");
        // 旧样本让 m 进入 stale 列表
        db.latency_insert("m", 1, 1.0, false, 1).await.unwrap();
        let resolver = ModelResolver::new(db.clone(), [0u8;32]);
        probe_once(&db, &resolver, 100_000, 3600).await;
        // 现在应有一条 is_probe=1 样本
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM latency_samples WHERE is_probe=1")
            .fetch_one(&db.pool).await.unwrap();
        assert!(n >= 1);
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib probe` → FAIL。

- [ ] **Step 3: 实现 probe.rs**

```rust
use std::time::Instant;

use crate::db::Db;
use crate::router::ModelResolver;
use crate::unified::{ContentBlock, Item, Role, UnifiedRequest};

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn probe_request() -> UnifiedRequest {
    UnifiedRequest {
        items: vec![Item::Message { role: Role::User, content: vec![ContentBlock::Text("ping".into())] }],
        tools: vec![], max_tokens: Some(8), temperature: None, stream: false, raw_extra: serde_json::Value::Null }
}

/// 对最近 stale_window_secs 内无样本的模型各发一次最小请求，记 is_probe=1 样本。
pub async fn probe_once(db: &Db, resolver: &ModelResolver, now_ts: i64, stale_window_secs: i64) {
    let since = now_ts - stale_window_secs;
    let stale = match db.latency_models_without_recent(since).await { Ok(v) => v, Err(_) => return };
    for model_id in stale {
        let member = match resolver.resolve(&model_id).await { Ok(m) => m, Err(_) => continue };
        let start = Instant::now();
        if let Ok(resp) = member.connector.complete(&probe_request(), &member.egress).await {
            let secs = start.elapsed().as_secs_f64();
            let out = resp.usage.output_tokens as i64;
            let _ = db.latency_insert(&model_id, out.max(1), secs.max(0.001), true, now_ts).await;
        }
    }
}

/// 后台循环（main 装配时 spawn）。
pub fn spawn_probe_loop(db: Db, enc_key: [u8; 32], interval_secs: u64) {
    tokio::spawn(async move {
        let resolver = ModelResolver::new(db.clone(), enc_key);
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        loop {
            ticker.tick().await;
            probe_once(&db, &resolver, now_secs(), interval_secs as i64 * 2).await;
        }
    });
}
```

- [ ] **Step 4: 运行确认通过 + 提交**

```bash
cargo test --lib probe && cargo clippy --all-targets
git add src/lib.rs src/probe.rs
git commit -m "feat: speed 探测后台任务(对陈旧模型发探测请求记样本)"
```
