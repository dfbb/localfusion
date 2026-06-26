# P4-T04 Anthropic 入口翻译

**阶段:** 4 装配 · **前置:** P4-T02 · 见全局约束: `00-index.md`

**Goal:** Anthropic body ↔ Unified（`system` + `messages[].content` 可为 string 或 block 数组）。

**Files:** Modify: `src/ingress/anthropic.rs`

**Produces:** `parse_request`、`format_response`、`sse_events`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::unified::*;
    #[test]
    fn parse_system_and_messages() {
        let b = serde_json::json!({"model":"vm","system":"sys","max_tokens":40,
            "messages":[{"role":"user","content":"hi"}]});
        let req = parse_request(&b).unwrap();
        assert_eq!(req.items.len(), 2);
        assert_eq!(req.max_tokens, Some(40));
    }
    #[test]
    fn format_response_content_blocks() {
        let resp = UnifiedResponse { items: vec![Item::Message { role: Role::Assistant,
            content: vec![ContentBlock::Text("a".into())] }],
            usage: Usage { input_tokens: 3, output_tokens: 5 }, model_id: "m".into(), calls: vec![] };
        let j = format_response(&resp);
        assert_eq!(j["type"], "message");
        assert_eq!(j["content"][0]["text"], "a");
        assert_eq!(j["usage"]["output_tokens"], 5);
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib ingress::anthropic` → FAIL。

- [ ] **Step 3: 实现 anthropic.rs**

```rust
use serde_json::{json, Value};

use crate::error::FusionError;
use crate::unified::*;

fn content_to_text(content: &Value) -> String {
    if let Some(s) = content.as_str() { return s.to_string(); }
    if let Some(arr) = content.as_array() {
        return arr.iter().filter_map(|b| b.get("text").and_then(|t| t.as_str()).map(String::from)).collect();
    }
    String::new()
}

pub fn parse_request(body: &Value) -> Result<UnifiedRequest, FusionError> {
    let mut items = Vec::new();
    if let Some(sys) = body.get("system").and_then(|v| v.as_str()) {
        if !sys.is_empty() {
            items.push(Item::Message { role: Role::System, content: vec![ContentBlock::Text(sys.into())] });
        }
    }
    let msgs = body.get("messages").and_then(|v| v.as_array())
        .ok_or_else(|| FusionError::InvalidRequest("messages required".into()))?;
    for m in msgs {
        let role = match m.get("role").and_then(|v| v.as_str()).unwrap_or("user") {
            "assistant" => Role::Assistant, _ => Role::User };
        let text = m.get("content").map(content_to_text).unwrap_or_default();
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
        "id": "msg-localfusion", "type": "message", "role": "assistant", "model": resp.model_id,
        "stop_reason": "end_turn", "content": [{"type": "text", "text": answer_text(resp)}],
        "usage": {"input_tokens": resp.usage.input_tokens, "output_tokens": resp.usage.output_tokens}})
}

pub fn sse_events(ev: &UnifiedStreamEvent) -> Vec<String> {
    match ev {
        UnifiedStreamEvent::Started { .. } => vec![
            json!({"type":"message_start","message":{"type":"message","role":"assistant","content":[]}}).to_string(),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}).to_string()],
        UnifiedStreamEvent::TextDelta { text } => vec![
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}).to_string()],
        UnifiedStreamEvent::Done { usage, .. } => vec![
            json!({"type":"content_block_stop","index":0}).to_string(),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                "usage":{"output_tokens":usage.output_tokens}}).to_string(),
            json!({"type":"message_stop"}).to_string()],
        UnifiedStreamEvent::Error { message, .. } => vec![
            json!({"type":"error","error":{"message":message}}).to_string()],
        _ => vec![],
    }
}
```

> 注：Anthropic SSE 规范用 `event:` 行区分类型，但多数客户端按 `data:` 内的 `type` 字段解析即可。v1 用 sse_out 的 `data:` 帧统一发送。

- [ ] **Step 4: 运行确认通过 + 提交**

```bash
cargo test --lib ingress::anthropic && cargo clippy --all-targets
git add src/ingress/anthropic.rs
git commit -m "feat: Anthropic 入口(system+messages + content blocks + SSE 翻译)"
```
