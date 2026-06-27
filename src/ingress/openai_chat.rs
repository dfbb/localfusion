use serde_json::{json, Value};

use crate::error::FusionError;
use crate::unified::*;

/// 提取 OpenAI Chat 消息的 content：字符串直接取；数组型(多段/多模态)拼接其中 text 段
fn content_to_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
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

/// 将 OpenAI Chat Completions 请求体解析为 UnifiedRequest
pub fn parse_request(body: &Value) -> Result<UnifiedRequest, FusionError> {
    let msgs = body
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| FusionError::InvalidRequest("messages required".into()))?;

    let mut items = Vec::new();
    for m in msgs {
        let role = match m.get("role").and_then(|v| v.as_str()).unwrap_or("user") {
            "system" => Role::System,
            "assistant" => Role::Assistant,
            "tool" => Role::Tool,
            _ => Role::User,
        };
        let text = content_to_text(m.get("content"));
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

/// 从 UnifiedResponse 中提取第一条 assistant 文本
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

/// 将 UnifiedResponse 格式化为 OpenAI Chat Completions 响应体
pub fn format_response(resp: &UnifiedResponse) -> Value {
    json!({
        "id": "chatcmpl-localfusion",
        "object": "chat.completion",
        "model": resp.model_id,
        "choices": [{
            "index": 0,
            "finish_reason": "stop",
            "message": {
                "role": "assistant",
                "content": answer_text(resp)
            }
        }],
        "usage": {
            "prompt_tokens": resp.usage.input_tokens,
            "completion_tokens": resp.usage.output_tokens,
            "total_tokens": resp.usage.input_tokens + resp.usage.output_tokens
        }
    })
}

/// 按 OpenAI 协议格式化错误体（非流式）
pub fn format_error(message: &str) -> Value {
    json!({"error": {"message": message, "type": "invalid_request_error"}})
}

/// 将 UnifiedStreamEvent 转换为 OpenAI SSE 事件字符串列表
pub fn sse_events(ev: &UnifiedStreamEvent) -> Vec<String> {
    match ev {
        UnifiedStreamEvent::TextDelta { text } => vec![json!({
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {"content": text}}]
        })
        .to_string()],
        UnifiedStreamEvent::Done { usage, .. } => vec![
            json!({
                "object": "chat.completion.chunk",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                "usage": {
                    "prompt_tokens": usage.input_tokens,
                    "completion_tokens": usage.output_tokens,
                    "total_tokens": usage.input_tokens + usage.output_tokens
                }
            })
            .to_string(),
            "[DONE]".to_string(),
        ],
        UnifiedStreamEvent::Error { message, .. } => vec![
            json!({"error": {"message": message}}).to_string(),
            "[DONE]".to_string(),
        ],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_array_content() {
        // OpenAI 多段/多模态 content：拼接其中 text 段，忽略非文本块
        let body = serde_json::json!({"model":"vm","messages":[
            {"role":"user","content":[
                {"type":"text","text":"hello "},
                {"type":"image_url","image_url":{"url":"data:..."}},
                {"type":"text","text":"world"}
            ]}]});
        let req = parse_request(&body).unwrap();
        assert_eq!(req.items.len(), 1);
        match &req.items[0] {
            Item::Message { content, .. } => match &content[0] {
                ContentBlock::Text(t) => assert_eq!(t, "hello world"),
                _ => panic!("expected text block"),
            },
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn format_response_shape() {
        let resp = UnifiedResponse {
            items: vec![Item::Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text("hi".into())],
            }],
            usage: Usage {
                input_tokens: 1,
                output_tokens: 2,
            },
            model_id: "m".into(),
            calls: vec![],
        };
        let j = format_response(&resp);
        assert_eq!(j["choices"][0]["message"]["content"], "hi");
        assert_eq!(j["usage"]["completion_tokens"], 2);
        assert_eq!(j["object"], "chat.completion");
    }
}
