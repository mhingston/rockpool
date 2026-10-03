# rockpool

**Deterministic knowledge-graph retrieval with bounded semantic routing — in Rust.**

rockpool is a research prototype that answers questions over a small typed
knowledge graph. It pairs **cheap deterministic graph signals** (PageRank, hop
distance, edge/node types) with **small bounded semantic judgements**
(yes/no probability questions sent to a System One–compatible decision API),
while **Rust code owns all policy**: thresholds, budgets, traversal, mutation,
and termination. The model never explores freely — it only answers crisp
questions, and every answer traces back to inspectable source evidence.

```
question → seed resolution → graph traversal (+ semantic routing)
→ evidence references → source passages → cited answer
```

## Quickstart

Prerequisites: Rust 1.75+ (edition 2021), Python 3 (only to regenerate fixtures).

```bash
git clone https://github.com/mhingston/rockpool.git
cd rockpool
cargo build

# Ask a question (works fully offline with the deterministic stand-in)
cargo run -- query "What policies affect renewal pricing?"

# Get a grounded, cited answer
cargo run -- answer "What policies affect renewal pricing?"

# Run the evaluation suite
cargo test
cargo run -- eval
```

## Example

```bash
$ cargo run -- answer "What policies affect renewal pricing?"

Based on the retrieved evidence:

- loyalty consideration at term [document-14#document-14-section-4-2]
- standard tariff schedule [document-07#document-07-section-2-1]

citations:
  document-07#document-07-section-2-1
  document-14#document-14-section-4-2

[retrieval: 2 evidence, stop=evidence_sufficient]
```

Every query also renders its traversal trace — which seeds fired, every
candidate edge with its graph prior, semantic score, first-class verdict
(`ACCEPT` / `ACCEPT(fallback)` / `REVIEW` / `REJECT` / `UNAVAILABLE`) and the
complete seed → evidence path — so retrieval decisions are inspectable:

```bash
cargo run -- query "What policies affect renewal pricing?"
```

## Why this shape?

- **A graph node is not an answer.** Nodes and edges only *locate* evidence;
  the answer model receives grounded passages plus provenance.
- **Code owns policy.** Thresholds, budgets, tie-breaking, fallbacks and
  termination live in Rust and are deterministic: same fixture + same
  decisions ⇒ same traversal, bit-for-bit (tested).
- **Models answer small questions.** Independent yes/no judgements per
  candidate path (several branches can matter at once), plus an
  evidence-sufficiency check that only advises whether to keep traversing.
- **The System One contract is the dependency** — Jev, Von, Decider, or any
  conforming backend plug in behind one trait with config-only changes. No
  provider-specific defaults, no leaked credentials, malformed answers become
  errors at the boundary.

## Commands

| Command | What it does |
|---|---|
| `query "<q>" [--mode hybrid\|semantic\|deterministic] [--live] [--record f]` | Retrieve evidence with a full traversal trace |
| `answer "<q>" [--live] [--answer-live]` | Retrieve, then synthesize a cited answer (abstains when evidence is thin) |
| `eval [--live] [--json] [--cases f]` | Compare substring-lexical / BM25 / deterministic-graph / semantic / hybrid (+ BM25 curves) |
| `answer-eval [--live] [--json]` | Citation validity (resolvability), expected-evidence coverage, abstention behaviour |
| `construct <source-id> [--live]` | Propose → validate → apply graph mutations (in-memory demo) |
| `decide --state s --instructions i` | Single bounded judgement against the live endpoint |

Live modes read `DECISION_ENDPOINT`, optional `DECISION_MODEL`, optional
`DECISION_API_KEY` (Bearer sent only when set). Verified against TypeSafe Jev
(`jev-1.13.0`, `https://api.typesafe.ai/v1/systemone`); the answer stage can
additionally use any OpenAI-compatible chat endpoint via `ANSWER_*`.

## How well does it work?

### Recorded live retrieval evaluation (2026-10-02)

30 development + 8 held-out cases over corpus A (61 nodes / 102 edges /
14 sources), plus 10 cases over corpus B (40 nodes / 64 edges / 6 sources,
disjoint harbor-domain vocabulary with genuine multi-hop and high-fan-out
cases). Live Jev backend unless noted:

| system | ev recall (A-dev / A-hold / B) | ev precision | nodes examined |
|---|---|---|---|
| substring lexical | 0.967 / 1.000 / 0.900 | 0.43 / 0.63 / 0.45 | 2.7 / 1.9 / 1.7 |
| BM25 (Tantivy) | 0.967 / 1.000 / 0.900 | 0.24 / 0.23 / 0.22 | 4.0 / 4.1 / 3.5 |
| deterministic graph | 1.000 / 0.875 / 0.950 | 0.34 / 0.48 / 0.49 | 8.7 / 4.1 / 9.9 |
| semantic + graph priors | **1.000 / 0.875 / 0.950** | **0.57 / 0.62 / 0.68** | **5.1 / 2.8 / 3.3** |

Read honestly: on shared-vocabulary queries BM25 reaches equal or better
recall — the graph's edge is precision (≈2.4× BM25) at less exploration.
The topology argument rests on targeted cases, not averages: corpus-B b02
(both lexical baselines 0.0 → graph 1.0, seed carries no evidence) and b06
(prior-ranked truncation keeps the protocol where uniform truncation drops
it). The holdout shares corpus A's generator, so treat it as tuning
protection, not an independent corpus.

Answers: citation validity 1.0 (every cited ID resolves to supplied
evidence — resolvability, not semantic support), expected-evidence coverage
30/30, correct abstention on unanswerable queries. Mean live retrieval
latency ≈ 1.1 s over ~5 small decision calls. These checked-in Jev reports
predate the seed-ranking hardening below; treat them as a recorded baseline,
not a post-change live rerun. Full analysis, verdicts and limitations:
[`docs/EVALUATION.md`](docs/EVALUATION.md).

### Seed-resolution robustness benchmark

A failure analysis of corpus-B case `b07` found that generic wreck terms
created 28 tied/near-tied seed candidates; lexicographic tie-breaking ranked
`meridian-wreck` 26th and `wreck-buoy-protocol` 27th, outside top-3 before
semantic routing began. Seed resolution now removes stop words before
stemming, deduplicates query tokens, and weights overlap by inverse document
frequency so rarer terms carry more signal.

Offline replay over the frozen labelled fixtures (top-3 seeds):

| split | expected-entity recall@3 before | after | labelled evidence reachable within hop budget before | after |
|---|---:|---:|---:|---:|
| corpus A dev | 0.727 | **0.841** | 30/30 | 30/30 |
| corpus A holdout | 0.778 | **0.889** | 7/8 | 7/8 |
| corpus B | 0.500 | **0.900** | 9/10 | **10/10** |

This is a deterministic seed-stage benchmark, not a replacement for the live
retrieval evaluation. The remaining holdout miss (`h03`) has no lexical bridge
at all, which is a useful boundary rather than something to hide with further
threshold tuning.

### Scale benchmark

`cargo bench --bench scale` exercises cold versus cached PageRank, seed
resolution, and the full deterministic retrieval path at 1k, 10k and 50k nodes
plus a separate fan-out-25 graph. Cached PageRank is averaged over 100,000 hot
calls so sub-microsecond timings are not dominated by timer resolution; seed
resolution and deterministic retrieval report p50/p95 over 41 warmed samples.
Cold PageRank remains a single informational build measurement because it is
the deliberately expensive O(iterations × (V + E)) path.

Retrieval uses an `Arc`-backed PageRank cache keyed by configuration;
node/edge mutation invalidates it automatically. CI runs the benchmark so
performance regressions remain visible without making timing thresholds
correctness gates.

## Project structure

```
src/
  graph/       typed nodes/edges/evidence refs, petgraph store, PageRank
  decision/    DecisionClient trait, generic System One HTTP client,
               deterministic fixture + record/replay client
  retrieval/   seed resolution, candidate priors, ranked frontier,
               budgeted traversal, human-readable traces
  answer/      AnswerClient trait, extractive baseline, OpenAI-compatible
               client with Rust-side citation grounding
  construct/   proposal → resolution → validation-gate → policy-gated apply
  evidence/    EvidenceStore trait + checked-in fixture adapter
  eval/        labelled cases, retrieval + answer metrics, BM25 baseline,
               five-way runner with budget curves
fixtures/      corpus A: graph, 14 sources, 30 dev + 8 holdout cases
               (regenerate: python3 tools/generate_fixture.py)
               corpus_b/: 40-node harbor domain, 10 cases incl. multi-hop +
               high-fan-out (regenerate: python3 tools/generate_corpus_b.py)
```

## Status and scope

Implemented: deterministic retrieval, generic decision client, semantic
routing, evidence sufficiency, cited answer generation, and a construction
pipeline scaffold — all behind the boundaries above, with unit/integration tests
and a dependency-free scale benchmark. Deliberately **not** included: embeddings, vector
search, community detection, graph databases, agents, MCP, UIs. See
[`docs/EVALUATION.md`](docs/EVALUATION.md) for what was validated, what
wasn't (e.g. graph-prior efficiency at scale), and the recommended next steps.

## License

MIT — see [LICENSE](LICENSE).
