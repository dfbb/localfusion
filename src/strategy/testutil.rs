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
                    input_tokens: in_tok, output_tokens: out_tok, billable_input_tokens: in_tok,
                    cache_read_tokens: 0, cache_write_tokens: 0,
                    cost: 0.0,
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
                        input_tokens: in_tok, output_tokens: out_tok, billable_input_tokens: in_tok,
                        cache_read_tokens: 0, cache_write_tokens: 0,
                        cost: 0.0,
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
