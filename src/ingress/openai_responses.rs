// P4-T03: OpenAI Responses API ingress translation layer
use serde_json::{json, Value};

use crate::error::FusionError;
use crate::unified::*;

/// Parse an OpenAI Responses API request body.
/// `input` may be a string (single user message) or an array (multiple message objects).
pub fn parse_request(body: &Value) -> Result<UnifiedRequest, FusionError> {
    let mut items = Vec::new();
    match body.get("input") {
        Some(Value::String(s)) => {
            items.push(Item::Message {
                role: Role::User,
                content: vec![ContentBlock::Text(s.clone())],
            });
        }
        Some(Value::Array(arr)) => {
            for it in arr {
                let role = match it.get("role").and_then(|v| v.as_str()).unwrap_or("user") {
                    "system" => Role::System,
                    "assistant" => Role::Assistant,
                    "tool" => Role::Tool,
                    _ => Role::User,
                };
                // content may be an array (blocks) or a plain string
                let text = it
                    .get("content")
                    .and_then(|c| c.as_array())
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|b| b.get("text").and_then(|t| t.as_str()).map(String::from))
                            .collect::<String>()
                    })
                    .or_else(|| it.get("content").and_then(|v| v.as_str()).map(String::from))
                    .unwrap_or_default();
                items.push(Item::Message {
                    role,
                    content: vec![ContentBlock::Text(text)],
                });
            }
        }
        _ => return Err(FusionError::InvalidRequest("input required".into())),
    }
    Ok(UnifiedRequest {
        items,
        tools: vec![],
        max_tokens: body.get("max_output_tokens").and_then(|v| v.as_u64()).map(|v| v as u32),
        temperature: body.get("temperature").and_then(|v| v.as_f64()).map(|v| v as f32),
        stream: body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false),
        raw_extra: Value::Null,
    })
}

/// Extract the first assistant text from a response.
fn answer_text(resp: &UnifiedResponse) -> String {
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

/// Format an OpenAI Responses API response body.
pub fn format_response(resp: &UnifiedResponse) -> Value {
    json!({
        "id": "resp-localfusion",
        "object": "response",
        "model": resp.model_id,
        "status": "completed",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": answer_text(resp)}]
        }],
        "usage": {
            "input_tokens": resp.usage.input_tokens,
            "output_tokens": resp.usage.output_tokens,
            "total_tokens": resp.usage.input_tokens + resp.usage.output_tokens
        }
    })
}

/// Format an error body following the Responses protocol (non-streaming).
pub fn format_error(message: &str) -> Value {
    json!({"error": {"message": message, "type": "invalid_request_error"}})
}

/// Map a UnifiedStreamEvent to a list of Responses API SSE event JSON strings.
pub fn sse_events(ev: &UnifiedStreamEvent) -> Vec<String> {
    match ev {
        UnifiedStreamEvent::TextDelta { text } => {
            vec![json!({"type": "response.output_text.delta", "delta": text}).to_string()]
        }
        UnifiedStreamEvent::Done { usage, .. } => vec![json!({
            "type": "response.completed",
            "response": {
                "status": "completed",
                "usage": {
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens
                }
            }
        })
        .to_string()],
        UnifiedStreamEvent::Error { message, .. } => {
            vec![json!({"type": "response.failed", "response": {"error": {"message": message}}})
                .to_string()]
        }
        _ => vec![],
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_input_string_and_array() {
        let b1 = serde_json::json!({"model":"vm","input":"hello","stream":false});
        assert_eq!(parse_request(&b1).unwrap().items.len(), 1);
        let b2 = serde_json::json!({"model":"vm","input":[
            {"role":"user","content":[{"type":"input_text","text":"hi"}]}]});
        assert_eq!(parse_request(&b2).unwrap().items.len(), 1);
    }

    #[test]
    fn format_response_output_array() {
        let resp = UnifiedResponse {
            items: vec![Item::Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text("a".into())],
            }],
            usage: Usage { input_tokens: 1, output_tokens: 2 },
            model_id: "m".into(),
            calls: vec![],
        };
        let j = format_response(&resp);
        assert_eq!(j["object"], "response");
        assert_eq!(j["output"][0]["content"][0]["text"], "a");
        assert_eq!(j["usage"]["output_tokens"], 2);
    }
}
