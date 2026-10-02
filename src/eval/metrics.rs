use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub case_id: String,
    pub evidence_recall: f64,
    pub evidence_precision: f64,
    pub entity_recall: f64,
    pub nodes_examined: usize,
    pub nodes_expanded: usize,
    pub edges_examined: usize,
    pub decision_calls: usize,
    pub noul_questions: usize,
    pub evidence_items: usize,
    pub stop_reason: String,
    pub max_depth: u32,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvalSummary {
    pub n: usize,
    pub mean_evidence_recall: f64,
    pub mean_evidence_precision: f64,
    pub mean_entity_recall: f64,
    pub mean_nodes_examined: f64,
    pub mean_nodes_expanded: f64,
    pub mean_decision_calls: f64,
    pub mean_nouls: f64,
    pub mean_latency_ms: f64,
    pub stop_reasons: std::collections::BTreeMap<String, usize>,
}

pub fn evidence_recall(expected: &[String], got_sources: &[String]) -> f64 {
    if expected.is_empty() {
        return 1.0;
    }
    let got: HashSet<&str> = got_sources.iter().map(|s| s.as_str()).collect();
    let hits = expected.iter().filter(|e| got.contains(e.as_str())).count();
    hits as f64 / expected.len() as f64
}

pub fn evidence_precision(expected: &[String], got_sources: &[String]) -> f64 {
    if got_sources.is_empty() {
        return 0.0;
    }
    let exp: HashSet<&str> = expected.iter().map(|s| s.as_str()).collect();
    let hits = got_sources.iter().filter(|g| exp.contains(g.as_str())).count();
    hits as f64 / got_sources.len() as f64
}

pub fn entity_recall(expected: &[String], visited: &[String]) -> f64 {
    if expected.is_empty() {
        return 1.0;
    }
    let v: HashSet<&str> = visited.iter().map(|s| s.as_str()).collect();
    let hits = expected.iter().filter(|e| v.contains(e.as_str())).count();
    hits as f64 / expected.len() as f64
}

pub fn summarize(results: &[CaseResult]) -> EvalSummary {
    let n = results.len();
    if n == 0 {
        return EvalSummary::default();
    }
    let mean = |f: fn(&CaseResult) -> f64| results.iter().map(f).sum::<f64>() / n as f64;
    let mut stop_reasons = std::collections::BTreeMap::new();
    for r in results {
        *stop_reasons.entry(r.stop_reason.clone()).or_insert(0) += 1;
    }
    EvalSummary {
        n,
        mean_evidence_recall: mean(|r| r.evidence_recall),
        mean_evidence_precision: mean(|r| r.evidence_precision),
        mean_entity_recall: mean(|r| r.entity_recall),
        mean_nodes_examined: mean(|r| r.nodes_examined as f64),
        mean_nodes_expanded: mean(|r| r.nodes_expanded as f64),
        mean_decision_calls: mean(|r| r.decision_calls as f64),
        mean_nouls: mean(|r| r.noul_questions as f64),
        mean_latency_ms: mean(|r| r.latency_ms as f64),
        stop_reasons,
    }
}
