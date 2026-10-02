use super::client::AnswerClient;
use super::types::{AnswerError, AnswerEvidence, AnswerRequest, AnswerResponse, Citation};
use async_trait::async_trait;
use std::collections::HashSet;

/// Deterministic extractive baseline: composes an answer strictly from
/// supplied evidence passages. Grounded by construction — every claim cites
/// a passage, and every citation resolves to supplied evidence.
/// Abstains when there is nothing to cite.
#[derive(Debug, Clone, Default)]
pub struct ExtractiveAnswerClient {
    pub backend_name: String,
}

impl ExtractiveAnswerClient {
    pub fn new() -> Self {
        Self {
            backend_name: "extractive".into(),
        }
    }
}

fn tokenize(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 3)
        .map(|t| t.to_string())
        .collect()
}

fn overlap(query: &HashSet<String>, text: &str) -> usize {
    let t = tokenize(text);
    query.intersection(&t).count()
}

#[async_trait]
impl AnswerClient for ExtractiveAnswerClient {
    async fn answer(&self, request: AnswerRequest) -> Result<AnswerResponse, AnswerError> {
        if request.evidence.is_empty() {
            return Ok(AnswerResponse {
                text: "I don't have enough evidence to answer this question.".into(),
                citations: vec![],
                abstained: true,
                backend: Some(self.backend_name.clone()),
                latency_ms: Some(0),
                dropped_citations: vec![],
            });
        }
        let q = tokenize(&request.query);
        let mut ranked: Vec<(&AnswerEvidence, usize)> = request
            .evidence
            .iter()
            .map(|e| {
                let quote = e.quote.as_deref().unwrap_or(&e.text);
                (e, overlap(&q, quote))
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.source_id.cmp(&b.0.source_id))
        });
        let k = request.max_citations.max(1).min(ranked.len());
        let mut text = String::from("Based on the retrieved evidence:\n");
        let mut citations = vec![];
        for (ev, _) in ranked.into_iter().take(k) {
            let snippet = ev.quote.as_deref().unwrap_or(ev.text.as_str());
            let short: String = snippet.chars().take(280).collect();
            text.push_str(&format!(
                "\n- {} [{}#{}]\n",
                short.trim(),
                ev.document_id,
                ev.source_id
            ));
            citations.push(Citation {
                document_id: ev.document_id.clone(),
                source_id: ev.source_id.clone(),
                quote: ev.quote.clone(),
            });
        }
        Ok(AnswerResponse {
            text,
            citations,
            abstained: false,
            backend: Some(self.backend_name.clone()),
            latency_ms: Some(0),
            dropped_citations: vec![],
        })
    }
}
