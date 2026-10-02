use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecisionError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("malformed response: {0}")]
    Malformed(String),
    #[error("budget exhausted: {0}")]
    Budget(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    Noul { instructions: String },
    Choice {
        instructions: String,
        choices: BTreeMap<String, String>,
    },
    Score {
        instructions: String,
        levels: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub state: serde_json::Value,
    pub questions: BTreeMap<String, Question>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Answer {
    Noul { p_yes: f64 },
    Choice {
        probabilities: BTreeMap<String, f64>,
        selected: String,
    },
    Score { level: String, value: f64 },
}

impl Answer {
    /// Scalar relevance in [0,1] for ranking. Noul -> p_yes.
    /// Choice -> max prob. Score -> normalized value.
    pub fn relevance(&self) -> f64 {
        match self {
            Answer::Noul { p_yes } => p_yes.clamp(0.0, 1.0),
            Answer::Choice { probabilities, .. } => {
                probabilities.values().cloned().fold(0.0, f64::max).clamp(0.0, 1.0)
            }
            Answer::Score { value, .. } => value.clamp(0.0, 1.0),
        }
    }
    pub fn p_yes(&self) -> Option<f64> {
        match self {
            Answer::Noul { p_yes } => Some(*p_yes),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionResponse {
    pub answers: BTreeMap<String, Answer>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub case_id: Option<String>,
    pub request: DecisionRequest,
    pub response: DecisionResponse,
}
