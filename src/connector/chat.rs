// P2-T03 实现
use async_trait::async_trait;
use super::{Connector, EgressCtx};
use crate::unified::{ConnError, UnifiedRequest, UnifiedResponse, UnifiedStream};

pub struct ChatConnector;

#[async_trait]
impl Connector for ChatConnector {
    async fn complete(
        &self,
        _: &UnifiedRequest,
        _: &EgressCtx,
    ) -> Result<UnifiedResponse, ConnError> {
        Err(ConnError::HardFail("todo".into()))
    }

    async fn stream(
        &self,
        _: &UnifiedRequest,
        _: &EgressCtx,
    ) -> Result<UnifiedStream, ConnError> {
        Err(ConnError::HardFail("todo".into()))
    }
}
