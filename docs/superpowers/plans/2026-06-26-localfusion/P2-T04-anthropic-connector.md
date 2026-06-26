# P2-T04 AnthropicConnector

**阶段:** 2 Connector · **前置:** P2-T01, P2-T02 · 见全局约束: `00-index.md`

**Goal:** Anthropic Messages 出口。参考 llm-switch `anthropic_req.rs`/`anthropic_sse.rs`（设计 §6.2）。请求体 `{model,max_tokens,system,messages:[{role,content:[{type:text,text}]}]}`；SSE `content_block_delta`/`message_delta`/`message_stop`。

**Files:** Modify: `src/connector/anthropic.rs`

**Produces:** `AnthropicConnector` 实现 `Connector`；`build_anthropic_request`、`parse_anthropic_response`、`AnthropicSseState`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::unified::*;
    fn req() -> UnifiedRequest {
        UnifiedRequest { items: vec![
            Item::Message { role: Role::System, content: vec![ContentBlock::Text("sys".into())] },
            Item::Message { role: Role::User, content: vec![ContentBlock::Text("hi".into())] }],
            tools: vec![], max_tokens: Some(64), temperature: None, stream: false, raw_extra: serde_json::Value::Null }
    }
    fn ctx() -> EgressCtx {
        EgressCtx { base_url:"u".into(), model:"claude-x".into(), auth: AuthKind::XApiKey,
            key: Some("k".into()), anthropic_version: Some("2023-06-01".into()),
            default_max_tokens: Some(1024), http: reqwest::Client::new() }
    }
    #[test]
    fn build_request_splits_system_and_messages() {
        let b = build_anthropic_request(&req(), &ctx());
        assert_eq!(b["model"], "claude-x");
        assert_eq!(b["system"], "sys");
        assert_eq!(b["messages"][0]["role"], "user");
        assert_eq!(b["max_tokens"], 64);
    }
    #[test]
    fn max_tokens_falls_back_to_default() {
        let mut r = req(); r.max_tokens = None;
        assert_eq!(build_anthropic_request(&r, &ctx())["max_tokens"], 1024);
    }
    #[test]
    fn parse_response_text_and_usage() {
        let json = serde_json::json!({"content":[{"type":"text","text":"答"}],
            "usage":{"input_tokens":4,"output_tokens":6}});
        let r = parse_anthropic_response(&json, "claude-x");
        assert_eq!(r.usage.input_tokens, 4);
        assert_eq!(r.usage.output_tokens, 6);
    }
    #[test]
    fn sse_accumulates_and_done() {
        let mut st = AnthropicSseState::new("claude-x".into());
        let e = st.push(&serde_json::json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"嗨"}})).unwrap();
        assert!(matches!(e[0], UnifiedStreamEvent::TextDelta { .. }));
        st.push(&serde_json::json!({"type":"message_delta","usage":{"output_tokens":9}})).unwrap();
        let fin = st.finish();
        assert!(fin.iter().any(|e| matches!(e, UnifiedStreamEvent::Done { call: Some(c), .. } if c.output_tokens == 9)));
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib connector::anthropic` → FAIL。

- [ ] **Step 3: 实现（请求 + 响应）**

```rust
use async_trait::async_trait;
use serde_json::{json, Value};

use super::sse::{run_egress, SseTranslator};
use super::{build_headers, egress_url, Connector, ConnectorKind, EgressCtx};
use crate::unified::*;

pub struct AnthropicConnector;

pub(super) fn build_anthropic_request(req: &UnifiedRequest, ctx: &EgressCtx) -> Value {
    let mut system = String::new();
    let mut messages: Vec<Value> = Vec::new();
    for item in &req.items {
        if let Item::Message { role, content } = item {
            let text: String = content.iter().filter_map(|c| match c {
                ContentBlock::Text(t) => Some(t.clone()), _ => None }).collect();
            match role {
                Role::System => { if !system.is_empty() { system.push('\n'); } system.push_str(&text); }
                Role::User | Role::Tool => messages.push(json!({"role":"user","content":[{"type":"text","text":text}]})),
                Role::Assistant => messages.push(json!({"role":"assistant","content":[{"type":"text","text":text}]})),
            }
        }
    }
    let max_tokens = req.max_tokens.or(ctx.default_max_tokens).unwrap_or(1024);
    let mut body = json!({"model": ctx.model, "max_tokens": max_tokens, "messages": messages, "stream": req.stream});
    if !system.is_empty() { body["system"] = json!(system); }
    if let Some(t) = req.temperature { body["temperature"] = json!(t); }
    body
}

pub(super) fn parse_anthropic_response(json: &Value, model_id: &str) -> UnifiedResponse {
    let text: String = json.get("content").and_then(|c| c.as_array()).map(|arr| {
        arr.iter().filter_map(|b| if b.get("type").and_then(|t| t.as_str()) == Some("text") {
            b.get("text").and_then(|t| t.as_str()).map(String::from) } else { None }).collect()
    }).unwrap_or_default();
    let input = json.pointer("/usage/input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output = json.pointer("/usage/output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let call = ModelUsage { model_id: model_id.into(), role: CallRole::Member,
        input_tokens: input, output_tokens: output, cost: 0.0, status: CallStatus::Ok,
        estimated: json.get("usage").is_none(), latency_secs: 0.0 };
    UnifiedResponse { items: vec![Item::Message { role: Role::Assistant, content: vec![ContentBlock::Text(text)] }],
        usage: Usage { input_tokens: input, output_tokens: output }, model_id: model_id.into(), calls: vec![call] }
}
```

- [ ] **Step 4: 实现（AnthropicSseState）**

```rust
pub(super) struct AnthropicSseState {
    model_id: String, text: String, input_tokens: u64, output_tokens: u64,
    has_usage: bool, stop_reason: Option<String>,
}
impl AnthropicSseState {
    pub(super) fn new(model_id: String) -> Self {
        Self { model_id, text: String::new(), input_tokens: 0, output_tokens: 0, has_usage: false, stop_reason: None }
    }
}
impl SseTranslator for AnthropicSseState {
    fn push(&mut self, evt: &Value) -> Result<Vec<UnifiedStreamEvent>, ConnError> {
        let ty = evt.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let mut out = Vec::new();
        match ty {
            "content_block_delta" => {
                if let Some(t) = evt.pointer("/delta/text").and_then(|v| v.as_str()) {
                    if !t.is_empty() { self.text.push_str(t); out.push(UnifiedStreamEvent::TextDelta { text: t.into() }); }
                }
            }
            "message_start" => {
                if let Some(u) = evt.pointer("/message/usage/input_tokens").and_then(|v| v.as_u64()) {
                    self.input_tokens = u; self.has_usage = true;
                }
            }
            "message_delta" => {
                if let Some(o) = evt.pointer("/usage/output_tokens").and_then(|v| v.as_u64()) {
                    self.output_tokens = o; self.has_usage = true;
                }
                if let Some(sr) = evt.pointer("/delta/stop_reason").and_then(|v| v.as_str()) {
                    self.stop_reason = Some(sr.to_string());
                }
            }
            "error" => return Err(ConnError::HardFail(format!("anthropic sse error: {evt}"))),
            _ => {}
        }
        Ok(out)
    }
    fn finish(&mut self) -> Vec<UnifiedStreamEvent> {
        let estimated = !self.has_usage;
        let out_tokens = if self.output_tokens > 0 { self.output_tokens } else { (self.text.chars().count()/4) as u64 };
        let usage = Usage { input_tokens: self.input_tokens, output_tokens: out_tokens };
        let call = ModelUsage { model_id: self.model_id.clone(), role: CallRole::Member,
            input_tokens: self.input_tokens, output_tokens: out_tokens, cost: 0.0,
            status: CallStatus::Ok, estimated, latency_secs: 0.0 };
        vec![UnifiedStreamEvent::Done { usage, call: Some(call), finish_reason: self.stop_reason.clone() }]
    }
}
```

- [ ] **Step 5: 实现（Connector impl）**

```rust
#[async_trait]
impl Connector for AnthropicConnector {
    async fn complete(&self, req: &UnifiedRequest, ctx: &EgressCtx) -> Result<UnifiedResponse, ConnError> {
        let mut body = build_anthropic_request(req, ctx);
        body["stream"] = json!(false);
        let url = egress_url(&ctx.base_url, ConnectorKind::Anthropic);
        let headers = build_headers(ctx.auth, ctx.key.as_deref(), ctx.anthropic_version.as_deref())?;
        let resp = ctx.http.post(&url).headers(headers).json(&body).send().await
            .map_err(|e| ConnError::Http(format!("request failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let t = resp.text().await.unwrap_or_default();
            return Err(ConnError::Http(format!("upstream {status}: {t}")));
        }
        let json: Value = resp.json().await.map_err(|e| ConnError::Http(format!("bad json: {e}")))?;
        Ok(parse_anthropic_response(&json, &ctx.model))
    }
    async fn stream(&self, req: &UnifiedRequest, ctx: &EgressCtx) -> Result<UnifiedStream, ConnError> {
        let mut body = build_anthropic_request(req, ctx);
        body["stream"] = json!(true);
        let url = egress_url(&ctx.base_url, ConnectorKind::Anthropic);
        let headers = build_headers(ctx.auth, ctx.key.as_deref(), ctx.anthropic_version.as_deref())?;
        run_egress(url, headers, body, ctx.http.clone(),
            Box::new(AnthropicSseState::new(ctx.model.clone())), ctx.model.clone()).await
    }
}
```

- [ ] **Step 6: 运行确认通过 + 提交**

```bash
cargo test --lib connector::anthropic && cargo clippy --all-targets
git add src/connector/anthropic.rs
git commit -m "feat: AnthropicConnector(请求/响应/SSE翻译)"
```
