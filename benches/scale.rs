use rockpool::graph::model::{EdgeKind, FixtureEdge, GraphFixture, Node, NodeKind};
use rockpool::graph::rank::pagerank_shared;
use rockpool::graph::store::KnowledgeGraph;
use rockpool::retrieval::seed::resolve_seeds;
use std::sync::Arc;
use std::time::Instant;

fn synthetic_graph(nodes: usize, fanout: usize) -> KnowledgeGraph {
    let graph_nodes: Vec<Node> = (0..nodes)
        .map(|i| Node {
            id: format!("node-{i:06}"),
            kind: if i % 7 == 0 {
                NodeKind::Policy
            } else {
                NodeKind::Concept
            },
            label: format!("Graph node {i}"),
            description: Some(if i + 1 == nodes {
                "Rare needle routing policy target".into()
            } else {
                "Routine graph metadata for traversal benchmarking".into()
            }),
            aliases: vec![],
            evidence: vec![],
        })
        .collect();

    let mut edges = Vec::with_capacity(nodes * fanout);
    for i in 0..nodes {
        for offset in 1..=fanout {
            edges.push(FixtureEdge {
                from: format!("node-{i:06}"),
                to: format!("node-{:06}", (i + offset) % nodes),
                kind: EdgeKind::RelatedTo,
                evidence: vec![],
                confidence: None,
            });
        }
    }

    KnowledgeGraph::from_fixture(GraphFixture {
        nodes: graph_nodes,
        edges,
    })
    .unwrap()
}

fn run_case(nodes: usize, fanout: usize) {
    let kg = synthetic_graph(nodes, fanout);

    let cold_start = Instant::now();
    let cold = pagerank_shared(&kg, 0.85, 50);
    let cold_elapsed = cold_start.elapsed();

    let cached_start = Instant::now();
    let cached = pagerank_shared(&kg, 0.85, 50);
    let cached_elapsed = cached_start.elapsed();
    assert!(Arc::ptr_eq(&cold, &cached));

    let all = kg.all_nodes();
    let seed_start = Instant::now();
    let seeds = resolve_seeds("Which rare needle routing policy applies?", &all, 3);
    let seed_elapsed = seed_start.elapsed();

    println!(
        "nodes={nodes} edges={} fanout={fanout} pagerank_cold_ms={:.3} pagerank_cached_us={:.3} seed_resolution_ms={:.3} top_seed={}",
        kg.edge_count(),
        cold_elapsed.as_secs_f64() * 1_000.0,
        cached_elapsed.as_secs_f64() * 1_000_000.0,
        seed_elapsed.as_secs_f64() * 1_000.0,
        seeds
            .first()
            .map(|s| s.node_id.as_str())
            .unwrap_or("<none>")
    );
}

fn main() {
    for (nodes, fanout) in [(1_000, 4), (10_000, 4), (50_000, 4), (5_000, 25)] {
        run_case(nodes, fanout);
    }
}
