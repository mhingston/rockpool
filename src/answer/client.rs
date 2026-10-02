use super::types::{AnswerError, AnswerRequest, AnswerResponse};
use async_trait::async_trait;

#[async_trait]
pub trait AnswerClient: Send + Sync {
    async fn answer(&self, request: AnswerRequest) -> Result<AnswerResponse, AnswerError>;
}
