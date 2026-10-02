use super::client::DecisionClient;
use super::types::{Answer, DecisionError, DecisionRequest, DecisionResponse, Question};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::time::Instant;

/// Generic System One HTTP client.
///
/// Posts to a configured endpoint implementing the System One wire contract
/// (conceptually POST /v1/systemone with `{ state, questions }`).
/// Compatible with Jev, Von, Decider, or any conforming backend —
/// traversal code must never depend on which one answers.
///
/// Config (env-inspired):
/// - DECISION_ENDPOINT (full URL, required)
/// - DECISION_MODEL (optional; only sent when set)
/// - DECISION_API_KEY (optional; Bearer only when set)
#[derive(Debug, Clone)]
pub struct DecisionEndpointConfig {
    pub endpoint: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
}

impl DecisionEndpointConfig {
    pub fn from_env() -> Result<Self, DecisionError> {
        let endpoint = std::env::var("DECISION_ENDPOINT")
            .map_err(|_| DecisionError::Transport("DECISION_ENDPOINT not set".into()))?;
        Ok(Self {
            endpoint,
            model: std::env::var("DECISION_MODEL").ok().filter(|s| !s.is_empty()),
            api_key: std::env::var("DECISION_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct SystemOneHttpClient {
    pub config: DecisionEndpointConfig,
    http: reqwest::Client,
}

impl SystemOneHttpClient {
    pub fn new(config: DecisionEndpointConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    fn wire_questions(req: &DecisionRequest) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (k, q) in &req.questions {
            let v = match q {
                Question::Noul { instructions } => serde_json::json!({
                    "type": "noul",
                    "instructions": instructions,
                }),
                Question::Choice {
                    instructions,
                    choices,
                } => serde_json::json!({
                    "type": "choice",
                    "instructions": instructions,
                    "choices": choices,
                }),
                Question::Score {
                    instructions,
                    levels,
                } => serde_json::json!({
                    "type": "score",
                    "instructions": instructions,
                    "levels": levels,
                }),
            };
            map.insert(k.clone(), v);
        }
        serde_json::Value::Object(map)
    }
}

/// Normalize provider answers at the boundary.
/// Malformed answers become errors — never propagate loosely-typed JSON.
fn normalize_answers(
    questions: &BTreeMap<String, Question>,
    body: &serde_json::Value,
) -> Result<BTreeMap<String, Answer>, DecisionError> {
    // Accept several envelope shapes:
    // { answers: {...} } | { results: {...} } | { nouls: {...} } | flat map
    let envelope = if let Some(a) = body.get("answers") {
        a
    } else if let Some(r) = body.get("results") {
        r
    } else if body.get("nouls").is_some() {
        body
    } else {
        body
    };
    let obj = envelope.as_object().ok_or_else(|| {
        DecisionError::Malformed("response answers is not an object".into())
    })?;
    // If envelope had `nouls` wrapper, drill in.
    let inner: &serde_json::Map<String, serde_json::Value>;
    let owned;
    if let Some(nouls) = obj.get("nouls").and_then(|v| v.as_object()) {
        owned = nouls.clone();
        // SAFETY: owned lives for this scope; use reference via owned.
        // Workaround: collect below using owned.
        let mut out = BTreeMap::new();
        for (k, q) in questions {
            let raw = owned.get(k).ok_or_else(|| {
                DecisionError::Malformed(format!("missing answer for question '{k}'"))
            })?;
            out.insert(k.clone(), normalize_one(q, raw)?);
        }
        return Ok(out);
    } else {
        // borrow directly
        let mut out = BTreeMap::new();
        for (k, q) in questions {
            let raw = obj.get(k).ok_or_else(|| {
                DecisionError::Malformed(format!("missing answer for question '{k}'"))
            })?;
            out.insert(k.clone(), normalize_one(q, raw)?);
        }
        let _ = inner; // silence
        return Ok(out);
    }
}

fn normalize_one(q: &Question, raw: &serde_json::Value) -> Result<Answer, DecisionError> {
    match q {
        Question::Noul { .. } => {
            let p = extract_p_yes(raw)?;
            Ok(Answer::Noul { p_yes: p })
        }
        Question::Score { levels, .. } => {
            if let Some(s) = raw.as_str() {
                let idx = levels.iter().position(|l| l == s).unwrap_or(0);
                let value = if levels.len() > 1 {
                    idx as f64 / (levels.len() - 1) as f64
                } else {
                    0.0
                };
                return Ok(Answer::Score {
                    level: s.to_string(),
                    value,
                });
            }
            if let Some(obj) = raw.as_object() {
                if let Some(level) = obj.get("level").and_then(|v| v.as_str()) {
                    let value = obj
                        .get("value")
                        .and_then(|v| v.as_f64())
                        .unwrap_or_else(|| {
                            let idx = levels.iter().position(|l| l == level).unwrap_or(0);
                            if levels.len() > 1 {
                                idx as f64 / (levels.len() - 1) as f64
                            } else {
                                0.0
                            }
                        });
                    return Ok(Answer::Score {
                        level: level.to_string(),
                        value: value.clamp(0.0, 1.0),
                    });
                }
                if let Some(p) = obj
                    .get("p_yes")
                    .or_else(|| obj.get("pYes"))
                    .or_else(|| obj.get("probability"))
                    .and_then(|v| v.as_f64())
                {
                    let idx = ((p * levels.len() as f64).floor() as usize)
                        .min(levels.len().saturating_sub(1));
                    return Ok(Answer::Score {
                        level: levels.get(idx).cloned().unwrap_or_default(),
                        value: p.clamp(0.0, 1.0),
                    });
                }
            }
            if let Some(f) = raw.as_f64() {
                let idx =
                    ((f * levels.len() as f64).floor() as usize).min(levels.len().saturating_sub(1));
                return Ok(Answer::Score {
                    level: levels.get(idx).cloned().unwrap_or_default(),
                    value: f.clamp(0.0, 1.0),
                });
            }
            Err(DecisionError::Malformed(format!(
                "cannot normalize score answer: {raw}"
            )))
        }
        Question::Choice { choices, .. } => {
            if let Some(s) = raw.as_str() {
                if choices.contains_key(s) {
                    let mut probs = BTreeMap::new();
                    for k in choices.keys() {
                        probs.insert(k.clone(), if k == s { 1.0 } else { 0.0 });
                    }
                    return Ok(Answer::Choice {
                        probabilities: probs,
                        selected: s.to_string(),
                    });
                }
                return Err(DecisionError::Malformed(format!(
                    "unknown choice '{s}'"
                )));
            }
            if let Some(obj) = raw.as_object() {
                if let Some(probs_val) = obj.get("probabilities").or_else(|| obj.get("probs")) {
                    let probs_obj = probs_val.as_object().ok_or_else(|| {
                        DecisionError::Malformed("choice probabilities not an object".into())
                    })?;
                    let mut probs = BTreeMap::new();
                    for (k, v) in probs_obj {
                        let p = v.as_f64().ok_or_else(|| {
                            DecisionError::Malformed(format!("non-numeric prob for '{k}'"))
                        })?;
                        probs.insert(k.clone(), p.clamp(0.0, 1.0));
                    }
                    let selected = obj
                        .get("selected")
                        .or_else(|| obj.get("choice"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            probs
                                .iter()
                                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                                .map(|(k, _)| k.clone())
                        })
                        .ok_or_else(|| {
                            DecisionError::Malformed("choice missing selected".into())
                        })?;
                    return Ok(Answer::Choice {
                        probabilities: probs,
                        selected,
                    });
                }
            }
            Err(DecisionError::Malformed(format!(
                "cannot normalize choice answer: {raw}"
            )))
        }
    }
}

fn extract_p_yes(raw: &serde_json::Value) -> Result<f64, DecisionError> {
    if let Some(f) = raw.as_f64() {
        return Ok(f.clamp(0.0, 1.0));
    }
    if let Some(b) = raw.as_bool() {
        return Ok(if b { 1.0 } else { 0.0 });
    }
    if let Some(s) = raw.as_str() {
        match s.to_lowercase().as_str() {
            "yes" | "true" | "y" => return Ok(1.0),
            "no" | "false" | "n" => return Ok(0.0),
            _ => {}
        }
    }
    if let Some(obj) = raw.as_object() {
        for key in ["p_yes", "pYes", "probability", "p", "score", "noul"] {
            if let Some(v) = obj.get(key).and_then(|v| v.as_f64()) {
                return Ok(v.clamp(0.0, 1.0));
            }
        }
        // { yes: 0.8, no: 0.2 }
        if let (Some(y), Some(_n)) = (
            obj.get("yes").and_then(|v| v.as_f64()),
            obj.get("no").and_then(|v| v.as_f64()),
        ) {
            return Ok(y.clamp(0.0, 1.0));
        }
    }
    Err(DecisionError::Malformed(format!(
        "cannot normalize noul answer: {raw}"
    )))
}

#[async_trait]
impl DecisionClient for SystemOneHttpClient {
    async fn decide(
        &self,
        request: DecisionRequest,
    ) -> Result<DecisionResponse, DecisionError> {
        // Model resolution: explicit request model wins, else configured default.
        // Generic endpoints receive NO injected model.
        let model = request.model.clone().or_else(|| self.config.model.clone());
        let mut body = serde_json::Map::new();
        body.insert("state".to_string(), request.state.clone());
        body.insert("questions".to_string(), Self::wire_questions(&request));
        if let Some(m) = model {
            body.insert("model".to_string(), serde_json::Value::String(m));
        }
        let t0 = Instant::now();
        let mut rb = self.http.post(&self.config.endpoint).json(&body);
        // Only send Bearer when explicitly configured. Never leak across endpoints:
        // this client holds exactly one endpoint + one optional key.
        if let Some(key) = &self.config.api_key {
            rb = rb.bearer_auth(key);
        }
        let resp = rb.send().await.map_err(|e| DecisionError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| DecisionError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(DecisionError::Transport(format!("HTTP {status}: {text}")));
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| DecisionError::Malformed(e.to_string()))?;
        let answers = normalize_answers(&request.questions, &parsed)?;
        Ok(DecisionResponse {
            answers,
            backend: parsed
                .get("backend")
                .or_else(|| parsed.get("model"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            latency_ms: Some(t0.elapsed().as_millis() as u64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_flat_noul_float() {
        let mut q = BTreeMap::new();
        q.insert(
            "a".into(),
            Question::Noul {
                instructions: "x".into(),
            },
        );
        let body = serde_json::json!({ "answers": { "a": 0.9 } });
        let out = normalize_answers(&q, &body).unwrap();
        assert_eq!(out["a"], Answer::Noul { p_yes: 0.9 });
    }

    #[test]
    fn normalize_envelope_variants() {
        let mut q = BTreeMap::new();
        q.insert(
            "a".into(),
            Question::Noul {
                instructions: "x".into(),
            },
        );
        for body in [
            serde_json::json!({ "results": { "a": {"p_yes": 0.7} } }),
            serde_json::json!({ "a": true }),
            serde_json::json!({ "answers": { "a": {"probability": 0.2} } }),
        ] {
            let out = normalize_answers(&q, &body).unwrap();
            assert!(out["a"].relevance() >= 0.0);
        }
    }

    #[test]
    fn malformed_missing_key_errors() {
        let mut q = BTreeMap::new();
        q.insert(
            "a".into(),
            Question::Noul {
                instructions: "x".into(),
            },
        );
        let body = serde_json::json!({ "answers": {} });
        assert!(normalize_answers(&q, &body).is_err());
    }
}
