use async_trait::async_trait;

use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::CallRole;

pub struct Speed;

#[async_trait]
impl Strategy for Speed {
    fn name(&self) -> &str { "speed" }
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        if ctx.members.is_empty() {
            return Err(FusionError::StrategyError("speed: no members".into()));
        }
        let explore = ctx.params.get("explore").and_then(|v| v.as_bool()).unwrap_or(true);
        let mut best_idx = 0usize;
        let mut best_score = f64::MIN;
        for (i, m) in ctx.members.iter().enumerate() {
            let avg = ctx.db.latency_avg_recent(&m.model_id, 10).await?;
            let score = match avg { Some(v) => v, None => if explore { f64::MAX } else { 0.0 } };
            if let Some(t) = ctx.trace {
                t.add_candidate(&m.model_id, serde_json::json!({"avg_throughput": avg}));
            }
            if score > best_score { best_score = score; best_idx = i; }
        }
        let member = &ctx.members[best_idx];
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
    use crate::db::Db;
    use crate::strategy::testutil::{mock_member, simple_req, MockReply};
    use crate::strategy::{StrategyCtx, StrategyOutput};
    use crate::unified::CallRecorder;
    #[tokio::test]
    async fn picks_highest_throughput_member() {
        let db = Db::open_memory().await.unwrap();
        db.latency_insert("a", 10, 1.0, false, 1).await.unwrap(); // 吞吐 10
        db.latency_insert("b", 30, 1.0, false, 2).await.unwrap(); // 吞吐 30
        let resolver = crate::router::ModelResolver::new(db.clone(), [0u8;32]);
        let recorder = CallRecorder::default();
        let members = vec![
            mock_member("a", vec![]),
            mock_member("b", vec![MockReply::Ok { text: "b".into(), in_tok: 1, out_tok: 1 }]) ];
        let ctx = StrategyCtx { req: simple_req(), members, resolver: &resolver,
            params: serde_json::json!({"explore": false}), db: &db, want_stream: false,
            recorder: &recorder, trace: None };
        match Speed.execute(ctx).await.unwrap() {
            StrategyOutput::Full(r) => assert_eq!(r.model_id, "b"), _ => panic!() }
    }
}
