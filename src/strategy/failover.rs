use async_trait::async_trait;
use crate::error::FusionError;
use super::{Strategy, StrategyCtx, StrategyOutput};

pub struct Failover;

#[async_trait]
impl Strategy for Failover {
    fn name(&self) -> &str { "failover" }
    async fn execute(&self, _ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        Err(FusionError::StrategyError("todo".into()))
    }
}
