use async_trait::async_trait;
use crate::error::FusionError;
use super::{Strategy, StrategyCtx, StrategyOutput};

pub struct Cheapest;

#[async_trait]
impl Strategy for Cheapest {
    fn name(&self) -> &str { "cheapest" }
    async fn execute(&self, _ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        Err(FusionError::StrategyError("todo".into()))
    }
}
