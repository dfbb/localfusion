//! OpenAI Responses API egress connector
//! Request format: input array (each item contains role + content[{type:input_text,text}])
//! SSE events: response.output_text.delta / response.completed

use async_trait::async_trait;
use serde_json::{json, Value};

use super::sse::{run_egress, SseTranslator};
use super::{build_headers, egress_url, Connector, ConnectorKind, EgressCtx};
use crate::unified::{
    CallRole, CallStatus, ConnError, ContentBlock, Item, ModelUsage, Role, UnifiedRequest,
    UnifiedResponse, UnifiedStream, UnifiedStreamEvent, Usage,
};

// ── Request Building ──────────────────────────────────────────────────────────

/// Convert a UnifiedRequest into an OpenAI Responses API request body JSON
/// Each message is mapped to an entry in the input array as {role, content:[{type:input_text,text}]}
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
            // Concatenate all text blocks (ignore non-text blocks such as images)
            let text: String = content
                .iter()
                .filter_map(|c| match c {
                    ContentBlock::Text(t) => Some(t.clone()),
                    _ => None,
                })
                .collect();
            // assistant history message content type should be output_text; user/system/tool use input_text
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

    // max_output_tokens: use the request field first, fall back to ctx default
    if let Some(mt) = req.max_tokens.or(ctx.default_max_tokens) {
        body["max_output_tokens"] = json!(mt);
    }
    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }

    body
}

// ── Non-streaming Response Parsing ───────────────────────────────────────────

/// Parse a Responses API non-streaming response JSON, returning a UnifiedResponse
/// Response format: output:[{type:message, content:[{type:output_text, text}]}] + usage
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
        billable_input_tokens: input,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
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

// ── SSE Translator ────────────────────────────────────────────────────────────

/// Responses API SSE stream translation state machine
/// Listens for response.output_text.delta incremental events and response.completed usage events
pub(super) struct ResponsesSseState {
    model_id: String,
    /// Accumulated text (used for token estimation when usage is absent)
    text: String,
    input_tokens: u64,
    output_tokens: u64,
    /// Whether a usage field has been received
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
            // Text delta event
            "response.output_text.delta" => {
                if let Some(d) = evt.get("delta").and_then(|v| v.as_str()) {
                    if !d.is_empty() {
                        self.text.push_str(d);
                        out.push(UnifiedStreamEvent::TextDelta { text: d.into() });
                    }
                }
            }
            // Completion/truncation event, carries usage data
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
            // Upstream error event, terminate the stream immediately
            "error" | "response.failed" => {
                return Err(ConnError::HardFail(format!("responses sse error: {evt}")));
            }
            _ => {}
        }

        Ok(out)
    }

    /// Emit a Done event at stream end, carrying usage and ModelUsage
    fn finish(&mut self) -> Vec<UnifiedStreamEvent> {
        let estimated = !self.has_usage;
        // If no usage field, rough-estimate from character count (approx 1 token per 4 chars)
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
            billable_input_tokens: self.input_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
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

// ── Connector Implementation ──────────────────────────────────────────────────

pub struct ResponsesConnector;

#[async_trait]
impl Connector for ResponsesConnector {
    /// Non-streaming completion: send a Responses API request and parse the JSON response
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
            super::log_http_exchange("POST", &url, &body, status.as_u16(), &t);
            return Err(super::upstream_error(status, &t));
        }

        let text = resp.text().await.map_err(|e| ConnError::Http(format!("read body: {e}")))?;
        super::log_http_exchange("POST", &url, &body, status.as_u16(), &text);
        if text.trim().is_empty() {
            return Err(ConnError::Http("upstream returned empty response body".into()));
        }
        let json: Value = serde_json::from_str(&text)
            .map_err(|e| ConnError::Http(format!("bad json: {e}")))?;

        Ok(parse_responses_response(&json, &ctx.model))
    }

    /// Streaming completion: send an SSE request and translate events using ResponsesSseState
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

// ── Unit Tests ────────────────────────────────────────────────────────────────

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
