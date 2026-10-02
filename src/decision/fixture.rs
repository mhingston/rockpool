use super::client::DecisionClient;
use super::types::{Answer, DecisionError, DecisionRequest, DecisionResponse, Question};use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Deterministic fixture client for tests and offline replay.
/// Maps each question key to a fixed probability via a resolver fn.
#[derive(Clone)]
pub struct FixtureDecisionClient {
    resolver: Arc<dyn Fn(&str, &Question, &DecisionRequest) -> f64 + Send + Sync>,
    pub backend_name: String,
}

impl FixtureDecisionClient {
    pub fn new<F>(resolver: F) -> Self
    where
        F: Fn(&str, &Question, &DecisionRequest) -> f64 + Send + Sync + 'static,
    {
        Self {
            resolver: Arc::new(resolver),
            backend_name: "fixture".into(),
        }
    }

    /// Fixed map: question_key -> p_yes. Unknown keys -> 0.0.
    pub fn from_pyes_map(map: BTreeMap<String, f64>) -> Self {
        Self::new(move |key, _q, _req| map.get(key).copied().unwrap_or(0.0))
    }

    /// Offline replay: rebuild deterministic answers from recorded
    /// [`DecisionRecord`]s (live run -> captured fixture -> replay).
    /// Uses the first recorded answer per question key. Never persists secrets:
    /// records contain only state/questions/probabilities.
    pub fn from_records(records: &[super::types::DecisionRecord]) -> Self {
        let mut map = BTreeMap::new();
        for rec in records {
            for (k, a) in &rec.response.answers {
                map.entry(k.clone()).or_insert_with(|| a.relevance());
            }
        }
        Self::from_pyes_map(map)
    }

    /// Heuristic resolver: token overlap between query state and question
    /// instructions. Useful for offline eval without a model endpoint.
    /// Not a product truth — a deterministic stand-in.
    pub fn heuristic() -> Self {
        Self::new(|_key, question, req| {
            let query = req
                .state
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let instructions = match question {
                Question::Noul { instructions } => instructions.clone(),
                Question::Choice { instructions, .. } => instructions.clone(),
                Question::Score { instructions, .. } => instructions.clone(),
            };
            heuristic_overlap(query, &instructions)
        })
    }

    /// Evidence-sufficiency heuristic: p_yes grows with evidence count.
    pub fn sufficiency_heuristic(threshold_items: usize) -> Self {
        Self::new(move |_key, _q, req| {
            let n = req
                .state
                .get("evidence_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            if n == 0 {
                0.05
            } else if n >= threshold_items {
                0.95
            } else {
                0.3 + 0.6 * (n as f64 / threshold_items as f64)
            }
        })
    }
}

fn stem(t: &str) -> String {
    if t.ends_with("ies") && t.len() > 4 {
        return format!("{}y", &t[..t.len() - 3]);
    }
    if (t.ends_with("ses") || t.ends_with("xes") || t.ends_with("zes")) && t.len() > 4 {
        return t[..t.len() - 2].to_string();
    }
    if t.ends_with('s') && t.len() > 3 && !t.ends_with("ss") {
        return t[..t.len() - 1].to_string();
    }
    t.to_string()
}

fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 3)
        .map(stem)
        .filter(|t| !is_stop(t))
        .collect()
}

fn is_stop(t: &str) -> bool {
    matches!(
        t,
        "what" | "which" | "does" | "how" | "why" | "when" | "with" | "from"
            | "that" | "this" | "these" | "those" | "were" | "have"
            | "could" | "should" | "would" | "there" | "their"
            | "about" | "into" | "tell"
    )
}

fn heuristic_overlap(query: &str, instructions: &str) -> f64 {
    // Instructions embed the user query verbatim; strip it so the score
    // reflects query↔candidate overlap, not query self-match.
    let stripped = instructions.replacen(query, "", 1);
    let q = tokenize(query);
    let inst_tokens: Vec<String> = tokenize(&stripped);
    if q.is_empty() {
        return 0.0;
    }
    // Bidirectional substring on stemmed tokens: "work" matches "workforce",
    // "policy" matches "policies". Deterministic stand-in only.
    let hits = q
        .iter()
        .filter(|t| {
            inst_tokens
                .iter()
                .any(|c| c.contains(t.as_str()) || t.contains(c.as_str()))
        })
        .count();
    let frac = hits as f64 / q.len() as f64;
    // Map to [0.05, 0.95] deterministically.
    0.05 + 0.9 * frac
}

#[async_trait]
impl DecisionClient for FixtureDecisionClient {
    async fn decide(
        &self,
        request: DecisionRequest,
    ) -> Result<DecisionResponse, DecisionError> {
        let mut answers = BTreeMap::new();
        for (key, q) in &request.questions {
            match q {
                Question::Noul { .. } | Question::Score { .. } | Question::Choice { .. } => {
                    let p = (self.resolver)(key, q, &request).clamp(0.0, 1.0);
                    let ans = match q {
                        Question::Noul { .. } => Answer::Noul { p_yes: p },
                        Question::Score { levels, .. } => {
                            let idx = ((p * levels.len() as f64).floor() as usize)
                                .min(levels.len().saturating_sub(1));
                            let level = levels.get(idx).cloned().unwrap_or_default();
                            Answer::Score { level, value: p }
                        }
                        Question::Choice { choices, .. } => {
                            // Deterministic: all mass on lexicographically-first
                            // choice scaled by p is wrong; instead put p on best
                            // overlapping choice. Simplify: uniform fallback with
                            // selected = first key when p < 0.5 else last key.
                            let mut keys: Vec<&String> = choices.keys().collect();
                            keys.sort();
                            let selected = if p >= 0.5 {
                                keys.last().cloned().cloned().unwrap_or_default()
                            } else {
                                keys.first().cloned().cloned().unwrap_or_default()
                            };
                            let mut probs = BTreeMap::new();
                            for k in choices.keys() {
                                probs.insert(
                                    k.clone(),
                                    if *k == selected { p } else { (1.0 - p) / (choices.len().max(1) - 1).max(1) as f64 },
                                );
                            }
                            Answer::Choice {
                                probabilities: probs,
                                selected,
                            }
                        }
                    };
                    answers.insert(key.clone(), ans);
                }
            }
        }
        Ok(DecisionResponse {
            answers,
            backend: Some(self.backend_name.clone()),
            latency_ms: Some(0),
        })
    }
}
