use crate::graph::model::{edge_type_weight, EdgeKind, NodeKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub from: String,
    pub to: String,
    pub edge_kind: EdgeKind,
    pub node_kind: NodeKind,
    pub label: String,
    pub hops: u32,
    pub graph_prior: f32,
    pub semantic: Option<f64>,
    pub frontier_score: Option<f64>,
    pub decision: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CandidateFilter {
    pub allowed_edges: Vec<EdgeKind>,
    pub allowed_nodes: Vec<NodeKind>,
}

impl Default for CandidateFilter {
    fn default() -> Self {
        Self {
            allowed_edges: EdgeKind::all().to_vec(),
            allowed_nodes: vec![
                NodeKind::Document,
                NodeKind::Section,
                NodeKind::Entity,
                NodeKind::Concept,
                NodeKind::Policy,
                NodeKind::Event,
                NodeKind::Claim,
            ],
        }
    }
}

/// Deterministic graph prior (structure only, no semantics):
///   graph_prior = seed_proximity + pagerank_component + edge_type + evidence
/// Weights are explicit and tunable; exact arithmetic lives in Rust.
pub fn graph_prior(
    hops_from_seed: u32,
    pagerank: f32,
    edge_kind: EdgeKind,
    evidence_count: usize,
) -> f32 {
    let proximity = match hops_from_seed {
        0 => 0.40,
        1 => 0.30,
        2 => 0.18,
        3 => 0.08,
        _ => 0.02,
    };
    // PageRank scaled: central nodes get a small boost, capped so a globally
    // central but irrelevant node cannot dominate routing.
    let pr_component = (pagerank * 10.0).min(0.20);
    let edge_component = edge_type_weight(edge_kind).min(0.30);
    let ev_component = ((evidence_count as f32) * 0.05).min(0.15);
    proximity + pr_component + edge_component + ev_component
}
