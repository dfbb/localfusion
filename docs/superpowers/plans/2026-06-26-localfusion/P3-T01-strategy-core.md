# P3-T01 Strategy trait + 注册表 + call_member + schema

**阶段:** 3 策略 · **前置:** 阶段1,2 · 见全局约束: `00-index.md`

**Goal:** 编排层抽象 + 策略注册表 + 通用记账 helper + 参数 schema（设计 §6.3）。

**Files:** Modify: `src/lib.rs`（加 `pub mod strategy; pub mod router;`）；Create: `src/strategy/mod.rs` + 占位 6 策略文件 + `src/router.rs` 占位

**Produces:**
- `MemberHandle{model_id,connector:Box<dyn Connector>,egress:EgressCtx}`
- `StrategyCtx<'a>{req,members:Vec<MemberHandle>,resolver:&'a ModelResolver,params,db:&'a Db,want_stream,recorder:&'a CallRecorder,trace:Option<&'a StrategyTrace>}`
- `enum StrategyOutput{Stream(UnifiedStream),Full(UnifiedResponse)}`
- `trait Strategy{name,execute}`、`make_strategy(name)->Option<Box<dyn Strategy>>`、`params_schema(name)->Value`
- `pub(crate) async fn call_member(member,req,role,recorder)->Result<UnifiedResponse,FusionError>`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_has_six_strategies() {
        for n in ["failover","speed","cheapest","synthesize","best-of-n","multimodal"] {
            assert!(make_strategy(n).is_some(), "missing {n}");
        }
        assert!(make_strategy("nope").is_none());
    }
    #[test]
    fn schema_for_synthesize_has_judge() {
        assert!(params_schema("synthesize")["properties"]["judge"].is_object());
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib strategy::tests` → FAIL。

- [ ] **Step 3: 实现 strategy/mod.rs（类型 + trait + 注册表）**

```rust
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

pub struct MemberHandle {
    pub model_id: String,
    pub connector: Box<dyn Connector>,
    pub egress: EgressCtx,
}

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

pub enum StrategyOutput {
    Stream(UnifiedStream),
    Full(UnifiedResponse),
}

#[async_trait]
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError>;
}

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
```

- [ ] **Step 4: 实现 call_member + params_schema（续 mod.rs）**

```rust
pub(crate) async fn call_member(
    member: &MemberHandle, req: &UnifiedRequest, role: CallRole, recorder: &CallRecorder,
) -> Result<UnifiedResponse, FusionError> {
    let start = Instant::now();
    match member.connector.complete(req, &member.egress).await {
        Ok(mut resp) => {
            let secs = start.elapsed().as_secs_f64();
            if let Some(c) = resp.calls.first_mut() { c.role = role; c.latency_secs = secs; }
            for c in &resp.calls { recorder.record(c.clone()); }
            Ok(resp)
        }
        Err(e) => {
            let secs = start.elapsed().as_secs_f64();
            recorder.record(ModelUsage { model_id: member.model_id.clone(), role,
                input_tokens: 0, output_tokens: 0, cost: 0.0, status: CallStatus::Failed,
                estimated: true, latency_secs: secs });
            Err(e.into())
        }
    }
}

pub fn params_schema(name: &str) -> serde_json::Value {
    use serde_json::json;
    match name {
        "synthesize" | "best-of-n" => json!({"type":"object","properties":{
            "judge":{"type":"string","x-ref":"model","required":true},
            "min_answers":{"type":"integer","default":1},
            "strict":{"type":"boolean","default":false}}}),
        "failover" => json!({"type":"object","properties":{"timeout_secs":{"type":"integer","default":60}}}),
        "speed" => json!({"type":"object","properties":{
            "explore":{"type":"boolean","default":true},
            "probe_interval_min":{"type":"integer","default":30}}}),
        "cheapest" => json!({"type":"object","properties":{
            "tokenizer":{"type":"string","enum":["approx","tiktoken"],"default":"approx"},
            "output_estimate_max":{"type":"integer","default":512}}}),
        "multimodal" => json!({"type":"object","properties":{
            "web_search":{"type":"string","x-ref":"model"},
            "image_generation":{"type":"string","x-ref":"model"},
            "tool_search":{"type":"string","x-ref":"model"},
            "image_query":{"type":"string","x-ref":"model"},
            "max_iterations":{"type":"integer","default":6}}}),
        _ => json!({"type":"object","properties":{}}),
    }
}
```

- [ ] **Step 5: 占位文件 + lib.rs + router 占位**

- `lib.rs` 加 `pub mod strategy; pub mod router;`
- 6 个 `src/strategy/<name>.rs`：空 struct + `Strategy` 实现返回 `Err(FusionError::StrategyError("todo".into()))`，`name()` 返回对应名。`synthesize.rs` 需 `pub struct Synthesize;`（mod 声明为 `pub mod synthesize`）。
- `src/strategy/testutil.rs`：`// filled in P3-T02`
- `src/router.rs`：占位

```rust
use crate::db::Db;
pub struct ModelResolver { #[allow(dead_code)] db: Db, #[allow(dead_code)] enc_key: [u8; 32] }
impl ModelResolver {
    pub fn new(db: Db, enc_key: [u8; 32]) -> Self { ModelResolver { db, enc_key } }
}
```

- [ ] **Step 6: 运行确认通过 + 提交**

```bash
cargo test --lib strategy::tests && cargo build
git add src/lib.rs src/strategy/ src/router.rs
git commit -m "feat: Strategy trait + StrategyCtx + 注册表 + params_schema + call_member"
```
