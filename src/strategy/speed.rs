use async_trait::async_trait;
use crate::error::FusionError;
use super::{Strategy, StrategyCtx, StrategyOutput};

pub struct Speed;

#[async_trait]
impl Strategy for Speed {
    fn name(&self) -> &str { "speed" }
    async fn execute(&self, _ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        Err(FusionError::StrategyError("todo".into()))
    }
}
