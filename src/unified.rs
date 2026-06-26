use serde::{Deserialize, Serialize};
use std::sync::Mutex;

// ── 请求 ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedRequest {
    pub items: Vec<Item>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stream: bool,
    #[serde(default)]
    pub raw_extra: serde_json::Value,
}

// ── 会话条目 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    Message { role: Role, content: Vec<ContentBlock> },
    Reasoning { content: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
    ToolResult { id: String, content: Vec<ContentBlock> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role { System, User, Assistant, Tool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentBlock {
    Text(String),
    Image { url: String },
}

// ── 工具定义 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    #[serde(default)] pub description: String,
    #[serde(default)] pub parameters: serde_json::Value,
    #[serde(default)] pub builtin: Option<String>,
}

// ── 用量 ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

// ── 调用元数据 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallRole { Member, Judge, Tool }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallStatus { Ok, Failed }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model_id: String,
    pub role: CallRole,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: f64,
    pub status: CallStatus,
    pub estimated: bool,
    pub latency_secs: f64,
}

// ── 响应 ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedResponse {
    pub items: Vec<Item>,
    pub usage: Usage,
    pub model_id: String,
    pub calls: Vec<ModelUsage>,
}

// ── 流式事件 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum UnifiedStreamEvent {
    Started { model_id: String },
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
    Done { usage: Usage, call: Option<ModelUsage>, finish_reason: Option<String> },
    Error { message: String, call: Option<ModelUsage> },
}

/// 流式响应句柄；rx 由 connector 写入，消费方逐事件读取
pub struct UnifiedStream {
    pub rx: tokio::sync::mpsc::Receiver<Result<UnifiedStreamEvent, ConnError>>,
    pub upstream_request_id: Option<String>,
}

// ── 连接器错误 ────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ConnError {
    /// 连接器不支持该操作，不可重试
    #[error("connector unsupported: {0}")]
    HardFail(String),
    /// 上游 HTTP 错误，可能可重试
    #[error("connector http: {0}")]
    Http(String),
}

impl From<ConnError> for crate::error::FusionError {
    fn from(e: ConnError) -> Self {
        match e {
            ConnError::HardFail(m) => crate::error::FusionError::InvalidRequest(m),
            ConnError::Http(m) => crate::error::FusionError::UpstreamError { status: 502, message: m },
        }
    }
}

// ── CallRecorder ──────────────────────────────────────────────────────────────

/// 线程安全的调用记录收集器；drain() 是唯一统计权威
#[derive(Default)]
pub struct CallRecorder {
    calls: Mutex<Vec<ModelUsage>>,
}

impl CallRecorder {
    /// 记录一条调用元数据
    pub fn record(&self, usage: ModelUsage) {
        self.calls.lock().expect("recorder lock").push(usage);
    }
    /// 取走并清空全部记录
    pub fn drain(&self) -> Vec<ModelUsage> {
        std::mem::take(&mut *self.calls.lock().expect("recorder lock"))
    }
}

// ── StrategyTrace ─────────────────────────────────────────────────────────────

#[derive(Default)]
struct TraceData {
    status: Option<String>,
    member_answers: Vec<serde_json::Value>,
    judge: Option<serde_json::Value>,
    candidates: Vec<serde_json::Value>,
    attempts: Vec<serde_json::Value>,
    turns: Vec<serde_json::Value>,
}

/// 策略执行轨迹收集器，用于调试和可观测性
#[derive(Default)]
pub struct StrategyTrace {
    data: Mutex<TraceData>,
}

impl StrategyTrace {
    pub fn set_status(&self, status: &str) {
        self.data.lock().unwrap().status = Some(status.into());
    }
    pub fn add_member_answer(&self, model_id: &str, text: &str, usage: &ModelUsage) {
        self.data.lock().unwrap().member_answers.push(serde_json::json!({
            "model_id": model_id, "text": text, "usage": usage
        }));
    }
    pub fn set_judge(&self, input: &str, output: &str, usage: &ModelUsage) {
        self.data.lock().unwrap().judge = Some(serde_json::json!({
            "input": input, "output": output, "usage": usage
        }));
    }
    pub fn add_candidate(&self, model_id: &str, metric: serde_json::Value) {
        self.data.lock().unwrap().candidates.push(serde_json::json!({
            "model_id": model_id, "metric": metric
        }));
    }
    pub fn add_attempt(&self, model_id: &str, ok: bool, error: Option<&str>) {
        self.data.lock().unwrap().attempts.push(serde_json::json!({
            "model_id": model_id, "ok": ok, "error": error
        }));
    }
    pub fn add_turn(&self, turn: serde_json::Value) {
        self.data.lock().unwrap().turns.push(turn);
    }
    /// 返回当前快照（JSON），不清空内部状态
    pub fn snapshot(&self) -> serde_json::Value {
        let d = self.data.lock().unwrap();
        serde_json::json!({
            "status": d.status,
            "member_answers": d.member_answers,
            "judge": d.judge,
            "candidates": d.candidates,
            "attempts": d.attempts,
            "turns": d.turns
        })
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(m: &str, st: CallStatus) -> ModelUsage {
        ModelUsage {
            model_id: m.into(),
            role: CallRole::Member,
            input_tokens: 1,
            output_tokens: 2,
            cost: 0.0,
            status: st,
            estimated: false,
            latency_secs: 0.1,
        }
    }

    #[test]
    fn recorder_collects_and_drains() {
        let r = CallRecorder::default();
        r.record(usage("a", CallStatus::Ok));
        r.record(usage("b", CallStatus::Failed));
        assert_eq!(r.drain().len(), 2);
        assert!(r.drain().is_empty());
    }

    #[test]
    fn trace_snapshot_has_fields() {
        let t = StrategyTrace::default();
        t.set_status("full");
        t.add_attempt("m1", false, Some("timeout"));
        t.add_attempt("m2", true, None);
        let snap = t.snapshot();
        assert_eq!(snap["status"], "full");
        assert_eq!(snap["attempts"].as_array().unwrap().len(), 2);
        assert_eq!(snap["attempts"][1]["ok"], true);
    }

    #[test]
    fn connerror_maps_to_fusionerror() {
        let f: crate::error::FusionError = ConnError::HardFail("x".into()).into();
        assert_eq!(f.http_status(), 400);
    }
}
