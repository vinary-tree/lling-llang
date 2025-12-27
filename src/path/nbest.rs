//! N-best path extraction using lazy enumeration.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use smallvec::SmallVec;

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticePath, NodeId, EdgeId};
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

    fn into_lattice_path(self) -> LatticePath<W> {
        let mut path = LatticePath::with_weight(self.weight);
        path.edges = self.edges;
        path.mark_complete();
        path
    }
}

/// Wrapper for priority queue ordering.
///
/// BinaryHeap is a max-heap, so we reverse the ordering to get a min-heap
/// for TropicalWeight (smaller = better).
struct OrderedPath<W: Semiring>(PartialPath<W>);

impl<W: Semiring> PartialEq for OrderedPath<W> {
    fn eq(&self, other: &Self) -> bool {
        // Compare by weight
        self.0.weight == other.0.weight
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
        // Reverse ordering for min-heap behavior
        // Use natural_less if available, otherwise treat as equal
        match self.0.weight.natural_less(&other.0.weight) {
            Some(true) => Ordering::Greater,  // Reversed for min-heap
            Some(false) => {
                match other.0.weight.natural_less(&self.0.weight) {
                    Some(true) => Ordering::Less,
                    _ => Ordering::Equal,
                }
            }
            None => Ordering::Equal,
        }
    }
}

/// Lazy iterator for N-best paths.
///
/// Uses a priority queue to enumerate paths in order of increasing weight.
/// Based on the algorithm from Huang & Chiang (2005).
pub struct NBestIterator<'a, W: Semiring, B: LatticeBackend> {
    lattice: &'a Lattice<W, B>,
    /// Priority queue of partial paths.
    heap: BinaryHeap<OrderedPath<W>>,
    /// Target node (end of lattice).
    end: NodeId,
    /// Maximum number of paths to return.
    limit: usize,
    /// Number of paths returned so far.
    count: usize,
}

impl<'a, W: Semiring, B: LatticeBackend> NBestIterator<'a, W, B> {
    /// Create a new N-best iterator.
    pub fn new(lattice: &'a Lattice<W, B>, n: usize) -> Self {
        let mut heap = BinaryHeap::new();
        let start = lattice.start();
        let end = lattice.end();

        // Initialize with partial path from start
        heap.push(OrderedPath(PartialPath::new(start)));

        Self {
            lattice,
            heap,
            end,
            limit: n,
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

        while let Some(OrderedPath(partial)) = self.heap.pop() {
            // Check if we've reached the end
            if partial.node == self.end {
                self.count += 1;
                return Some(partial.into_lattice_path());
            }

            // Expand to successors
            for edge in self.lattice.outgoing_edges(partial.node) {
                let extended = partial.extend(edge.id, edge.target, edge.weight);
                self.heap.push(OrderedPath(extended));
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
/// O(k log k) for extracting k paths (after topological sort).
///
/// # Space Complexity
///
/// O(k × path_length) for storing partial paths in the heap.
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
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::{LatticeBuilder, EdgeMetadata};
    use crate::semiring::TropicalWeight;

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
                0, 1,
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
    fn test_nbest_single_path() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "hello", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "world", TropicalWeight::new(2.0), EdgeMetadata::default());

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
}
