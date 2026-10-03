use crate::graph::store::KnowledgeGraph;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Iterative PageRank over the directed graph.
///
/// Use `pagerank_shared` on hot retrieval paths so repeated queries reuse
/// the cached rank vector without cloning it. This compatibility wrapper keeps
/// the original owned return type for callers that need an independent map.
pub fn pagerank(kg: &KnowledgeGraph, damping: f32, iterations: usize) -> BTreeMap<String, f32> {
    (*pagerank_shared(kg, damping, iterations)).clone()
}

/// Cached PageRank over the directed graph.
///
/// Structural-importance signal only — NOT query relevance. The cache is keyed
/// by damping/iteration configuration and is invalidated by graph mutation.
/// Returns normalized ranks summing to 1.0 (uniform fallback for empty graph).
/// BTreeMaps keep float summation deterministic: same graph -> bit-identical
/// ranks.
pub fn pagerank_shared(
    kg: &KnowledgeGraph,
    damping: f32,
    iterations: usize,
) -> Arc<BTreeMap<String, f32>> {
    if let Some(cached) = kg.cached_pagerank(damping, iterations) {
        return cached;
    }

    let ids = kg.all_node_ids();
    let n = ids.len();
    let mut ranks: BTreeMap<String, f32> = BTreeMap::new();
    if n == 0 {
        let ranks = Arc::new(ranks);
        kg.store_pagerank(damping, iterations, Arc::clone(&ranks));
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
        for (target, _kind, _conf, _egev) in outs {
            incoming.entry(target).or_default().push((id.clone(), 1.0));
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

    let ranks = Arc::new(ranks);
    kg.store_pagerank(damping, iterations, Arc::clone(&ranks));
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

    #[test]
    fn shared_pagerank_reuses_cache_and_mutation_invalidates_it() {
        let mut kg = tiny_graph();
        let first = pagerank_shared(&kg, 0.85, 50);
        let second = pagerank_shared(&kg, 0.85, 50);
        assert!(Arc::ptr_eq(&first, &second));

        kg.add_node(Node {
            id: "c".into(),
            kind: NodeKind::Concept,
            label: "C".into(),
            description: None,
            aliases: vec![],
            evidence: vec![],
        })
        .unwrap();

        let third = pagerank_shared(&kg, 0.85, 50);
        assert!(!Arc::ptr_eq(&first, &third));
        assert_eq!(third.len(), 3);
    }
}
