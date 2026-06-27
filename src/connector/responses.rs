//! OpenAI Responses API 出口连接器
//! 请求格式：input 数组（每项含 role + content[{type:input_text,text}]）
//! SSE 事件：response.output_text.delta / response.completed

use async_trait::async_trait;
use serde_json::{json, Value};

use super::sse::{run_egress, SseTranslator};
use super::{build_headers, egress_url, Connector, ConnectorKind, EgressCtx};
use crate::unified::{
    CallRole, CallStatus, ConnError, ContentBlock, Item, ModelUsage, Role, UnifiedRequest,
    UnifiedResponse, UnifiedStream, UnifiedStreamEvent, Usage,
};

// ── 请求构建 ──────────────────────────────────────────────────────────────────

/// 将 UnifiedRequest 转换为 OpenAI Responses API 请求体 JSON
/// 每条消息映射为 input 数组中的 {role, content:[{type:input_text,text}]}
pub(super) fn build_responses_request(req: &UnifiedRequest, ctx: &EgressCtx) -> Value {
    let mut input: Vec<Value> = Vec::new();

    for item in &req.items {
        if let Item::Message { role, content } = item {
            let role_s = match role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "user",
            };
            // 拼接所有文本块（忽略图片等非文本块）
            let text: String = content
                .iter()
                .filter_map(|c| match c {
                    ContentBlock::Text(t) => Some(t.clone()),
                    _ => None,
                })
                .collect();
            // assistant 历史消息的内容部件应为 output_text，user/system/tool 为 input_text
            let content_type = match role {
                Role::Assistant => "output_text",
                _ => "input_text",
            };
            input.push(json!({
                "role": role_s,
                "content": [{"type": content_type, "text": text}]
            }));
        }
    }

    let mut body = json!({
        "model": ctx.model,
        "input": input,
        "stream": req.stream,
    });

    // max_output_tokens：优先取请求字段，其次取 ctx 默认值
    if let Some(mt) = req.max_tokens.or(ctx.default_max_tokens) {
        body["max_output_tokens"] = json!(mt);
    }
    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }

    body
}

// ── 非流式响应解析 ────────────────────────────────────────────────────────────

/// 解析 Responses API 非流式响应 JSON，返回 UnifiedResponse
/// 响应格式：output:[{type:message, content:[{type:output_text, text}]}] + usage
pub(super) fn parse_responses_response(json: &Value, model_id: &str) -> UnifiedResponse {
    let mut text = String::new();

    if let Some(output) = json.get("output").and_then(|v| v.as_array()) {
        for item in output {
            if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                        if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                            text.push_str(t);
                        }
                    }
                }
            }
        }
    }

    let input = json
        .pointer("/usage/input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = json
        .pointer("/usage/output_tokens")
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

/// Responses API SSE 流翻译状态机
/// 监听 response.output_text.delta 增量事件和 response.completed 用量事件
pub(super) struct ResponsesSseState {
    model_id: String,
    /// 累积文本（用于无 usage 时的 token 估算）
    text: String,
    input_tokens: u64,
    output_tokens: u64,
    /// 是否收到过 usage 字段
    has_usage: bool,
}

impl ResponsesSseState {
    pub(super) fn new(model_id: String) -> Self {
        Self {
            model_id,
            text: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            has_usage: false,
        }
    }
}

impl SseTranslator for ResponsesSseState {
    fn push(&mut self, evt: &Value) -> Result<Vec<UnifiedStreamEvent>, ConnError> {
        let ty = evt.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let mut out = Vec::new();

        match ty {
            // 文本增量事件
            "response.output_text.delta" => {
                if let Some(d) = evt.get("delta").and_then(|v| v.as_str()) {
                    if !d.is_empty() {
                        self.text.push_str(d);
                        out.push(UnifiedStreamEvent::TextDelta { text: d.into() });
                    }
                }
            }
            // 完成/截断事件，携带用量数据
            "response.completed" | "response.incomplete" => {
                if let Some(u) = evt.pointer("/response/usage") {
                    self.input_tokens = u
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    self.output_tokens = u
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    self.has_usage = true;
                }
            }
            // 上游错误事件，立即终止流
            "error" | "response.failed" => {
                return Err(ConnError::HardFail(format!("responses sse error: {evt}")));
            }
            _ => {}
        }

        Ok(out)
    }

    /// 流结束时产出 Done 事件，携带 usage 和 ModelUsage
    fn finish(&mut self) -> Vec<UnifiedStreamEvent> {
        let estimated = !self.has_usage;
        // 若无 usage 字段则按字符数粗估（每4字符约1个 token）
        let out_tokens = if self.output_tokens > 0 {
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
            finish_reason: None,
        }]
    }
}

// ── Connector 实现 ────────────────────────────────────────────────────────────

pub struct ResponsesConnector;

#[async_trait]
impl Connector for ResponsesConnector {
    /// 非流式补全：发送 Responses API 请求并解析 JSON 响应
    async fn complete(
        &self,
        req: &UnifiedRequest,
        ctx: &EgressCtx,
    ) -> Result<UnifiedResponse, ConnError> {
        let mut body = build_responses_request(req, ctx);
        body["stream"] = json!(false);

        let url = egress_url(&ctx.base_url, ConnectorKind::Responses);
        let headers =
            build_headers(ctx.auth, ctx.key.as_deref(), ctx.anthropic_version.as_deref())?;

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

        Ok(parse_responses_response(&json, &ctx.model))
    }

    /// 流式补全：发送 SSE 请求，使用 ResponsesSseState 翻译事件
    async fn stream(
        &self,
        req: &UnifiedRequest,
        ctx: &EgressCtx,
    ) -> Result<UnifiedStream, ConnError> {
        let mut body = build_responses_request(req, ctx);
        body["stream"] = json!(true);

        let url = egress_url(&ctx.base_url, ConnectorKind::Responses);
        let headers =
            build_headers(ctx.auth, ctx.key.as_deref(), ctx.anthropic_version.as_deref())?;

        run_egress(
            url,
            headers,
            body,
            ctx.http.clone(),
            Box::new(ResponsesSseState::new(ctx.model.clone())),
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

    fn ctx() -> EgressCtx {
        EgressCtx {
            base_url: "u".into(),
            model: "gpt-5".into(),
            auth: AuthKind::Bearer,
            key: Some("k".into()),
            anthropic_version: None,
            default_max_tokens: None,
            http: reqwest::Client::new(),
        }
    }

    fn req() -> UnifiedRequest {
        UnifiedRequest {
            items: vec![Item::Message {
                role: Role::User,
                content: vec![ContentBlock::Text("hi".into())],
            }],
            tools: vec![],
            max_tokens: Some(40),
            temperature: None,
            stream: false,
            raw_extra: serde_json::Value::Null,
        }
    }

    #[test]
    fn build_request_uses_input_array() {
        let b = build_responses_request(&req(), &ctx());
        assert_eq!(b["model"], "gpt-5");
        assert_eq!(b["input"][0]["role"], "user");
        assert_eq!(b["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(b["max_output_tokens"], 40);
    }

    #[test]
    fn parse_response_output_text_and_usage() {
        let json = serde_json::json!({
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "答"}]}],
            "usage": {"input_tokens": 2, "output_tokens": 4}
        });
        let r = parse_responses_response(&json, "gpt-5");
        assert_eq!(r.usage.output_tokens, 4);
        match &r.items[0] {
            Item::Message { content, .. } => match &content[0] {
                ContentBlock::Text(t) => assert_eq!(t, "答"),
                _ => panic!("expected text block"),
            },
            _ => panic!("expected message item"),
        }
    }

    #[test]
    fn sse_delta_and_completed() {
        let mut st = ResponsesSseState::new("gpt-5".into());
        let e = st
            .push(&serde_json::json!({"type": "response.output_text.delta", "delta": "你"}))
            .unwrap();
        assert!(matches!(e[0], UnifiedStreamEvent::TextDelta { .. }));
        st.push(&serde_json::json!({
            "type": "response.completed",
            "response": {"usage": {"input_tokens": 1, "output_tokens": 5}}
        }))
        .unwrap();
        let fin = st.finish();
        assert!(fin.iter().any(|e| matches!(
            e,
            UnifiedStreamEvent::Done { call: Some(c), .. } if c.output_tokens == 5
        )));
    }
}
