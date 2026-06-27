//! OpenAI Chat Completions 出口连接器
//! 覆盖：请求构建、非流式响应解析、SSE 翻译

use async_trait::async_trait;
use serde_json::{json, Value};

use super::sse::{run_egress, SseTranslator};
use super::{build_headers, egress_url, Connector, ConnectorKind, EgressCtx};
use crate::unified::{
    CallRole, CallStatus, ConnError, ContentBlock, Item, ModelUsage, Role, UnifiedRequest,
    UnifiedResponse, UnifiedStream, UnifiedStreamEvent, Usage,
};

// ── 请求构建 ──────────────────────────────────────────────────────────────────

/// 将 UnifiedRequest 转换为 OpenAI Chat Completions 请求体 JSON
pub(super) fn build_chat_request(req: &UnifiedRequest, ctx: &EgressCtx) -> Value {
    let mut messages: Vec<Value> = Vec::new();

    for item in &req.items {
        match item {
            Item::Message { role, content } => {
                let role_s = match role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                // 将多个 ContentBlock::Text 拼接为单一字符串（忽略 Image）
                let text: String = content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect();
                messages.push(json!({"role": role_s, "content": text}));
            }
            Item::ToolCall { id, name, args } => {
                // 助手发起的工具调用
                messages.push(json!({
                    "role": "assistant",
                    "tool_calls": [{
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": args.to_string()}
                    }]
                }));
            }
            Item::ToolResult { id, content } => {
                // 工具返回结果
                let text: String = content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect();
                messages.push(json!({"role": "tool", "tool_call_id": id, "content": text}));
            }
            // Reasoning 块不映射到 OpenAI messages
            Item::Reasoning { .. } => {}
        }
    }

    let mut body = json!({
        "model": ctx.model,
        "messages": messages,
        "stream": req.stream,
    });

    // max_tokens：优先取请求字段，其次取 ctx 默认值
    if let Some(mt) = req.max_tokens.or(ctx.default_max_tokens) {
        body["max_tokens"] = json!(mt);
    }
    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }

    // 将自定义工具定义转换为 OpenAI function calling 格式（跳过内置工具）
    let tools: Vec<Value> = req
        .tools
        .iter()
        .filter(|t| t.builtin.is_none())
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters
                }
            })
        })
        .collect();
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }

    body
}

// ── 非流式响应解析 ────────────────────────────────────────────────────────────

/// 解析 OpenAI Chat Completions 非流式响应 JSON，返回 UnifiedResponse
pub(super) fn parse_chat_response(json: &Value, model_id: &str) -> UnifiedResponse {
    let text = json
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let input = json
        .pointer("/usage/prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = json
        .pointer("/usage/completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let call = ModelUsage {
        model_id: model_id.into(),
        role: CallRole::Member,
        input_tokens: input,
        output_tokens: output,
        cost: 0.0,
        status: CallStatus::Ok,
        estimated: json.get("usage").is_none(),
        latency_secs: 0.0,
    };

    UnifiedResponse {
        items: vec![Item::Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text(text)],
        }],
        usage: Usage {
            input_tokens: input,
            output_tokens: output,
        },
        model_id: model_id.into(),
        calls: vec![call],
    }
}

// ── SSE 翻译器 ────────────────────────────────────────────────────────────────

/// OpenAI Chat Completions SSE 流翻译状态机
pub(super) struct ChatSseState {
    model_id: String,
    /// 累积的文本内容（用于 usage 估算）
    text: String,
    input_tokens: u64,
    output_tokens: u64,
    /// 是否收到过 usage 字段（stream_options.include_usage=true 时）
    has_usage: bool,
    finish_reason: Option<String>,
}

impl ChatSseState {
    pub(super) fn new(model_id: String) -> Self {
        Self {
            model_id,
            text: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            has_usage: false,
            finish_reason: None,
        }
    }
}

impl SseTranslator for ChatSseState {
    fn push(&mut self, chunk: &Value) -> Result<Vec<UnifiedStreamEvent>, ConnError> {
        // 上游返回错误对象时立即终止
        if let Some(err) = chunk.get("error") {
            return Err(ConnError::HardFail(format!("upstream sse error: {err}")));
        }

        // 解析 usage（stream_options.include_usage=true 时出现在最后一个 chunk）
        if let Some(u) = chunk.get("usage") {
            if !u.is_null() {
                self.input_tokens = u
                    .get("prompt_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                self.output_tokens = u
                    .get("completion_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                self.has_usage = true;
            }
        }

        // 记录 finish_reason
        if let Some(fr) = chunk
            .pointer("/choices/0/finish_reason")
            .and_then(|v| v.as_str())
        {
            if !fr.is_empty() {
                self.finish_reason = Some(fr.to_string());
            }
        }

        // 提取 delta.content 文本增量
        let mut out = Vec::new();
        if let Some(c) = chunk
            .pointer("/choices/0/delta/content")
            .and_then(|v| v.as_str())
        {
            if !c.is_empty() {
                self.text.push_str(c);
                out.push(UnifiedStreamEvent::TextDelta { text: c.into() });
            }
        }

        Ok(out)
    }

    /// 流结束时产出 Done 事件，携带 usage 和 ModelUsage（供统计）
    fn finish(&mut self) -> Vec<UnifiedStreamEvent> {
        let estimated = !self.has_usage;
        // 若无 usage 字段则按字符数粗估（每4字符约1个 token）
        let out_tokens = if self.has_usage {
            self.output_tokens
        } else {
            (self.text.chars().count() / 4) as u64
        };

        let usage = Usage {
            input_tokens: self.input_tokens,
            output_tokens: out_tokens,
        };
        let call = ModelUsage {
            model_id: self.model_id.clone(),
            role: CallRole::Member,
            input_tokens: self.input_tokens,
            output_tokens: out_tokens,
            cost: 0.0,
            status: CallStatus::Ok,
            estimated,
            latency_secs: 0.0,
        };

        vec![UnifiedStreamEvent::Done {
            usage,
            call: Some(call),
            finish_reason: self.finish_reason.clone(),
        }]
    }
}

// ── Connector 实现 ────────────────────────────────────────────────────────────

pub struct ChatConnector;

#[async_trait]
impl Connector for ChatConnector {
    /// 非流式补全：发送请求并解析 JSON 响应
    async fn complete(
        &self,
        req: &UnifiedRequest,
        ctx: &EgressCtx,
    ) -> Result<UnifiedResponse, ConnError> {
        let mut body = build_chat_request(req, ctx);
        body["stream"] = json!(false);

        let url = egress_url(&ctx.base_url, ConnectorKind::Chat);
        let headers = build_headers(ctx.auth, ctx.key.as_deref(), ctx.anthropic_version.as_deref())?;

        let resp = ctx
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| ConnError::Http(format!("request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let t = resp.text().await.unwrap_or_default();
            return Err(super::upstream_error(status, &t));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| ConnError::Http(format!("bad json: {e}")))?;

        Ok(parse_chat_response(&json, &ctx.model))
    }

    /// 流式补全：发送 SSE 请求，使用 ChatSseState 翻译事件
    async fn stream(
        &self,
        req: &UnifiedRequest,
        ctx: &EgressCtx,
    ) -> Result<UnifiedStream, ConnError> {
        let mut body = build_chat_request(req, ctx);
        body["stream"] = json!(true);
        // 要求上游在最后一个 chunk 包含 usage 统计
        body["stream_options"] = json!({"include_usage": true});

        let url = egress_url(&ctx.base_url, ConnectorKind::Chat);
        let headers = build_headers(ctx.auth, ctx.key.as_deref(), ctx.anthropic_version.as_deref())?;

        run_egress(
            url,
            headers,
            body,
            ctx.http.clone(),
            Box::new(ChatSseState::new(ctx.model.clone())),
            ctx.model.clone(),
        )
        .await
    }
}

// ── 单元测试 ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::AuthKind;
    use crate::unified::*;

    fn req(text: &str) -> UnifiedRequest {
        UnifiedRequest {
            items: vec![Item::Message {
                role: Role::User,
                content: vec![ContentBlock::Text(text.into())],
            }],
            tools: vec![],
            max_tokens: Some(100),
            temperature: None,
            stream: false,
            raw_extra: serde_json::Value::Null,
        }
    }

    fn ctx() -> EgressCtx {
        EgressCtx {
            base_url: "u".into(),
            model: "gpt-4o".into(),
            auth: AuthKind::Bearer,
            key: Some("k".into()),
            anthropic_version: None,
            default_max_tokens: None,
            http: reqwest::Client::new(),
        }
    }

    #[test]
    fn build_request_maps_messages_and_model() {
        let body = build_chat_request(&req("hi"), &ctx());
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hi");
        assert_eq!(body["max_tokens"], 100);
    }

    #[test]
    fn parse_response_extracts_text_and_usage() {
        let json = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "答案"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 5}
        });
        let resp = parse_chat_response(&json, "gpt-4o");
        assert_eq!(resp.usage.input_tokens, 3);
        assert_eq!(resp.usage.output_tokens, 5);
        match &resp.items[0] {
            Item::Message { content, .. } => match &content[0] {
                ContentBlock::Text(t) => assert_eq!(t, "答案"),
                _ => panic!("expected text block"),
            },
            _ => panic!("expected message item"),
        }
    }

    #[test]
    fn sse_state_accumulates_text_and_finishes_with_done() {
        let mut st = ChatSseState::new("gpt-4o".into());
        let evs = st
            .push(&serde_json::json!({"choices":[{"delta":{"content":"你"}}]}))
            .unwrap();
        assert!(matches!(evs[0], UnifiedStreamEvent::TextDelta { .. }));
        st.push(&serde_json::json!({
            "choices":[{"delta":{"content":"好"}}],
            "usage":{"prompt_tokens":1,"completion_tokens":2}
        }))
        .unwrap();
        let fin = st.finish();
        let done = fin
            .iter()
            .find_map(|e| match e {
                UnifiedStreamEvent::Done { usage, call, .. } => {
                    Some((usage.output_tokens, call.is_some()))
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(done.0, 2);
        assert!(done.1);
    }
}
