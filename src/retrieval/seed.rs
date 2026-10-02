use crate::graph::model::Node;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seed {
    pub node_id: String,
    pub reason: String,
    pub score: f64,
}

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
        .map(stem)
        .filter(|t| !is_stop(t))
        .collect()
}

fn token_overlap(a: &str, b: &str) -> f64 {
    let ta = tokenize(a);
    let tb_set: std::collections::HashSet<String> = tokenize(b).into_iter().collect();
    if ta.is_empty() {
        return 0.0;
    }
    let hits = ta.iter().filter(|t| tb_set.contains(*t)).count();
    hits as f64 / ta.len() as f64
}

/// Deterministic seed resolution: exact label, alias, token overlap, explicit IDs.
/// No embeddings. No free-form agent.
pub fn resolve_seeds(query: &str, nodes: &[&Node], top_k: usize) -> Vec<Seed> {
    let qnorm = query.trim().to_lowercase();
    let mut scored: Vec<Seed> = vec![];
    for n in nodes {
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
        // Alias substring / label substring.
        let hay = n.text_for_matching().to_lowercase();
        if hay.contains(&qnorm) && qnorm.len() > 3 {
            scored.push(Seed {
                node_id: n.id.clone(),
                reason: "substring".into(),
                score: 0.7,
            });
            continue;
        }
        let overlap = token_overlap(query, &n.text_for_matching());
        if overlap >= 0.25 {
            scored.push(Seed {
                node_id: n.id.clone(),
                reason: format!("token_overlap:{overlap:.2}"),
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
