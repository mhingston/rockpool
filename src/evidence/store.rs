use super::types::Evidence;
use crate::graph::model::EvidenceRef;
use async_trait::async_trait;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("evidence not found: {0}")]
    NotFound(String),
    #[error("store error: {0}")]
    Store(String),
}

#[async_trait]
pub trait EvidenceStore: Send + Sync {
    async fn fetch(&self, refs: &[EvidenceRef]) -> Result<Vec<Evidence>, EvidenceError>;
}

/// In-memory store backed by checked-in fixture text.
/// Later adapters (PageIndex, files, Confluence, ...) implement the same trait.
pub struct MemoryEvidenceStore {
    /// source_id -> full text
    sources: HashMap<String, (String, String)>, // source_id -> (document_id, text)
}

impl MemoryEvidenceStore {
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
        }
    }

    pub fn insert(&mut self, source_id: String, document_id: String, text: String) {
        self.sources.insert(source_id, (document_id, text));
    }

    pub fn load_dir(dir: &std::path::Path) -> Result<Self, EvidenceError> {
        let mut store = Self::new();
        let entries = std::fs::read_dir(dir).map_err(|e| EvidenceError::Store(e.to_string()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let text =
                std::fs::read_to_string(&path).map_err(|e| EvidenceError::Store(e.to_string()))?;
            let source_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            // First line `# doc: <document_id>` optionally declares the document.
            let mut document_id = source_id.clone();
            for line in text.lines().take(5) {
                if let Some(rest) = line.strip_prefix("# doc:") {
                    document_id = rest.trim().to_string();
                    break;
                }
                if let Some(rest) = line.strip_prefix("<!-- doc:") {
                    document_id = rest.trim_end_matches("-->").trim().to_string();
                    break;
                }
            }
            store.insert(source_id, document_id, text);
        }
        Ok(store)
    }

    pub fn all_source_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.sources.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn get_text(&self, source_id: &str) -> Option<&str> {
        self.sources.get(source_id).map(|(_, t)| t.as_str())
    }

    pub fn document_of(&self, source_id: &str) -> Option<&str> {
        self.sources.get(source_id).map(|(d, _)| d.as_str())
    }

    /// Lexical scan over source texts (Baseline A foundation).
    pub fn lexical_search(&self, query: &str, top_k: usize) -> Vec<(String, f64)> {
        let qtokens = tokenize(query);
        if qtokens.is_empty() {
            return vec![];
        }
        let mut scored: Vec<(String, f64)> = self
            .sources
            .iter()
            .map(|(sid, (_, text))| {
                let lt = text.to_lowercase();
                let hits = qtokens.iter().filter(|t| lt.contains(t.as_str())).count();
                (sid.clone(), hits as f64 / qtokens.len() as f64)
            })
            .filter(|(_, s)| *s > 0.0)
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0)));
        scored.truncate(top_k);
        scored
    }
}

impl Default for MemoryEvidenceStore {
    fn default() -> Self {
        Self::new()
    }
}

fn stem(t: &str) -> String {
    if t.ends_with("ies") && t.len() > 4 {
        return format!("{}y", &t[..t.len() - 3]);
    }
    if t.ends_with('s') && t.len() > 3 && !t.ends_with("ss") {
        return t[..t.len() - 1].to_string();
    }
    t.to_string()
}

fn is_stop(t: &str) -> bool {
    matches!(
        t,
        "what"
            | "which"
            | "does"
            | "how"
            | "why"
            | "when"
            | "with"
            | "from"
            | "that"
            | "this"
            | "these"
            | "those"
            | "were"
            | "have"
            | "could"
            | "should"
            | "would"
            | "there"
            | "their"
            | "about"
            | "into"
            | "tell"
    )
}

fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 3)
        .map(stem)
        .filter(|t| !is_stop(t))
        .collect()
}

#[async_trait]
impl EvidenceStore for MemoryEvidenceStore {
    async fn fetch(&self, refs: &[EvidenceRef]) -> Result<Vec<Evidence>, EvidenceError> {
        let mut out = vec![];
        for r in refs {
            let (doc, text) = self
                .sources
                .get(&r.source_id)
                .ok_or_else(|| EvidenceError::NotFound(r.source_id.clone()))?;
            let snippet = match (r.start, r.end) {
                (Some(s), Some(e)) => {
                    let s = s as usize;
                    let e = (e as usize).min(text.len());
                    if s < e {
                        text[s..e].to_string()
                    } else {
                        text.clone()
                    }
                }
                _ => {
                    if let Some(q) = &r.quote {
                        // Return surrounding context when possible.
                        if let Some(pos) = text.find(q.as_str()) {
                            let s = pos.saturating_sub(200);
                            let e = (pos + q.len() + 200).min(text.len());
                            text[s..e].to_string()
                        } else {
                            text.clone()
                        }
                    } else {
                        text.clone()
                    }
                }
            };
            out.push(Evidence {
                document_id: r.document_id.clone().if_empty(doc.clone()),
                source_id: r.source_id.clone(),
                text: snippet,
                quote: r.quote.clone(),
                path: vec![],
                reason: String::new(),
            });
        }
        Ok(out)
    }
}

trait IfEmpty {
    fn if_empty(self, fallback: String) -> String;
}
impl IfEmpty for String {
    fn if_empty(self, fallback: String) -> String {
        if self.is_empty() {
            fallback
        } else {
            self
        }
    }
}
