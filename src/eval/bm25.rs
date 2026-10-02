use crate::evidence::store::MemoryEvidenceStore;
use serde::{Deserialize, Serialize};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, STORED, TEXT};
use tantivy::{doc, Index};

/// A serious lexical baseline: real BM25 over source passages via Tantivy,
/// replacing the token-substring scan. Built in RAM from the frozen corpus;
/// deterministic for a fixed index (doc order + score tie-break by id).
pub struct Bm25Index {
    index: Index,
    id_field: tantivy::schema::Field,
    text_field: tantivy::schema::Field,
    doc_field: tantivy::schema::Field,
}

impl Bm25Index {
    pub fn build(store: &MemoryEvidenceStore) -> anyhow::Result<Self> {
        let mut schema_builder = Schema::builder();
        let id_field = schema_builder.add_text_field("source_id", STORED);
        let doc_field = schema_builder.add_text_field("document_id", STORED);
        let text_field = schema_builder.add_text_field("text", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        let mut writer = index.writer(15_000_000)?;
        let mut ids = store.all_source_ids();
        ids.sort();
        for sid in ids {
            let text = store.get_text(&sid).unwrap_or("");
            let doc_id = store.document_of(&sid).unwrap_or(&sid);
            writer.add_document(doc!(
                id_field => sid.clone(),
                doc_field => doc_id.to_string(),
                text_field => text,
            ))?;
        }
        writer.commit()?;
        Ok(Self {
            index,
            id_field,
            text_field,
            doc_field,
        })
    }

    /// Top-k evidence keys (`document#source`) for a query.
    pub fn search(&self, query: &str, top_k: usize) -> anyhow::Result<Vec<String>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        let parser = QueryParser::for_index(&self.index, vec![self.text_field]);
        // An empty/whitespace query parses to nothing — return no hits.
        let parsed = match parser.parse_query(query) {
            Ok(q) => q,
            Err(_) => return Ok(vec![]),
        };
        let hits = searcher.search(&parsed, &TopDocs::with_limit(top_k.max(1)).order_by_score())?;
        let mut scored: Vec<(f32, String, String)> = vec![];
        for (score, addr) in hits {
            let doc: tantivy::TantivyDocument = searcher.doc(addr)?;
            let sid = doc
                .get_first(self.id_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let did = doc
                .get_first(self.doc_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            scored.push((score, sid, did));
        }
        // Deterministic tie-break: score desc, source id asc.
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then_with(|| a.1.cmp(&b.1)));
        Ok(scored
            .into_iter()
            .map(|(_, sid, did)| format!("{did}#{sid}"))
            .collect())
    }
}

/// Recall/precision at matched retrieval budgets k for one expected set.
pub fn pr_at_k(expected: &[String], retrieved: &[String]) -> (f64, f64) {
    let r = crate::eval::metrics::evidence_recall(expected, retrieved);
    let p = crate::eval::metrics::evidence_precision(expected, retrieved);
    (r, p)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvePoint {
    pub k: usize,
    pub recall: f64,
    pub precision: f64,
}
