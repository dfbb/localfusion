# P3-T07 multimodal 策略

**阶段:** 3 策略 · **前置:** P3-T05 · 见全局约束: `00-index.md`

**Goal:** buffered agentic loop：主模型多轮，拦截 ToolCall 按能力路由表转发后端执行回填，终结轮返回 `Full`（设计 §7.6）。

**Files:** Modify: `src/strategy/multimodal.rs`

**Produces:** `Multimodal` 实现 `Strategy`。

- [ ] **Step 1: 写失败测试**

```rust
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
        let resolver = crate::router::ModelResolver::new(db.clone(), [0u8;32]);
        let recorder = CallRecorder::default();
        let members = vec![ mock_member("main", vec![MockReply::Ok { text: "final".into(), in_tok: 1, out_tok: 1 }]) ];
        let ctx = StrategyCtx { req: simple_req(), members, resolver: &resolver,
            params: serde_json::json!({"max_iterations": 6}), db: &db, want_stream: false,
            recorder: &recorder, trace: None };
        match Multimodal.execute(ctx).await.unwrap() {
            StrategyOutput::Full(r) => assert_eq!(r.model_id, "main"), _ => panic!() }
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib strategy::multimodal` → FAIL。

- [ ] **Step 3: 实现 multimodal.rs**

```rust
use async_trait::async_trait;

use super::synthesize::make_text_request;
use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::*;

pub struct Multimodal;

fn extract_tool_calls(resp: &UnifiedResponse) -> Vec<(String, String, serde_json::Value)> {
    resp.items.iter().filter_map(|i| match i {
        Item::ToolCall { id, name, args } => Some((id.clone(), name.clone(), args.clone())),
        _ => None }).collect()
}

fn message_text(resp: &UnifiedResponse) -> String {
    resp.items.iter().find_map(|i| match i {
        Item::Message { content, .. } => Some(content.iter().filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.clone()), _ => None }).collect::<String>()),
        _ => None }).unwrap_or_default()
}

#[async_trait]
impl Strategy for Multimodal {
    fn name(&self) -> &str { "multimodal" }
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        let max_iter = ctx.params.get("max_iterations").and_then(|v| v.as_u64()).unwrap_or(6);
        let main = ctx.members.first()
            .ok_or_else(|| FusionError::StrategyError("multimodal: no main model".into()))?;
        let mut req = ctx.req.clone();
        for _ in 0..max_iter {
            let resp = call_member(main, &req, CallRole::Member, ctx.recorder).await?;
            let tool_calls = extract_tool_calls(&resp);
            if tool_calls.is_empty() {
                if let Some(t) = ctx.trace {
                    t.add_turn(serde_json::json!({"main_output": message_text(&resp), "tool_calls": 0}));
                }
                return Ok(StrategyOutput::Full(resp));
            }
            for (id, name, args) in tool_calls {
                let route = ctx.params.get(&name).and_then(|v| v.as_str());
                let result_text = match route {
                    Some(model_id) => {
                        let backend = ctx.resolver.resolve(model_id).await?;
                        let tool_req = make_text_request(&args.to_string(), Some(512));
                        let r = call_member(&backend, &tool_req, CallRole::Tool, ctx.recorder).await?;
                        message_text(&r)
                    }
                    None => format!("(no backend configured for tool '{name}')"),
                };
                if let Some(t) = ctx.trace {
                    t.add_turn(serde_json::json!({"tool": name, "route": route, "result": result_text}));
                }
                req.items.push(Item::ToolResult { id, content: vec![ContentBlock::Text(result_text)] });
            }
        }
        Err(FusionError::StrategyError(format!("multimodal: exceeded max_iterations={max_iter}")))
    }
}
```

- [ ] **Step 4: 运行确认通过 + 提交**

```bash
cargo test --lib strategy::multimodal && cargo clippy --all-targets
git add src/strategy/multimodal.rs
git commit -m "feat: multimodal 策略(buffered agentic loop + 能力路由回填)"
```
