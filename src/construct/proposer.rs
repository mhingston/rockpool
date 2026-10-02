use super::types::ProposalSet;
use crate::graph::model::EdgeKind;
use crate::graph::store::KnowledgeGraph;
use async_trait::async_trait;

#[async_trait]
pub trait Proposer: Send + Sync {
    async fn propose(
        &self,
        source_id: &str,
        passage: &str,
        kg: &KnowledgeGraph,
    ) -> ProposalSet;
}

/// Deterministic mention proposer: for each known node whose label or alias
/// appears in the passage, propose a `mentions` relation from the owning
/// document node. A deliberately weak proposer — validation and policy decide
/// what survives. Live LLM proposers implement the same trait later.
pub struct MentionProposer {
    pub min_label_len: usize,
}

impl Default for MentionProposer {
    fn default() -> Self {
        Self { min_label_len: 4 }
    }
}

#[async_trait]
impl Proposer for MentionProposer {
    async fn propose(
        &self,
        source_id: &str,
        passage: &str,
        kg: &KnowledgeGraph,
    ) -> ProposalSet {
        let lower = passage.to_lowercase();
        let mut relations = vec![];
        // Owning node: prefer the Document node carrying evidence for this
        // source; fall back to any evidence-carrying node (e.g. a Section),
        // then to an id match. Deterministic: first match in graph order.
        let rank = |n: &crate::graph::model::Node| -> u8 {
            if matches!(n.kind, crate::graph::model::NodeKind::Document)
                && n.evidence.iter().any(|e| e.source_id == source_id)
            {
                0
            } else if n.evidence.iter().any(|e| e.source_id == source_id) {
                1
            } else if n.id == source_id {
                2
            } else {
                3
            }
        };
        let doc_node = kg
            .all_nodes()
            .into_iter()
            .filter(|n| rank(n) < 3)
            .min_by_key(|n| (rank(n), n.id.clone()))
            .map(|n| n.id.clone());
        if let Some(doc) = doc_node {
            for node in kg.all_nodes() {
                if node.id == doc {
                    continue;
                }
                let mut names = vec![node.label.clone()];
                names.extend(node.aliases.clone());
                let hit = names.iter().any(|nm| {
                    nm.len() >= self.min_label_len && lower.contains(&nm.to_lowercase())
                });
                if hit {
                    relations.push(super::types::RelationProposal {
                        from: doc.clone(),
                        to: node.id.clone(),
                        kind: EdgeKind::Mentions,
                        evidence: vec![crate::graph::model::EvidenceRef {
                            document_id: String::new(),
                            source_id: source_id.to_string(),
                            start: None,
                            end: None,
                            quote: None,
                        }],
                        confidence: Some(0.4),
                    });
                }
            }
        }
        ProposalSet {
            source_id: source_id.to_string(),
            entities: vec![],
            relations,
        }
    }
}
