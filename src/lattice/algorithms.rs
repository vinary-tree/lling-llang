//! Graph algorithms for lattices.

use rustc_hash::FxHashSet;

use super::lattice::Lattice;
use super::types::{Edge, Node, NodeId};
use crate::backend::LatticeBackend;
use crate::semiring::Semiring;

#[cfg(test)]
use super::builder::LatticeBuilder;
#[cfg(test)]
use super::types::{EdgeId, EdgeMetadata};

fn node_index(node_id: NodeId, node_count: usize) -> Option<usize> {
    let idx = node_id.0 as usize;
    (idx < node_count).then_some(idx)
}

fn validate_node_ids(nodes: &[Node]) -> bool {
    let mut seen = vec![false; nodes.len()];
    for node in nodes {
        let Some(idx) = node_index(node.id, nodes.len()) else {
            return false;
        };
        if seen[idx] {
            return false;
        }
        seen[idx] = true;
    }
    true
}

fn adjacency_from_edges<W: Semiring>(
    nodes: &[Node],
    edges: &[Edge<W>],
) -> Option<(Vec<Vec<NodeId>>, Vec<usize>)> {
    if !validate_node_ids(nodes) {
        return None;
    }

    let mut adjacency: Vec<Vec<NodeId>> = vec![Vec::new(); nodes.len()];
    let mut in_degree: Vec<usize> = vec![0; nodes.len()];

    for edge in edges {
        let source_idx = node_index(edge.source, nodes.len())?;
        let target_idx = node_index(edge.target, nodes.len())?;
        adjacency[source_idx].push(edge.target);
        in_degree[target_idx] = in_degree[target_idx].checked_add(1)?;
    }

    Some((adjacency, in_degree))
}

fn reachable_adjacency_from_edges<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
) -> Vec<Vec<NodeId>> {
    if !validate_node_ids(lattice.nodes()) {
        return Vec::new();
    }

    let mut adjacency: Vec<Vec<NodeId>> = vec![Vec::new(); lattice.num_nodes()];
    for edge in lattice.edges() {
        let Some(source_idx) = node_index(edge.source, lattice.num_nodes()) else {
            continue;
        };
        if node_index(edge.target, lattice.num_nodes()).is_some() {
            adjacency[source_idx].push(edge.target);
        }
    }

    adjacency
}

/// Compute topological order using Kahn's algorithm.
///
/// Returns `None` if the graph contains a cycle.
///
/// Time complexity: O(V + E)
/// Space complexity: O(V + E) for the edge adjacency table
pub fn topological_sort<W: Semiring>(nodes: &[Node], edges: &[Edge<W>]) -> Option<Vec<NodeId>> {
    if nodes.is_empty() {
        return Some(Vec::new());
    }

    let (adjacency, mut in_degree) = adjacency_from_edges(nodes, edges)?;

    let mut queue: Vec<NodeId> = Vec::with_capacity(nodes.len());
    let mut result: Vec<NodeId> = Vec::with_capacity(nodes.len());

    // Start with nodes that have no incoming edges
    for node in nodes {
        let idx = node_index(node.id, nodes.len())?;
        if in_degree[idx] == 0 {
            queue.push(node.id);
        }
    }

    while let Some(node_id) = queue.pop() {
        result.push(node_id);

        // Decrease in-degree for all neighbors: O(out_degree) per node
        let node_idx = node_index(node_id, nodes.len())?;
        for &target in &adjacency[node_idx] {
            let target_idx = node_index(target, nodes.len())?;
            in_degree[target_idx] = in_degree[target_idx].checked_sub(1)?;
            if in_degree[target_idx] == 0 {
                queue.push(target);
            }
        }
    }

    if result.len() == nodes.len() {
        Some(result)
    } else {
        // Cycle detected
        None
    }
}

/// Check if the graph is acyclic using DFS.
///
/// Time complexity: O(V + E)
pub fn is_acyclic(nodes: &[Node], edges: &[Edge<impl Semiring>]) -> bool {
    if nodes.is_empty() {
        return true;
    }

    let Some((adj, _)) = adjacency_from_edges(nodes, edges) else {
        return false;
    };

    // DFS with coloring: 0 = white (unvisited), 1 = gray (in progress), 2 = black (done)
    let mut color: Vec<u8> = vec![0; nodes.len()];

    fn dfs(node: usize, adj: &[Vec<NodeId>], color: &mut [u8]) -> bool {
        color[node] = 1; // Gray - currently being processed

        for &neighbor in &adj[node] {
            let Some(idx) = node_index(neighbor, color.len()) else {
                return false;
            };
            match color[idx] {
                1 => return false, // Back edge - cycle detected
                0 => {
                    if !dfs(idx, adj, color) {
                        return false;
                    }
                }
                _ => {} // Already processed (black)
            }
        }

        color[node] = 2; // Black - done
        true
    }

    // Check all nodes (in case graph is disconnected)
    for i in 0..nodes.len() {
        if color[i] == 0 && !dfs(i, &adj, &mut color) {
            return false;
        }
    }

    true
}

/// Count the number of paths from start to end using dynamic programming.
///
/// Returns `None` if the count would overflow or if graph has cycles.
///
/// Time complexity: O(V + E) after topological sort
pub fn count_paths<W: Semiring, B: LatticeBackend>(lattice: &mut Lattice<W, B>) -> Option<usize> {
    let topo_order = lattice.topological_order()?.to_vec();

    if topo_order.is_empty() {
        return Some(0);
    }

    let n = lattice.num_nodes();
    let mut path_count: Vec<usize> = vec![0; n];
    let start_idx = lattice.start().0 as usize;
    let end_idx = lattice.end().0 as usize;
    if start_idx >= n || end_idx >= n {
        return None;
    }

    // Start node has 1 path (the empty path to itself)
    path_count[start_idx] = 1;
    let (adjacency, _) = adjacency_from_edges(lattice.nodes(), lattice.edges())?;

    // Process in topological order
    for node_id in topo_order {
        let node_idx = node_id.0 as usize;
        if node_idx >= path_count.len() {
            return None;
        }
        let current_count = path_count[node_idx];
        if current_count == 0 {
            continue;
        }

        for &target in &adjacency[node_idx] {
            let target_idx = node_index(target, path_count.len())?;
            path_count[target_idx] = path_count[target_idx].checked_add(current_count)?;
        }
    }

    Some(path_count[end_idx])
}

/// Find all reachable nodes from a given starting node.
///
/// Uses BFS for level-order traversal.
pub fn reachable_nodes<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
    start: NodeId,
) -> FxHashSet<NodeId> {
    let mut visited = FxHashSet::default();
    let mut queue = vec![start];
    let adjacency = reachable_adjacency_from_edges(lattice);

    while let Some(node_id) = queue.pop() {
        let Some(node_idx) = node_index(node_id, adjacency.len()) else {
            continue;
        };
        if !visited.insert(node_id) {
            continue;
        }

        for &target in &adjacency[node_idx] {
            if !visited.contains(&target) {
                queue.push(target);
            }
        }
    }

    visited
}

/// Check if a path exists from source to target.
pub fn path_exists<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
    source: NodeId,
    target: NodeId,
) -> bool {
    if source == target {
        return lattice.node(source).is_some();
    }

    if lattice.node(source).is_none() || lattice.node(target).is_none() {
        return false;
    }

    let reachable = reachable_nodes(lattice, source);
    reachable.contains(&target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::semiring::TropicalWeight;

    fn linear_lattice(n: usize) -> Lattice<TropicalWeight, HashMapBackend> {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        for i in 0..n {
            builder.add_correction(
                i,
                i + 1,
                &format!("word{}", i),
                TropicalWeight::new(1.0),
                EdgeMetadata::default(),
            );
        }

        builder.build(n)
    }

    fn diamond_lattice() -> Lattice<TropicalWeight, HashMapBackend> {
        // Diamond shape: 0 -> (1, 2) -> 3
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 2, "b", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 3, "c", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(2, 3, "d", TropicalWeight::new(1.0), EdgeMetadata::default());

        builder.build(3)
    }

    fn lattice_with_stale_incoming() -> Lattice<TropicalWeight, HashMapBackend> {
        let mut backend = HashMapBackend::new();
        let label = backend.intern("a");
        let edge_id = EdgeId::new(0);
        let mut nodes = vec![
            Node::with_position(NodeId::new(0), 0),
            Node::with_position(NodeId::new(1), 1),
        ];
        nodes[0].outgoing.push(edge_id);

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

    fn lattice_with_stale_outgoing() -> Lattice<TropicalWeight, HashMapBackend> {
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

    fn lattice_with_malformed_target() -> Lattice<TropicalWeight, HashMapBackend> {
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

    fn lattice_with_invalid_start() -> Lattice<TropicalWeight, HashMapBackend> {
        let backend = HashMapBackend::new();
        let nodes = vec![Node::with_position(NodeId::new(0), 0)];

        Lattice::new(nodes, Vec::new(), NodeId::new(99), NodeId::new(0), backend)
    }

    #[test]
    fn test_topological_sort_linear() {
        let lattice = linear_lattice(3);
        let order = topological_sort(lattice.nodes(), lattice.edges())
            .expect("lattice/algorithms.rs: required value was None/Err");

        assert_eq!(order.len(), 4); // 4 nodes for 3 positions

        // Verify order respects edges
        for i in 0..order.len() - 1 {
            // Earlier positions should come before later positions
            let pos_i = lattice
                .node(order[i])
                .expect("lattice/algorithms.rs: required value was None/Err")
                .position;
            let pos_j = lattice
                .node(order[i + 1])
                .expect("lattice/algorithms.rs: required value was None/Err")
                .position;
            assert!(pos_i <= pos_j);
        }
    }

    #[test]
    fn test_topological_sort_diamond() {
        let lattice = diamond_lattice();
        let order = topological_sort(lattice.nodes(), lattice.edges())
            .expect("lattice/algorithms.rs: required value was None/Err");

        assert_eq!(order.len(), 4);

        // Start should be first, end should be last
        let start_pos = order
            .iter()
            .position(|&n| n == lattice.start())
            .expect("lattice/algorithms.rs: required value was None/Err");
        let end_pos = order
            .iter()
            .position(|&n| n == lattice.end())
            .expect("lattice/algorithms.rs: required value was None/Err");
        assert_eq!(start_pos, 0);
        assert_eq!(end_pos, 3);
    }

    #[test]
    fn test_topological_sort_empty() {
        let empty_edges: &[Edge<TropicalWeight>] = &[];
        let order = topological_sort(&[], empty_edges);
        assert_eq!(order, Some(vec![]));
    }

    #[test]
    fn test_topological_sort_uses_edge_list_for_in_degree() {
        let lattice = lattice_with_stale_incoming();
        let order = topological_sort(lattice.nodes(), lattice.edges())
            .expect("stale incoming adjacency should not poison topological order");

        assert_eq!(order, vec![NodeId::new(0), NodeId::new(1)]);
    }

    #[test]
    fn test_topological_sort_uses_edges_when_outgoing_cache_is_stale() {
        let lattice = lattice_with_stale_outgoing();
        let order = topological_sort(lattice.nodes(), lattice.edges())
            .expect("stale outgoing adjacency should not hide edge-list connectivity");

        assert_eq!(order, vec![NodeId::new(0), NodeId::new(1)]);
    }

    #[test]
    fn test_topological_sort_rejects_malformed_target() {
        let lattice = lattice_with_malformed_target();

        assert!(topological_sort(lattice.nodes(), lattice.edges()).is_none());
    }

    #[test]
    fn test_topological_sort_rejects_non_contiguous_node_ids() {
        let nodes = vec![Node::with_position(NodeId::new(99), 0)];
        let empty_edges: &[Edge<TropicalWeight>] = &[];

        assert!(topological_sort(&nodes, empty_edges).is_none());
    }

    #[test]
    fn test_is_acyclic_linear() {
        let lattice = linear_lattice(3);
        assert!(is_acyclic(lattice.nodes(), lattice.edges()));
    }

    #[test]
    fn test_is_acyclic_diamond() {
        let lattice = diamond_lattice();
        assert!(is_acyclic(lattice.nodes(), lattice.edges()));
    }

    #[test]
    fn test_is_acyclic_rejects_malformed_target() {
        let lattice = lattice_with_malformed_target();

        assert!(!is_acyclic(lattice.nodes(), lattice.edges()));
    }

    #[test]
    fn test_is_acyclic_rejects_non_contiguous_node_ids() {
        let nodes = vec![Node::with_position(NodeId::new(99), 0)];
        let empty_edges: &[Edge<TropicalWeight>] = &[];

        assert!(!is_acyclic(&nodes, empty_edges));
    }

    #[test]
    fn test_count_paths_linear() {
        let mut lattice = linear_lattice(3);
        assert_eq!(count_paths(&mut lattice), Some(1));
    }

    #[test]
    fn test_count_paths_diamond() {
        let mut lattice = diamond_lattice();
        assert_eq!(count_paths(&mut lattice), Some(2)); // Two paths: a->c and b->d
    }

    #[test]
    fn test_count_paths_multi_edge() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        // 2 edges at position 0, 3 edges at position 1
        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "b", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "c", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "d", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "e", TropicalWeight::new(1.0), EdgeMetadata::default());

        let mut lattice = builder.build(2);
        assert_eq!(count_paths(&mut lattice), Some(6)); // 2 * 3 = 6
    }

    #[test]
    fn test_count_paths_rejects_malformed_target() {
        let mut lattice = lattice_with_malformed_target();

        assert_eq!(count_paths(&mut lattice), None);
    }

    #[test]
    fn test_count_paths_rejects_invalid_start_or_end() {
        let mut lattice = lattice_with_invalid_start();

        assert_eq!(count_paths(&mut lattice), None);
    }

    #[test]
    fn test_count_paths_uses_edges_when_outgoing_cache_is_stale() {
        let mut lattice = lattice_with_stale_outgoing();

        assert_eq!(count_paths(&mut lattice), Some(1));
    }

    #[test]
    fn test_reachable_nodes() {
        let lattice = diamond_lattice();
        let reachable = reachable_nodes(&lattice, lattice.start());

        assert_eq!(reachable.len(), 4); // All nodes reachable from start
        assert!(reachable.contains(&lattice.start()));
        assert!(reachable.contains(&lattice.end()));
    }

    #[test]
    fn test_reachable_nodes_from_end() {
        let lattice = diamond_lattice();
        let reachable = reachable_nodes(&lattice, lattice.end());

        assert_eq!(reachable.len(), 1); // Only end itself
        assert!(reachable.contains(&lattice.end()));
    }

    #[test]
    fn test_reachable_nodes_ignores_malformed_targets() {
        let lattice = lattice_with_malformed_target();
        let reachable = reachable_nodes(&lattice, lattice.start());

        assert!(reachable.contains(&lattice.start()));
        assert!(!reachable.contains(&NodeId::new(99)));
    }

    #[test]
    fn test_reachable_nodes_uses_edges_when_outgoing_cache_is_stale() {
        let lattice = lattice_with_stale_outgoing();
        let reachable = reachable_nodes(&lattice, lattice.start());

        assert!(reachable.contains(&lattice.start()));
        assert!(reachable.contains(&lattice.end()));
    }

    #[test]
    fn test_path_exists() {
        let lattice = diamond_lattice();

        assert!(path_exists(&lattice, lattice.start(), lattice.end()));
        assert!(path_exists(&lattice, lattice.start(), lattice.start()));
        assert!(!path_exists(&lattice, lattice.end(), lattice.start()));
    }

    #[test]
    fn test_path_exists_rejects_invalid_nodes() {
        let lattice = lattice_with_malformed_target();

        assert!(!path_exists(&lattice, lattice.start(), NodeId::new(99)));
        assert!(!path_exists(&lattice, NodeId::new(99), NodeId::new(99)));
    }

    #[test]
    fn test_count_paths_empty() {
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let mut lattice = builder.build(0);

        // Empty lattice has 1 path (empty path from start=end to itself)
        assert_eq!(count_paths(&mut lattice), Some(1));
    }
}
