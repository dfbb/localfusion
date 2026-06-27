// P4-T04: Anthropic Messages API ingress translation layer
// Handles Anthropic body ↔ Unified format conversion (system + messages[].content supports string or block array)
use serde_json::{json, Value};

use crate::error::FusionError;
use crate::unified::*;

/// Converts an Anthropic content field (string or block array) to plain text
fn content_to_text(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        return arr
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()).map(String::from))
            .collect();
    }
    String::new()
}

/// Parses an Anthropic Messages request body into a UnifiedRequest
pub fn parse_request(body: &Value) -> Result<UnifiedRequest, FusionError> {
    let mut items = Vec::new();

    // Extract the optional system field
    if let Some(sys) = body.get("system").and_then(|v| v.as_str()) {
        if !sys.is_empty() {
            items.push(Item::Message {
                role: Role::System,
                content: vec![ContentBlock::Text(sys.into())],
            });
        }
    }

    // Extract the required messages array
    let msgs = body
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| FusionError::InvalidRequest("messages required".into()))?;

    for m in msgs {
        let role = match m.get("role").and_then(|v| v.as_str()).unwrap_or("user") {
            "assistant" => Role::Assistant,
            _ => Role::User,
        };
        let text = m.get("content").map(content_to_text).unwrap_or_default();
        items.push(Item::Message {
            role,
            content: vec![ContentBlock::Text(text)],
        });
    }

    Ok(UnifiedRequest {
        items,
        tools: vec![],
        max_tokens: body
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        temperature: body
            .get("temperature")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32),
        stream: body
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        raw_extra: Value::Null,
    })
}

/// Extracts the text content of the first assistant message from a UnifiedResponse
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

/// Formats a UnifiedResponse as an Anthropic Messages response body
pub fn format_response(resp: &UnifiedResponse) -> Value {
    json!({
        "id": "msg-localfusion",
        "type": "message",
        "role": "assistant",
        "model": resp.model_id,
        "stop_reason": "end_turn",
        "content": [{"type": "text", "text": answer_text(resp)}],
        "usage": {
            "input_tokens": resp.usage.input_tokens,
            "output_tokens": resp.usage.output_tokens
        }
    })
}

/// Formats an error body per the Anthropic protocol (non-streaming)
pub fn format_error(message: &str) -> Value {
    json!({"type": "error", "error": {"type": "invalid_request_error", "message": message}})
}

/// Converts a UnifiedStreamEvent into a list of Anthropic SSE data lines
/// Note: v1 sends everything via sse_out's `data:` frames; no separate `event:` lines are emitted
pub fn sse_events(ev: &UnifiedStreamEvent) -> Vec<String> {
    match ev {
        UnifiedStreamEvent::Started { .. } => vec![
            json!({"type":"message_start","message":{"type":"message","role":"assistant","content":[]}}).to_string(),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}).to_string(),
        ],
        UnifiedStreamEvent::TextDelta { text } => vec![
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}).to_string(),
        ],
        UnifiedStreamEvent::Done { usage, .. } => vec![
            json!({"type":"content_block_stop","index":0}).to_string(),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                "usage":{"output_tokens":usage.output_tokens}}).to_string(),
            json!({"type":"message_stop"}).to_string(),
        ],
        UnifiedStreamEvent::Error { message, .. } => vec![
            json!({"type":"error","error":{"message":message}}).to_string(),
        ],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let resp = UnifiedResponse {
            items: vec![Item::Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text("a".into())],
            }],
            usage: Usage {
                input_tokens: 3,
                output_tokens: 5,
            },
            model_id: "m".into(),
            calls: vec![],
        };
        let j = format_response(&resp);
        assert_eq!(j["type"], "message");
        assert_eq!(j["content"][0]["text"], "a");
        assert_eq!(j["usage"]["output_tokens"], 5);
    }
}
