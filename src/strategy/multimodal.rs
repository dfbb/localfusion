use async_trait::async_trait;

use super::synthesize::make_text_request;
use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::*;

pub struct Multimodal;

/// Extract all ToolCall entries from the response, returning a list of (id, name, args) tuples
fn extract_tool_calls(resp: &UnifiedResponse) -> Vec<(String, String, serde_json::Value)> {
    resp.items
        .iter()
        .filter_map(|i| match i {
            Item::ToolCall { id, name, args } => Some((id.clone(), name.clone(), args.clone())),
            _ => None,
        })
        .collect()
}

/// Extract the plain text content of the assistant message from the response
fn message_text(resp: &UnifiedResponse) -> String {
    resp.items
        .iter()
        .find_map(|i| match i {
            Item::Message { content, .. } => Some(
                content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

#[async_trait]
impl Strategy for Multimodal {
    fn name(&self) -> &str {
        "multimodal"
    }

    /// Execute the buffered agentic loop:
    /// 1. Call the primary model (members[0])
    /// 2. If the response contains ToolCalls, route each to the corresponding backend via the capability routing table in params
    /// 3. Append tool results as ToolResult items to the request and continue to the next iteration
    /// 4. Terminate when there are no tool calls; always returns Full (non-streaming)
    /// 5. Return an error if max_iterations is exceeded
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        let max_iter = ctx
            .params
            .get("max_iterations")
            .and_then(|v| v.as_u64())
            .unwrap_or(6);

        // Use the first member as the primary model
        let main = ctx
            .members
            .first()
            .ok_or_else(|| FusionError::StrategyError("multimodal: no main model".into()))?;

        let mut req = ctx.req.clone();

        for _ in 0..max_iter {
            // Call the primary model
            let resp = call_member(main, &req, CallRole::Member, ctx.recorder).await?;
            let tool_calls = extract_tool_calls(&resp);

            // No tool calls: terminate the loop and return the final response
            if tool_calls.is_empty() {
                if let Some(t) = ctx.trace {
                    t.add_turn(serde_json::json!({
                        "main_output": message_text(&resp),
                        "tool_calls": 0
                    }));
                }
                return Ok(StrategyOutput::Full(resp));
            }

            // Tool calls present: route each one to the appropriate backend via the capability routing table, then collect results
            for (id, name, args) in tool_calls {
                // Look up the backend model id for this tool name in params
                let route = ctx.params.get(&name).and_then(|v| v.as_str());

                let result_text = match route {
                    Some(model_id) => {
                        // Resolve the backend MemberHandle and execute the tool call
                        let backend = ctx.resolver.resolve(model_id).await?;
                        let tool_req = make_text_request(&args.to_string(), Some(512));
                        let r = call_member(&backend, &tool_req, CallRole::Tool, ctx.recorder).await?;
                        message_text(&r)
                    }
                    None => {
                        // No route configured: return a placeholder message without breaking the loop
                        format!("(no backend configured for tool '{name}')")
                    }
                };

                if let Some(t) = ctx.trace {
                    t.add_turn(serde_json::json!({
                        "tool": name,
                        "route": route,
                        "result": result_text
                    }));
                }

                // Append the tool result to the request for use in the next iteration
                req.items.push(Item::ToolResult {
                    id,
                    content: vec![ContentBlock::Text(result_text)],
                });
            }
        }

        Err(FusionError::StrategyError(format!(
            "multimodal: exceeded max_iterations={max_iter}"
        )))
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
    async fn terminates_when_no_tool_call() {
        let db = Db::open_memory().await.unwrap();
        let resolver = crate::router::ModelResolver::new(db.clone(), [0u8; 32]);
        let recorder = CallRecorder::default();
        let members = vec![mock_member(
            "main",
            vec![MockReply::Ok {
                text: "final".into(),
                in_tok: 1,
                out_tok: 1,
            }],
        )];
        let ctx = StrategyCtx {
            req: simple_req(),
            members,
            resolver: &resolver,
            params: serde_json::json!({"max_iterations": 6}),
            db: &db,
            want_stream: false,
            recorder: &recorder,
            trace: None,
        };
        match Multimodal.execute(ctx).await.unwrap() {
            StrategyOutput::Full(r) => assert_eq!(r.model_id, "main"),
            _ => panic!(),
        }
    }

    /// Returns an error when there are no members
    #[tokio::test]
    async fn no_members_returns_error() {
        let db = Db::open_memory().await.unwrap();
        let resolver = crate::router::ModelResolver::new(db.clone(), [0u8; 32]);
        let recorder = CallRecorder::default();
        let ctx = StrategyCtx {
            req: simple_req(),
            members: vec![],
            resolver: &resolver,
            params: serde_json::json!({}),
            db: &db,
            want_stream: false,
            recorder: &recorder,
            trace: None,
        };
        assert!(Multimodal.execute(ctx).await.is_err());
    }

    /// Returns an error when the primary model call fails
    #[tokio::test]
    async fn main_model_fail_returns_error() {
        let db = Db::open_memory().await.unwrap();
        let resolver = crate::router::ModelResolver::new(db.clone(), [0u8; 32]);
        let recorder = CallRecorder::default();
        let members = vec![mock_member("main", vec![MockReply::Fail("boom".into())])];
        let ctx = StrategyCtx {
            req: simple_req(),
            members,
            resolver: &resolver,
            params: serde_json::json!({}),
            db: &db,
            want_stream: false,
            recorder: &recorder,
            trace: None,
        };
        assert!(Multimodal.execute(ctx).await.is_err());
    }
}
