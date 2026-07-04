//! N-best path extraction using lazy enumeration.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use smallvec::SmallVec;

use super::adjacency::{
    best_suffix_distances, compare_weights, edge_adjacency, node_index, path_priority,
};
use crate::backend::LatticeBackend;
use crate::lattice::{EdgeId, Lattice, LatticePath, NodeId};
use crate::semiring::Semiring;

/// A partial path in the N-best search.
#[derive(Clone, Debug)]
struct PartialPath<W: Semiring> {
    /// Current node.
    node: NodeId,
    /// Edges traversed.
    edges: SmallVec<[EdgeId; 16]>,
    /// Accumulated weight.
    weight: W,
}

impl<W: Semiring> PartialPath<W> {
    fn new(start: NodeId) -> Self {
        Self {
            node: start,
            edges: SmallVec::new(),
            weight: W::one(),
        }
    }

    fn extend(&self, edge_id: EdgeId, target: NodeId, edge_weight: W) -> Self {
        let mut new_edges = self.edges.clone();
        new_edges.push(edge_id);
        Self {
            node: target,
            edges: new_edges,
            weight: self.weight.times(&edge_weight),
        }
    }

    /// Extend by taking ownership (avoids clone for the last extension).
    fn extend_move(mut self, edge_id: EdgeId, target: NodeId, edge_weight: W) -> Self {
        self.edges.push(edge_id);
        self.node = target;
        self.weight = self.weight.times(&edge_weight);
        self
    }

    fn into_lattice_path(self) -> LatticePath<W> {
        let mut path = LatticePath::with_weight(self.weight);
        path.edges = self.edges;
        path.mark_complete();
        path
    }
}

fn compare_partial_paths<W: Semiring>(left: &PartialPath<W>, right: &PartialPath<W>) -> Ordering {
    compare_weights(&left.weight, &right.weight)
        .then_with(|| left.edges.as_slice().cmp(right.edges.as_slice()))
        .then_with(|| left.node.cmp(&right.node))
}

/// A queued path and the ordering key used by the heap.
#[derive(Clone, Debug)]
struct QueuedPath<W: Semiring> {
    path: PartialPath<W>,
    priority: W,
}

fn compare_queued_paths<W: Semiring>(left: &QueuedPath<W>, right: &QueuedPath<W>) -> Ordering {
    compare_weights(&left.priority, &right.priority)
        .then_with(|| compare_partial_paths(&left.path, &right.path))
}

/// Wrapper for priority queue ordering.
///
/// BinaryHeap is a max-heap, so we reverse the ordering to get a min-heap
/// for TropicalWeight (smaller = better).
struct OrderedPath<W: Semiring>(QueuedPath<W>);

impl<W: Semiring> PartialEq for OrderedPath<W> {
    fn eq(&self, other: &Self) -> bool {
        compare_queued_paths(&self.0, &other.0).is_eq()
    }
}

impl<W: Semiring> Eq for OrderedPath<W> {}

impl<W: Semiring> PartialOrd for OrderedPath<W> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<W: Semiring> Ord for OrderedPath<W> {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_queued_paths(&self.0, &other.0).reverse()
    }
}

/// Lazy iterator for N-best paths.
///
/// Uses a priority queue to enumerate paths in order of increasing weight.
/// Based on the algorithm from Huang & Chiang (2005).
pub struct NBestIterator<'a, W: Semiring, B: LatticeBackend> {
    lattice: &'a Lattice<W, B>,
    /// Edge IDs grouped by source node, derived from the edge list.
    adjacency: Vec<Vec<EdgeId>>,
    /// Exact best suffix weight from each node to `end` for acyclic lattices.
    ///
    /// When the lattice is cyclic, this is `None` and the iterator keeps the
    /// existing bounded prefix-cost search.
    suffix_best: Option<Vec<Option<W>>>,
    /// Priority queue of partial paths.
    heap: BinaryHeap<OrderedPath<W>>,
    /// Target node (end of lattice).
    end: NodeId,
    /// Maximum number of paths to return.
    limit: usize,
    /// Maximum number of edges to explore in one path.
    max_depth: usize,
    /// Number of paths returned so far.
    count: usize,
}

impl<'a, W: Semiring, B: LatticeBackend> NBestIterator<'a, W, B> {
    /// Create a new N-best iterator.
    pub fn new(lattice: &'a Lattice<W, B>, n: usize) -> Self {
        let mut heap = BinaryHeap::with_capacity(n.max(1));
        let start = lattice.start();
        let end = lattice.end();
        let adjacency = edge_adjacency(lattice).unwrap_or_default();
        let suffix_best = best_suffix_distances(lattice, &adjacency);

        // Initialize with partial path from start
        if n > 0
            && node_index(start, adjacency.len()).is_some()
            && node_index(end, adjacency.len()).is_some()
        {
            let path = PartialPath::new(start);
            if let Some(priority) = path_priority(
                suffix_best.as_deref(),
                adjacency.len(),
                path.node,
                path.weight,
            ) {
                heap.push(OrderedPath(QueuedPath { path, priority }));
            }
        }

        Self {
            lattice,
            adjacency,
            suffix_best,
            heap,
            end,
            limit: n,
            max_depth: lattice.num_nodes().saturating_sub(1),
            count: 0,
        }
    }
}

impl<'a, W: Semiring, B: LatticeBackend> Iterator for NBestIterator<'a, W, B> {
    type Item = LatticePath<W>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.count >= self.limit {
            return None;
        }

        while let Some(OrderedPath(queued)) = self.heap.pop() {
            let partial = queued.path;
            // Check if we've reached the end
            if partial.node == self.end {
                self.count += 1;
                return Some(partial.into_lattice_path());
            }

            if partial.edges.len() >= self.max_depth {
                continue;
            }

            // Expand to successors - use move for the last edge to avoid one clone
            let Some(edge_ids) = node_index(partial.node, self.adjacency.len())
                .and_then(|node_idx| self.adjacency.get(node_idx))
            else {
                continue;
            };
            let mut edges_iter = edge_ids
                .iter()
                .filter_map(|&edge_id| self.lattice.edge(edge_id));
            if let Some(first_edge) = edges_iter.next() {
                let mut last_edge = (first_edge.id, first_edge.target, first_edge.weight);

                for edge in edges_iter {
                    // Process the previous edge with clone (more edges follow)
                    let extended = partial.extend(last_edge.0, last_edge.1, last_edge.2);
                    if let Some(priority) = path_priority(
                        self.suffix_best.as_deref(),
                        self.adjacency.len(),
                        extended.node,
                        extended.weight,
                    ) {
                        self.heap.push(OrderedPath(QueuedPath {
                            path: extended,
                            priority,
                        }));
                    }
                    last_edge = (edge.id, edge.target, edge.weight);
                }

                // Process the last edge with move (no more edges)
                let extended = partial.extend_move(last_edge.0, last_edge.1, last_edge.2);
                if let Some(priority) = path_priority(
                    self.suffix_best.as_deref(),
                    self.adjacency.len(),
                    extended.node,
                    extended.weight,
                ) {
                    self.heap.push(OrderedPath(QueuedPath {
                        path: extended,
                        priority,
                    }));
                }
            }
        }

        None
    }
}

/// Extract the N best paths from a lattice.
///
/// Uses a priority queue to lazily enumerate paths in order of
/// increasing weight (for TropicalWeight). Paths are computed on-demand,
/// making this efficient when only a few paths are needed.
///
/// # Time Complexity
///
/// O(V + E + P log P) for V nodes, E edges, and P explored partial paths.
///
/// # Space Complexity
///
/// O(V + E + P × path_length) for suffix costs, adjacency, and queued paths.
///
/// # Example
///
/// ```rust
/// use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
/// use lling_llang::backend::HashMapBackend;
/// use lling_llang::semiring::TropicalWeight;
/// use lling_llang::path::nbest;
///
/// let backend = HashMapBackend::new();
/// let mut builder = LatticeBuilder::new(backend);
///
/// builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::default());
/// builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
///
/// let mut lattice = builder.build(1);
/// let top3 = nbest(&mut lattice, 3);
///
/// for (i, path) in top3.iter().enumerate() {
///     println!("Path {}: {:?}", i + 1, path.to_words(&lattice));
/// }
/// ```
pub fn nbest<W: Semiring, B: LatticeBackend>(
    lattice: &mut Lattice<W, B>,
    n: usize,
) -> Vec<LatticePath<W>> {
    NBestIterator::new(lattice, n).collect()
}

#[cfg(test)]
mod tests {
    use super::super::adjacency::test_support::{
        lattice_with_invalid_start, lattice_with_malformed_target, lattice_with_stale_outgoing,
    };
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::{EdgeMetadata, LatticeBuilder};
    use crate::semiring::TropicalWeight;

    fn ordered_path(edge_id: u32, node: u32, weight: f64) -> OrderedPath<TropicalWeight> {
        let mut edges = SmallVec::new();
        edges.push(EdgeId::new(edge_id));

        let path = PartialPath {
            node: NodeId::new(node),
            edges,
            weight: TropicalWeight::new(weight),
        };
        OrderedPath(QueuedPath {
            priority: path.weight,
            path,
        })
    }

    #[test]
    fn test_ordered_path_tie_breaks_equal_weights() {
        let first = ordered_path(0, 1, 1.0);
        let second = ordered_path(1, 1, 1.0);

        assert_eq!(first.cmp(&second), Ordering::Greater);
        assert_eq!(second.cmp(&first), Ordering::Less);
        assert!(first != second);
    }

    #[test]
    fn test_ordered_path_tie_breaks_equal_edges_by_node() {
        let first = ordered_path(0, 1, 1.0);
        let second = ordered_path(0, 2, 1.0);

        assert_eq!(first.cmp(&second), Ordering::Greater);
        assert_eq!(second.cmp(&first), Ordering::Less);
        assert!(first != second);
    }

    #[test]
    fn test_nbest_simple() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "b", TropicalWeight::new(2.0), EdgeMetadata::default());

        let mut lattice = builder.build(1);
        let paths = nbest(&mut lattice, 10);

        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].weight.value(), 1.0);
        assert_eq!(paths[1].weight.value(), 2.0);
    }

    #[test]
    fn test_nbest_multi_position() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "b", TropicalWeight::new(2.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "c", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "d", TropicalWeight::new(2.0), EdgeMetadata::default());

        let mut lattice = builder.build(2);
        let paths = nbest(&mut lattice, 10);

        // 4 paths: ac (2.0), ad (3.0), bc (3.0), bd (4.0)
        assert_eq!(paths.len(), 4);

        let weights: Vec<_> = paths.iter().map(|p| p.weight.value()).collect();
        assert_eq!(weights[0], 2.0); // a + c
                                     // ad and bc have same weight (3.0), order depends on heap
        assert!(weights[1] == 3.0 || weights[1] == 3.0);
        assert!(weights[2] == 3.0 || weights[2] == 3.0);
        assert_eq!(weights[3], 4.0); // b + d
    }

    #[test]
    fn test_nbest_limit() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        for i in 0..10 {
            builder.add_correction(
                0,
                1,
                &format!("word{}", i),
                TropicalWeight::new(i as f64),
                EdgeMetadata::default(),
            );
        }

        let mut lattice = builder.build(1);
        let paths = nbest(&mut lattice, 3);

        assert_eq!(paths.len(), 3);
        assert_eq!(paths[0].weight.value(), 0.0);
        assert_eq!(paths[1].weight.value(), 1.0);
        assert_eq!(paths[2].weight.value(), 2.0);
    }

    #[test]
    fn test_nbest_empty_lattice() {
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let mut lattice = builder.build(0);

        let paths = nbest(&mut lattice, 10);

        // Empty lattice with start == end yields one empty path
        assert_eq!(paths.len(), 1);
        assert!(paths[0].is_empty());
    }

    #[test]
    fn test_nbest_rejects_invalid_start_or_end() {
        let mut lattice = lattice_with_invalid_start();
        let paths = nbest(&mut lattice, 10);

        assert!(paths.is_empty());
    }

    #[test]
    fn test_nbest_rejects_malformed_target() {
        let mut lattice = lattice_with_malformed_target();
        let paths = nbest(&mut lattice, 10);

        assert!(paths.is_empty());
    }

    #[test]
    fn test_nbest_uses_edges_when_outgoing_cache_is_stale() {
        let mut lattice = lattice_with_stale_outgoing();
        let paths = nbest(&mut lattice, 10);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].weight.value(), 1.0);
        assert_eq!(paths[0].to_words(&lattice), vec!["a"]);
    }

    #[test]
    fn test_nbest_single_path() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(
            0,
            1,
            "hello",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            1,
            2,
            "world",
            TropicalWeight::new(2.0),
            EdgeMetadata::default(),
        );

        let mut lattice = builder.build(2);
        let paths = nbest(&mut lattice, 10);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].weight.value(), 3.0);

        let words = paths[0].to_words(&lattice);
        assert_eq!(words, vec!["hello", "world"]);
    }

    #[test]
    fn test_nbest_diamond() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 2, "b", TropicalWeight::new(2.0), EdgeMetadata::default());
        builder.add_correction(1, 3, "c", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(2, 3, "d", TropicalWeight::new(0.5), EdgeMetadata::default());

        let mut lattice = builder.build(3);
        let paths = nbest(&mut lattice, 10);

        assert_eq!(paths.len(), 2);

        // Best: a + c = 2.0
        // Second: b + d = 2.5
        assert_eq!(paths[0].weight.value(), 2.0);
        assert_eq!(paths[1].weight.value(), 2.5);
    }

    #[test]
    fn test_nbest_uses_suffix_priority_for_negative_dag_edges() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(
            0,
            1,
            "expensive-prefix",
            TropicalWeight::new(10.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            1,
            3,
            "large-credit",
            TropicalWeight::new(-20.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            0,
            2,
            "cheap-prefix",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            2,
            3,
            "neutral-suffix",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );

        let mut lattice = builder.build(3);
        let paths = nbest(&mut lattice, 2);

        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].weight.value(), -10.0);
        assert_eq!(
            paths[0].to_words(&lattice),
            vec!["expensive-prefix", "large-credit"]
        );
        assert_eq!(paths[1].weight.value(), 0.0);
        assert_eq!(
            paths[1].to_words(&lattice),
            vec!["cheap-prefix", "neutral-suffix"]
        );
    }

    #[test]
    fn test_nbest_iterator() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "b", TropicalWeight::new(2.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "c", TropicalWeight::new(3.0), EdgeMetadata::default());

        let lattice = builder.build(1);
        let mut iter = NBestIterator::new(&lattice, 2);

        let first = iter.next().expect("first path");
        assert_eq!(first.weight.value(), 1.0);

        let second = iter.next().expect("second path");
        assert_eq!(second.weight.value(), 2.0);

        assert!(iter.next().is_none()); // Limit reached
    }

    #[test]
    fn test_nbest_preserves_order() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        // Add in random weight order
        builder.add_correction(0, 1, "c", TropicalWeight::new(3.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "b", TropicalWeight::new(2.0), EdgeMetadata::default());

        let mut lattice = builder.build(1);
        let paths = nbest(&mut lattice, 3);

        // Should be sorted by weight
        assert_eq!(paths[0].weight.value(), 1.0);
        assert_eq!(paths[1].weight.value(), 2.0);
        assert_eq!(paths[2].weight.value(), 3.0);
    }

    #[test]
    fn test_nbest_cycle_before_end_is_bounded() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(
            1,
            0,
            "loop",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            1,
            2,
            "done",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );

        let mut lattice = builder.build(2);
        assert!(!lattice.is_acyclic());

        let paths = nbest(&mut lattice, 8);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].to_words(&lattice), vec!["a", "done"]);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::test_utils::{arb_diamond_lattice, arb_linear_lattice, arb_tropical_lattice};
    use proptest::prelude::*;
    use proptest::strategy::ValueTree;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// N-best on a linear lattice returns exactly 1 path.
        #[test]
        fn nbest_linear_returns_one(
            mut lattice in arb_linear_lattice(4)
        ) {
            let paths = nbest(&mut lattice, 100);
            prop_assert_eq!(paths.len(), 1);
        }

        /// N-best returns paths in sorted order (ascending weight).
        #[test]
        fn nbest_returns_sorted(
            mut lattice in arb_tropical_lattice(3, 3)
        ) {
            let paths = nbest(&mut lattice, 50);

            // Verify sorted order
            for i in 1..paths.len() {
                prop_assert!(
                    paths[i - 1].weight.value() <= paths[i].weight.value() + 1e-9,
                    "Path {} (weight {}) > Path {} (weight {})",
                    i - 1, paths[i - 1].weight.value(),
                    i, paths[i].weight.value()
                );
            }
        }

        /// N-best respects limit parameter.
        #[test]
        fn nbest_respects_limit(
            mut lattice in arb_diamond_lattice(4),  // 2^4 = 16 paths
            n in 1usize..10
        ) {
            let paths = nbest(&mut lattice, n);
            prop_assert!(paths.len() <= n);
        }

        /// N-best returns at least one path for non-empty lattice.
        #[test]
        fn nbest_returns_at_least_one(
            mut lattice in arb_tropical_lattice(2, 2)
        ) {
            let paths = nbest(&mut lattice, 10);
            prop_assert!(!paths.is_empty());
        }

        /// Diamond lattice produces 2^n paths.
        #[test]
        fn nbest_diamond_path_count(n in 1usize..5) {
            let mut lattice = arb_diamond_lattice(n)
                .new_tree(&mut proptest::test_runner::TestRunner::deterministic())
                .expect("generate lattice")
                .current();

            let expected_count = 1usize << n; // 2^n
            let paths = nbest(&mut lattice, 1000);
            prop_assert_eq!(paths.len(), expected_count);
        }

        /// All n-best paths are complete.
        #[test]
        fn nbest_paths_complete(
            mut lattice in arb_tropical_lattice(3, 2)
        ) {
            let paths = nbest(&mut lattice, 10);
            for path in &paths {
                prop_assert!(path.is_complete);
            }
        }

        /// All n-best paths have the same length (equal to positions).
        #[test]
        fn nbest_paths_same_length(
            mut lattice in arb_tropical_lattice(4, 2)
        ) {
            let paths = nbest(&mut lattice, 20);
            for path in &paths {
                prop_assert_eq!(path.len(), 4, "Path length {} != expected 4", path.len());
            }
        }
    }
}
