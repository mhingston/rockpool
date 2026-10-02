use crate::graph::model::{EdgeKind, EvidenceRef, NodeKind};
use serde::{Deserialize, Serialize};

/// A candidate entity extracted from a source passage. Never mutated into
/// the graph directly — it must pass resolution + validation first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityProposal {
    pub label: String,
    pub kind: NodeKind,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub evidence: Vec<EvidenceRef>,
}

/// A candidate typed relationship. `kind` must come from the closed
/// [`EdgeKind`] taxonomy — the ingestion layer may not invent labels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationProposal {
    /// Existing node id, or empty when the endpoint is itself proposed.
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub evidence: Vec<EvidenceRef>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalSet {
    pub source_id: String,
    pub entities: Vec<EntityProposal>,
    pub relations: Vec<RelationProposal>,
}
