use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub document_id: String,
    pub source_id: String,
    pub text: String,
    pub quote: Option<String>,
    /// Graph path that found this evidence, e.g. ["renewal-pricing", "doc-14-s4.2"].
    pub path: Vec<String>,
    pub reason: String,
}
