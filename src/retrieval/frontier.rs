use crate::retrieval::candidates::Candidate;
use ordered_float::OrderedFloat;

/// Deterministic frontier ranking with explicit tie-breaking:
/// semantic desc, graph_prior desc, hops asc, node id asc.
pub fn rank_frontier(cands: &mut [Candidate]) {
    cands.sort_by(|a, b| {
        let sa = OrderedFloat(a.frontier_score.unwrap_or(0.0));
        let sb = OrderedFloat(b.frontier_score.unwrap_or(0.0));
        sb.cmp(&sa)
            .then_with(|| OrderedFloat(b.graph_prior).cmp(&OrderedFloat(a.graph_prior)))
            .then_with(|| a.hops.cmp(&b.hops))
            .then_with(|| a.to.cmp(&b.to))
    });
}
