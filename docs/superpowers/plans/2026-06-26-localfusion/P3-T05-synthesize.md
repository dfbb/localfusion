# P3-T05 synthesize 策略

**阶段:** 3 策略 · **前置:** P3-T02, P3-T08(ModelResolver::with_mock) · 见全局约束: `00-index.md`

> 注：本 task 测试依赖 P3-T08 的 `ModelResolver::with_mock` 测试钩子。若先做本 task，可临时在 router.rs 占位加该钩子；推荐顺序：P3-T08 先于 P3-T05/06/07，或本 task 测试待 P3-T08 完成后再跑。

**Goal:** 并行收集成员答案 + judge 合成 + diversity 分级（设计 §7.4）。恒返回 `Full`。

**Files:** Modify: `src/strategy/synthesize.rs`（mod 声明 `pub mod synthesize`）

**Produces:** `Synthesize` 实现 `Strategy`；`pub(super) fn synthesis_prompt`、`pub(super) fn question_text`、`pub(super) fn make_text_request`、`pub(crate) fn answer_text_pub`（供 best-of-n 与测试复用）。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{models::ModelRow, Db};
    use crate::strategy::testutil::{mock_member, simple_req, MockReply};
    use crate::strategy::{StrategyCtx, StrategyOutput};
    use crate::unified::{CallRecorder, StrategyTrace};
    async fn seed_judge(db: &Db) {
        db.model_upsert(&ModelRow { id: "j".into(), connector: "chat".into(), base_url: "u".into(),
            api_key_enc: None, api_key_env: Some("E".into()), model: "j".into(),
            anthropic_version: None, extra: None }).await.unwrap();
    }
    #[tokio::test]
    async fn collects_members_and_calls_judge() {
        let db = Db::open_memory().await.unwrap();
        seed_judge(&db).await;
        let resolver = crate::router::ModelResolver::with_mock(db.clone(),
            |_id| mock_member("j", vec![MockReply::Ok { text: "synth".into(), in_tok: 1, out_tok: 1 }]));
        let recorder = CallRecorder::default();
        let trace = StrategyTrace::default();
        let members = vec![
            mock_member("a", vec![MockReply::Ok { text: "ans-a".into(), in_tok: 1, out_tok: 1 }]),
            mock_member("b", vec![MockReply::Ok { text: "ans-b".into(), in_tok: 1, out_tok: 1 }]) ];
        let ctx = StrategyCtx { req: simple_req(), members, resolver: &resolver,
            params: serde_json::json!({"judge":"j"}), db: &db, want_stream: false,
            recorder: &recorder, trace: Some(&trace) };
        match Synthesize.execute(ctx).await.unwrap() {
            StrategyOutput::Full(r) => assert_eq!(answer_text_pub(&r), "synth"), _ => panic!() }
        let snap = trace.snapshot();
        assert_eq!(snap["member_answers"].as_array().unwrap().len(), 2);
        assert!(snap["judge"].is_object());
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib strategy::synthesize` → FAIL。

- [ ] **Step 3: 实现 synthesize.rs**

```rust
use async_trait::async_trait;
use futures::future::join_all;

use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::*;

pub struct Synthesize;

pub(super) fn synthesis_prompt(question: &str, answers: &[(String, String)]) -> String {
    let mut p = format!("Question:\n{question}\n\nCandidate answers from different models:\n");
    for (i, (model, ans)) in answers.iter().enumerate() {
        p.push_str(&format!("\n[Answer {} from {}]\n{}\n", i + 1, model, ans));
    }
    p.push_str("\nReconcile these into one best answer. Note consensus, contradictions, gaps, and blind spots, then write a single superior response.");
    p
}

fn answer_text(resp: &UnifiedResponse) -> String {
    resp.items.iter().find_map(|i| match i {
        Item::Message { content, .. } => Some(content.iter().filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.clone()), _ => None }).collect::<String>()),
        _ => None }).unwrap_or_default()
}

pub(crate) fn answer_text_pub(r: &UnifiedResponse) -> String { answer_text(r) }

pub(super) fn question_text(req: &UnifiedRequest) -> String {
    req.items.iter().filter_map(|i| match i {
        Item::Message { role: Role::User, content } => Some(content.iter().filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.clone()), _ => None }).collect::<String>()),
        _ => None }).collect::<Vec<_>>().join("\n")
}

pub(super) fn make_text_request(prompt: &str, max_tokens: Option<u32>) -> UnifiedRequest {
    UnifiedRequest {
        items: vec![Item::Message { role: Role::User, content: vec![ContentBlock::Text(prompt.into())] }],
        tools: vec![], max_tokens, temperature: None, stream: false, raw_extra: serde_json::Value::Null }
}

#[async_trait]
impl Strategy for Synthesize {
    fn name(&self) -> &str { "synthesize" }
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        let min_answers = ctx.params.get("min_answers").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let strict = ctx.params.get("strict").and_then(|v| v.as_bool()).unwrap_or(false);
        let judge_id = ctx.params.get("judge").and_then(|v| v.as_str())
            .ok_or_else(|| FusionError::StrategyError("synthesize requires params.judge".into()))?;
        let futs = ctx.members.iter().map(|m| call_member(m, &ctx.req, CallRole::Member, ctx.recorder));
        let results = join_all(futs).await;
        let mut answers: Vec<(String, String)> = Vec::new();
        for (m, r) in ctx.members.iter().zip(results.into_iter()) {
            if let Ok(resp) = r {
                let text = answer_text(&resp);
                if let Some(t) = ctx.trace {
                    let u = resp.calls.first().cloned().unwrap_or(ModelUsage { model_id: m.model_id.clone(),
                        role: CallRole::Member, input_tokens:0, output_tokens:0, cost:0.0,
                        status: CallStatus::Ok, estimated:true, latency_secs:0.0 });
                    t.add_member_answer(&m.model_id, &text, &u);
                }
                if !text.trim().is_empty() { answers.push((m.model_id.clone(), text)); }
            }
        }
        if answers.is_empty() {
            return Err(FusionError::AllMembersFailed("synthesize: no panel answers".into()));
        }
        let status = if answers.len() >= ctx.members.len() { "full" }
            else if answers.len() >= min_answers { "degraded" } else { "stop" };
        if let Some(t) = ctx.trace { t.set_status(status); }
        if status == "stop" && strict {
            return Err(FusionError::StrategyError(format!("synthesize strict: only {} answers", answers.len())));
        }
        let judge_member = ctx.resolver.resolve(judge_id).await?;
        let prompt = synthesis_prompt(&question_text(&ctx.req), &answers);
        let judge_req = make_text_request(&prompt, ctx.req.max_tokens);
        let judge_resp = call_member(&judge_member, &judge_req, CallRole::Judge, ctx.recorder).await?;
        if let Some(t) = ctx.trace {
            let u = judge_resp.calls.first().cloned().unwrap();
            t.set_judge(&prompt, &answer_text(&judge_resp), &u);
        }
        Ok(StrategyOutput::Full(judge_resp))
    }
}
```

- [ ] **Step 4: 运行确认通过 + 提交**

```bash
cargo test --lib strategy::synthesize && cargo clippy --all-targets
git add src/strategy/synthesize.rs
git commit -m "feat: synthesize 策略(并行收集 + judge 合成 + diversity 分级)"
```
