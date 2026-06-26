use async_trait::async_trait;
use crate::error::FusionError;
use super::{Strategy, StrategyCtx, StrategyOutput};

pub struct BestOfN;

#[async_trait]
impl Strategy for BestOfN {
    fn name(&self) -> &str { "best-of-n" }
    async fn execute(&self, _ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        Err(FusionError::StrategyError("todo".into()))
    }
}
