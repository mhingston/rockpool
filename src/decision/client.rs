use super::types::{DecisionError, DecisionRequest, DecisionResponse};
use async_trait::async_trait;

#[async_trait]
pub trait DecisionClient: Send + Sync {
    async fn decide(
        &self,
        request: DecisionRequest,
    ) -> Result<DecisionResponse, DecisionError>;
}
