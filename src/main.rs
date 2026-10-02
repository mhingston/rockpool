use clap::{Parser, Subcommand};
use rockpool::answer::{AnswerEndpointConfig, AnswerRequest, ExtractiveAnswerClient, OpenAiCompatAnswerClient};
use rockpool::answer::client::AnswerClient;
use rockpool::construct::{apply_proposals, ConstructionPolicy, MentionProposer, Proposer};
use rockpool::decision::fixture::FixtureDecisionClient;
use rockpool::decision::system_one::{DecisionEndpointConfig, SystemOneHttpClient};
use rockpool::decision::client::DecisionClient;
use rockpool::decision::types::DecisionRequest;
use rockpool::eval::answers::{run_answer_eval, to_answer_evidence, AnswerEvalConfig};
use rockpool::eval::cases::EvalCase;
use rockpool::eval::runner::{run_eval, EvalConfig};
use rockpool::evidence::store::MemoryEvidenceStore;
use rockpool::graph::store::KnowledgeGraph;
use rockpool::retrieval::candidates::CandidateFilter;
use rockpool::retrieval::policy::{RetrievalMode, Thresholds, TraversalBudgets, Weights};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rockpool", about = "Rust graph retrieval vertical slice")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    #[arg(long, default_value = "fixtures/graph.json")]
    graph: PathBuf,
    #[arg(long, default_value = "fixtures/sources")]
    sources: PathBuf,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a single query with traversal trace.
    Query {
        query: String,
        #[arg(long, default_value = "hybrid")]
        mode: String,
        #[arg(long)]
        json: bool,
        /// Optional live System One endpoint (else heuristic fixture).
        #[arg(long)]
        live: bool,
        /// Write recorded decision exchanges (JSON, no secrets) for offline replay.
        #[arg(long)]
        record: Option<PathBuf>,
    },
    /// Run evaluation across dev (and optionally holdout) cases.
    Eval {
        #[arg(long, default_value = "fixtures/cases_dev.json")]
        cases: PathBuf,
        #[arg(long)]
        holdout: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        /// Use the live System One endpoint (DECISION_* env) instead of the
        /// deterministic fixture stand-in for semantic modes.
        #[arg(long)]
        live: bool,
    },
    /// Replay/validate decision normalization against a live endpoint.
    Decide {
        #[arg(long)]
        state: String,
        #[arg(long)]
        instructions: String,
    },
    /// Answer a query: retrieve evidence, then synthesize a cited answer.
    Answer {
        query: String,
        #[arg(long, default_value = "hybrid")]
        mode: String,
        /// Use live System One decisions for retrieval.
        #[arg(long)]
        live: bool,
        /// Use the generative answer endpoint (ANSWER_* env) instead of the
        /// deterministic extractive baseline.
        #[arg(long)]
        answer_live: bool,
    },
    /// Evaluate answer quality: citation precision, expected-evidence
    /// coverage, abstention behaviour. Retrieval metrics reported alongside.
    AnswerEval {
        #[arg(long, default_value = "fixtures/cases_dev.json")]
        cases: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        live: bool,
        #[arg(long)]
        answer_live: bool,
    },
    /// Construction demo: propose + validate + apply graph mutations for one
    /// source passage, printing the accept/reject report. Mutations apply to
    /// the in-memory graph only.
    Construct {
        source: String,
        /// Use live System One validation instead of the fixture map.
        #[arg(long)]
        live: bool,
    },
}

fn parse_mode(s: &str) -> RetrievalMode {
    match s.to_lowercase().as_str() {
        "lexical" => RetrievalMode::Deterministic, // handled separately in eval
        "deterministic" | "det" => RetrievalMode::Deterministic,
        "semantic" | "sem" => RetrievalMode::SemanticOnly,
        _ => RetrievalMode::Hybrid,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    let graph_json = std::fs::read_to_string(&cli.graph)?;
    let kg = KnowledgeGraph::load_json(&graph_json)?;
    let store = MemoryEvidenceStore::load_dir(&cli.sources)?;

    match cli.cmd {
        Cmd::Query { query, mode, json, live, record } => {
            let mode = parse_mode(&mode);
            let budgets = TraversalBudgets::default();
            let thresholds = Thresholds::default();
            let weights = Weights::default();
            let filter = CandidateFilter::default();
            if live {
                let cfg = DecisionEndpointConfig::from_env()?;
                let client = SystemOneHttpClient::new(cfg);
                let suff = client.clone();
                let out = rockpool::retrieval::traversal::retrieve(
                    &kg,
                    &store,
                    Some(&client),
                    Some(&suff),
                    &query,
                    mode,
                    &budgets,
                    &thresholds,
                    &weights,
                    &filter,
                )
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&out.trace)?);
                } else {
                    println!("{}", out.trace.render_text());
                    println!("--- evidence passages ---");
                    for e in &out.evidence {
                        println!("\n## {}#{}\n{}\n", e.document_id, e.source_id, e.text.chars().take(800).collect::<String>());
                    }
                }
                if let Some(path) = &record {
                    std::fs::write(path, serde_json::to_string_pretty(&out.decisions)?)?;
                    eprintln!("recorded {} decision exchange(s) -> {}", out.decisions.len(), path.display());
                }
            } else {
                let dec = FixtureDecisionClient::heuristic();
                let suff = FixtureDecisionClient::sufficiency_heuristic(2);
                let out = rockpool::retrieval::traversal::retrieve(
                    &kg,
                    &store,
                    Some(&dec),
                    Some(&suff),
                    &query,
                    mode,
                    &budgets,
                    &thresholds,
                    &weights,
                    &filter,
                )
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&out.trace)?);
                } else {
                    println!("{}", out.trace.render_text());
                    println!("--- evidence passages ---");
                    for e in &out.evidence {
                        println!("\n## {}#{}\n{}\n", e.document_id, e.source_id, e.text.chars().take(800).collect::<String>());
                    }
                }
                if let Some(path) = record {
                    std::fs::write(&path, serde_json::to_string_pretty(&out.decisions)?)?;
                    eprintln!("recorded {} decision exchange(s) -> {}", out.decisions.len(), path.display());
                }
            }
        }
        Cmd::Eval { cases, holdout, json, live } => {
            let data = std::fs::read_to_string(&cases)?;
            let dev: Vec<EvalCase> = serde_json::from_str(&data)?;
            let cfg = EvalConfig::default();
            let label = if live { "DEV(live)" } else { "DEV" };
            if live {
                let endpoint = DecisionEndpointConfig::from_env()?;
                let client = SystemOneHttpClient::new(endpoint);
                let report = run_eval(&kg, &store, &dev, &cfg, &client, &client).await;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    print_report(label, &report);
                }
            } else {
                let decision = FixtureDecisionClient::heuristic();
                let sufficiency = FixtureDecisionClient::sufficiency_heuristic(2);
                let report =
                    run_eval(&kg, &store, &dev, &cfg, &decision, &sufficiency).await;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    print_report(label, &report);
                }
            }
            if let Some(h) = holdout {
                if h.exists() {
                    println!("\n[holdout present — not inspected for tuning; reporting separately]");
                }
            }
        }
        Cmd::Decide { state, instructions } => {
            let cfg = DecisionEndpointConfig::from_env()?;
            let client = SystemOneHttpClient::new(cfg);
            let mut questions = std::collections::BTreeMap::new();
            questions.insert(
                "q".into(),
                rockpool::decision::types::Question::Noul { instructions },
            );
            let req = DecisionRequest {
                state: serde_json::from_str(&state)?,
                questions,
                model: std::env::var("DECISION_MODEL").ok().filter(|s| !s.is_empty()),
            };
            let resp = client.decide(req).await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Cmd::Answer { query, mode, live, answer_live } => {
            let mode = parse_mode(&mode);
            let budgets = TraversalBudgets::default();
            let thresholds = Thresholds::default();
            let weights = Weights::default();
            let filter = CandidateFilter::default();
            let answer_req_for = |evidence: &[rockpool::evidence::types::Evidence]| AnswerRequest {
                query: query.clone(),
                evidence: to_answer_evidence(evidence, 1500),
                max_citations: 5,
                model: None,
            };
            if live {
                let c = SystemOneHttpClient::new(DecisionEndpointConfig::from_env()?);
                let out = rockpool::retrieval::traversal::retrieve(
                    &kg, &store, Some(&c), Some(&c), &query, mode,
                    &budgets, &thresholds, &weights, &filter,
                )
                .await?;
                answer_and_print(answer_req_for(&out.evidence), answer_live).await?;
                println!("\n[retrieval: {} evidence, stop={}]", out.evidence.len(), out.trace.stop_reason);
            } else {
                let d = FixtureDecisionClient::heuristic();
                let s = FixtureDecisionClient::sufficiency_heuristic(2);
                let out = rockpool::retrieval::traversal::retrieve(
                    &kg, &store, Some(&d), Some(&s), &query, mode,
                    &budgets, &thresholds, &weights, &filter,
                )
                .await?;
                answer_and_print(answer_req_for(&out.evidence), answer_live).await?;
                println!("\n[retrieval: {} evidence, stop={}]", out.evidence.len(), out.trace.stop_reason);
            }
        }
        Cmd::AnswerEval { cases, json, live, answer_live } => {
            let data = std::fs::read_to_string(&cases)?;
            let dev: Vec<EvalCase> = serde_json::from_str(&data)?;
            let cfg = AnswerEvalConfig::default();
            // Note: sufficiency uses the same backend as routing here.
            match (answer_live, live) {
                (true, true) => {
                    let a = OpenAiCompatAnswerClient::new(AnswerEndpointConfig::from_env()?);
                    let c = SystemOneHttpClient::new(DecisionEndpointConfig::from_env()?);
                    emit_answer_report(&run_answer_eval(&kg, &store, &dev, &cfg, &a, &c, &c).await, json);
                }
                (true, false) => {
                    let a = OpenAiCompatAnswerClient::new(AnswerEndpointConfig::from_env()?);
                    let d = FixtureDecisionClient::heuristic();
                    emit_answer_report(&run_answer_eval(&kg, &store, &dev, &cfg, &a, &d, &d).await, json);
                }
                (false, true) => {
                    let a = ExtractiveAnswerClient::new();
                    let c = SystemOneHttpClient::new(DecisionEndpointConfig::from_env()?);
                    emit_answer_report(&run_answer_eval(&kg, &store, &dev, &cfg, &a, &c, &c).await, json);
                }
                (false, false) => {
                    let a = ExtractiveAnswerClient::new();
                    let d = FixtureDecisionClient::heuristic();
                    emit_answer_report(&run_answer_eval(&kg, &store, &dev, &cfg, &a, &d, &d).await, json);
                }
            }
        }
        Cmd::Construct { source, live } => {
            let text = store
                .get_text(&source)
                .ok_or_else(|| anyhow::anyhow!("unknown source: {source}"))?
                .to_string();
            let proposer = MentionProposer::default();
            let proposals = proposer.propose(&source, &text, &kg).await;
            println!(
                "proposed: {} entities, {} relations",
                proposals.entities.len(),
                proposals.relations.len()
            );
            let policy = ConstructionPolicy::default();
            let mut kg = kg;
            if live {
                let c = SystemOneHttpClient::new(DecisionEndpointConfig::from_env()?);
                let report =
                    apply_proposals(&mut kg, &proposals, &text, &c, &policy).await;
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                let d = fixture_validation_client(&proposals);
                let report =
                    apply_proposals(&mut kg, &proposals, &text, &d, &policy).await;
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            println!(
                "\n(note: mutations applied to the in-memory graph only; fixture unchanged)"
            );
        }
    }
    Ok(())
}

async fn answer_and_print(req: AnswerRequest, answer_live: bool) -> anyhow::Result<()> {
    if answer_live {
        let acfg = AnswerEndpointConfig::from_env()?;
        let aclient = OpenAiCompatAnswerClient::new(acfg);
        print_answer(&aclient.answer(req).await?);
    } else {
        let aclient = ExtractiveAnswerClient::new();
        print_answer(&aclient.answer(req).await?);
    }
    Ok(())
}

/// Fixture validation client for the construct demo: accepts relations whose
/// target is not a known hub/decoy, rejects the rest. Mechanics demo only —
/// live validation (`--live`) is the real gate.
fn fixture_validation_client(
    proposals: &rockpool::construct::types::ProposalSet,
) -> FixtureDecisionClient {
    let keep: Vec<String> = proposals
        .relations
        .iter()
        .filter(|r| {
            !r.to.contains("handbook")
                && !r.to.contains("generic-pricing")
                && !r.to.contains("billing-operations")
        })
        .map(|r| format!("{} --{:?}--> {}", r.from, r.kind, r.to))
        .collect();
    FixtureDecisionClient::new(move |_key, _q, req| {
        let to = req
            .state
            .get("to")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if to.contains("handbook")
            || to.contains("generic-pricing")
            || to.contains("billing-operations")
        {
            0.1
        } else {
            let _ = &keep;
            0.85
        }
    })
}

fn print_answer(resp: &rockpool::answer::types::AnswerResponse) {
    if resp.abstained {
        println!("(abstained)\n{}", resp.text);
    } else {
        println!("{}", resp.text);
    }
    println!("\ncitations:");
    for c in &resp.citations {
        println!("  {}#{}", c.document_id, c.source_id);
    }
    if !resp.dropped_citations.is_empty() {
        println!("dropped (ungrounded): {:?}", resp.dropped_citations);
    }
    println!("backend: {:?}", resp.backend);
}

fn emit_answer_report(report: &rockpool::eval::answers::AnswerEvalReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        let s = &report.summary;
        println!("=== answer eval ===");
        println!("n={} citation_precision={:.3} expected_coverage={:.3}", s.n, s.mean_citation_precision, s.mean_expected_coverage);
        println!("abstention_rate={:.3} abstain_correct={:.3} retrieval_recall={:.3}", s.abstention_rate, s.abstain_correct_rate, s.mean_retrieval_recall);
        println!("\nper-case misses (coverage<1, expected non-empty):");
        for c in &report.cases {
            if c.expected_coverage < 1.0 {
                println!("  {} coverage={:.2} cited={:?}", c.case_id, c.expected_coverage, c.cited);
            }
        }
    }
}

fn print_report(split: &str, r: &rockpool::eval::runner::EvalReport) {
    println!("=== eval [{split}] ===");
    println!(
        "{:<14} {:>8} {:>8} {:>8} {:>10} {:>10} {:>8} {:>8}",
        "system", "ev_rec", "ev_prec", "ent_rec", "examined", "expanded", "calls", "nouls"
    );
    for (name, s) in [
        ("lexical", &r.lexical),
        ("det-graph", &r.deterministic),
        ("semantic", &r.semantic),
        ("hybrid", &r.hybrid),
    ] {
        println!(
            "{:<14} {:>8.3} {:>8.3} {:>8.3} {:>10.1} {:>10.1} {:>8.1} {:>8.1}",
            name,
            s.mean_evidence_recall,
            s.mean_evidence_precision,
            s.mean_entity_recall,
            s.mean_nodes_examined,
            s.mean_nodes_expanded,
            s.mean_decision_calls,
            s.mean_nouls,
        );
    }
    println!("\nstop reasons (hybrid): {:?}", r.hybrid.stop_reasons);
}
