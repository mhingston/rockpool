use crate::decision::client::DecisionClient;
use crate::eval::cases::EvalCase;
use crate::eval::metrics::{self, CaseResult};
use crate::evidence::store::MemoryEvidenceStore;
use crate::graph::store::KnowledgeGraph;
use crate::retrieval::candidates::CandidateFilter;
use crate::retrieval::policy::{RetrievalMode, Thresholds, TraversalBudgets, Weights};
use crate::retrieval::traversal::retrieve;
use std::time::Instant;

/// Lexical retrieval baseline (Baseline A): no graph, no decisions.
/// Text matching over source texts + node labels, top-k evidence.
pub async fn lexical_baseline(
    kg: &KnowledgeGraph,
    store: &MemoryEvidenceStore,
    query: &str,
    top_k: usize,
) -> (Vec<String>, Vec<String>) {
    let hits = store.lexical_search(query, top_k);
    let sources: Vec<String> = hits.iter().map(|(s, _)| s.clone()).collect();
    // Evidence keys as document#source for metric compat.
    let ev_keys: Vec<String> = sources
        .iter()
        .map(|s| {
            let doc = store.document_of(s).unwrap_or(s);
            format!("{doc}#{s}")
        })
        .collect();
    // Entities: nodes whose label text overlaps query tokens (same seed fn, top_k).
    let all: Vec<_> = kg.all_nodes();
    let seeds = crate::retrieval::seed::resolve_seeds(query, &all, top_k);
    let entities: Vec<String> = seeds.into_iter().map(|s| s.node_id).collect();
    (ev_keys, entities)
}

pub struct EvalConfig {
    pub budgets: TraversalBudgets,
    pub thresholds: Thresholds,
    pub weights: Weights,
    pub filter: CandidateFilter,
    pub top_k_lexical: usize,
}

impl Default for EvalConfig {
    fn default() -> Self {
        Self {
            budgets: TraversalBudgets::default(),
            thresholds: Thresholds::default(),
            weights: Weights::default(),
            filter: CandidateFilter::default(),
            top_k_lexical: 5,
        }
    }
}

pub async fn run_single_graph_mode<D: DecisionClient>(
    kg: &KnowledgeGraph,
    store: &MemoryEvidenceStore,
    case: &EvalCase,
    mode: RetrievalMode,
    cfg: &EvalConfig,
    decision: Option<&D>,
    sufficiency: Option<&D>,
) -> CaseResult {
    let t0 = Instant::now();
    let mut budgets = cfg.budgets.clone();
    budgets.max_hops = budgets.max_hops.min(case.max_hops.max(1));
    let out = retrieve(
        kg,
        store,
        decision,
        sufficiency,
        &case.query,
        mode,
        &budgets,
        &cfg.thresholds,
        &cfg.weights,
        &cfg.filter,
    )
    .await
    .expect("retrieval failed");
    let got: Vec<String> = out
        .evidence
        .iter()
        .map(|e| format!("{}#{}", e.document_id, e.source_id))
        .collect();
    CaseResult {
        case_id: case.id.clone(),
        evidence_recall: metrics::evidence_recall(&case.expected_evidence, &got),
        evidence_precision: metrics::evidence_precision(&case.expected_evidence, &got),
        entity_recall: metrics::entity_recall(&case.expected_entities, &out.visited),
        nodes_examined: out.trace.stats.nodes_examined,
        nodes_expanded: out.trace.stats.nodes_expanded,
        edges_examined: out.trace.stats.edges_examined,
        decision_calls: out.trace.stats.decision_calls,
        noul_questions: out.trace.stats.noul_questions,
        evidence_items: out.evidence.len(),
        stop_reason: out.trace.stop_reason.clone(),
        max_depth: out.trace.stats.max_depth,
        latency_ms: t0.elapsed().as_millis() as u64,
    }
}

/// Run all four conditions: lexical, deterministic, semantic-only, hybrid.
/// `decision`/`sufficiency` are the semantic backends for the two semantic
/// modes (any `DecisionClient`: fixture stand-in or live System One).
/// Deterministic graph mode uses no decision API by definition.
pub async fn run_eval<D: DecisionClient>(
    kg: &KnowledgeGraph,
    store: &MemoryEvidenceStore,
    cases: &[EvalCase],
    cfg: &EvalConfig,
    decision: &D,
    sufficiency: &D,
) -> EvalReport {

    let mut lexical = vec![];
    let mut deterministic = vec![];
    let mut semantic = vec![];
    let mut hybrid = vec![];

    for case in cases {
        // Lexical
        let t0 = Instant::now();
        let (ev, ent) = lexical_baseline(kg, store, &case.query, cfg.top_k_lexical).await;
        lexical.push(CaseResult {
            case_id: case.id.clone(),
            evidence_recall: metrics::evidence_recall(&case.expected_evidence, &ev),
            evidence_precision: metrics::evidence_precision(&case.expected_evidence, &ev),
            entity_recall: metrics::entity_recall(&case.expected_entities, &ent),
            nodes_examined: ev.len(),
            nodes_expanded: 0,
            edges_examined: 0,
            decision_calls: 0,
            noul_questions: 0,
            evidence_items: ev.len(),
            stop_reason: "lexical_topk".into(),
            max_depth: 0,
            latency_ms: t0.elapsed().as_millis() as u64,
        });
        deterministic.push(
            run_single_graph_mode::<D>(
                kg,
                store,
                case,
                RetrievalMode::Deterministic,
                cfg,
                None,
                None,
            )
            .await,
        );
        semantic.push(
            run_single_graph_mode(
                kg,
                store,
                case,
                RetrievalMode::SemanticOnly,
                cfg,
                Some(decision),
                Some(sufficiency),
            )
            .await,
        );
        hybrid.push(
            run_single_graph_mode(
                kg,
                store,
                case,
                RetrievalMode::Hybrid,
                cfg,
                Some(decision),
                Some(sufficiency),
            )
            .await,
        );
    }

    EvalReport {
        lexical: metrics::summarize(&lexical),
        deterministic: metrics::summarize(&deterministic),
        semantic: metrics::summarize(&semantic),
        hybrid: metrics::summarize(&hybrid),
        lexical_cases: lexical,
        deterministic_cases: deterministic,
        semantic_cases: semantic,
        hybrid_cases: hybrid,
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvalReport {
    pub lexical: crate::eval::metrics::EvalSummary,
    pub deterministic: crate::eval::metrics::EvalSummary,
    pub semantic: crate::eval::metrics::EvalSummary,
    pub hybrid: crate::eval::metrics::EvalSummary,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub lexical_cases: Vec<CaseResult>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub deterministic_cases: Vec<CaseResult>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub semantic_cases: Vec<CaseResult>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub hybrid_cases: Vec<CaseResult>,
}

// Re-export for downstream convenience.
pub use crate::decision::types::{DecisionRequest, Question};
