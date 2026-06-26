use async_trait::async_trait;

use super::synthesize::make_text_request;
use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::*;

pub struct Multimodal;

/// 从响应中提取所有 ToolCall 条目，返回 (id, name, args) 三元组列表
fn extract_tool_calls(resp: &UnifiedResponse) -> Vec<(String, String, serde_json::Value)> {
    resp.items
        .iter()
        .filter_map(|i| match i {
            Item::ToolCall { id, name, args } => Some((id.clone(), name.clone(), args.clone())),
            _ => None,
        })
        .collect()
}

/// 从响应中提取助手消息的纯文本内容
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

    /// 执行 buffered agentic loop：
    /// 1. 调用主模型（members[0]）
    /// 2. 若响应含 ToolCall，按 params 中的能力路由表找对应后端执行
    /// 3. 把工具结果作为 ToolResult 追加到请求，继续下一轮
    /// 4. 无工具调用时终止，始终返回 Full（非流式）
    /// 5. 超过 max_iterations 返回错误
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        let max_iter = ctx
            .params
            .get("max_iterations")
            .and_then(|v| v.as_u64())
            .unwrap_or(6);

        // 取第一个成员作为主模型
        let main = ctx
            .members
            .first()
            .ok_or_else(|| FusionError::StrategyError("multimodal: no main model".into()))?;

        let mut req = ctx.req.clone();

        for _ in 0..max_iter {
            // 调用主模型
            let resp = call_member(main, &req, CallRole::Member, ctx.recorder).await?;
            let tool_calls = extract_tool_calls(&resp);

            // 无工具调用：终止循环，返回最终响应
            if tool_calls.is_empty() {
                if let Some(t) = ctx.trace {
                    t.add_turn(serde_json::json!({
                        "main_output": message_text(&resp),
                        "tool_calls": 0
                    }));
                }
                return Ok(StrategyOutput::Full(resp));
            }

            // 有工具调用：逐个按能力路由表转发后端执行，回填结果
            for (id, name, args) in tool_calls {
                // 从 params 中查找该工具名对应的后端模型 id
                let route = ctx.params.get(&name).and_then(|v| v.as_str());

                let result_text = match route {
                    Some(model_id) => {
                        // 解析后端 MemberHandle 并执行工具调用
                        let backend = ctx.resolver.resolve(model_id).await?;
                        let tool_req = make_text_request(&args.to_string(), Some(512));
                        let r = call_member(&backend, &tool_req, CallRole::Tool, ctx.recorder).await?;
                        message_text(&r)
                    }
                    None => {
                        // 未配置路由：返回占位提示，不中断循环
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

                // 把工具结果追加到请求，供下一轮主模型使用
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

    /// 无成员时返回错误
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

    /// 主模型调用失败时返回错误
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
