use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type NodeId = String;
pub type DocumentId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Document,
    Section,
    Entity,
    Concept,
    Policy,
    Event,
    Claim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Contains,
    Mentions,
    RefersTo,
    PartOf,
    RelatedTo,
    Supports,
    Contradicts,
}

impl EdgeKind {
    /// Closed taxonomy: ingestion must map to one of these.
    pub fn all() -> &'static [EdgeKind] {
        use EdgeKind::*;
        &[
            Contains,
            Mentions,
            RefersTo,
            PartOf,
            RelatedTo,
            Supports,
            Contradicts,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub document_id: DocumentId,
    pub source_id: String,
    pub start: Option<u32>,
    pub end: Option<u32>,
    pub quote: Option<String>,
}

impl EvidenceRef {
    pub fn has_grounding(&self) -> bool {
        self.quote.is_some() || (self.start.is_some() && self.end.is_some())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub label: String,
    pub description: Option<String>,
    pub aliases: Vec<String>,
    pub evidence: Vec<EvidenceRef>,
}

impl Node {
    pub fn has_evidence(&self) -> bool {
        !self.evidence.is_empty()
    }
    /// True when the node carries no evidence refs (inferred/unverified).
    pub fn is_inferred(&self) -> bool {
        self.evidence.is_empty()
    }
    pub fn text_for_matching(&self) -> String {
        let mut s = self.label.clone();
        if let Some(d) = &self.description {
            s.push(' ');
            s.push_str(d);
        }
        for a in &self.aliases {
            s.push(' ');
            s.push_str(a);
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub kind: EdgeKind,
    pub evidence: Vec<EvidenceRef>,
    pub confidence: Option<f32>,
}

impl Edge {
    pub fn has_evidence(&self) -> bool {
        !self.evidence.is_empty()
    }
}

// Fixture (de)serialization shapes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphFixture {
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<FixtureEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bm25Placeholder {
    #[serde(default)]
    pub note: String,
}

/// Edge-type prior weight used in deterministic graph_prior.
/// Structural signal only — never a relevance verdict on its own.
pub fn edge_type_weight(kind: EdgeKind) -> f32 {
    match kind {
        EdgeKind::Supports => 0.30,
        EdgeKind::Contains => 0.25,
        EdgeKind::PartOf => 0.20,
        EdgeKind::RefersTo => 0.15,
        EdgeKind::Mentions => 0.12,
        EdgeKind::RelatedTo => 0.10,
        EdgeKind::Contradicts => 0.05,
    }
}

/// Node-type prior weight (small structural component).
pub fn node_type_weight(kind: NodeKind) -> f32 {
    match kind {
        NodeKind::Section => 0.15,
        NodeKind::Document => 0.12,
        NodeKind::Policy => 0.12,
        NodeKind::Claim => 0.10,
        NodeKind::Concept => 0.08,
        NodeKind::Entity => 0.06,
        NodeKind::Event => 0.05,
    }
}

pub fn _example_btreemap_use() -> BTreeMap<String, String> {
    BTreeMap::new()
}
