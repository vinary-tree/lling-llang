//! Edge-list adjacency and suffix-priority helpers for path extraction algorithms.

use std::cmp::Ordering;

use crate::backend::LatticeBackend;
use crate::lattice::{EdgeId, Lattice, NodeId};
use crate::semiring::Semiring;

pub(super) fn node_index(node_id: NodeId, node_count: usize) -> Option<usize> {
    let idx = node_id.0 as usize;
    (idx < node_count).then_some(idx)
}

pub(super) fn compare_weights<W: Semiring>(left: &W, right: &W) -> Ordering {
    match (left.natural_less(right), right.natural_less(left)) {
        (Some(true), _) => Ordering::Less,
        (_, Some(true)) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

pub(super) fn edge_adjacency<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
) -> Option<Vec<Vec<EdgeId>>> {
    let node_count = lattice.num_nodes();
    let mut seen_nodes = vec![false; node_count];
    for node in lattice.nodes() {
        let idx = node_index(node.id, node_count)?;
        if seen_nodes[idx] {
            return None;
        }
        seen_nodes[idx] = true;
    }

    let mut out_degree = vec![0usize; node_count];
    for (edge_index, edge) in lattice.edges().iter().enumerate() {
        if edge.id.0 as usize != edge_index {
            return None;
        }
        let source_idx = node_index(edge.source, node_count)?;
        node_index(edge.target, node_count)?;
        out_degree[source_idx] = out_degree[source_idx].checked_add(1)?;
    }

    let mut adjacency: Vec<Vec<EdgeId>> = out_degree.into_iter().map(Vec::with_capacity).collect();
    for edge in lattice.edges() {
        let source_idx = node_index(edge.source, node_count)?;
        adjacency[source_idx].push(edge.id);
    }

    Some(adjacency)
}

pub(super) fn edge_topological_order<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
    adjacency: &[Vec<EdgeId>],
) -> Option<Vec<NodeId>> {
    if adjacency.len() != lattice.num_nodes() {
        return None;
    }

    let mut in_degree = vec![0usize; adjacency.len()];
    for edge_ids in adjacency {
        for &edge_id in edge_ids {
            let edge = lattice.edge(edge_id)?;
            let target_idx = node_index(edge.target, adjacency.len())?;
            in_degree[target_idx] = in_degree[target_idx].checked_add(1)?;
        }
    }

    let mut ready = Vec::with_capacity(adjacency.len());
    let mut order = Vec::with_capacity(adjacency.len());
    for node_idx in 0..adjacency.len() {
        if in_degree[node_idx] == 0 {
            ready.push(NodeId::new(node_idx as u32));
        }
    }

    while let Some(node_id) = ready.pop() {
        let node_idx = node_index(node_id, adjacency.len())?;
        order.push(node_id);

        for &edge_id in &adjacency[node_idx] {
            let edge = lattice.edge(edge_id)?;
            let target_idx = node_index(edge.target, adjacency.len())?;
            in_degree[target_idx] = in_degree[target_idx].checked_sub(1)?;
            if in_degree[target_idx] == 0 {
                ready.push(edge.target);
            }
        }
    }

    (order.len() == adjacency.len()).then_some(order)
}

pub(super) fn best_suffix_distances<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
    adjacency: &[Vec<EdgeId>],
) -> Option<Vec<Option<W>>> {
    let order = edge_topological_order(lattice, adjacency)?;
    let end_idx = node_index(lattice.end(), adjacency.len())?;
    let mut suffix_best = vec![None; adjacency.len()];
    suffix_best[end_idx] = Some(W::one());

    for &node_id in order.iter().rev() {
        let node_idx = node_index(node_id, adjacency.len())?;
        for &edge_id in &adjacency[node_idx] {
            let edge = lattice.edge(edge_id)?;
            let target_idx = node_index(edge.target, adjacency.len())?;
            let target_suffix = match suffix_best[target_idx] {
                Some(weight) => weight,
                None => continue,
            };
            let candidate = edge.weight.times(&target_suffix);

            match suffix_best[node_idx] {
                Some(existing) if compare_weights(&candidate, &existing).is_ge() => {}
                _ => suffix_best[node_idx] = Some(candidate),
            }
        }
    }

    Some(suffix_best)
}

pub(super) fn path_priority<W: Semiring>(
    suffix_best: Option<&[Option<W>]>,
    adjacency_len: usize,
    node: NodeId,
    prefix_weight: W,
) -> Option<W> {
    let Some(suffix_best) = suffix_best else {
        return Some(prefix_weight);
    };

    let node_idx = node_index(node, adjacency_len)?;
    suffix_best[node_idx].map(|suffix| prefix_weight.times(&suffix))
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::backend::{HashMapBackend, LatticeBackend};
    use crate::lattice::{Edge, EdgeId, EdgeMetadata, Lattice, Node, NodeId};
    use crate::semiring::TropicalWeight;

    pub(crate) fn lattice_with_stale_outgoing() -> Lattice<TropicalWeight, HashMapBackend> {
        let mut backend = HashMapBackend::new();
        let label = backend.intern("a");
        let edge_id = EdgeId::new(0);
        let nodes = vec![
            Node::with_position(NodeId::new(0), 0),
            Node::with_position(NodeId::new(1), 1),
        ];

        let edges = vec![Edge::new(
            edge_id,
            NodeId::new(0),
            NodeId::new(1),
            label,
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        )];

        Lattice::new(nodes, edges, NodeId::new(0), NodeId::new(1), backend)
    }

    pub(crate) fn lattice_with_stale_multihop_outgoing() -> Lattice<TropicalWeight, HashMapBackend>
    {
        let mut backend = HashMapBackend::new();
        let first_label = backend.intern("a");
        let second_label = backend.intern("b");
        let nodes = vec![
            Node::with_position(NodeId::new(0), 0),
            Node::with_position(NodeId::new(1), 1),
            Node::with_position(NodeId::new(2), 2),
        ];

        let edges = vec![
            Edge::new(
                EdgeId::new(0),
                NodeId::new(0),
                NodeId::new(1),
                first_label,
                TropicalWeight::new(1.0),
                EdgeMetadata::default(),
            ),
            Edge::new(
                EdgeId::new(1),
                NodeId::new(1),
                NodeId::new(2),
                second_label,
                TropicalWeight::new(2.0),
                EdgeMetadata::default(),
            ),
        ];

        Lattice::new(nodes, edges, NodeId::new(0), NodeId::new(2), backend)
    }

    pub(crate) fn lattice_with_malformed_target() -> Lattice<TropicalWeight, HashMapBackend> {
        let mut backend = HashMapBackend::new();
        let label = backend.intern("bad");
        let edge_id = EdgeId::new(0);
        let mut nodes = vec![
            Node::with_position(NodeId::new(0), 0),
            Node::with_position(NodeId::new(1), 1),
        ];
        nodes[0].outgoing.push(edge_id);

        let edges = vec![Edge::new(
            edge_id,
            NodeId::new(0),
            NodeId::new(99),
            label,
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        )];

        Lattice::new(nodes, edges, NodeId::new(0), NodeId::new(1), backend)
    }

    pub(crate) fn lattice_with_invalid_start() -> Lattice<TropicalWeight, HashMapBackend> {
        let backend = HashMapBackend::new();
        let nodes = vec![Node::with_position(NodeId::new(0), 0)];

        Lattice::new(nodes, Vec::new(), NodeId::new(99), NodeId::new(0), backend)
    }
}
