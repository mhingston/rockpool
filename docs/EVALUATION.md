# Evaluation report — Rust graph retrieval vertical slice

Date: 2026-10-02 (refreshed with fallback/sufficiency/provenance fixes,
BM25 baseline, corpus B).
Corpus A: 61 nodes / 102 edges / 14 sources (billing/SaaS domain).
Dev: 30 cases (incl. negatives + ambiguous-alias + dead-end).
Holdout: 8 cases, tuned-against never; authored from the same graph and
generator, so protected from threshold tuning but NOT an independent corpus.
Corpus B: 40 nodes / 64 edges / 6 sources (harbor domain, disjoint
vocabulary), 10 cases incl. genuine multi-hop topology cases and a
high-fan-out node. Frozen: `tools/generate_corpus_{b,}.py` are deterministic;
per-case JSON in `docs/report_live_*.json`.

Budgets: `max_hops=3, max_nodes_examined=50, max_nodes_expanded=12,
max_frontier_size=20, max_decision_calls=10, max_evidence_items=20`.
Thresholds: `accept=0.6, review=0.35, sufficiency=0.7`.

Two semantic backends are reported:
- **heuristic** — deterministic token-overlap `FixtureDecisionClient`, a
  stand-in, not a model. Systematically *understates* semantic routing.
- **live** — TypeSafe Jev `jev-1.13.0` via `https://api.typesafe.ai/v1/systemone`
  (`DECISION_ENDPOINT`, `DECISION_MODEL=jev-1.13.0` config-only,
  Bearer from configured key). Full per-case JSON: `docs/report_live_dev.json`.

## Post-report deterministic hardening benchmark

The checked-in live reports below were recorded on 2026-10-02 before the
seed-ranking hardening in this PR. They remain useful as the live-model
baseline; they have not been relabelled as post-change results.

Failure analysis of corpus-B `b07` found 28 tied/near-tied seed candidates.
Lexicographic tie-breaking placed `meridian-wreck` at rank 26 and
`wreck-buoy-protocol` at rank 27, so top-3 made one labelled passage
unreachable before semantic routing. Seed resolution now:

- removes stop words before stemming;
- deduplicates query tokens;
- uses IDF-weighted metadata overlap so rare terms outrank generic terms.

Frozen-fixture seed replay (top-3):

| split | expected-entity recall@3 before | after | evidence reachable within hop budget before | after |
|---|---:|---:|---:|---:|
| A dev | 0.727 | **0.841** | 30/30 | 30/30 |
| A holdout | 0.778 | **0.889** | 7/8 | 7/8 |
| B | 0.500 | **0.900** | 9/10 | **10/10** |

This metric isolates seed resolution; it is not a substitute for rerunning the
live Jev retrieval evaluation. The remaining holdout miss (`h03`) has no
lexical bridge in node metadata.

Retrieval now also uses an `Arc`-backed PageRank cache keyed by
(damping, iterations), invalidated on node/edge mutation. Run
`cargo bench --bench scale` for the dependency-free size/fan-out benchmark.

## Headline numbers (live model)

### Development (30 cases, live)

| system    | ev recall | ev precision | ent recall | examined | expanded | dec calls | nouls | lat ms |
|-----------|----------:|-------------:|-----------:|---------:|---------:|----------:|------:|-------:|
| lexical   |     0.967 |        0.433 |      0.817 |      2.7 |      0.0 |       0.0 |   0.0 |      5 |
| bm25-top5 |     0.967 |        0.235 |      0.000 |      4.0 |      0.0 |       0.0 |   0.0 |     19 |
| det-graph |     1.000 |        0.341 |      0.867 |      8.7 |      3.7 |       0.0 |   0.0 |     34 |
| semantic  |     1.000 |        0.611 |      0.850 |      4.7 |      2.6 |       4.8 |   6.7 |   1096 |
| hybrid    |     1.000 |        0.572 |      0.850 |      5.1 |      2.6 |       4.9 |   6.9 |   1123 |

BM25 curve (matched budgets): k=1 → 0.88/0.80; k=3 → 0.97/0.32;
k=5 → 0.97/0.24; k=10 → 0.97/0.18; k=20 → 0.97/0.17.
Hybrid: **zero cases with recall < 1** (30/30). BM25 misses q11
("How long do SSO sessions last?" — the source never says "SSO"): retrieved
only via graph alias seed + semantic routing.
Hybrid stop reasons: `evidence_sufficient` 5, `frontier_exhausted` 24,
`no_seeds` 1 (nonsense-query negative — correct). With passages now in the
sufficiency judgement, the live model is stricter than the old count-based
heuristic — most runs exhaust the (small, bounded) frontier.

(Heuristic stand-in numbers — semantic 0.80 recall / 3.6 examined — are
superseded by the live rows above; retained in git history for comparison.)

### Holdout (8 cases, protected, live)

| system    | ev recall | ev precision | ent recall | examined | expanded | dec calls | nouls |
|-----------|----------:|-------------:|-----------:|---------:|---------:|----------:|------:|
| lexical   |     1.000 |        0.629 |      0.750 |      1.9 |      0.0 |       0.0 |   0.0 |
| bm25-top5 |     1.000 |        0.229 |      0.000 |      4.1 |      0.0 |       0.0 |   0.0 |
| det-graph |     0.875 |        0.479 |      0.750 |      4.1 |      1.8 |       0.0 |   0.0 |
| semantic  |     0.875 |        0.688 |      0.750 |      2.4 |      1.1 |       2.2 |   2.8 |
| hybrid    |     0.875 |        0.625 |      0.750 |      2.8 |      1.2 |       2.5 |   3.1 |

Read this split honestly: lexical/BM25 reach 1.000 recall here against 0.875
for the graph systems — on shared-vocabulary queries, simple retrieval wins
recall outright. The graph's advantage on this split is precision (0.62–0.69
vs 0.23) at roughly half the node exploration of unbounded deterministic
traversal. That is promising evidence for better precision/efficiency, not
yet proof the architecture beats simpler retrieval. BM25 curve: k=1 →
0.81/0.75; k=3+ → 1.00/0.33→0.18.

### Corpus B, live (10 cases, harbor domain)

| system    | ev recall | ev precision | examined | dec calls |
|-----------|----------:|-------------:|---------:|----------:|
| lexical   |     0.900 |        0.450 |      1.7 |       0.0 |
| bm25-top5 |     0.900 |        0.223 |      3.5 |       0.0 |
| det-graph |     0.950 |        0.492 |      9.9 |       0.0 |
| semantic  |     0.850 |        0.650 |      2.8 |       2.3 |
| hybrid    |     0.950 |        0.683 |      3.3 |       2.6 |

BM25 curve: k=1 → 0.70/0.60; k=3+ → 0.90/0.32→0.22.
Two cases carry the argument:
- **b02** (lexical 0.0, BM25 0.0, graph 1.0): the seed node carries no
  evidence and the passage shares no vocabulary with the query; the answer
  is reachable only via seed → relationship → evidence. This isolates
  topology from metadata — the H1 test the SSO case could not provide.
- **b06** (semantic-only 0.0, hybrid 1.0): the 25-edge fan-out forces
  truncation; graph-prior-ranked truncation keeps the protocol while uniform
  truncation drops it. First direct H5 evidence.

## Hypotheses

- **H1 — graph structure adds retrieval signal.** *Supported, with the
  confound now isolated.* The corpus-A SSO case (q11) shows curated
  metadata/indexing value (alias seed + evidence on the seed node), not
  topology. The genuine topology evidence is corpus-B **b02**: seed carries no
  evidence, passage shares no query vocabulary (both lexical baselines score
  0.0), answer reachable only via relationships — graph systems score 1.0.
  Hybrid dev recall is 1.00 at 2.4× BM25 precision (0.57 vs 0.24).
- **H2 — bounded semantic routing reduces traversal.** *Supported (live).*
  Dev: 5.1 vs 8.7 nodes examined at identical recall (1.00), ~5 decision calls
  / ~7 Nouls per query, ~1.1 s total. Corpus B: 3.3 vs 9.9 at equal-or-better
  recall (0.95 vs 0.95). Precision roughly doubles BM25 in every split.
- **H3 — deterministic control improves reproducibility.** *Supported with a
  live-model qualification.* Tie-broken ranking + recorded decisions give
  bit-identical replays (`tests/integration.rs::deterministic_replay`; the
  fix required BTreeMap-ordered PageRank summation). Every trace renders
  hop-by-hop with per-edge prior/semantic/verdict plus the complete path.
  Caveat: live answers vary slightly run to run, so reproducibility means
  deterministic policy + replayable decisions, not identical live output.
- **H4 — independent Nouls for multi-label traversal.** *Architecture
  validated, empirical comparison pending.* Sibling branches are scored
  independently and co-accepted
  (`independent_nouls_accept_multiple_branches`); no forced-choice path exists
  in traversal code. This does not yet establish Nouls empirically beat
  forced Choice — that needs Noul-vs-Choice runs over labelled multi-branch
  decisions.
- **H5 — graph priors improve routing efficiency.** *First direct evidence
  (corpus B b06).* On corpus A no frontier exceeds the cap, so hybrid ≡
  semantic there. On corpus B's 25-edge fan-out, prior-ranked truncation keeps
  the relevant protocol (hybrid 1.0) while uniform truncation drops it
  (semantic-only 0.0). Small-n, but mechanistic: truncation ranking is where
  priors bite.

## Stop-rule check

Per the brief, stop if: semantic routing doesn't reduce exploration (it does:
5.1 vs 8.7 dev, 3.3 vs 9.9 corpus B); recall materially worse than simple
retrieval (no: hybrid 1.00 vs BM25 0.967 dev, 0.95 vs 0.90 corpus B; the one
split where BM25 wins recall is the non-independent holdout, 1.00 vs 0.875 at
n=8, with far lower precision); graph priors add nothing (refuted by b06);
decision cost unattractive (~5 calls/query, ~1.1 s — acceptable, batching in
place); lexical matches with lower complexity (BM25 trails on precision 0.24
vs 0.57 and misses both paraphrase cases).

**No stop rule fires.**

## Implementation fixes applied (review round)

1. **Fallback is now a first-class verdict** (`Accept{fallback}`, `Review`,
   `Reject`, `Unavailable`) instead of control state in strings. Hybrid genuinely
   falls back to deterministic thresholds on API failure/budget exhaustion
   (tested); SemanticOnly degrades explicitly (`UNAVAILABLE`, `decision_degraded`
   stop) rather than pretending. Stats track `fallback_accepts`/`unavailable`.
2. **Sufficiency judges passages, not counts.** The Noul receives the query
   plus up to 6 clipped passages (and the same structured `evidence` array in
   state). The live model is now stricter than the old count heuristic —
   `evidence_sufficient` fires 5× on dev instead of rubber-stamping.
3. **Provenance preserves complete paths.** Every visited node records its
   seed → node path; node evidence, edge evidence (now consumed — previously
   ignored by retrieval), and trace events all carry it. CLI traces render
   `path=a → b → c` per candidate.
4. **CI** (`.github/workflows/ci.yml`): `cargo fmt --check`, clippy with
   `-D warnings`, `cargo test`. Zero warnings.
5. **BM25 baseline** (Tantivy, in-RAM, deterministic tie-breaks) with
   recall/precision curves at k ∈ {1,3,5,10,20} in every eval report.
6. **Von/Decider compatibility fixtures** copied from jev-cli PR #13 into
   `decision::system_one` tests (no-model/no-auth request parity is by
   construction; live validation remains Jev-only).

## Recommendation

**Proceed — Slice 5 done, KG construction scaffolded.**

Slice 5 (this turn): `AnswerClient` boundary (query + evidence + provenance
in, cited answer out; graph never visible to the answer model), deterministic
extractive baseline, generic chat-completions client with Rust-side grounding
enforcement (`[[source_id]]` markers resolved against supplied evidence;
unresolvable markers dropped, never cited), and separate answer metrics:

| answer backend | citation validity | expected coverage | abstain correct |
|---|---|---|---|
| extractive + heuristic retrieval (dev) | 1.000 | 0.833 (= retrieval recall) | 1.000 |
| extractive + live Jev retrieval (dev) | 1.000 | **1.000** (30/30) | 1.000 |

Answer quality is retrieval-bound by design. "Citation validity" here means
resolvability — every cited ID resolves to supplied evidence (Rust drops
invented IDs). It is NOT semantic citation correctness: a cited passage might
still fail to support the claim. That needs claim↔evidence entailment labels
or a bounded support evaluation, not yet built.

KG construction (this turn, scaffold): `Proposer` trait + deterministic
mention proposer, `validate_relation` Noul gate ("Does the passage support
this relationship?"), and `apply_proposals` staging resolution → schema gate
→ bounded validation → policy-gated mutation (`accept_threshold`,
`min_proposer_confidence`, `require_evidence`, `allow_new_entities`,
`max_validations`). Mechanics verified by tests and the `construct` demo
(4 mentions proposed for `document-14-section-4-2`, hub targets rejected by
policy). No construction-quality claims: proposal quality and live validation
accuracy are the next evaluation, kept separate from retrieval quality.

The brief's continue-criteria are met with a live backend: high evidence
recall (1.00 dev / 0.875 holdout) + meaningfully reduced exploration
(~40% fewer nodes) + bounded decision cost (~6 calls, ~1.3 s) +
reproducible traces with offline replay. Concretely, next:

1. ~~Slice 5~~ done (extractive validated; generative client awaiting a live
   LLM endpoint to evaluate answer accuracy beyond citation validity).
2. ~~KG construction scaffold~~ done (mechanics); next is its own evaluation:
   proposal quality + live validation accuracy on held-out passages, scored
   against hand-labelled relations — independently of retrieval quality.
3. ~~H5 fixture~~ first evidence in hand (b06); replicate at larger fan-outs
   and run the Noul-vs-Choice comparison H4 still owes.
4. Next corpus work: a genuinely independent corpus (new documents, new
   labels, authored separately from the system) before any headline claim
   that the architecture beats simpler retrieval.
5. Explicitly do **not** add embeddings, vector search, communities, graph DB,
   agents, or UI before steps 1–4.

## Repro

```bash
cargo test
cargo run -- eval
cargo run -- eval --cases fixtures/cases_holdout.json
cargo run -- query "What policies affect renewal pricing?"
cargo run -- query "..." --record /tmp/decisions.json
# Live-model runs (key never printed; see README):
set -a; source .env.local; set +a
export DECISION_ENDPOINT=https://api.typesafe.ai/v1/systemone
export DECISION_MODEL=jev-1.13.0 DECISION_API_KEY="$TYPESAFE_API_KEY"
cargo run -- eval --live
cargo run -- eval --live --cases fixtures/cases_holdout.json
cargo run -- answer-eval --live
cargo run -- construct document-14-section-4-2
```
