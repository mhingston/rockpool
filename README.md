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
candidate edge with its graph prior, semantic score and ACCEPT/REVIEW/REJECT
verdict — so retrieval decisions are inspectable, not opaque:

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
| `eval [--live] [--json] [--cases f]` | Compare lexical / deterministic-graph / semantic / hybrid retrieval |
| `answer-eval [--live] [--json]` | Citation precision, expected-evidence coverage, abstention behaviour |
| `construct <source-id> [--live]` | Propose → validate → apply graph mutations (in-memory demo) |
| `decide --state s --instructions i` | Single bounded judgement against the live endpoint |

Live modes read `DECISION_ENDPOINT`, optional `DECISION_MODEL`, optional
`DECISION_API_KEY` (Bearer sent only when set). Verified against TypeSafe Jev
(`jev-1.13.0`, `https://api.typesafe.ai/v1/systemone`); the answer stage can
additionally use any OpenAI-compatible chat endpoint via `ANSWER_*`.

## How well does it work?

30 development + 8 held-out cases over a 61-node / 102-edge / 14-source
fixture (live Jev backend):

| system | evidence recall (dev/holdout) | evidence precision | nodes examined |
|---|---|---|---|
| lexical | 0.967 / 1.000 | 0.25 / 0.23 | 3.9 / 4.2 |
| deterministic graph | 1.000 / 0.875 | 0.34 / 0.48 | 8.7 / 4.1 |
| semantic + graph priors | **1.000 / 0.875** | **0.56 / 0.62** | **5.2 / 2.5** |

Answers: citation precision 1.0, expected-evidence coverage 30/30, correct
abstention on unanswerable queries. Mean live retrieval latency ≈ 1.3 s over
~6 small decision calls. Full analysis, per-hypothesis verdicts and
limitations: [`docs/EVALUATION.md`](docs/EVALUATION.md).

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
  eval/        labelled cases, retrieval + answer metrics, four-way runner
fixtures/      graph, 14 sources, 30 dev + 8 holdout cases (regenerate:
               python3 tools/generate_fixture.py)
```

## Status and scope

Implemented: deterministic retrieval, generic decision client, semantic
routing, evidence sufficiency, cited answer generation, and a construction
pipeline scaffold — all behind the boundaries above, all tested
(`cargo test`: 12 tests). Deliberately **not** included: embeddings, vector
search, community detection, graph databases, agents, MCP, UIs. See
[`docs/EVALUATION.md`](docs/EVALUATION.md) for what was validated, what
wasn't (e.g. graph-prior efficiency at scale), and the recommended next steps.

## License

MIT — see [LICENSE](LICENSE).
