use super::client::AnswerClient;
use super::types::{AnswerError, AnswerRequest, AnswerResponse, Citation};
use async_trait::async_trait;
use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

/// Generic OpenAI-compatible chat client for answer synthesis.
/// The model receives ONLY the query plus numbered evidence passages —
/// never the graph. Rust enforces grounding: `[[source_id]]` markers in the
/// generated text are resolved against supplied evidence; markers that do
/// not resolve are dropped and reported, never passed through as citations.
///
/// Config (env-inspired):
/// - ANSWER_ENDPOINT (full chat-completions URL, required)
/// - ANSWER_MODEL (required by the API; sent always)
/// - ANSWER_API_KEY (optional; Bearer only when set)
#[derive(Debug, Clone)]
pub struct AnswerEndpointConfig {
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl AnswerEndpointConfig {
    pub fn from_env() -> Result<Self, AnswerError> {
        let endpoint = std::env::var("ANSWER_ENDPOINT")
            .map_err(|_| AnswerError::Transport("ANSWER_ENDPOINT not set".into()))?;
        let model = std::env::var("ANSWER_MODEL")
            .map_err(|_| AnswerError::Transport("ANSWER_MODEL not set".into()))?;
        Ok(Self {
            endpoint,
            model,
            api_key: std::env::var("ANSWER_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatAnswerClient {
    pub config: AnswerEndpointConfig,
    http: reqwest::Client,
}

impl OpenAiCompatAnswerClient {
    pub fn new(config: AnswerEndpointConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    fn user_prompt(req: &AnswerRequest) -> String {
        let mut p = format!(
            "Answer the question using ONLY the evidence passages below. \
             Cite every factual claim with a [[source_id]] marker placed right \
             after the claim. If the passages do not contain the answer, reply \
             with exactly: INSUFFICIENT EVIDENCE.\n\nQuestion: {}\n",
            req.query
        );
        for ev in req.evidence.iter().take(req.max_citations.max(1)) {
            p.push_str(&format!(
                "\n--- passage [[{}]] (document {}) ---\n{}\n",
                ev.source_id, ev.document_id, ev.text
            ));
        }
        p
    }
}

/// Resolve `[[source_id]]` markers against supplied evidence.
/// Returns (cleaned_text, citations, dropped).
fn resolve_citations(
    text: &str,
    evidence: &[super::types::AnswerEvidence],
    max: usize,
) -> (String, Vec<Citation>, Vec<String>) {
    let by_source: BTreeMap<&str, &super::types::AnswerEvidence> = evidence
        .iter()
        .map(|e| (e.source_id.as_str(), e))
        .collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut citations = vec![];
    let mut dropped = vec![];
    let mut cleaned = text.to_string();
    // Extract markers of the form [[...]].
    let mut markers: Vec<String> = vec![];
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find("]]") {
            markers.push(after[..end].to_string());
            rest = &after[end + 2..];
        } else {
            break;
        }
    }
    for m in markers {
        let marker = format!("[[{m}]]");
        if let Some(ev) = by_source.get(m.as_str()) {
            cleaned = cleaned.replacen(&marker, &format!("[{}#{}]", ev.document_id, ev.source_id), 1);
            if seen.insert(m.clone()) && citations.len() < max {
                citations.push(Citation {
                    document_id: ev.document_id.clone(),
                    source_id: ev.source_id.clone(),
                    quote: ev.quote.clone(),
                });
            }
        } else {
            cleaned = cleaned.replacen(&marker, "[uncited]", 1);
            if !dropped.contains(&m) {
                dropped.push(m);
            }
        }
    }
    (cleaned, citations, dropped)
}

#[async_trait]
impl AnswerClient for OpenAiCompatAnswerClient {
    async fn answer(&self, request: AnswerRequest) -> Result<AnswerResponse, AnswerError> {
        if request.evidence.is_empty() {
            return Err(AnswerError::NoEvidence);
        }
        let model = request.model.clone().unwrap_or_else(|| self.config.model.clone());
        let body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": "You answer strictly from the provided evidence and cite every claim."},
                {"role": "user", "content": Self::user_prompt(&request)},
            ],
        });
        let t0 = Instant::now();
        let mut rb = self.http.post(&self.config.endpoint).json(&body);
        if let Some(key) = &self.config.api_key {
            rb = rb.bearer_auth(key);
        }
        let resp = rb.send().await.map_err(|e| AnswerError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| AnswerError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(AnswerError::Transport(format!("HTTP {status}: {text}")));
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| AnswerError::Malformed(e.to_string()))?;
        let content = parsed
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| AnswerError::Malformed("missing choices[0].message.content".into()))?;
        if content.trim() == "INSUFFICIENT EVIDENCE" {
            return Ok(AnswerResponse {
                text: "I don't have enough evidence to answer this question.".into(),
                citations: vec![],
                abstained: true,
                backend: Some(model),
                latency_ms: Some(t0.elapsed().as_millis() as u64),
                dropped_citations: vec![],
            });
        }
        let (cleaned, citations, dropped) =
            resolve_citations(content, &request.evidence, request.max_citations.max(1));
        Ok(AnswerResponse {
            text: cleaned,
            citations,
            abstained: false,
            backend: Some(model),
            latency_ms: Some(t0.elapsed().as_millis() as u64),
            dropped_citations: dropped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str) -> super::super::types::AnswerEvidence {
        super::super::types::AnswerEvidence {
            document_id: "doc-1".into(),
            source_id: id.into(),
            text: "passage".into(),
            quote: None,
        }
    }

    #[test]
    fn citations_resolve_and_hallucinations_drop() {
        let evidence = vec![ev("sec-1"), ev("sec-2")];
        let (cleaned, cites, dropped) = resolve_citations(
            "Claim one [[sec-1]] and invented [[sec-9]] plus repeat [[sec-1]].",
            &evidence,
            5,
        );
        assert_eq!(cites.len(), 1);
        assert_eq!(cites[0].source_id, "sec-1");
        assert_eq!(dropped, vec!["sec-9".to_string()]);
        assert!(!cleaned.contains("[[sec-9]]"));
        assert!(cleaned.contains("[uncited]"));
    }

    #[test]
    fn citation_cap_respected() {
        let evidence = vec![ev("a"), ev("b"), ev("c")];
        let (_, cites, _) = resolve_citations("[[a]] [[b]] [[c]]", &evidence, 2);
        assert_eq!(cites.len(), 2);
    }
}
