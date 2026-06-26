# P3-T06 best-of-n 策略

**阶段:** 3 策略 · **前置:** P3-T05, P3-T08 · 见全局约束: `00-index.md`

**Goal:** 并行收候选 + judge 选最优并修复 + Verify by 行（设计 §7.5）。

**Files:** Modify: `src/strategy/best_of_n.rs`

**Produces:** `BestOfN` 实现 `Strategy`，复用 `synthesize::{question_text, make_text_request, answer_text_pub}`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{models::ModelRow, Db};
    use crate::strategy::testutil::{mock_member, simple_req, MockReply};
    use crate::strategy::{synthesize::answer_text_pub, StrategyCtx, StrategyOutput};
    use crate::unified::CallRecorder;
    #[tokio::test]
    async fn selects_and_repairs_via_judge() {
        let db = Db::open_memory().await.unwrap();
        db.model_upsert(&ModelRow { id: "j".into(), connector: "chat".into(), base_url: "u".into(),
            api_key_enc: None, api_key_env: Some("E".into()), model: "j".into(),
            anthropic_version: None, extra: None }).await.unwrap();
        let resolver = crate::router::ModelResolver::with_mock(db.clone(),
            |_id| mock_member("j", vec![MockReply::Ok { text: "best".into(), in_tok: 1, out_tok: 1 }]));
        let recorder = CallRecorder::default();
        let members = vec![
            mock_member("a", vec![MockReply::Ok { text: "cand-a".into(), in_tok: 1, out_tok: 1 }]),
            mock_member("b", vec![MockReply::Ok { text: "cand-b".into(), in_tok: 1, out_tok: 1 }]) ];
        let ctx = StrategyCtx { req: simple_req(), members, resolver: &resolver,
            params: serde_json::json!({"judge":"j"}), db: &db, want_stream: false,
            recorder: &recorder, trace: None };
        match BestOfN.execute(ctx).await.unwrap() {
            StrategyOutput::Full(r) => assert!(answer_text_pub(&r).contains("best")), _ => panic!() }
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib strategy::best_of_n` → FAIL。

- [ ] **Step 3: 实现 best_of_n.rs**

```rust
use async_trait::async_trait;
use futures::future::join_all;

use super::synthesize::{answer_text_pub, make_text_request, question_text};
use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::CallRole;

pub struct BestOfN;

fn selection_prompt(question: &str, answers: &[(String, String)]) -> String {
    let mut p = format!("Question:\n{question}\n\nCandidate solutions:\n");
    for (i, (model, ans)) in answers.iter().enumerate() {
        p.push_str(&format!("\n[Candidate {} from {}]\n{}\n", i + 1, model, ans));
    }
    p.push_str("\nPick the strongest candidate and repair its flaws. Output the final solution, then a line starting with 'Verify by:' describing how to verify it.");
    p
}

#[async_trait]
impl Strategy for BestOfN {
    fn name(&self) -> &str { "best-of-n" }
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        let judge_id = ctx.params.get("judge").and_then(|v| v.as_str())
            .ok_or_else(|| FusionError::StrategyError("best-of-n requires params.judge".into()))?;
        let futs = ctx.members.iter().map(|m| call_member(m, &ctx.req, CallRole::Member, ctx.recorder));
        let results = join_all(futs).await;
        let mut answers: Vec<(String, String)> = Vec::new();
        for (m, r) in ctx.members.iter().zip(results.into_iter()) {
            if let Ok(resp) = r {
                let text = answer_text_pub(&resp);
                if let Some(t) = ctx.trace {
                    let u = resp.calls.first().cloned().unwrap();
                    t.add_member_answer(&m.model_id, &text, &u);
                }
                if !text.trim().is_empty() { answers.push((m.model_id.clone(), text)); }
            }
        }
        if answers.is_empty() {
            return Err(FusionError::AllMembersFailed("best-of-n: no candidates".into()));
        }
        if let Some(t) = ctx.trace { t.set_status(if answers.len() >= ctx.members.len() {"full"} else {"degraded"}); }
        let judge_member = ctx.resolver.resolve(judge_id).await?;
        let prompt = selection_prompt(&question_text(&ctx.req), &answers);
        let judge_req = make_text_request(&prompt, ctx.req.max_tokens);
        let judge_resp = call_member(&judge_member, &judge_req, CallRole::Judge, ctx.recorder).await?;
        if let Some(t) = ctx.trace {
            let u = judge_resp.calls.first().cloned().unwrap();
            t.set_judge(&prompt, &answer_text_pub(&judge_resp), &u);
        }
        Ok(StrategyOutput::Full(judge_resp))
    }
}
```

- [ ] **Step 4: 运行确认通过 + 提交**

```bash
cargo test --lib strategy::best_of_n && cargo clippy --all-targets
git add src/strategy/best_of_n.rs
git commit -m "feat: best-of-n 策略(选最优并修复 + Verify by)"
```
