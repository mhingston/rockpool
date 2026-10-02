use super::types::{EntityProposal, ProposalSet};
use super::validator::validate_relation;
use crate::decision::client::DecisionClient;
use crate::graph::model::{FixtureEdge, Node};
use crate::graph::store::KnowledgeGraph;
use serde::{Deserialize, Serialize};

/// Mutation policy. Owned by Rust — the model never decides these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstructionPolicy {
    /// Minimum P(supported) to accept a proposed relation.
    pub accept_threshold: f64,
    /// Minimum proposer confidence to even send for validation.
    pub min_proposer_confidence: f32,
    /// Reject evidence-less proposals unless explicitly allowed.
    pub require_evidence: bool,
    /// Allow new entity nodes, or only relations between existing nodes.
    pub allow_new_entities: bool,
    /// Maximum proposals validated per call (decision-call budget).
    pub max_validations: usize,
}

impl Default for ConstructionPolicy {
    fn default() -> Self {
        Self {
            accept_threshold: 0.6,
            min_proposer_confidence: 0.1,
            require_evidence: true,
            allow_new_entities: false,
            max_validations: 20,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConstructionReport {
    pub accepted_relations: Vec<String>,
    pub accepted_entities: Vec<String>,
    pub rejected: Vec<RejectedProposal>,
    pub validations_used: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedProposal {
    pub summary: String,
    pub reason: String,
    pub p_yes: Option<f64>,
}

/// Validate a proposal set and mutate the graph. Stages:
/// entity resolution → schema gate → bounded validation → policy-gated apply.
pub async fn apply_proposals<D: DecisionClient>(
    kg: &mut KnowledgeGraph,
    proposals: &ProposalSet,
    passage: &str,
    decision: &D,
    policy: &ConstructionPolicy,
) -> ConstructionReport {
    let mut report = ConstructionReport::default();

    // Stage 1 — entity resolution (deterministic): exact id/label/alias match.
    let mut new_entities: Vec<EntityProposal> = vec![];
    if policy.allow_new_entities {
        for e in &proposals.entities {
            if resolve_entity(kg, &e.label, &e.aliases).is_none() {
                new_entities.push(e.clone());
            } else {
                report.rejected.push(RejectedProposal {
                    summary: format!("entity '{}'", e.label),
                    reason: "duplicate_of_existing".into(),
                    p_yes: None,
                });
            }
        }
    } else {
        for e in &proposals.entities {
            report.rejected.push(RejectedProposal {
                summary: format!("entity '{}'", e.label),
                reason: "new_entities_disabled".into(),
                p_yes: None,
            });
        }
    }

    // Stage 2+3 — relation schema gate + bounded validation + gated apply.
    let mut validated = 0usize;
    for rel in &proposals.relations {
        let summary = format!("{} --{:?}--> {}", rel.from, rel.kind, rel.to);
        if validated >= policy.max_validations {
            report.rejected.push(RejectedProposal {
                summary,
                reason: "validation_budget_exhausted".into(),
                p_yes: None,
            });
            continue;
        }
        if rel.confidence.unwrap_or(1.0) < policy.min_proposer_confidence {
            report.rejected.push(RejectedProposal {
                summary,
                reason: "below_proposer_confidence".into(),
                p_yes: None,
            });
            continue;
        }
        if policy.require_evidence && rel.evidence.is_empty() {
            report.rejected.push(RejectedProposal {
                summary,
                reason: "missing_evidence".into(),
                p_yes: None,
            });
            continue;
        }
        // Endpoints must exist (v0: no implicit entity creation from relations).
        let from_label = match kg.get(&rel.from) {
            Some(n) => n.label.clone(),
            None => {
                report.rejected.push(RejectedProposal {
                    summary,
                    reason: "unknown_from_endpoint".into(),
                    p_yes: None,
                });
                continue;
            }
        };
        let to_label = match kg.get(&rel.to) {
            Some(n) => n.label.clone(),
            None => {
                report.rejected.push(RejectedProposal {
                    summary,
                    reason: "unknown_to_endpoint".into(),
                    p_yes: None,
                });
                continue;
            }
        };
        validated += 1;
        let p = match validate_relation(decision, passage, rel, &from_label, &to_label).await {
            Ok(p) => p,
            Err(e) => {
                report.rejected.push(RejectedProposal {
                    summary,
                    reason: format!("validation_error:{e}"),
                    p_yes: None,
                });
                continue;
            }
        };
        report.validations_used = validated;
        if p >= policy.accept_threshold {
            match kg.add_fixture_edge(FixtureEdge {
                from: rel.from.clone(),
                to: rel.to.clone(),
                kind: rel.kind,
                evidence: rel.evidence.clone(),
                confidence: rel.confidence,
            }) {
                Ok(()) => report.accepted_relations.push(summary),
                Err(e) => report.rejected.push(RejectedProposal {
                    summary,
                    reason: format!("mutation_failed:{e}"),
                    p_yes: Some(p),
                }),
            }
        } else {
            report.rejected.push(RejectedProposal {
                summary,
                reason: "below_accept_threshold".into(),
                p_yes: Some(p),
            });
        }
    }

    // Accepted entities are added only after relations validate (v0 keeps it
    // simple: entities without surviving relations are still recorded when
    // explicitly allowed, since resolution already deduped them).
    for e in new_entities {
        let id = slug(&e.label);
        if kg.get(&id).is_none() {
            let node = Node {
                id: id.clone(),
                kind: e.kind,
                label: e.label.clone(),
                description: e.description.clone(),
                aliases: e.aliases.clone(),
                evidence: e.evidence.clone(),
            };
            if kg.add_node(node).is_ok() {
                report.accepted_entities.push(id);
            }
        }
    }
    report
}

/// Deterministic entity resolution: existing id, label, or alias match.
pub fn resolve_entity(kg: &KnowledgeGraph, label: &str, aliases: &[String]) -> Option<String> {
    let l = label.to_lowercase();
    for n in kg.all_nodes() {
        if n.id.to_lowercase() == l || n.label.to_lowercase() == l {
            return Some(n.id.clone());
        }
        if n.aliases.iter().any(|a| {
            a.to_lowercase() == l || aliases.iter().any(|b| b.to_lowercase() == a.to_lowercase())
        }) {
            return Some(n.id.clone());
        }
    }
    None
}

fn slug(label: &str) -> String {
    label
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
