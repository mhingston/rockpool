use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub hop: u32,
    pub from: String,
    pub to: String,
    pub edge: String,
    pub pagerank: f32,
    pub graph_prior: f32,
    pub semantic: Option<f64>,
    pub decision: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trace {
    pub query: String,
    pub seeds: Vec<(String, String)>,
    pub events: Vec<TraceEvent>,
    pub evidence: Vec<String>,
    pub stop_reason: String,
    pub stats: TraceStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceStats {
    pub nodes_examined: usize,
    pub nodes_expanded: usize,
    pub edges_examined: usize,
    pub decision_calls: usize,
    pub noul_questions: usize,
    pub evidence_items: usize,
    pub max_depth: u32,
}

impl Trace {
    pub fn render_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("query:\n  {}\n\n", self.query));
        s.push_str("seeds:\n");
        for (id, reason) in &self.seeds {
            s.push_str(&format!("  {id} [{reason}]\n"));
        }
        s.push('\n');
        let mut last_hop: Option<u32> = None;
        // Group events by hop of expansion for readability.
        let mut by_hop: std::collections::BTreeMap<u32, Vec<&TraceEvent>> =
            std::collections::BTreeMap::new();
        for e in &self.events {
            by_hop.entry(e.hop).or_default().push(e);
        }
        for (hop, evs) in by_hop {
            if Some(hop) != last_hop {
                s.push_str(&format!("hop {hop}:\n"));
                last_hop = Some(hop);
            }
            // Show current frontier parents once.
            let mut parents: Vec<String> = evs.iter().map(|e| e.from.clone()).collect();
            parents.sort();
            parents.dedup();
            for p in &parents {
                s.push_str(&format!("  {p}\n"));
            }
            for e in evs {
                s.push_str(&format!(
                    "\n  -> {}\n     edge={} pagerank={:.3} graph_prior={:.2} semantic={} decision={}\n",
                    e.to,
                    e.edge,
                    e.pagerank,
                    e.graph_prior,
                    e.semantic
                        .map(|v| format!("{v:.2}"))
                        .unwrap_or_else(|| "-".into()),
                    e.decision,
                ));
            }
            s.push('\n');
        }
        s.push_str("evidence:\n");
        for ev in &self.evidence {
            s.push_str(&format!("  {ev}\n"));
        }
        s.push_str(&format!(
            "\nstop_reason:\n  {}\n",
            self.stop_reason
        ));
        s.push_str(&format!(
            "\nstats: examined={} expanded={} edges={} decisions={} nouls={} evidence={} depth={}\n",
            self.stats.nodes_examined,
            self.stats.nodes_expanded,
            self.stats.edges_examined,
            self.stats.decision_calls,
            self.stats.noul_questions,
            self.stats.evidence_items,
            self.stats.max_depth,
        ));
        s
    }
}
