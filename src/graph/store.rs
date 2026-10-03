use super::model::{Edge, EdgeKind, EvidenceRef, FixtureEdge, GraphFixture, Node, NodeId};
use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("duplicate node id: {0}")]
    DuplicateNode(String),
    #[error("unknown node in edge: {0} -> {1}")]
    UnknownNode(String, String),
    #[error("fixture load failed: {0}")]
    Load(String),
}

struct PageRankCache {
    damping_bits: u32,
    iterations: usize,
    ranks: Arc<BTreeMap<String, f32>>,
}

pub struct KnowledgeGraph {
    graph: StableDiGraph<Node, Edge>,
    index: HashMap<NodeId, NodeIndex>,
    pagerank_cache: RwLock<Option<PageRankCache>>,
}

impl KnowledgeGraph {
    pub fn empty() -> Self {
        Self {
            graph: StableDiGraph::new(),
            index: HashMap::new(),
            pagerank_cache: RwLock::new(None),
        }
    }

    pub fn from_fixture(fixture: GraphFixture) -> Result<Self, GraphError> {
        let mut kg = Self::empty();
        for node in fixture.nodes {
            kg.add_node(node)?;
        }
        for e in fixture.edges {
            kg.add_fixture_edge(e)?;
        }
        Ok(kg)
    }

    pub fn load_json(json: &str) -> Result<Self, GraphError> {
        let fixture: GraphFixture =
            serde_json::from_str(json).map_err(|e| GraphError::Load(e.to_string()))?;
        Self::from_fixture(fixture)
    }

    fn invalidate_pagerank_cache(&mut self) {
        *self
            .pagerank_cache
            .get_mut()
            .expect("PageRank cache lock poisoned") = None;
    }

    pub(crate) fn cached_pagerank(
        &self,
        damping: f32,
        iterations: usize,
    ) -> Option<Arc<BTreeMap<String, f32>>> {
        let cache = self.pagerank_cache.read().ok()?;
        match cache.as_ref() {
            Some(c) if c.damping_bits == damping.to_bits() && c.iterations == iterations => {
                Some(Arc::clone(&c.ranks))
            }
            _ => None,
        }
    }

    pub(crate) fn store_pagerank(
        &self,
        damping: f32,
        iterations: usize,
        ranks: Arc<BTreeMap<String, f32>>,
    ) {
        if let Ok(mut cache) = self.pagerank_cache.write() {
            *cache = Some(PageRankCache {
                damping_bits: damping.to_bits(),
                iterations,
                ranks,
            });
        }
    }

    pub fn add_node(&mut self, node: Node) -> Result<(), GraphError> {
        if self.index.contains_key(&node.id) {
            return Err(GraphError::DuplicateNode(node.id));
        }
        let idx = self.graph.add_node(node.clone());
        self.index.insert(node.id, idx);
        self.invalidate_pagerank_cache();
        Ok(())
    }

    pub fn add_fixture_edge(&mut self, e: FixtureEdge) -> Result<(), GraphError> {
        let from = *self
            .index
            .get(&e.from)
            .ok_or_else(|| GraphError::UnknownNode(e.from.clone(), e.to.clone()))?;
        let to = *self
            .index
            .get(&e.to)
            .ok_or_else(|| GraphError::UnknownNode(e.from.clone(), e.to.clone()))?;
        self.graph.add_edge(
            from,
            to,
            Edge {
                kind: e.kind,
                evidence: e.evidence,
                confidence: e.confidence,
            },
        );
        self.invalidate_pagerank_cache();
        Ok(())
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn get(&self, id: &str) -> Option<&Node> {
        self.index.get(id).map(|i| &self.graph[*i])
    }

    pub fn node_index(&self, id: &str) -> Option<NodeIndex> {
        self.index.get(id).copied()
    }

    pub fn node_id_of(&self, idx: NodeIndex) -> &str {
        &self.graph[idx].id
    }

    /// Outgoing neighbours with edge payloads (directed traversal),
    /// including edge-level evidence so retrieval can consume it.
    pub fn out_neighbours(
        &self,
        id: &str,
    ) -> Vec<(String, EdgeKind, Option<f32>, Vec<EvidenceRef>)> {
        let Some(idx) = self.node_index(id) else {
            return vec![];
        };
        self.graph
            .edges_directed(idx, Direction::Outgoing)
            .map(|e| {
                let target = self.graph[e.target()].id.clone();
                (
                    target,
                    e.weight().kind,
                    e.weight().confidence,
                    e.weight().evidence.clone(),
                )
            })
            .collect()
    }

    pub fn all_nodes(&self) -> Vec<&Node> {
        self.graph.node_weights().collect()
    }

    pub fn all_node_ids(&self) -> Vec<String> {
        self.graph.node_weights().map(|n| n.id.clone()).collect()
    }
}
