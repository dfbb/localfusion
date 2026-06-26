use async_trait::async_trait;
use crate::error::FusionError;
use super::{Strategy, StrategyCtx, StrategyOutput};

pub struct Multimodal;

#[async_trait]
impl Strategy for Multimodal {
    fn name(&self) -> &str { "multimodal" }
    async fn execute(&self, _ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        Err(FusionError::StrategyError("todo".into()))
    }
}
