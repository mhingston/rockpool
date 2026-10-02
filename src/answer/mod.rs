pub mod client;
pub mod extractive;
pub mod openai;
pub mod types;

pub use client::AnswerClient;
pub use extractive::ExtractiveAnswerClient;
pub use openai::{AnswerEndpointConfig, OpenAiCompatAnswerClient};
pub use types::{AnswerError, AnswerRequest, AnswerResponse, Citation};
