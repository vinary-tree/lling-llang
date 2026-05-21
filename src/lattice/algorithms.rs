//! Graph algorithms for lattices.

use rustc_hash::FxHashSet;

use super::lattice::Lattice;
use super::types::{Edge, Node, NodeId};
use crate::backend::LatticeBackend;
use crate::semiring::Semiring;

/// Compute topological order using Kahn's algorithm.
///
/// Returns `None` if the graph contains a cycle.
///
/// Time complexity: O(V + E)
/// Space complexity: O(V + E) for the edge target lookup
pub fn topological_sort<W: Semiring>(nodes: &[Node], edges: &[Edge<W>]) -> Option<Vec<NodeId>> {
    if nodes.is_empty() {
        return Some(Vec::new());
    }

    let n = nodes.len();

    // Build edge_id -> target lookup table: O(E)
    // This is the key optimization: instead of scanning all nodes for each edge,
    // we do a single pass over edges to build a direct lookup.
    let edge_targets: Vec<NodeId> = edges.iter().map(|e| e.target).collect();

    let mut in_degree: Vec<usize> = nodes.iter().map(|node| node.incoming.len()).collect();
    let mut queue: Vec<NodeId> = Vec::with_capacity(n);
    let mut result: Vec<NodeId> = Vec::with_capacity(n);

    // Start with nodes that have no incoming edges
    for node in nodes {
        if node.incoming.is_empty() {
            queue.push(node.id);
        }
    }

    while let Some(node_id) = queue.pop() {
        result.push(node_id);

        // Decrease in-degree for all neighbors: O(out_degree) per node
        if let Some(node) = nodes.get(node_id.0 as usize) {
            for &edge_id in &node.outgoing {
                // O(1) lookup instead of O(V) scan
                let target = edge_targets[edge_id.0 as usize];
                let idx = target.0 as usize;
                in_degree[idx] -= 1;
                if in_degree[idx] == 0 {
                    queue.push(target);
                }
            }
        }
    }

    if result.len() == n {
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

    // Build adjacency list for DFS
    let mut adj: Vec<Vec<NodeId>> = vec![Vec::new(); nodes.len()];
    for edge in edges {
        let src = edge.source.0 as usize;
        if src < adj.len() {
            adj[src].push(edge.target);
        }
    }

    // DFS with coloring: 0 = white (unvisited), 1 = gray (in progress), 2 = black (done)
    let mut color: Vec<u8> = vec![0; nodes.len()];

    fn dfs(node: usize, adj: &[Vec<NodeId>], color: &mut [u8]) -> bool {
        color[node] = 1; // Gray - currently being processed

        for &neighbor in &adj[node] {
            let idx = neighbor.0 as usize;
            if idx >= color.len() {
                continue;
            }
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

    // Start node has 1 path (the empty path to itself)
    path_count[lattice.start().0 as usize] = 1;

    // Process in topological order
    for node_id in topo_order {
        let current_count = path_count[node_id.0 as usize];
        if current_count == 0 {
            continue;
        }

        // Add paths to all successors
        let outgoing: Vec<_> = lattice.outgoing_edges(node_id).map(|e| e.target).collect();

        for target in outgoing {
            let target_idx = target.0 as usize;
            path_count[target_idx] = path_count[target_idx].checked_add(current_count)?;
        }
    }

    Some(path_count[lattice.end().0 as usize])
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

    while let Some(node_id) = queue.pop() {
        if !visited.insert(node_id) {
            continue;
        }

        for edge in lattice.outgoing_edges(node_id) {
            if !visited.contains(&edge.target) {
                queue.push(edge.target);
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
        return true;
    }

    let reachable = reachable_nodes(lattice, source);
    reachable.contains(&target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::builder::LatticeBuilder;
    use crate::lattice::types::EdgeMetadata;
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

    #[test]
    fn test_topological_sort_linear() {
        let lattice = linear_lattice(3);
        let order = topological_sort(lattice.nodes(), lattice.edges()).unwrap();

        assert_eq!(order.len(), 4); // 4 nodes for 3 positions

        // Verify order respects edges
        for i in 0..order.len() - 1 {
            // Earlier positions should come before later positions
            let pos_i = lattice.node(order[i]).unwrap().position;
            let pos_j = lattice.node(order[i + 1]).unwrap().position;
            assert!(pos_i <= pos_j);
        }
    }

    #[test]
    fn test_topological_sort_diamond() {
        let lattice = diamond_lattice();
        let order = topological_sort(lattice.nodes(), lattice.edges()).unwrap();

        assert_eq!(order.len(), 4);

        // Start should be first, end should be last
        let start_pos = order.iter().position(|&n| n == lattice.start()).unwrap();
        let end_pos = order.iter().position(|&n| n == lattice.end()).unwrap();
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
    fn test_path_exists() {
        let lattice = diamond_lattice();

        assert!(path_exists(&lattice, lattice.start(), lattice.end()));
        assert!(path_exists(&lattice, lattice.start(), lattice.start()));
        assert!(!path_exists(&lattice, lattice.end(), lattice.start()));
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
