use rockpool::decision::fixture::FixtureDecisionClient;
use rockpool::evidence::store::MemoryEvidenceStore;
use rockpool::graph::model::{EdgeKind, FixtureEdge, GraphFixture, Node, NodeKind};
use rockpool::graph::rank::pagerank_shared;
use rockpool::graph::store::KnowledgeGraph;
use rockpool::retrieval::candidates::CandidateFilter;
use rockpool::retrieval::policy::{RetrievalMode, Thresholds, TraversalBudgets, Weights};
use rockpool::retrieval::seed::resolve_seeds;
use rockpool::retrieval::traversal::retrieve;
use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

const TIMED_SAMPLES: usize = 101;
const CACHED_PAGERANK_ITERS: usize = 100_000;

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

fn percentile(samples: &[Duration], percentile: f64) -> Duration {
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    let rank = ((ordered.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(ordered.len() - 1);
    ordered[rank]
}

fn time_samples<F>(mut f: F) -> Vec<Duration>
where
    F: FnMut(),
{
    (0..TIMED_SAMPLES)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect()
}

fn micros(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000_000.0
}

fn millis(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}

fn run_case(runtime: &tokio::runtime::Runtime, nodes: usize, fanout: usize) {
    let kg = synthetic_graph(nodes, fanout);
    let query = "Which rare needle routing policy applies?";

    // Cold PageRank is intentionally a single measurement because it performs
    // the expensive O(iterations * (V + E)) rank build. The hot-path cache hit
    // is measured over many iterations to avoid reporting timer-resolution
    // noise as a meaningful sub-microsecond result.
    let cold_start = Instant::now();
    let cold = pagerank_shared(&kg, 0.85, 50);
    let cold_elapsed = cold_start.elapsed();

    let cached_start = Instant::now();
    for _ in 0..CACHED_PAGERANK_ITERS {
        black_box(pagerank_shared(&kg, 0.85, 50));
    }
    let cached_total = cached_start.elapsed();
    let cached_per_call = cached_total / CACHED_PAGERANK_ITERS as u32;
    let cached = pagerank_shared(&kg, 0.85, 50);
    assert!(Arc::ptr_eq(&cold, &cached));

    let all = kg.all_nodes();

    // Warm seed resolution separately so allocator/cache setup is not counted
    // as the first timed observation.
    let seed_warm = resolve_seeds(black_box(query), black_box(&all), 3);
    black_box(seed_warm);

    let seed_samples = time_samples(|| {
        let seeds = resolve_seeds(black_box(query), black_box(&all), 3);
        black_box(seeds);
    });
    let seed_p50 = percentile(&seed_samples, 0.50);
    let seed_p95 = percentile(&seed_samples, 0.95);

    let store = MemoryEvidenceStore::new();
    let budgets = TraversalBudgets::default();
    let thresholds = Thresholds::default();
    let weights = Weights::default();
    let filter = CandidateFilter::default();
    let no_decision: Option<&FixtureDecisionClient> = None;

    // Warm the exact deterministic retrieval path before timing it.
    let warm = runtime
        .block_on(retrieve(
            &kg,
            &store,
            no_decision,
            no_decision,
            query,
            RetrievalMode::Deterministic,
            &budgets,
            &thresholds,
            &weights,
            &filter,
        ))
        .unwrap();
    black_box(warm.visited.len());

    let retrieval_samples = time_samples(|| {
        let output = runtime
            .block_on(retrieve(
                &kg,
                &store,
                no_decision,
                no_decision,
                black_box(query),
                RetrievalMode::Deterministic,
                &budgets,
                &thresholds,
                &weights,
                &filter,
            ))
            .unwrap();
        black_box((output.visited.len(), output.trace.stats.nodes_expanded));
    });
    let retrieval_p50 = percentile(&retrieval_samples, 0.50);
    let retrieval_p95 = percentile(&retrieval_samples, 0.95);

    let seeds = resolve_seeds(query, &all, 3);
    println!(
        "nodes={nodes} edges={} fanout={fanout} pagerank_cold_ms={:.3} pagerank_cached_avg_us={:.3} seed_p50_ms={:.3} seed_p95_ms={:.3} deterministic_retrieval_p50_ms={:.3} deterministic_retrieval_p95_ms={:.3} samples={} cache_iters={} top_seed={}",
        kg.edge_count(),
        millis(cold_elapsed),
        micros(cached_per_call),
        millis(seed_p50),
        millis(seed_p95),
        millis(retrieval_p50),
        millis(retrieval_p95),
        TIMED_SAMPLES,
        CACHED_PAGERANK_ITERS,
        seeds
            .first()
            .map(|s| s.node_id.as_str())
            .unwrap_or("<none>")
    );
}

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    for (nodes, fanout) in [(1_000, 4), (10_000, 4), (50_000, 4), (5_000, 25)] {
        run_case(&runtime, nodes, fanout);
    }
}
