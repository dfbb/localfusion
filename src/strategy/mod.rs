mod best_of_n;
mod cheapest;
mod failover;
mod multimodal;
mod speed;
pub mod synthesize;
#[cfg(test)]
pub(crate) mod testutil;

use async_trait::async_trait;
use std::time::Instant;

use crate::connector::{Connector, EgressCtx};
use crate::db::Db;
use crate::error::FusionError;
use crate::router::ModelResolver;
use crate::unified::*;

/// Single member model handle, holds the connector and egress context
pub struct MemberHandle {
    pub model_id: String,
    pub connector: Box<dyn Connector>,
    pub egress: EgressCtx,
}

/// Strategy execution context, passed into each strategy's execute call
pub struct StrategyCtx<'a> {
    pub req: UnifiedRequest,
    pub members: Vec<MemberHandle>,
    pub resolver: &'a ModelResolver,
    pub params: serde_json::Value,
    pub db: &'a Db,
    pub want_stream: bool,
    pub recorder: &'a CallRecorder,
    pub trace: Option<&'a StrategyTrace>,
}

/// Strategy output: streaming or full response
pub enum StrategyOutput {
    Stream(UnifiedStream),
    Full(UnifiedResponse),
}

/// Orchestration strategy trait; each strategy implements this once
#[async_trait]
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError>;
}

/// Create the corresponding strategy instance by name; returns None for unknown names
pub fn make_strategy(name: &str) -> Option<Box<dyn Strategy>> {
    match name {
        "failover" => Some(Box::new(failover::Failover)),
        "speed" => Some(Box::new(speed::Speed)),
        "cheapest" => Some(Box::new(cheapest::Cheapest)),
        "synthesize" => Some(Box::new(synthesize::Synthesize)),
        "best-of-n" => Some(Box::new(best_of_n::BestOfN)),
        "multimodal" => Some(Box::new(multimodal::Multimodal)),
        _ => None,
    }
}

/// Returns the parameter JSON Schema for the specified strategy
pub fn params_schema(name: &str) -> serde_json::Value {
    use serde_json::json;
    match name {
        "synthesize" | "best-of-n" => json!({
            "type": "object",
            "properties": {
                "judge": { "type": "string", "x-ref": "model", "required": true },
                "min_answers": { "type": "integer", "default": 1 },
                "strict": { "type": "boolean", "default": false }
            }
        }),
        "failover" => json!({
            "type": "object",
            "properties": {
                "timeout_secs": { "type": "integer", "default": 60 }
            }
        }),
        "speed" => json!({
            "type": "object",
            "properties": {
                "explore": { "type": "boolean", "default": true },
                "probe_interval_min": { "type": "integer", "default": 30 }
            }
        }),
        "cheapest" => json!({
            "type": "object",
            "properties": {
                "tokenizer": { "type": "string", "enum": ["approx"], "default": "approx", "description": "Input token estimation method (currently only approx: char_count/4)" },
                "output_estimate_max": { "type": "integer", "default": 512, "description": "Upper bound for output token estimate when max_tokens is not set" }
            }
        }),
        "multimodal" => json!({
            "type": "object",
            "properties": {
                "web_search": { "type": "string", "x-ref": "model" },
                "image_generation": { "type": "string", "x-ref": "model" },
                "tool_search": { "type": "string", "x-ref": "model" },
                "image_query": { "type": "string", "x-ref": "model" },
                "max_iterations": { "type": "integer", "default": 6 }
            }
        }),
        _ => json!({ "type": "object", "properties": {} }),
    }
}

/// Call a single member; record success/failure metadata to recorder
/// Authoritative statistics: recorder.drain(), does not read UnifiedResponse.calls
#[allow(dead_code)] // Called by P3-T02+ strategy implementations
pub(crate) async fn call_member(
    member: &MemberHandle,
    req: &UnifiedRequest,
    role: CallRole,
    recorder: &CallRecorder,
) -> Result<UnifiedResponse, FusionError> {
    let start = Instant::now();
    match member.connector.complete(req, &member.egress).await {
        Ok(mut resp) => {
            let secs = start.elapsed().as_secs_f64();
            // Update the role and latency on the first record
            if let Some(c) = resp.calls.first_mut() {
                c.role = role;
                c.latency_secs = secs;
            }
            for c in &resp.calls {
                recorder.record(c.clone());
            }
            Ok(resp)
        }
        Err(e) => {
            let secs = start.elapsed().as_secs_f64();
            recorder.record(ModelUsage {
                model_id: member.model_id.clone(),
                role,
                input_tokens: 0,
                output_tokens: 0,
                cost: 0.0,
                status: CallStatus::Failed,
                estimated: true,
                latency_secs: secs,
            });
            Err(e.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_six_strategies() {
        for n in ["failover", "speed", "cheapest", "synthesize", "best-of-n", "multimodal"] {
            assert!(make_strategy(n).is_some(), "missing {n}");
        }
        assert!(make_strategy("nope").is_none());
    }

    #[test]
    fn schema_for_synthesize_has_judge() {
        assert!(params_schema("synthesize")["properties"]["judge"].is_object());
    }
}
