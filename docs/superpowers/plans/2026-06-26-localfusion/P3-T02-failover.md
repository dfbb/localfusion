# P3-T02 mock connector 测试设施 + failover

**阶段:** 3 策略 · **前置:** P3-T01 · 见全局约束: `00-index.md`

**Goal:** 共享 mock connector（供所有策略测试）+ failover 策略（设计 §7.1）。

**Files:** Create: `src/strategy/testutil.rs`；Modify: `src/strategy/failover.rs`；（router 的 `ModelResolver::with_mock` 测试钩子在 P3-T08 实现，本 task 测试只用 `ModelResolver::new`）

**Produces（testutil）:** `MockReply{Ok{text,in_tok,out_tok},Fail(String)}`、`MockConnector`、`mock_member(id,replies)->MemberHandle`、`simple_req()->UnifiedRequest`。
**Produces（failover）:** `Failover` 实现 `Strategy`。

- [ ] **Step 1: 写 testutil.rs**

```rust
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::connector::{AuthKind, Connector, EgressCtx};
use crate::strategy::MemberHandle;
use crate::unified::*;

#[derive(Clone)]
pub enum MockReply { Ok { text: String, in_tok: u64, out_tok: u64 }, Fail(String) }

pub struct MockConnector { pub replies: std::sync::Mutex<Vec<MockReply>> }

#[async_trait]
impl Connector for MockConnector {
    async fn complete(&self, _req: &UnifiedRequest, ctx: &EgressCtx) -> Result<UnifiedResponse, ConnError> {
        let reply = self.replies.lock().unwrap().remove(0);
        match reply {
            MockReply::Ok { text, in_tok, out_tok } => Ok(UnifiedResponse {
                items: vec![Item::Message { role: Role::Assistant, content: vec![ContentBlock::Text(text)] }],
                usage: Usage { input_tokens: in_tok, output_tokens: out_tok },
                model_id: ctx.model.clone(),
                calls: vec![ModelUsage { model_id: ctx.model.clone(), role: CallRole::Member,
                    input_tokens: in_tok, output_tokens: out_tok, cost: 0.0,
                    status: CallStatus::Ok, estimated: false, latency_secs: 0.0 }] }),
            MockReply::Fail(m) => Err(ConnError::Http(m)),
        }
    }
    async fn stream(&self, _req: &UnifiedRequest, ctx: &EgressCtx) -> Result<UnifiedStream, ConnError> {
        let reply = self.replies.lock().unwrap().remove(0);
        match reply {
            MockReply::Fail(m) => Err(ConnError::Http(m)),
            MockReply::Ok { text, in_tok, out_tok } => {
                let (tx, rx) = mpsc::channel(8);
                let mid = ctx.model.clone();
                tx.send(Ok(UnifiedStreamEvent::Started { model_id: mid.clone() })).await.ok();
                tx.send(Ok(UnifiedStreamEvent::TextDelta { text })).await.ok();
                tx.send(Ok(UnifiedStreamEvent::Done {
                    usage: Usage { input_tokens: in_tok, output_tokens: out_tok },
                    call: Some(ModelUsage { model_id: mid, role: CallRole::Member,
                        input_tokens: in_tok, output_tokens: out_tok, cost: 0.0,
                        status: CallStatus::Ok, estimated: false, latency_secs: 0.0 }),
                    finish_reason: Some("stop".into()) })).await.ok();
                Ok(UnifiedStream { rx, upstream_request_id: None })
            }
        }
    }
}

pub fn mock_member(id: &str, replies: Vec<MockReply>) -> MemberHandle {
    MemberHandle { model_id: id.into(),
        connector: Box::new(MockConnector { replies: std::sync::Mutex::new(replies) }),
        egress: EgressCtx { base_url: "u".into(), model: id.into(), auth: AuthKind::Bearer,
            key: Some("k".into()), anthropic_version: None, default_max_tokens: None, http: reqwest::Client::new() } }
}

pub fn simple_req() -> UnifiedRequest {
    UnifiedRequest { items: vec![Item::Message { role: Role::User, content: vec![ContentBlock::Text("q".into())] }],
        tools: vec![], max_tokens: Some(64), temperature: None, stream: false, raw_extra: serde_json::Value::Null }
}
```

- [ ] **Step 2: 写 failover 失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::strategy::testutil::{mock_member, simple_req, MockReply};
    use crate::strategy::{StrategyCtx, StrategyOutput};
    use crate::unified::{CallRecorder, CallStatus};
    #[tokio::test]
    async fn first_success_returns_and_records() {
        let db = Db::open_memory().await.unwrap();
        let resolver = crate::router::ModelResolver::new(db.clone(), [0u8;32]);
        let recorder = CallRecorder::default();
        let members = vec![
            mock_member("a", vec![MockReply::Fail("boom".into())]),
            mock_member("b", vec![MockReply::Ok { text: "ok".into(), in_tok: 1, out_tok: 2 }]) ];
        let ctx = StrategyCtx { req: simple_req(), members, resolver: &resolver,
            params: serde_json::json!({}), db: &db, want_stream: false, recorder: &recorder, trace: None };
        let out = Failover.execute(ctx).await.unwrap();
        assert!(matches!(out, StrategyOutput::Full(_)));
        let calls = recorder.drain();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].status, CallStatus::Failed);
        assert_eq!(calls[1].status, CallStatus::Ok);
    }
    #[tokio::test]
    async fn all_fail_returns_err() {
        let db = Db::open_memory().await.unwrap();
        let resolver = crate::router::ModelResolver::new(db.clone(), [0u8;32]);
        let recorder = CallRecorder::default();
        let members = vec![ mock_member("a", vec![MockReply::Fail("x".into())]) ];
        let ctx = StrategyCtx { req: simple_req(), members, resolver: &resolver,
            params: serde_json::json!({}), db: &db, want_stream: false, recorder: &recorder, trace: None };
        assert!(Failover.execute(ctx).await.is_err());
    }
}
```

- [ ] **Step 3: 运行确认失败** — Run: `cargo test --lib strategy::failover` → FAIL。

- [ ] **Step 4: 实现 failover.rs**

```rust
use async_trait::async_trait;

use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::{CallRole, CallStatus, ModelUsage};

pub struct Failover;

#[async_trait]
impl Strategy for Failover {
    fn name(&self) -> &str { "failover" }
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        let mut last_err = None;
        for member in &ctx.members {
            if ctx.want_stream {
                match member.connector.stream(&ctx.req, &member.egress).await {
                    Ok(stream) => {
                        if let Some(t) = ctx.trace { t.add_attempt(&member.model_id, true, None); }
                        return Ok(StrategyOutput::Stream(stream));
                    }
                    Err(e) => {
                        ctx.recorder.record(ModelUsage { model_id: member.model_id.clone(), role: CallRole::Member,
                            input_tokens: 0, output_tokens: 0, cost: 0.0, status: CallStatus::Failed,
                            estimated: true, latency_secs: 0.0 });
                        if let Some(t) = ctx.trace { t.add_attempt(&member.model_id, false, Some(&e.to_string())); }
                        last_err = Some(FusionError::from(e));
                    }
                }
            } else {
                match call_member(member, &ctx.req, CallRole::Member, ctx.recorder).await {
                    Ok(resp) => {
                        if let Some(t) = ctx.trace { t.add_attempt(&member.model_id, true, None); }
                        return Ok(StrategyOutput::Full(resp));
                    }
                    Err(e) => {
                        if let Some(t) = ctx.trace { t.add_attempt(&member.model_id, false, Some(&e.to_string())); }
                        last_err = Some(e);
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| FusionError::AllMembersFailed("no members".into())))
    }
}
```

- [ ] **Step 5: 运行确认通过 + 提交**

```bash
cargo test --lib strategy::failover && cargo clippy --all-targets
git add src/strategy/testutil.rs src/strategy/failover.rs
git commit -m "feat: mock connector 测试设施 + failover 策略"
```
