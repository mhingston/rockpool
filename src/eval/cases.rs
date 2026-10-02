use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    pub id: String,
    pub query: String,
    pub expected_entities: Vec<String>,
    pub expected_evidence: Vec<String>,
    #[serde(default = "default_hops")]
    pub max_hops: u32,
}

fn default_hops() -> u32 {
    3
}

pub fn load_cases(json: &str) -> anyhow::Result<Vec<EvalCase>> {
    Ok(serde_json::from_str(json)?)
}
