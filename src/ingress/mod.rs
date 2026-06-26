pub mod anthropic;
pub mod openai_chat;
pub mod openai_responses;
pub mod sse_out;

use serde_json::Value;

/// 从请求体中提取模型名
pub fn extract_model(body: &Value) -> Option<String> {
    body.get("model").and_then(|v| v.as_str()).map(String::from)
}

/// 判断请求是否要求流式响应
pub fn wants_stream(body: &Value) -> bool {
    body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)
}
