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
