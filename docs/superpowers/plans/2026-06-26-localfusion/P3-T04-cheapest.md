# P3-T04 cheapest 策略

**阶段:** 3 策略 · **前置:** P3-T02 · 见全局约束: `00-index.md`

**Goal:** 估算成本选最低，缺价格排最后（设计 §7.3）。

**Files:** Modify: `src/strategy/cheapest.rs`

**Produces:** `Cheapest` 实现 `Strategy`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{prices::PriceRow, Db};
    use crate::strategy::testutil::{mock_member, simple_req, MockReply};
    use crate::strategy::{StrategyCtx, StrategyOutput};
    use crate::unified::CallRecorder;
    #[tokio::test]
    async fn picks_cheapest_priced_member() {
        let db = Db::open_memory().await.unwrap();
        db.price_upsert(&PriceRow { model_id: "a".into(), price_in: 10.0, price_out: 10.0, updated_at: 0 }).await.unwrap();
        db.price_upsert(&PriceRow { model_id: "b".into(), price_in: 1.0, price_out: 1.0, updated_at: 0 }).await.unwrap();
        let resolver = crate::router::ModelResolver::new(db.clone(), [0u8;32]);
        let recorder = CallRecorder::default();
        let members = vec![
            mock_member("a", vec![]),
            mock_member("b", vec![MockReply::Ok { text: "b".into(), in_tok: 1, out_tok: 1 }]) ];
        let ctx = StrategyCtx { req: simple_req(), members, resolver: &resolver,
            params: serde_json::json!({}), db: &db, want_stream: false, recorder: &recorder, trace: None };
        match Cheapest.execute(ctx).await.unwrap() {
            StrategyOutput::Full(r) => assert_eq!(r.model_id, "b"), _ => panic!() }
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib strategy::cheapest` → FAIL。

- [ ] **Step 3: 实现 cheapest.rs**

```rust
use async_trait::async_trait;

use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::{CallRole, ContentBlock, Item, UnifiedRequest};

pub struct Cheapest;

fn estimate_input_tokens(req: &UnifiedRequest) -> u64 {
    let mut chars = 0usize;
    for item in &req.items {
        if let Item::Message { content, .. } = item {
            for c in content { if let ContentBlock::Text(t) = c { chars += t.chars().count(); } }
        }
    }
    (chars / 4) as u64
}

#[async_trait]
impl Strategy for Cheapest {
    fn name(&self) -> &str { "cheapest" }
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        let out_est = ctx.params.get("output_estimate_max").and_then(|v| v.as_u64()).unwrap_or(512);
        let in_est = estimate_input_tokens(&ctx.req);
        let mut best_idx = None::<usize>;
        let mut best_cost = f64::MAX;
        for (i, m) in ctx.members.iter().enumerate() {
            let price = ctx.db.price_get(&m.model_id).await?;
            let cost = match &price {
                Some(p) => p.price_in * in_est as f64 / 1e6 + p.price_out * out_est as f64 / 1e6,
                None => f64::MAX,
            };
            if let Some(t) = ctx.trace {
                t.add_candidate(&m.model_id, serde_json::json!({"est_cost": if cost.is_finite() {Some(cost)} else {None}}));
            }
            if cost < best_cost { best_cost = cost; best_idx = Some(i); }
        }
        let idx = best_idx.unwrap_or(0);
        let member = &ctx.members[idx];
        if ctx.want_stream {
            Ok(StrategyOutput::Stream(member.connector.stream(&ctx.req, &member.egress).await?))
        } else {
            Ok(StrategyOutput::Full(call_member(member, &ctx.req, CallRole::Member, ctx.recorder).await?))
        }
    }
}
```

- [ ] **Step 4: 运行确认通过 + 提交**

```bash
cargo test --lib strategy::cheapest && cargo clippy --all-targets
git add src/strategy/cheapest.rs
git commit -m "feat: cheapest 策略(估算成本选最低, 缺价格排后)"
```
