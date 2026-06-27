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
        if ctx.members.is_empty() {
            return Err(FusionError::StrategyError("cheapest: no members".into()));
        }
        // 输出估算:优先用请求的 max_tokens(spec §7.3「输出用 max_tokens 或历史均值」),
        // 缺省再退回 params.output_estimate_max(默认 512)
        let out_est = ctx
            .req
            .max_tokens
            .map(|m| m as u64)
            .or_else(|| ctx.params.get("output_estimate_max").and_then(|v| v.as_u64()))
            .unwrap_or(512);
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
