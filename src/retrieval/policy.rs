use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetrievalMode {
    /// Baseline B: deterministic graph only, no decision API.
    Deterministic,
    /// Candidate C: semantic relevance only (uniform graph prior).
    SemanticOnly,
    /// Candidate D: graph prior + semantic relevance.
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalBudgets {
    pub max_hops: u32,
    pub max_nodes_examined: usize,
    pub max_nodes_expanded: usize,
    pub max_frontier_size: usize,
    pub max_decision_calls: usize,
    pub max_evidence_items: usize,
    pub max_sources: usize,
}

impl Default for TraversalBudgets {
    fn default() -> Self {
        Self {
            max_hops: 3,
            max_nodes_examined: 50,
            max_nodes_expanded: 12,
            max_frontier_size: 20,
            max_decision_calls: 10,
            max_evidence_items: 20,
            max_sources: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    pub accept: f64,
    pub review: f64,
    pub sufficiency: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            accept: 0.6,
            review: 0.35,
            sufficiency: 0.7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weights {
    pub semantic: f64,
    pub graph_prior: f64,
    pub proximity: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            semantic: 0.65,
            graph_prior: 0.30,
            proximity: 0.05,
        }
    }
}
