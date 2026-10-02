use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnswerError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("malformed response: {0}")]
    Malformed(String),
    #[error("no evidence supplied")]
    NoEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    pub document_id: String,
    pub source_id: String,
    pub quote: Option<String>,
}

impl Citation {
    pub fn key(&self) -> String {
        format!("{}#{}", self.document_id, self.source_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerRequest {
    pub query: String,
    /// Grounded evidence only. The answer model must never receive
    /// unrestricted graph access in v0 — this list is its whole world.
    pub evidence: Vec<AnswerEvidence>,
    #[serde(default = "default_max_citations")]
    pub max_citations: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

fn default_max_citations() -> usize {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerEvidence {
    pub document_id: String,
    pub source_id: String,
    pub text: String,
    pub quote: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerResponse {
    pub text: String,
    pub citations: Vec<Citation>,
    pub abstained: bool,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    /// Citations the model emitted that did not resolve to supplied evidence
    /// and were dropped by Rust-side grounding enforcement.
    #[serde(default)]
    pub dropped_citations: Vec<String>,
}
