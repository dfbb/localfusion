// P4-T03 实现
use serde_json::Value;

use crate::error::FusionError;
use crate::unified::*;

/// 占位：解析 OpenAI Responses API 请求体（P4-T03 实现）
pub fn parse_request(_body: &Value) -> Result<UnifiedRequest, FusionError> {
    Err(FusionError::InvalidRequest("todo: openai_responses".into()))
}

/// 占位：格式化 OpenAI Responses API 响应体（P4-T03 实现）
pub fn format_response(_resp: &UnifiedResponse) -> Value {
    Value::Null
}

/// 占位：生成 OpenAI Responses API SSE 事件（P4-T03 实现）
pub fn sse_events(_ev: &UnifiedStreamEvent) -> Vec<String> {
    vec![]
}
