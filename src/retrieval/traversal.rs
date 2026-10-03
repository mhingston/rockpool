use crate::decision::client::DecisionClient;
use crate::decision::types::{DecisionRecord, DecisionRequest, Question};
use crate::evidence::store::EvidenceStore;
use crate::evidence::types::Evidence;
use crate::graph::model::{EdgeKind, EvidenceRef};
use crate::graph::rank::pagerank_shared;
use crate::graph::store::KnowledgeGraph;
use crate::retrieval::candidates::{graph_prior, Candidate, CandidateFilter, Verdict};
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
    let pr = pagerank_shared(kg, 0.85, 50);
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

    // BFS queue of (node_id, hops_from_seed), plus the complete
    // seed → node path per visited node for provenance.
    let mut queue: VecDeque<(String, u32)> = VecDeque::new();
    let mut node_paths: HashMap<String, Vec<String>> = HashMap::new();
    for s in &seeds {
        queue.push_back((s.node_id.clone(), 0));
        visited.insert(s.node_id.clone());
        node_paths.insert(s.node_id.clone(), vec![s.node_id.clone()]);
    }
    stats.nodes_examined = visited.len();

    // Seed evidence first (path = [seed itself]).
    let seed_walks: Vec<EvidenceWalk> = seeds
        .iter()
        .filter_map(|s| {
            kg.get(&s.node_id).map(|n| EvidenceWalk {
                refs: n.evidence.clone(),
                path: vec![s.node_id.clone()],
                reason: "seed".into(),
            })
        })
        .collect();
    collect_evidence_walks(
        evidence_store,
        &seed_walks,
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
        for (target, edge_kind, _conf, edge_evidence) in &neighbours {
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
                _ => graph_prior(
                    hops + 1,
                    pr.get(target).copied().unwrap_or(0.0),
                    *edge_kind,
                    ev_count,
                ),
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
                edge_evidence: edge_evidence.clone(),
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
                // Decision budget exhausted: Hybrid genuinely falls back to
                // deterministic thresholds; SemanticOnly degrades explicitly.
                apply_no_semantic(&mut cands, mode);
            } else if let Some(dec) = decision {
                // One System One request with N independent Noul questions.
                let mut questions = BTreeMap::new();
                for c in &cands {
                    let tnode = kg.get(&c.to).unwrap();
                    let tnode_kind = format!("{:?}", tnode.kind);
                    let current_label = kg
                        .get(&current)
                        .map(|n| n.label.clone())
                        .unwrap_or_default();
                    let description = tnode.description.clone().unwrap_or_default();
                    questions.insert(
                        c.to.clone(),
                        Question::Noul {
                            instructions: format!(
                                "User query: \"{query}\". Current node: {current} ({current_label}). Candidate: {} ({tnode_label} — {tnode_kind}) related by {:?}. Description: {description}. Could following this relationship lead to evidence needed to answer the user's query? Answer P(yes).",
                                c.to,
                                c.edge_kind,
                                query = query,
                                current = current,
                                current_label = current_label,
                                tnode_label = tnode.label,
                                tnode_kind = tnode_kind,
                                description = description,
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
                                Verdict::Accept { fallback: false }
                            } else if p >= thresholds.review {
                                Verdict::Review
                            } else {
                                Verdict::Reject
                            });
                        }
                    }
                    Err(_e) => {
                        // Decision API failure: never pretend. Hybrid falls back
                        // to deterministic thresholds; SemanticOnly degrades.
                        apply_no_semantic(&mut cands, mode);
                    }
                }
            } else {
                // No decision client configured at all: same rule as failure.
                apply_no_semantic(&mut cands, mode);
            }
        } else {
            // Deterministic mode: accept by graph-prior threshold.
            for c in &mut cands {
                let accept = c.graph_prior >= DETERMINISTIC_ACCEPT;
                c.semantic = None;
                c.frontier_score = Some(c.graph_prior as f64);
                c.decision = Some(if accept {
                    Verdict::Accept { fallback: false }
                } else {
                    Verdict::Reject
                });
            }
        }

        rank_frontier(&mut cands);

        // Enqueue accepted (+ secondary frontier on REVIEW if budget allows).
        // Verdicts are first-class: only Accept/Review enqueue, and every
        // enqueued node records its complete seed → node path.
        let parent_path = node_paths
            .get(&current)
            .cloned()
            .unwrap_or_else(|| vec![current.clone()]);
        let mut walks: Vec<EvidenceWalk> = vec![];
        for c in &cands {
            let verdict = c.decision.clone().unwrap_or(Verdict::Reject);
            let mut path = parent_path.clone();
            path.push(c.to.clone());
            trace.events.push(TraceEvent {
                hop: hops,
                from: c.from.clone(),
                to: c.to.clone(),
                edge: edge_kind_str(c.edge_kind),
                pagerank: pr.get(&c.to).copied().unwrap_or(0.0),
                graph_prior: c.graph_prior,
                semantic: c.semantic,
                decision: verdict.render().to_string(),
                path: path.clone(),
            });
            match &verdict {
                Verdict::Accept { fallback: true } => stats.fallback_accepts += 1,
                Verdict::Unavailable => stats.unavailable += 1,
                _ => {}
            }
            let enqueue = matches!(verdict, Verdict::Accept { .. })
                || (matches!(verdict, Verdict::Review) && queue.len() < budgets.max_frontier_size);
            if enqueue && visited.insert(c.to.clone()) {
                queue.push_back((c.to.clone(), c.hops));
                node_paths.insert(c.to.clone(), path.clone());
                stats.nodes_examined = visited.len();
                // Node evidence + edge evidence, both carrying the full path.
                if let Some(tnode) = kg.get(&c.to) {
                    if !tnode.evidence.is_empty() {
                        walks.push(EvidenceWalk {
                            refs: tnode.evidence.clone(),
                            path: path.clone(),
                            reason: format!(
                                "traversal {} --{}--> {}",
                                c.from,
                                edge_kind_str(c.edge_kind),
                                c.to
                            ),
                        });
                    }
                }
                if !c.edge_evidence.is_empty() {
                    walks.push(EvidenceWalk {
                        refs: c.edge_evidence.clone(),
                        path: path.clone(),
                        reason: format!(
                            "edge {} --{}--> {}",
                            c.from,
                            edge_kind_str(c.edge_kind),
                            c.to
                        ),
                    });
                }
            }
        }
        expanded_count += 1;
        stats.nodes_expanded = expanded_count;

        collect_evidence_walks(
            evidence_store,
            &walks,
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
        // The model judges the actual passages — query plus a bounded,
        // clipped representation of each evidence item — never a bare count.
        if let Some(sdec) = sufficiency_decision {
            if !evidence_pool.is_empty() && decision_calls < budgets.max_decision_calls {
                let shown = evidence_pool.iter().take(6).enumerate().map(|(i, e)| {
                    let snippet: String = e
                        .quote
                        .as_deref()
                        .unwrap_or(e.text.as_str())
                        .chars()
                        .take(400)
                        .collect();
                    format!(
                        "{}. {}#{} — {}",
                        i + 1,
                        e.document_id,
                        e.source_id,
                        snippet.trim()
                    )
                });
                let mut listing = shown.collect::<Vec<_>>().join("\n");
                if evidence_pool.len() > 6 {
                    listing.push_str(&format!("\n…and {} more.", evidence_pool.len() - 6));
                }
                let mut q = BTreeMap::new();
                q.insert(
                    "sufficient".into(),
                    Question::Noul {
                        instructions: format!(
                            "User query: \"{query}\".\nEvidence passages:\n{listing}\n\nDo these passages jointly contain enough information to answer the query? Answer P(yes).",
                        ),
                    },
                );
                let state_evidence: Vec<serde_json::Value> = evidence_pool
                    .iter()
                    .take(6)
                    .map(|e| {
                        serde_json::json!({
                            "document_id": e.document_id,
                            "source_id": e.source_id,
                            "text": e.quote.as_deref().unwrap_or(e.text.as_str())
                                .chars().take(400).collect::<String>(),
                        })
                    })
                    .collect();
                let req = DecisionRequest {
                    state: serde_json::json!({
                        "query": query,
                        "evidence_count": evidence_pool.len(),
                        "evidence": state_evidence,
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
            // Explicit degradation: the semantic backend failed and the mode
            // forbade silent fallback, and nothing was retrieved.
            stop_reason = if evidence_pool.is_empty() && stats.unavailable > 0 {
                "decision_degraded".into()
            } else {
                "frontier_exhausted".into()
            };
            break;
        }
    }

    if queue.is_empty() && stop_reason == "budget_exhausted" {
        stop_reason = if evidence_pool.is_empty() && stats.unavailable > 0 {
            "decision_degraded".into()
        } else {
            "frontier_exhausted".into()
        };
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

/// Deterministic acceptance threshold shared by Deterministic mode and
/// Hybrid fallback. A test parameter, not a product truth.
pub const DETERMINISTIC_ACCEPT: f32 = 0.35;

/// One evidence-collection step: refs to fetch plus the complete graph path
/// that found them and the reason they were selected.
struct EvidenceWalk {
    refs: Vec<EvidenceRef>,
    path: Vec<String>,
    reason: String,
}

async fn collect_evidence_walks<E: EvidenceStore>(
    store: &E,
    walks: &[EvidenceWalk],
    pool: &mut Vec<Evidence>,
    seen: &mut HashSet<String>,
    budgets: &TraversalBudgets,
) -> anyhow::Result<()> {
    for walk in walks {
        if walk.refs.is_empty() {
            continue;
        }
        let fetched = store.fetch(&walk.refs).await?;
        for mut ev in fetched {
            let key = format!("{}#{}", ev.document_id, ev.source_id);
            if seen.insert(key) {
                ev.path = walk.path.clone();
                ev.reason = walk.reason.clone();
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

/// No semantic answer available (API failure, exhausted budget, or no client).
/// Hybrid genuinely falls back to deterministic thresholds and says so;
/// SemanticOnly degrades explicitly instead of pretending.
fn apply_no_semantic(cands: &mut [Candidate], mode: RetrievalMode) {
    for c in cands.iter_mut() {
        match mode {
            RetrievalMode::Hybrid => {
                let accept = c.graph_prior >= DETERMINISTIC_ACCEPT;
                c.semantic = None;
                c.frontier_score = Some(c.graph_prior as f64);
                c.decision = Some(if accept {
                    Verdict::Accept { fallback: true }
                } else {
                    Verdict::Reject
                });
            }
            _ => {
                c.semantic = None;
                c.frontier_score = Some(0.0);
                c.decision = Some(Verdict::Unavailable);
            }
        }
    }
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
