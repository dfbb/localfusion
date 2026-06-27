pub mod anthropic;
pub mod handler;
pub mod openai_chat;
pub mod openai_responses;
pub mod sse_out;

use serde_json::Value;

/// Extract the model name from the request body
pub fn extract_model(body: &Value) -> Option<String> {
    body.get("model").and_then(|v| v.as_str()).map(String::from)
}

/// Check whether the request wants a streaming response
pub fn wants_stream(body: &Value) -> bool {
    body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)
}
