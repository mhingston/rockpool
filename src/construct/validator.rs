use super::types::RelationProposal;
use crate::decision::client::DecisionClient;
use crate::decision::types::{DecisionRequest, Question};
use std::collections::BTreeMap;

/// Bounded semantic validation of one proposed relation:
/// "Does the passage support this relationship?"
/// Returns P(yes). Rust callers own acceptance, never the model.
pub async fn validate_relation<D: DecisionClient>(
    decision: &D,
    passage: &str,
    proposal: &RelationProposal,
    from_label: &str,
    to_label: &str,
) -> Result<f64, crate::decision::types::DecisionError> {
    let mut questions = BTreeMap::new();
    questions.insert(
        "supported".into(),
        Question::Noul {
            instructions: format!(
                "Passage:\n\"{}\"\n\nCandidate relation:\n{} --{:?}--> {}\n\nQuestion: \
                 Does the passage support this relationship? Answer P(yes).",
                passage.chars().take(1200).collect::<String>(),
                from_label,
                proposal.kind,
                to_label,
            ),
        },
    );
    let req = DecisionRequest {
        state: serde_json::json!({
            "from": proposal.from,
            "to": proposal.to,
            "kind": format!("{:?}", proposal.kind),
        }),
        questions,
        model: None,
    };
    let resp = decision.decide(req).await?;
    Ok(resp
        .answers
        .get("supported")
        .map(|a| a.relevance())
        .unwrap_or(0.0))
}
