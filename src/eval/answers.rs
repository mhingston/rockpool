use crate::answer::client::AnswerClient;
use crate::answer::types::{AnswerEvidence, AnswerRequest};
use crate::decision::client::DecisionClient;
use crate::eval::cases::EvalCase;
use crate::evidence::store::MemoryEvidenceStore;
use crate::graph::store::KnowledgeGraph;
use crate::retrieval::candidates::CandidateFilter;
use crate::retrieval::policy::{RetrievalMode, Thresholds, TraversalBudgets, Weights};
use crate::retrieval::traversal::retrieve;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerCaseResult {
    pub case_id: String,
    pub cited: Vec<String>,
    pub citation_validity: f64,
    pub expected_coverage: f64,
    pub abstained: bool,
    pub abstain_correct: bool,
    pub retrieval_recall: f64,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnswerEvalSummary {
    pub n: usize,
    pub mean_citation_validity: f64,
    pub mean_expected_coverage: f64,
    pub abstention_rate: f64,
    pub abstain_correct_rate: f64,
    pub mean_retrieval_recall: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerEvalReport {
    pub summary: AnswerEvalSummary,
    pub cases: Vec<AnswerCaseResult>,
}

pub struct AnswerEvalConfig {
    pub budgets: TraversalBudgets,
    pub thresholds: Thresholds,
    pub weights: Weights,
    pub filter: CandidateFilter,
    pub max_citations: usize,
    pub passage_chars: usize,
}

impl Default for AnswerEvalConfig {
    fn default() -> Self {
        Self {
            budgets: TraversalBudgets::default(),
            thresholds: Thresholds::default(),
            weights: Weights::default(),
            filter: CandidateFilter::default(),
            max_citations: 5,
            passage_chars: 1500,
        }
    }
}

pub fn to_answer_evidence(
    evidence: &[crate::evidence::types::Evidence],
    passage_chars: usize,
) -> Vec<AnswerEvidence> {
    evidence
        .iter()
        .map(|e| AnswerEvidence {
            document_id: e.document_id.clone(),
            source_id: e.source_id.clone(),
            text: e.text.chars().take(passage_chars).collect(),
            quote: e.quote.clone(),
        })
        .collect()
}

pub async fn run_answer_eval<A, D>(
    kg: &KnowledgeGraph,
    store: &MemoryEvidenceStore,
    cases: &[EvalCase],
    cfg: &AnswerEvalConfig,
    answer_client: &A,
    decision: &D,
    sufficiency: &D,
) -> AnswerEvalReport
where
    A: AnswerClient,
    D: DecisionClient,
{
    let mut results = vec![];
    for case in cases {
        let t0 = Instant::now();
        let mut budgets = cfg.budgets.clone();
        budgets.max_hops = budgets.max_hops.min(case.max_hops.max(1));
        let out = retrieve(
            kg,
            store,
            Some(decision),
            Some(sufficiency),
            &case.query,
            RetrievalMode::Hybrid,
            &budgets,
            &cfg.thresholds,
            &cfg.weights,
            &cfg.filter,
        )
        .await
        .expect("retrieval failed");
        let retrieved: Vec<String> = out
            .evidence
            .iter()
            .map(|e| format!("{}#{}", e.document_id, e.source_id))
            .collect();
        let resp = answer_client
            .answer(AnswerRequest {
                query: case.query.clone(),
                evidence: to_answer_evidence(&out.evidence, cfg.passage_chars),
                max_citations: cfg.max_citations,
                model: None,
            })
            .await
            .expect("answer failed");
        let cited: Vec<String> = resp.citations.iter().map(|c| c.key()).collect();
        let ret_set: HashSet<&str> = retrieved.iter().map(|s| s.as_str()).collect();
        let citation_validity = if cited.is_empty() {
            if resp.abstained {
                1.0
            } else {
                0.0
            }
        } else {
            cited
                .iter()
                .filter(|c| ret_set.contains(c.as_str()))
                .count() as f64
                / cited.len() as f64
        };
        let expected_coverage = if case.expected_evidence.is_empty() {
            1.0
        } else {
            let exp: HashSet<&str> = case.expected_evidence.iter().map(|s| s.as_str()).collect();
            cited.iter().filter(|c| exp.contains(c.as_str())).count() as f64
                / case.expected_evidence.len() as f64
        };
        let abstain_correct =
            (resp.abstained && retrieved.is_empty()) || (!resp.abstained && !retrieved.is_empty());
        let retrieval_recall =
            crate::eval::metrics::evidence_recall(&case.expected_evidence, &retrieved);
        results.push(AnswerCaseResult {
            case_id: case.id.clone(),
            cited,
            citation_validity,
            expected_coverage,
            abstained: resp.abstained,
            abstain_correct,
            retrieval_recall,
            latency_ms: t0.elapsed().as_millis() as u64,
        });
    }
    let n = results.len().max(1) as f64;
    AnswerEvalReport {
        summary: AnswerEvalSummary {
            n: results.len(),
            mean_citation_validity: results.iter().map(|r| r.citation_validity).sum::<f64>() / n,
            mean_expected_coverage: results.iter().map(|r| r.expected_coverage).sum::<f64>() / n,
            abstention_rate: results.iter().filter(|r| r.abstained).count() as f64 / n,
            abstain_correct_rate: results.iter().filter(|r| r.abstain_correct).count() as f64 / n,
            mean_retrieval_recall: results.iter().map(|r| r.retrieval_recall).sum::<f64>() / n,
        },
        cases: results,
    }
}
