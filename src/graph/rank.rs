use crate::graph::store::KnowledgeGraph;
use std::collections::BTreeMap;

/// Iterative PageRank over the directed graph.
/// Structural-importance signal only — NOT query relevance.
/// Returns normalized ranks summing to 1.0 (uniform fallback for empty graph).
/// Uses BTreeMaps throughout so iteration (and hence float summation) order
/// is deterministic: same graph -> bit-identical ranks.
pub fn pagerank(kg: &KnowledgeGraph, damping: f32, iterations: usize) -> BTreeMap<String, f32> {
    let ids = kg.all_node_ids();
    let n = ids.len();
    let mut ranks: BTreeMap<String, f32> = BTreeMap::new();
    if n == 0 {
        return ranks;
    }
    let init = 1.0 / n as f32;
    for id in &ids {
        ranks.insert(id.clone(), init);
    }
    // Precompute out-degree (directed). Dangling nodes distribute uniformly.
    let mut out_degree: BTreeMap<String, usize> = BTreeMap::new();
    let mut incoming: BTreeMap<String, Vec<(String, f32)>> = BTreeMap::new();
    for id in &ids {
        let outs = kg.out_neighbours(id);
        out_degree.insert(id.clone(), outs.len());
        for (target, _kind, _conf) in outs {
            incoming
                .entry(target)
                .or_default()
                .push((id.clone(), 1.0));
        }
    }
    for _ in 0..iterations {
        let dangling_sum: f32 = ids
            .iter()
            .filter(|id| out_degree.get(*id).copied().unwrap_or(0) == 0)
            .map(|id| ranks[id])
            .sum();
        let mut next: BTreeMap<String, f32> = BTreeMap::new();
        for id in &ids {
            let mut rank = (1.0 - damping) / n as f32;
            rank += damping * dangling_sum / n as f32;
            if let Some(srcs) = incoming.get(id) {
                for (src, _w) in srcs {
                    let deg = out_degree.get(src).copied().unwrap_or(0).max(1) as f32;
                    rank += damping * ranks[src] / deg;
                }
            }
            next.insert(id.clone(), rank);
        }
        ranks = next;
    }
    // Renormalize defensively.
    let sum: f32 = ranks.values().sum();
    if sum > 0.0 {
        for v in ranks.values_mut() {
            *v /= sum;
        }
    }
    ranks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::model::*;

    fn tiny_graph() -> KnowledgeGraph {
        let fixture = GraphFixture {
            nodes: vec![
                Node {
                    id: "a".into(),
                    kind: NodeKind::Concept,
                    label: "A".into(),
                    description: None,
                    aliases: vec![],
                    evidence: vec![],
                },
                Node {
                    id: "b".into(),
                    kind: NodeKind::Concept,
                    label: "B".into(),
                    description: None,
                    aliases: vec![],
                    evidence: vec![],
                },
            ],
            edges: vec![FixtureEdge {
                from: "a".into(),
                to: "b".into(),
                kind: EdgeKind::RelatedTo,
                evidence: vec![],
                confidence: None,
            }],
        };
        KnowledgeGraph::from_fixture(fixture).unwrap()
    }

    #[test]
    fn pagerank_sums_to_one() {
        let kg = tiny_graph();
        let pr = pagerank(&kg, 0.85, 50);
        let sum: f32 = pr.values().sum();
        assert!((sum - 1.0).abs() < 1e-3, "sum={sum}");
        assert!(pr["b"] > pr["a"]);
    }
}
