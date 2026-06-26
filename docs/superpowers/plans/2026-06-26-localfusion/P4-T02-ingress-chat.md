# P4-T02 OpenAI Chat 入口翻译

**阶段:** 4 装配 · **前置:** P1-T04 · 见全局约束: `00-index.md`

**Goal:** Chat body ↔ UnifiedRequest/Response/SSE。

**Files:** Modify: `src/lib.rs`（加 `pub mod ingress;`）；Create: `src/ingress/mod.rs`, `src/ingress/openai_chat.rs`, `src/ingress/sse_out.rs` + 占位 `openai_responses.rs`/`anthropic.rs`

**Produces:**
- `ingress/mod.rs`: `pub mod {openai_chat,openai_responses,anthropic,sse_out};` + `extract_model(body)->Option<String>`、`wants_stream(body)->bool`
- `openai_chat.rs`: `parse_request(body)->Result<UnifiedRequest,FusionError>`、`format_response(resp)->Value`、`sse_events(ev)->Vec<String>`
- `sse_out.rs`: `frame(payload)->String`

- [ ] **Step 1: 写失败测试（openai_chat.rs）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::unified::*;
    #[test]
    fn parse_basic_chat() {
        let body = serde_json::json!({"model":"vm","messages":[
            {"role":"system","content":"s"},{"role":"user","content":"u"}],
            "max_tokens":50,"stream":true});
        let req = parse_request(&body).unwrap();
        assert_eq!(req.items.len(), 2);
        assert!(req.stream);
        assert_eq!(req.max_tokens, Some(50));
    }
    #[test]
    fn format_response_shape() {
        let resp = UnifiedResponse {
            items: vec![Item::Message { role: Role::Assistant, content: vec![ContentBlock::Text("hi".into())] }],
            usage: Usage { input_tokens: 1, output_tokens: 2 }, model_id: "m".into(), calls: vec![] };
        let j = format_response(&resp);
        assert_eq!(j["choices"][0]["message"]["content"], "hi");
        assert_eq!(j["usage"]["completion_tokens"], 2);
        assert_eq!(j["object"], "chat.completion");
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib ingress::openai_chat` → FAIL。

- [ ] **Step 3: 实现 ingress/mod.rs + sse_out.rs**

```rust
// ingress/mod.rs
pub mod anthropic;
pub mod openai_chat;
pub mod openai_responses;
pub mod sse_out;

use serde_json::Value;

pub fn extract_model(body: &Value) -> Option<String> {
    body.get("model").and_then(|v| v.as_str()).map(String::from)
}
pub fn wants_stream(body: &Value) -> bool {
    body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)
}
```

```rust
// ingress/sse_out.rs
pub fn frame(payload: &str) -> String { format!("data: {payload}\n\n") }
```

- [ ] **Step 4: 实现 openai_chat.rs**

```rust
use serde_json::{json, Value};

use crate::error::FusionError;
use crate::unified::*;

pub fn parse_request(body: &Value) -> Result<UnifiedRequest, FusionError> {
    let msgs = body.get("messages").and_then(|v| v.as_array())
        .ok_or_else(|| FusionError::InvalidRequest("messages required".into()))?;
    let mut items = Vec::new();
    for m in msgs {
        let role = match m.get("role").and_then(|v| v.as_str()).unwrap_or("user") {
            "system" => Role::System, "assistant" => Role::Assistant, "tool" => Role::Tool, _ => Role::User };
        let text = m.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
        items.push(Item::Message { role, content: vec![ContentBlock::Text(text)] });
    }
    Ok(UnifiedRequest {
        items, tools: vec![],
        max_tokens: body.get("max_tokens").and_then(|v| v.as_u64()).map(|v| v as u32),
        temperature: body.get("temperature").and_then(|v| v.as_f64()).map(|v| v as f32),
        stream: body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false),
        raw_extra: Value::Null })
}

fn answer_text(resp: &UnifiedResponse) -> String {
    resp.items.iter().find_map(|i| match i {
        Item::Message { content, .. } => Some(content.iter().filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.clone()), _ => None }).collect::<String>()),
        _ => None }).unwrap_or_default()
}

pub fn format_response(resp: &UnifiedResponse) -> Value {
    json!({
        "id": "chatcmpl-localfusion", "object": "chat.completion", "model": resp.model_id,
        "choices": [{"index": 0, "finish_reason": "stop",
            "message": {"role": "assistant", "content": answer_text(resp)}}],
        "usage": {"prompt_tokens": resp.usage.input_tokens, "completion_tokens": resp.usage.output_tokens,
            "total_tokens": resp.usage.input_tokens + resp.usage.output_tokens}})
}

pub fn sse_events(ev: &UnifiedStreamEvent) -> Vec<String> {
    match ev {
        UnifiedStreamEvent::TextDelta { text } => vec![json!({
            "object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":text}}]}).to_string()],
        UnifiedStreamEvent::Done { usage, .. } => vec![
            json!({"object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":usage.input_tokens,"completion_tokens":usage.output_tokens,
                    "total_tokens":usage.input_tokens+usage.output_tokens}}).to_string(),
            "[DONE]".to_string()],
        UnifiedStreamEvent::Error { message, .. } => vec![
            json!({"error":{"message":message}}).to_string(), "[DONE]".to_string()],
        _ => vec![],
    }
}
```

- [ ] **Step 5: 占位 openai_responses.rs / anthropic.rs**

各含 `parse_request`/`format_response`/`sse_events` 返回 todo（`Err(FusionError::InvalidRequest("todo".into()))` / 空 Value / 空 Vec），P4-T03/04 实现。

- [ ] **Step 6: 运行确认通过 + 提交**

```bash
cargo test --lib ingress::openai_chat && cargo clippy --all-targets
git add src/lib.rs src/ingress/
git commit -m "feat: OpenAI Chat 入口解析/响应/SSE 翻译 + sse_out helper"
```
