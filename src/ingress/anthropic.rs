// P4-T04 实现
use serde_json::Value;

use crate::error::FusionError;
use crate::unified::*;

/// 占位：解析 Anthropic Messages 请求体（P4-T04 实现）
pub fn parse_request(_body: &Value) -> Result<UnifiedRequest, FusionError> {
    Err(FusionError::InvalidRequest("todo: anthropic".into()))
}

/// 占位：格式化 Anthropic Messages 响应体（P4-T04 实现）
pub fn format_response(_resp: &UnifiedResponse) -> Value {
    Value::Null
}

/// 占位：生成 Anthropic Messages SSE 事件（P4-T04 实现）
pub fn sse_events(_ev: &UnifiedStreamEvent) -> Vec<String> {
    vec![]
}
