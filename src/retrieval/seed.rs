use crate::graph::model::Node;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seed {
    pub node_id: String,
    pub reason: String,
    pub score: f64,
}

const MIN_WEIGHTED_OVERLAP: f64 = 0.12;

fn stem(t: &str) -> String {
    if t.ends_with("ies") && t.len() > 4 {
        return format!("{}y", &t[..t.len() - 3]);
    }
    if t.ends_with('s') && t.len() > 3 && !t.ends_with("ss") {
        return t[..t.len() - 1].to_string();
    }
    t.to_string()
}

fn is_stop(t: &str) -> bool {
    matches!(
        t,
        "what"
            | "which"
            | "does"
            | "do"
            | "how"
            | "why"
            | "when"
            | "with"
            | "from"
            | "that"
            | "this"
            | "these"
            | "those"
            | "are"
            | "was"
            | "were"
            | "has"
            | "have"
            | "had"
            | "can"
            | "could"
            | "should"
            | "would"
            | "there"
            | "their"
            | "about"
            | "into"
            | "tell"
            | "the"
            | "and"
            | "for"
    )
}

fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 3)
        // Stop words must be removed before stemming: otherwise "does"
        // becomes "doe" and survives the stop-word filter.
        .filter(|t| !is_stop(t))
        .map(stem)
        .collect()
}

fn unique_tokens(s: &str) -> BTreeSet<String> {
    tokenize(s).into_iter().collect()
}

fn idf(total_docs: usize, document_frequency: usize) -> f64 {
    (((total_docs + 1) as f64) / ((document_frequency + 1) as f64)).ln() + 1.0
}

/// Deterministic seed resolution: exact label, alias, explicit IDs, then
/// IDF-weighted metadata overlap. No embeddings and no model call.
///
/// The weighted overlap deliberately favours rarer query terms over generic
/// terms. Query tokens are deduplicated so repeating a word cannot inflate a
/// candidate's score.
pub fn resolve_seeds(query: &str, nodes: &[&Node], top_k: usize) -> Vec<Seed> {
    let qnorm = query.trim().to_lowercase();

    let node_tokens: Vec<HashSet<String>> = nodes
        .iter()
        .map(|n| tokenize(&n.text_for_matching()).into_iter().collect())
        .collect();
    let mut document_frequency: HashMap<String, usize> = HashMap::new();
    for tokens in &node_tokens {
        for token in tokens {
            *document_frequency.entry(token.clone()).or_default() += 1;
        }
    }

    let query_tokens = unique_tokens(query);
    let query_weight: f64 = query_tokens
        .iter()
        .map(|t| idf(nodes.len(), document_frequency.get(t).copied().unwrap_or(0)))
        .sum();

    let mut scored: Vec<Seed> = vec![];
    for (idx, n) in nodes.iter().enumerate() {
        // Explicit ID: query equals node id or contains [node-id].
        if qnorm == n.id.to_lowercase() || query.contains(&n.id) {
            scored.push(Seed {
                node_id: n.id.clone(),
                reason: "explicit_id".into(),
                score: 1.0,
            });
            continue;
        }
        if n.label.to_lowercase() == qnorm {
            scored.push(Seed {
                node_id: n.id.clone(),
                reason: "exact_label".into(),
                score: 1.0,
            });
            continue;
        }
        if n.aliases.iter().any(|a| a.to_lowercase() == qnorm) {
            scored.push(Seed {
                node_id: n.id.clone(),
                reason: "exact_alias".into(),
                score: 0.95,
            });
            continue;
        }

        // Alias / label / description substring.
        let hay = n.text_for_matching().to_lowercase();
        if hay.contains(&qnorm) && qnorm.len() > 3 {
            scored.push(Seed {
                node_id: n.id.clone(),
                reason: "substring".into(),
                score: 0.7,
            });
            continue;
        }

        if query_weight == 0.0 {
            continue;
        }
        let matched_weight: f64 = query_tokens
            .iter()
            .filter(|t| node_tokens[idx].contains(*t))
            .map(|t| idf(nodes.len(), document_frequency.get(t).copied().unwrap_or(0)))
            .sum();
        let overlap = matched_weight / query_weight;
        if overlap >= MIN_WEIGHTED_OVERLAP {
            scored.push(Seed {
                node_id: n.id.clone(),
                reason: format!("idf_overlap:{overlap:.2}"),
                score: overlap * 0.8,
            });
        }
    }

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap()
            .then_with(|| a.node_id.cmp(&b.node_id))
    });
    scored.truncate(top_k.max(1));
    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::model::{Node, NodeKind};

    fn node(id: &str, label: &str, description: &str, aliases: &[&str]) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Concept,
            label: label.into(),
            description: Some(description.into()),
            aliases: aliases.iter().map(|s| (*s).to_string()).collect(),
            evidence: vec![],
        }
    }

    #[test]
    fn removes_stop_words_before_stemming() {
        assert_eq!(
            tokenize("What does the policy require?"),
            vec!["policy", "require"]
        );
    }

    #[test]
    fn duplicate_query_terms_do_not_change_weighting() {
        let nodes = vec![
            node("target", "Meridian wreck", "specific wreck", &["meridian"]),
            node("generic", "Wreck marker", "generic wreck marker", &[]),
        ];
        let refs: Vec<&Node> = nodes.iter().collect();
        let once = resolve_seeds("Meridian wreck", &refs, 2);
        let repeated = resolve_seeds("Meridian wreck wreck", &refs, 2);
        assert_eq!(once[0].node_id, "target");
        assert_eq!(repeated[0].node_id, "target");
    }

    #[test]
    fn rare_terms_break_generic_wreck_ties() {
        let mut nodes = vec![
            node(
                "meridian-wreck",
                "Coaster Meridian",
                "Wreck in twenty-one metres.",
                &["meridian"],
            ),
            node(
                "wreck-buoy-protocol",
                "Wreck buoy protocol",
                "Red spheres by day, occulting light by night.",
                &["wreck marking"],
            ),
            node(
                "buoyage-manual-wreck",
                "Buoyage manual (wreck marks)",
                "Wreck-marking buoy specification.",
                &[],
            ),
        ];
        for i in 1..=24 {
            nodes.push(node(
                &format!("marker-{i:02}"),
                &format!("Chart marker {i}"),
                "Routine charted mark with no bearing on wreck inquiries.",
                &[],
            ));
        }
        let refs: Vec<&Node> = nodes.iter().collect();
        let seeds = resolve_seeds(
            "What marks the Meridian wreck and what does the general wreck protocol require?",
            &refs,
            3,
        );
        let ids: Vec<&str> = seeds.iter().map(|s| s.node_id.as_str()).collect();

        assert!(ids.contains(&"meridian-wreck"), "{ids:?}");
        assert!(ids.contains(&"wreck-buoy-protocol"), "{ids:?}");
    }
}
