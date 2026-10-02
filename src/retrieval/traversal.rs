use crate::decision::client::DecisionClient;
use crate::decision::types::{DecisionRecord, DecisionRequest, Question};
use crate::evidence::store::EvidenceStore;
use crate::evidence::types::Evidence;
use crate::graph::model::EdgeKind;
use crate::graph::rank::pagerank;
use crate::graph::store::KnowledgeGraph;
use crate::retrieval::candidates::{graph_prior, Candidate, CandidateFilter};
use crate::retrieval::frontier::rank_frontier;
use crate::retrieval::policy::{RetrievalMode, Thresholds, TraversalBudgets, Weights};
use crate::retrieval::seed::resolve_seeds;
use crate::retrieval::trace::{Trace, TraceEvent, TraceStats};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

pub struct RetrievalOutput {
    pub evidence: Vec<Evidence>,
    pub trace: Trace,
    pub visited: Vec<String>,
    /// Recorded decision exchanges for offline replay (no secrets).
    pub decisions: Vec<DecisionRecord>,
}

#[allow(clippy::too_many_arguments)]
pub async fn retrieve<D, E>(
    kg: &KnowledgeGraph,
    evidence_store: &E,
    decision: Option<&D>,
    sufficiency_decision: Option<&D>,
    query: &str,
    mode: RetrievalMode,
    budgets: &TraversalBudgets,
    thresholds: &Thresholds,
    weights: &Weights,
    filter: &CandidateFilter,
) -> anyhow::Result<RetrievalOutput>
where
    D: DecisionClient,
    E: EvidenceStore,
{
    let pr = pagerank(kg, 0.85, 50);
    let all: Vec<_> = kg.all_nodes();
    let seeds = resolve_seeds(query, &all, 3);
    let mut trace = Trace {
        query: query.to_string(),
        seeds: seeds
            .iter()
            .map(|s| (s.node_id.clone(), s.reason.clone()))
            .collect(),
        ..Default::default()
    };
    let mut stats = TraceStats::default();
    let mut visited: HashSet<String> = HashSet::new();
    let mut expanded_count = 0usize;
    let mut decision_calls = 0usize;
    let mut noul_questions = 0usize;
    let mut evidence_pool: Vec<Evidence> = vec![];
    let mut seen_evidence: HashSet<String> = HashSet::new();
    let mut stop_reason = "budget_exhausted".to_string();
    let mut decision_records: Vec<DecisionRecord> = vec![];

    if seeds.is_empty() {
        trace.stop_reason = "no_seeds".to_string();
        trace.stats = stats;
        return Ok(RetrievalOutput {
            evidence: vec![],
            trace,
            visited: vec![],
            decisions: vec![],
        });
    }

    // BFS queue of (node_id, hops_from_seed).
    let mut queue: VecDeque<(String, u32)> = VecDeque::new();
    for s in &seeds {
        queue.push_back((s.node_id.clone(), 0));
        visited.insert(s.node_id.clone());
    }
    stats.nodes_examined = visited.len();

    // Seed evidence first.
    collect_node_evidence(
        kg,
        evidence_store,
        &seeds.iter().map(|s| s.node_id.clone()).collect::<Vec<_>>(),
        &vec![],
        "seed",
        &mut evidence_pool,
        &mut seen_evidence,
        budgets,
    )
    .await?;
    trace.evidence = evidence_pool
        .iter()
        .map(|e| format!("{}#{}", e.document_id, e.source_id))
        .collect();

    let mut max_depth = 0u32;

    while let Some((current, hops)) = queue.pop_front() {
        max_depth = max_depth.max(hops);
        if hops >= budgets.max_hops {
            continue;
        }
        if expanded_count >= budgets.max_nodes_expanded {
            stop_reason = "max_nodes_expanded".into();
            break;
        }
        if visited.len() >= budgets.max_nodes_examined {
            stop_reason = "max_nodes_examined".into();
            break;
        }
        let Some(cur_node) = kg.get(&current) else {
            continue;
        };
        let cur_kind = cur_node.kind;
        let _ = cur_kind;

        // Generate candidate neighbours with deterministic filtering.
        let neighbours = kg.out_neighbours(&current);
        let mut cands: Vec<Candidate> = vec![];
        for (target, edge_kind, _conf) in &neighbours {
            stats.edges_examined += 1;
            if visited.contains(target) {
                continue;
            }
            let Some(tnode) = kg.get(target) else {
                continue;
            };
            if !filter.allowed_edges.contains(edge_kind) {
                continue;
            }
            if !filter.allowed_nodes.contains(&tnode.kind) {
                continue;
            }
            let ev_count = tnode.evidence.len();
            let gp = match mode {
                RetrievalMode::SemanticOnly => 0.1,
                _ => graph_prior(hops + 1, pr.get(target).copied().unwrap_or(0.0), *edge_kind, ev_count),
            };
            cands.push(Candidate {
                from: current.clone(),
                to: target.clone(),
                edge_kind: *edge_kind,
                node_kind: tnode.kind,
                label: tnode.label.clone(),
                hops: hops + 1,
                graph_prior: gp,
                semantic: None,
                frontier_score: None,
                decision: None,
            });
        }
        if cands.is_empty() {
            continue;
        }
        // Cap frontier before semantic calls (H5: priors shrink decision load).
        cands.sort_by(|a, b| {
            b.graph_prior
                .partial_cmp(&a.graph_prior)
                .unwrap()
                .then_with(|| a.to.cmp(&b.to))
        });
        cands.truncate(budgets.max_frontier_size);

        // Semantic relevance via batched independent Nouls.
        let use_semantic = matches!(mode, RetrievalMode::SemanticOnly | RetrievalMode::Hybrid);
        if use_semantic {
            if decision_calls >= budgets.max_decision_calls {
                // Fallback: deterministic-only for remaining hops.
                for c in &mut cands {
                    c.semantic = Some(0.0);
                    c.frontier_score = Some(score_frontier(0.0, c.graph_prior, c.hops, weights, mode));
                    c.decision = Some("NO_DECISION_BUDGET".into());
                }
            } else if let Some(dec) = decision {
                // One System One request with N independent Noul questions.
                let mut questions = BTreeMap::new();
                for c in &cands {
                    let tnode = kg.get(&c.to).unwrap();
                    let state_desc = format!(
                        "candidate {} ({:?}) via {:?} from {}",
                        tnode.label, tnode.kind, c.edge_kind, current
                    );
                    let _ = state_desc;
                    questions.insert(
                        c.to.clone(),
                        Question::Noul {
                            instructions: format!(
                                "User query: \"{}\". Current node: {} ({}). Candidate: {} ({} — {}) related by {:?}. Description: {}. Could following this relationship lead to evidence needed to answer the user's query? Answer P(yes).",
                                query,
                                current,
                                kg.get(&current).map(|n| n.label.clone()).unwrap_or_default(),
                                c.to,
                                tnode.label,
                                format!("{:?}", tnode.kind),
                                c.edge_kind,
                                tnode.description.clone().unwrap_or_default(),
                            ),
                        },
                    );
                }
                noul_questions += questions.len();
                let req = DecisionRequest {
                    state: serde_json::json!({
                        "query": query,
                        "current": current,
                    }),
                    questions,
                    model: None,
                };
                decision_calls += 1;
                stats.decision_calls = decision_calls;
                stats.noul_questions = noul_questions;
                match dec.decide(req.clone()).await {
                    Ok(resp) => {
                        decision_records.push(DecisionRecord {
                            case_id: None,
                            request: req,
                            response: resp.clone(),
                        });
                        for c in &mut cands {
                            let p = resp
                                .answers
                                .get(&c.to)
                                .map(|a| a.relevance())
                                .unwrap_or(0.0);
                            c.semantic = Some(p);
                            c.frontier_score =
                                Some(score_frontier(p, c.graph_prior, c.hops, weights, mode));
                            c.decision = Some(if p >= thresholds.accept {
                                "ACCEPT".into()
                            } else if p >= thresholds.review {
                                "REVIEW".into()
                            } else {
                                "REJECT".into()
                            });
                        }
                    }
                    Err(e) => {
                        // Escalation: on decision error, fall back to deterministic priors.
                        for c in &mut cands {
                            c.semantic = Some(0.0);
                            c.frontier_score =
                                Some(score_frontier(0.0, c.graph_prior, c.hops, weights, mode));
                            c.decision = Some(format!("ERROR_FALLBACK:{e}"));
                        }
                    }
                }
            } else {
                for c in &mut cands {
                    c.semantic = Some(0.0);
                    c.frontier_score = Some(score_frontier(0.0, c.graph_prior, c.hops, weights, mode));
                    c.decision = Some("NO_CLIENT".into());
                }
            }
        } else {
            // Deterministic mode: accept by graph-prior threshold (median split).
            for c in &mut cands {
                let accept = c.graph_prior >= 0.35;
                c.semantic = None;
                c.frontier_score = Some(c.graph_prior as f64);
                c.decision = Some(if accept { "ACCEPT" } else { "REJECT" }.into());
            }
        }

        rank_frontier(&mut cands);

        // Enqueue accepted (+ secondary frontier on REVIEW if budget allows).
        for c in &cands {
            let dec = c.decision.clone().unwrap_or_default();
            trace.events.push(TraceEvent {
                hop: hops,
                from: c.from.clone(),
                to: c.to.clone(),
                edge: edge_kind_str(c.edge_kind),
                pagerank: pr.get(&c.to).copied().unwrap_or(0.0),
                graph_prior: c.graph_prior,
                semantic: c.semantic,
                decision: dec.clone(),
            });
            let accept = dec == "ACCEPT"
                || (mode == RetrievalMode::Deterministic && dec == "ACCEPT");
            let review = dec == "REVIEW";
            if accept || (review && queue.len() < budgets.max_frontier_size) {
                if visited.insert(c.to.clone()) {
                    queue.push_back((c.to.clone(), c.hops));
                    stats.nodes_examined = visited.len();
                }
            }
        }
        expanded_count += 1;
        stats.nodes_expanded = expanded_count;

        // Collect evidence from newly visited nodes + traversed edges.
        let newly: Vec<String> = cands
            .iter()
            .filter(|c| {
                c.decision.as_deref() == Some("ACCEPT")
                    || c.decision.as_deref() == Some("REVIEW")
            })
            .map(|c| c.to.clone())
            .collect();
        let paths: HashMap<String, Vec<String>> = newly
            .iter()
            .map(|n| (n.clone(), vec![current.clone(), n.clone()]))
            .collect();
        collect_node_evidence(
            kg,
            evidence_store,
            &newly,
            &paths_vec(&paths),
            "traversal",
            &mut evidence_pool,
            &mut seen_evidence,
            budgets,
        )
        .await?;
        trace.evidence = evidence_pool
            .iter()
            .map(|e| format!("{}#{}", e.document_id, e.source_id))
            .collect();
        stats.evidence_items = evidence_pool.len();

        // Evidence sufficiency check (bounded Noul, Rust owns the loop).
        if let Some(sdec) = sufficiency_decision {
            if !evidence_pool.is_empty() && decision_calls < budgets.max_decision_calls {
                let mut q = BTreeMap::new();
                q.insert(
                    "sufficient".into(),
                    Question::Noul {
                        instructions: format!(
                            "User query: \"{}\". Retrieved {} evidence items. Does the retrieved evidence contain enough information to answer the query? Answer P(yes).",
                            query,
                            evidence_pool.len()
                        ),
                    },
                );
                let req = DecisionRequest {
                    state: serde_json::json!({
                        "query": query,
                        "evidence_count": evidence_pool.len(),
                    }),
                    questions: q,
                    model: None,
                };
                decision_calls += 1;
                noul_questions += 1;
                stats.decision_calls = decision_calls;
                stats.noul_questions = noul_questions;
                if let Ok(resp) = sdec.decide(req.clone()).await {
                    decision_records.push(DecisionRecord {
                        case_id: None,
                        request: req,
                        response: resp.clone(),
                    });
                    if let Some(ans) = resp.answers.get("sufficient") {
                        if ans.relevance() >= thresholds.sufficiency {
                            stop_reason = "evidence_sufficient".into();
                            break;
                        }
                    }
                }
            }
        } else if evidence_pool.len() >= budgets.max_evidence_items {
            stop_reason = "evidence_budget".into();
            break;
        }
        if queue.is_empty() {
            stop_reason = "frontier_exhausted".into();
            break;
        }
    }

    if queue.is_empty() && stop_reason == "budget_exhausted" {
        stop_reason = "frontier_exhausted".into();
    }
    // Cap evidence to budgets.
    evidence_pool.truncate(budgets.max_evidence_items);
    stats.evidence_items = evidence_pool.len();
    stats.max_depth = max_depth;
    trace.stop_reason = stop_reason;
    trace.stats = stats;
    let visited_vec: Vec<String> = visited.into_iter().collect();
    Ok(RetrievalOutput {
        evidence: evidence_pool,
        trace,
        visited: visited_vec,
        decisions: decision_records,
    })
}

fn paths_vec(paths: &HashMap<String, Vec<String>>) -> Vec<(String, Vec<String>)> {
    paths.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

async fn collect_node_evidence<E: EvidenceStore>(
    kg: &KnowledgeGraph,
    store: &E,
    node_ids: &[String],
    _paths: &[(String, Vec<String>)],
    reason: &str,
    pool: &mut Vec<Evidence>,
    seen: &mut HashSet<String>,
    budgets: &TraversalBudgets,
) -> anyhow::Result<()> {
    for nid in node_ids {
        let Some(node) = kg.get(nid) else { continue };
        if node.evidence.is_empty() {
            continue;
        }
        let fetched = store.fetch(&node.evidence).await?;
        for mut ev in fetched {
            let key = format!("{}#{}", ev.document_id, ev.source_id);
            if seen.insert(key) {
                ev.path = vec![nid.clone()];
                ev.reason = reason.to_string();
                pool.push(ev);
                if pool.len() >= budgets.max_evidence_items {
                    return Ok(());
                }
            }
        }
        // Source-diversity cap.
        let sources: HashSet<String> = pool.iter().map(|e| e.source_id.clone()).collect();
        if sources.len() >= budgets.max_sources {
            return Ok(());
        }
    }
    Ok(())
}

fn score_frontier(sem: f64, graph_prior: f32, hops: u32, w: &Weights, mode: RetrievalMode) -> f64 {
    let proximity = match hops {
        0 => 1.0,
        1 => 0.8,
        2 => 0.5,
        3 => 0.25,
        _ => 0.1,
    };
    match mode {
        RetrievalMode::Deterministic => graph_prior as f64,
        RetrievalMode::SemanticOnly => sem,
        RetrievalMode::Hybrid => {
            w.semantic * sem + w.graph_prior * graph_prior as f64 + w.proximity * proximity
        }
    }
}

fn edge_kind_str(k: EdgeKind) -> String {
    match k {
        EdgeKind::Contains => "contains".into(),
        EdgeKind::Mentions => "mentions".into(),
        EdgeKind::RefersTo => "refers_to".into(),
        EdgeKind::PartOf => "part_of".into(),
        EdgeKind::RelatedTo => "related_to".into(),
        EdgeKind::Supports => "supports".into(),
        EdgeKind::Contradicts => "contradicts".into(),
    }
}
