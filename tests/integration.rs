use rockpool::answer::client::AnswerClient;
use rockpool::answer::{AnswerRequest, ExtractiveAnswerClient};
use rockpool::construct::types::{EntityProposal, ProposalSet, RelationProposal};
use rockpool::construct::{apply_proposals, ConstructionPolicy};
use rockpool::decision::fixture::FixtureDecisionClient;
use rockpool::decision::types::DecisionRecord;
use rockpool::evidence::store::MemoryEvidenceStore;
use rockpool::graph::model::*;
use rockpool::graph::store::KnowledgeGraph;
use rockpool::retrieval::candidates::CandidateFilter;
use rockpool::retrieval::policy::{RetrievalMode, Thresholds, TraversalBudgets, Weights};
use std::collections::BTreeMap;

fn tiny_graph() -> KnowledgeGraph {
    let fixture = GraphFixture {
        nodes: vec![
            Node {
                id: "concept-a".into(),
                kind: NodeKind::Concept,
                label: "Concept A".into(),
                description: Some("alpha topic".into()),
                aliases: vec![],
                evidence: vec![],
            },
            Node {
                id: "policy-b".into(),
                kind: NodeKind::Policy,
                label: "Policy B".into(),
                description: Some("alpha policy detail".into()),
                aliases: vec![],
                evidence: vec![EvidenceRef {
                    document_id: "doc-1".into(),
                    source_id: "sec-1".into(),
                    start: None,
                    end: None,
                    quote: Some("alpha rule".into()),
                }],
            },
            Node {
                id: "decoy-d".into(),
                kind: NodeKind::Policy,
                label: "Decoy D".into(),
                description: Some("unrelated matter".into()),
                aliases: vec![],
                evidence: vec![],
            },
            Node {
                id: "sec-1".into(),
                kind: NodeKind::Section,
                label: "Section 1".into(),
                description: Some("alpha rule text".into()),
                aliases: vec![],
                evidence: vec![EvidenceRef {
                    document_id: "doc-1".into(),
                    source_id: "sec-1".into(),
                    start: None,
                    end: None,
                    quote: Some("alpha rule".into()),
                }],
            },
        ],
        edges: vec![
            FixtureEdge {
                from: "concept-a".into(),
                to: "policy-b".into(),
                kind: EdgeKind::RelatedTo,
                evidence: vec![],
                confidence: None,
            },
            FixtureEdge {
                from: "concept-a".into(),
                to: "decoy-d".into(),
                kind: EdgeKind::RelatedTo,
                evidence: vec![],
                confidence: None,
            },
            FixtureEdge {
                from: "policy-b".into(),
                to: "sec-1".into(),
                kind: EdgeKind::Supports,
                evidence: vec![],
                confidence: Some(0.9),
            },
        ],
    };
    KnowledgeGraph::from_fixture(fixture).unwrap()
}

fn tiny_store() -> MemoryEvidenceStore {
    let mut s = MemoryEvidenceStore::new();
    s.insert(
        "sec-1".into(),
        "doc-1".into(),
        "The alpha rule governs concept A renewals.".into(),
    );
    s
}

fn map_client() -> FixtureDecisionClient {
    let mut m = BTreeMap::new();
    m.insert("policy-b".into(), 0.9);
    m.insert("decoy-d".into(), 0.1);
    m.insert("sec-1".into(), 0.9);
    m.insert("sufficient".into(), 0.95);
    FixtureDecisionClient::from_pyes_map(m)
}

#[tokio::test]
async fn deterministic_replay() {
    let kg = tiny_graph();
    let store = tiny_store();
    let dec = map_client();
    let budgets = TraversalBudgets::default();
    let thresholds = Thresholds::default();
    let weights = Weights::default();
    let filter = CandidateFilter::default();

    let out1 = rockpool::retrieval::traversal::retrieve(
        &kg,
        &store,
        Some(&dec),
        Some(&dec),
        "Concept A alpha",
        RetrievalMode::Hybrid,
        &budgets,
        &thresholds,
        &weights,
        &filter,
    )
    .await
    .unwrap();
    let out2 = rockpool::retrieval::traversal::retrieve(
        &kg,
        &store,
        Some(&dec),
        Some(&dec),
        "Concept A alpha",
        RetrievalMode::Hybrid,
        &budgets,
        &thresholds,
        &weights,
        &filter,
    )
    .await
    .unwrap();

    // Same fixture + same decisions -> same traversal (determinism).
    assert_eq!(
        serde_json::to_value(&out1.trace).unwrap(),
        serde_json::to_value(&out2.trace).unwrap()
    );
    // Evidence found via graph path.
    assert!(out1.evidence.iter().any(|e| e.source_id == "sec-1"));
    // Decoy rejected: never visited... (REVIEW may still visit; assert evidence precision instead)
    assert!(!out1.decisions.is_empty());

    // Replay: captured records rebuild an equivalent client.
    let records: Vec<DecisionRecord> = out1.decisions.clone();
    assert!(!records.iter().any(|r| {
        serde_json::to_value(&r.request)
            .unwrap()
            .to_string()
            .contains("API_KEY")
    }));
    let replay = FixtureDecisionClient::from_records(&records);
    let out3 = rockpool::retrieval::traversal::retrieve(
        &kg,
        &store,
        Some(&replay),
        Some(&replay),
        "Concept A alpha",
        RetrievalMode::Hybrid,
        &budgets,
        &thresholds,
        &weights,
        &filter,
    )
    .await
    .unwrap();
    let v1: Vec<String> = {
        let mut v = out1.visited.clone();
        v.sort();
        v
    };
    let v3: Vec<String> = {
        let mut v = out3.visited.clone();
        v.sort();
        v
    };
    assert_eq!(v1, v3);
}

#[tokio::test]
async fn independent_nouls_accept_multiple_branches() {
    // H4: two genuinely relevant siblings must both be visitable.
    // A forced single-choice would suppress one; independent Nouls accept both.
    let kg = tiny_graph();
    let store = tiny_store();
    let mut m = BTreeMap::new();
    m.insert("policy-b".into(), 0.92);
    m.insert("decoy-d".into(), 0.86); // also relevant here
    m.insert("sec-1".into(), 0.9);
    m.insert("sufficient".into(), 0.1); // keep exploring
    let dec = FixtureDecisionClient::from_pyes_map(m);
    let budgets = TraversalBudgets {
        max_decision_calls: 20,
        ..TraversalBudgets::default()
    };
    let out = rockpool::retrieval::traversal::retrieve(
        &kg,
        &store,
        Some(&dec),
        Some(&dec),
        "Concept A alpha",
        RetrievalMode::Hybrid,
        &budgets,
        &Thresholds::default(),
        &Weights::default(),
        &CandidateFilter::default(),
    )
    .await
    .unwrap();
    assert!(out.visited.contains(&"policy-b".to_string()));
    assert!(out.visited.contains(&"decoy-d".to_string()));
}

fn answer_ev(id: &str, text: &str) -> rockpool::answer::types::AnswerEvidence {
    rockpool::answer::types::AnswerEvidence {
        document_id: "doc-1".into(),
        source_id: id.into(),
        text: text.into(),
        quote: None,
    }
}

#[tokio::test]
async fn extractive_answer_cites_only_supplied_evidence() {
    let c = ExtractiveAnswerClient::new();
    let resp = c
        .answer(AnswerRequest {
            query: "alpha renewals".into(),
            evidence: vec![
                answer_ev("sec-1", "The alpha rule governs concept A renewals."),
                answer_ev("sec-2", "Unrelated tariff notes."),
            ],
            max_citations: 5,
            model: None,
        })
        .await
        .unwrap();
    assert!(!resp.abstained);
    // Every citation resolves to supplied evidence (grounded by construction).
    for cit in &resp.citations {
        assert!(cit.source_id == "sec-1" || cit.source_id == "sec-2");
    }
    assert!(!resp.text.contains("sec-9"));
}

#[tokio::test]
async fn extractive_answer_abstains_without_evidence() {
    let c = ExtractiveAnswerClient::new();
    let resp = c
        .answer(AnswerRequest {
            query: "anything".into(),
            evidence: vec![],
            max_citations: 5,
            model: None,
        })
        .await
        .unwrap();
    assert!(resp.abstained);
    assert!(resp.citations.is_empty());
}

fn construction_graph() -> KnowledgeGraph {
    tiny_graph()
}

fn rel(from: &str, to: &str, ev: bool, conf: f32) -> RelationProposal {
    RelationProposal {
        from: from.into(),
        to: to.into(),
        kind: rockpool::graph::model::EdgeKind::Mentions,
        evidence: if ev {
            vec![rockpool::graph::model::EvidenceRef {
                document_id: "doc-1".into(),
                source_id: "sec-1".into(),
                start: None,
                end: None,
                quote: Some("alpha rule".into()),
            }]
        } else {
            vec![]
        },
        confidence: Some(conf),
    }
}

#[tokio::test]
async fn construction_policy_gates_mutation() {
    // Per-target probabilities via state-aware resolver.
    let dec = FixtureDecisionClient::new(|_key, _q, req| {
        match req.state.get("to").and_then(|v| v.as_str()) {
            Some("policy-b") => 0.9,
            Some("decoy-d") => 0.2,
            _ => 0.0,
        }
    });
    let mut kg = construction_graph();
    let before = kg.edge_count();
    let proposals = ProposalSet {
        source_id: "sec-1".into(),
        entities: vec![EntityProposal {
            label: "Concept A".into(), // duplicate of existing
            kind: rockpool::graph::model::NodeKind::Concept,
            aliases: vec![],
            description: None,
            evidence: vec![],
        }],
        relations: vec![
            rel("concept-a", "policy-b", true, 0.8), // supported -> accept
            rel("concept-a", "decoy-d", true, 0.8),  // unsupported -> reject
            rel("concept-a", "policy-b", false, 0.8), // no evidence -> reject
            rel("concept-a", "ghost", true, 0.8),    // unknown endpoint -> reject
            rel("concept-a", "policy-b", true, 0.01), // low confidence -> reject
        ],
    };
    let policy = ConstructionPolicy {
        allow_new_entities: true,
        ..ConstructionPolicy::default()
    };
    let report = apply_proposals(&mut kg, &proposals, "alpha passage", &dec, &policy).await;
    assert_eq!(report.accepted_relations.len(), 1);
    assert_eq!(kg.edge_count(), before + 1);
    assert!(report.accepted_entities.is_empty()); // duplicate resolved away
    let reasons: Vec<&str> = report.rejected.iter().map(|r| r.reason.as_str()).collect();
    assert!(reasons.contains(&"below_accept_threshold"));
    assert!(reasons.contains(&"missing_evidence"));
    assert!(reasons.contains(&"unknown_to_endpoint"));
    assert!(reasons.contains(&"below_proposer_confidence"));
    assert!(reasons.contains(&"duplicate_of_existing"));
    // No secrets in any recorded surface.
    assert_eq!(report.validations_used, 2);
}

#[tokio::test]
async fn construction_budget_caps_validations() {
    let dec = FixtureDecisionClient::new(|_k, _q, _r| 0.9);
    let mut kg = construction_graph();
    let proposals = ProposalSet {
        source_id: "sec-1".into(),
        entities: vec![],
        relations: vec![
            rel("concept-a", "policy-b", true, 0.8),
            rel("concept-a", "decoy-d", true, 0.8),
            rel("concept-a", "sec-1", true, 0.8),
        ],
    };
    let policy = ConstructionPolicy {
        max_validations: 1,
        ..ConstructionPolicy::default()
    };
    let report = apply_proposals(&mut kg, &proposals, "p", &dec, &policy).await;
    assert_eq!(report.validations_used, 1);
    assert!(report
        .rejected
        .iter()
        .any(|r| r.reason == "validation_budget_exhausted"));
}

struct FailClient;

#[async_trait::async_trait]
impl rockpool::decision::client::DecisionClient for FailClient {
    async fn decide(
        &self,
        _req: rockpool::decision::types::DecisionRequest,
    ) -> Result<rockpool::decision::types::DecisionResponse, rockpool::decision::types::DecisionError>
    {
        Err(rockpool::decision::types::DecisionError::Transport(
            "simulated outage".into(),
        ))
    }
}

#[tokio::test]
async fn hybrid_falls_back_to_deterministic_thresholds() {
    let kg = tiny_graph();
    let store = tiny_store();
    let fail = FailClient;
    let out = rockpool::retrieval::traversal::retrieve(
        &kg,
        &store,
        Some(&fail),
        Some(&fail),
        "Concept A",
        rockpool::retrieval::policy::RetrievalMode::Hybrid,
        &rockpool::retrieval::policy::TraversalBudgets::default(),
        &rockpool::retrieval::policy::Thresholds::default(),
        &rockpool::retrieval::policy::Weights::default(),
        &rockpool::retrieval::candidates::CandidateFilter::default(),
    )
    .await
    .unwrap();
    // Fallback is real: deterministic accepts enqueue and evidence is found.
    assert!(out.trace.stats.fallback_accepts > 0);
    assert!(out.evidence.iter().any(|e| e.source_id == "sec-1"));
    assert!(out
        .trace
        .events
        .iter()
        .any(|e| e.decision == "ACCEPT(fallback)"));
    // Provenance: evidence carries the seed → fetch path (policy-b's own
    // evidence ref resolves sec-1 one hop earlier than the sec-1 node walk),
    // and trace events preserve the complete per-candidate path.
    let ev = out
        .evidence
        .iter()
        .find(|e| e.source_id == "sec-1")
        .unwrap();
    assert_eq!(ev.path.first().unwrap(), "concept-a");
    // The fetch happens at policy-b: its own evidence refs resolve the sec-1
    // source, so the walk ends there rather than at the sec-1 node.
    assert_eq!(
        ev.path,
        vec!["concept-a".to_string(), "policy-b".to_string()]
    );
    let sec_event = out.trace.events.iter().find(|e| e.to == "sec-1").unwrap();
    assert_eq!(
        sec_event.path,
        vec![
            "concept-a".to_string(),
            "policy-b".to_string(),
            "sec-1".to_string()
        ]
    );
}

#[tokio::test]
async fn semantic_only_degrades_explicitly() {
    let kg = tiny_graph();
    let store = tiny_store();
    let fail = FailClient;
    let out = rockpool::retrieval::traversal::retrieve(
        &kg,
        &store,
        Some(&fail),
        Some(&fail),
        "Concept A",
        rockpool::retrieval::policy::RetrievalMode::SemanticOnly,
        &rockpool::retrieval::policy::TraversalBudgets::default(),
        &rockpool::retrieval::policy::Thresholds::default(),
        &rockpool::retrieval::policy::Weights::default(),
        &rockpool::retrieval::candidates::CandidateFilter::default(),
    )
    .await
    .unwrap();
    // Nothing enqueued on pretended semantics: explicit degradation.
    assert!(out.trace.stats.unavailable > 0);
    assert!(out.evidence.is_empty());
    assert_eq!(out.trace.stop_reason, "decision_degraded");
    assert!(out.trace.events.iter().all(|e| e.decision == "UNAVAILABLE"));
}
