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
            model: std::env::var("DECISION_MODEL")
                .ok()
                .filter(|s| !s.is_empty()),
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
    let envelope = body
        .get("answers")
        .or_else(|| body.get("results"))
        .unwrap_or(body);
    let obj = envelope
        .as_object()
        .ok_or_else(|| DecisionError::Malformed("response answers is not an object".into()))?;
    // A `nouls` wrapper drills one level deeper; otherwise borrow directly.
    let owned;
    let map: &serde_json::Map<String, serde_json::Value> =
        match obj.get("nouls").and_then(|v| v.as_object()) {
            Some(nouls) => {
                owned = nouls.clone();
                &owned
            }
            None => obj,
        };
    let mut out = BTreeMap::new();
    for (k, q) in questions {
        let raw = map.get(k).ok_or_else(|| {
            DecisionError::Malformed(format!("missing answer for question '{k}'"))
        })?;
        out.insert(k.clone(), normalize_one(q, raw)?);
    }
    Ok(out)
}

/// Strict unit-interval check. Out-of-range or non-finite probabilities
/// are malformed responses, not values to be silently clamped: the
/// boundary rejects them so provider drift surfaces as an error.
fn check_unit(value: f64, what: &str) -> Result<f64, DecisionError> {
    if !value.is_finite() {
        return Err(DecisionError::Malformed(format!(
            "{what} is not finite: {value}"
        )));
    }
    if !(0.0..=1.0).contains(&value) {
        return Err(DecisionError::Malformed(format!(
            "{what} outside [0,1]: {value}"
        )));
    }
    Ok(value)
}

fn normalize_one(q: &Question, raw: &serde_json::Value) -> Result<Answer, DecisionError> {
    match q {
        Question::Noul { .. } => {
            let p = extract_p_yes(raw)?;
            Ok(Answer::Noul { p_yes: p })
        }
        Question::Score { levels, .. } => {
            if levels.is_empty() {
                return Err(DecisionError::Malformed(
                    "score question declares no levels".into(),
                ));
            }
            if let Some(s) = raw.as_str() {
                if !levels.iter().any(|l| l == s) {
                    return Err(DecisionError::Malformed(format!(
                        "unknown score level '{s}'"
                    )));
                }
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
                    if !levels.iter().any(|l| l == level) {
                        return Err(DecisionError::Malformed(format!(
                            "unknown score level '{level}'"
                        )));
                    }
                    let value = match obj.get("value").and_then(|v| v.as_f64()) {
                        Some(v) => check_unit(v, "score value")?,
                        None => {
                            let idx = levels.iter().position(|l| l == level).unwrap_or(0);
                            if levels.len() > 1 {
                                idx as f64 / (levels.len() - 1) as f64
                            } else {
                                0.0
                            }
                        }
                    };
                    return Ok(Answer::Score {
                        level: level.to_string(),
                        value,
                    });
                }
                if let Some(p) = obj
                    .get("p_yes")
                    .or_else(|| obj.get("pYes"))
                    .or_else(|| obj.get("probability"))
                    .and_then(|v| v.as_f64())
                {
                    let p = check_unit(p, "score probability")?;
                    let idx = ((p * levels.len() as f64).floor() as usize)
                        .min(levels.len().saturating_sub(1));
                    return Ok(Answer::Score {
                        level: levels.get(idx).cloned().unwrap_or_default(),
                        value: p,
                    });
                }
            }
            if let Some(f) = raw.as_f64() {
                let f = check_unit(f, "score value")?;
                let idx = ((f * levels.len() as f64).floor() as usize)
                    .min(levels.len().saturating_sub(1));
                return Ok(Answer::Score {
                    level: levels.get(idx).cloned().unwrap_or_default(),
                    value: f,
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
                return Err(DecisionError::Malformed(format!("unknown choice '{s}'")));
            }
            if let Some(obj) = raw.as_object() {
                if let Some(probs_val) = obj.get("probabilities").or_else(|| obj.get("probs")) {
                    let probs_obj = probs_val.as_object().ok_or_else(|| {
                        DecisionError::Malformed("choice probabilities not an object".into())
                    })?;
                    // Complete coverage: keys must match the declared choices
                    // exactly — no missing options, no invented ones.
                    let declared: std::collections::BTreeSet<&String> = choices.keys().collect();
                    let returned: std::collections::BTreeSet<&String> = probs_obj.keys().collect();
                    if returned != declared {
                        return Err(DecisionError::Malformed(format!(
                            "choice probability keys {returned:?} do not match declared choices {declared:?}"
                        )));
                    }
                    let mut probs = BTreeMap::new();
                    for (k, v) in probs_obj {
                        let p = v.as_f64().ok_or_else(|| {
                            DecisionError::Malformed(format!("non-numeric prob for '{k}'"))
                        })?;
                        probs.insert(k.clone(), check_unit(p, "choice probability")?);
                    }
                    // Well-formed distribution: probabilities must sum to 1.
                    let total: f64 = probs.values().sum();
                    if (total - 1.0).abs() > 1e-3 {
                        return Err(DecisionError::Malformed(format!(
                            "choice probabilities sum to {total}, not 1"
                        )));
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
                    if !choices.contains_key(&selected) {
                        return Err(DecisionError::Malformed(format!(
                            "selected choice '{selected}' is not declared"
                        )));
                    }
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
        return check_unit(f, "noul probability");
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
                return check_unit(v, "noul probability");
            }
        }
        // { yes: 0.8, no: 0.2 }: both sides must be well-formed; they must
        // also agree with each other.
        if let (Some(y), Some(n)) = (
            obj.get("yes").and_then(|v| v.as_f64()),
            obj.get("no").and_then(|v| v.as_f64()),
        ) {
            let y = check_unit(y, "noul yes probability")?;
            let n = check_unit(n, "noul no probability")?;
            if (y + n - 1.0).abs() > 1e-3 {
                return Err(DecisionError::Malformed(format!(
                    "noul yes/no probabilities sum to {}, not 1",
                    y + n
                )));
            }
            return Ok(y);
        }
    }
    Err(DecisionError::Malformed(format!(
        "cannot normalize noul answer: {raw}"
    )))
}

#[async_trait]
impl DecisionClient for SystemOneHttpClient {
    async fn decide(&self, request: DecisionRequest) -> Result<DecisionResponse, DecisionError> {
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
        let resp = rb
            .send()
            .await
            .map_err(|e| DecisionError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| DecisionError::Transport(e.to_string()))?;
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

    /// Von-style response shape (jev-cli PR #13 compatibility fixture):
    /// `{model, answers: {q: {type: "noul", noul: p}}, usage}`.
    #[test]
    fn normalize_von_style_noul() {
        let mut q = BTreeMap::new();
        q.insert(
            "urgent".into(),
            Question::Noul {
                instructions: "x".into(),
            },
        );
        let body = serde_json::json!({
            "model": "von-latest",
            "answers": { "urgent": { "type": "noul", "noul": 0.8 } },
            "usage": { "input_tokens": 8, "output_tokens": 1 },
        });
        let out = normalize_answers(&q, &body).unwrap();
        assert_eq!(out["urgent"], Answer::Noul { p_yes: 0.8 });
    }

    /// Decider-style response shape (jev-cli PR #13 compatibility fixture):
    /// `{model, answers: {q: {type: "choice", choice, confidence,
    /// probabilities}}, usage}` — no injected model, no auth in request.
    #[test]
    fn normalize_decider_style_choice() {
        let mut choices = BTreeMap::new();
        choices.insert("billing".into(), "Charges".into());
        choices.insert("technical".into(), "Bugs".into());
        let mut q = BTreeMap::new();
        q.insert(
            "route".into(),
            Question::Choice {
                instructions: "x".into(),
                choices,
            },
        );
        let body = serde_json::json!({
            "model": "decider",
            "answers": {
                "route": {
                    "type": "choice",
                    "choice": "billing",
                    "confidence": 0.7,
                    "probabilities": { "billing": 0.85, "technical": 0.15 },
                },
            },
            "usage": { "input_tokens": 10, "output_tokens": 0 },
        });
        let out = normalize_answers(&q, &body).unwrap();
        match &out["route"] {
            Answer::Choice {
                selected,
                probabilities,
            } => {
                assert_eq!(selected, "billing");
                assert_eq!(probabilities["billing"], 0.85);
            }
            other => panic!("expected choice, got {other:?}"),
        }
    }

    fn noul_q(key: &str) -> (String, Question) {
        (
            key.into(),
            Question::Noul {
                instructions: "x".into(),
            },
        )
    }

    fn choice_q() -> (String, Question) {
        let mut choices = BTreeMap::new();
        choices.insert("billing".into(), "Charges".into());
        choices.insert("technical".into(), "Bugs".into());
        (
            "route".into(),
            Question::Choice {
                instructions: "x".into(),
                choices,
            },
        )
    }

    fn score_q() -> (String, Question) {
        (
            "sev".into(),
            Question::Score {
                instructions: "x".into(),
                levels: vec!["low".into(), "high".into()],
            },
        )
    }

    fn questions(pairs: Vec<(String, Question)>) -> BTreeMap<String, Question> {
        pairs.into_iter().collect()
    }

    #[test]
    fn rejects_out_of_range_probabilities() {
        // Noul float, Noul object key, Score float, Score object value.
        for (q, body) in [
            (
                questions(vec![noul_q("a")]),
                serde_json::json!({ "answers": { "a": 1.5 } }),
            ),
            (
                questions(vec![noul_q("a")]),
                serde_json::json!({ "answers": { "a": { "noul": -0.2 } } }),
            ),
            (
                questions(vec![score_q()]),
                serde_json::json!({ "answers": { "sev": 2.0 } }),
            ),
            (
                questions(vec![score_q()]),
                serde_json::json!({ "answers": { "sev": { "level": "low", "value": -1.0 } } }),
            ),
        ] {
            assert!(normalize_answers(&q, &body).is_err(), "body: {body}");
        }
    }

    #[test]
    fn rejects_unknown_score_levels() {
        for body in [
            serde_json::json!({ "answers": { "sev": "critical" } }),
            serde_json::json!({ "answers": { "sev": { "level": "critical", "value": 0.5 } } }),
        ] {
            assert!(
                normalize_answers(&questions(vec![score_q()]), &body).is_err(),
                "body: {body}"
            );
        }
    }

    #[test]
    fn rejects_incomplete_or_incoherent_choice_distributions() {
        // Missing option, invented option, unnormalized sum, undeclared pick.
        for body in [
            serde_json::json!({ "answers": { "route": {
                "probabilities": { "billing": 1.0 }, "selected": "billing" } } }),
            serde_json::json!({ "answers": { "route": {
                "probabilities": { "billing": 0.5, "technical": 0.3, "other": 0.2 },
                "selected": "billing" } } }),
            serde_json::json!({ "answers": { "route": {
                "probabilities": { "billing": 0.5, "technical": 0.3 },
                "selected": "billing" } } }),
            serde_json::json!({ "answers": { "route": {
                "probabilities": { "billing": 0.85, "technical": 0.15 },
                "selected": "support" } } }),
            serde_json::json!({ "answers": { "route": "support" } }),
        ] {
            assert!(
                normalize_answers(&questions(vec![choice_q()]), &body).is_err(),
                "body: {body}"
            );
        }
    }

    #[test]
    fn rejects_incoherent_yes_no_split() {
        let body = serde_json::json!({ "answers": { "a": { "yes": 0.8, "no": 0.8 } } });
        assert!(normalize_answers(&questions(vec![noul_q("a")]), &body).is_err());
    }
}
