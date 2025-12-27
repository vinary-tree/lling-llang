//! Path iteration for lattices.

use smallvec::SmallVec;

use crate::backend::{LatticeBackend, VocabId};
use crate::semiring::Semiring;
use super::types::{NodeId, EdgeId};
use super::lattice::Lattice;

/// A path through a lattice.
///
/// Represents a sequence of edges from start to end, with the accumulated weight.
#[derive(Clone, Debug)]
pub struct LatticePath<W: Semiring> {
    /// The edges traversed in order.
    pub edges: SmallVec<[EdgeId; 16]>,
    /// The accumulated weight of the path.
    pub weight: W,
    /// Whether this is a complete path (reaches the end node).
    pub is_complete: bool,
}

impl<W: Semiring> LatticePath<W> {
    /// Create a new empty path starting from the start node.
    #[inline]
    pub fn new() -> Self {
        Self {
            edges: SmallVec::new(),
            weight: W::one(),
            is_complete: false,
        }
    }

    /// Create a path with initial weight.
    #[inline]
    pub fn with_weight(weight: W) -> Self {
        Self {
            edges: SmallVec::new(),
            weight,
            is_complete: false,
        }
    }

    /// Extend the path with an edge.
    #[inline]
    pub fn extend(&mut self, edge_id: EdgeId, edge_weight: W) {
        self.edges.push(edge_id);
        self.weight = self.weight.times(&edge_weight);
    }

    /// Mark the path as complete.
    #[inline]
    pub fn mark_complete(&mut self) {
        self.is_complete = true;
    }

    /// Get the number of edges in the path.
    #[inline]
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// Check if the path is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Get the labels (vocabulary IDs) for this path.
    pub fn labels<'a, B: LatticeBackend>(
        &'a self,
        lattice: &'a Lattice<W, B>,
    ) -> impl Iterator<Item = VocabId> + 'a {
        self.edges.iter().filter_map(move |&edge_id| {
            lattice.edge(edge_id).map(|e| e.label)
        })
    }

    /// Get the words for this path.
    pub fn words<'a, B: LatticeBackend>(
        &'a self,
        lattice: &'a Lattice<W, B>,
    ) -> impl Iterator<Item = &'a str> + 'a {
        self.edges.iter().filter_map(move |&edge_id| {
            lattice.edge(edge_id).and_then(|e| lattice.word(e.label))
        })
    }

    /// Convert the path to a vector of words.
    pub fn to_words<B: LatticeBackend>(&self, lattice: &Lattice<W, B>) -> Vec<String> {
        self.words(lattice).map(|s| s.to_string()).collect()
    }
}

impl<W: Semiring> Default for LatticePath<W> {
    fn default() -> Self {
        Self::new()
    }
}

/// Iterator over all paths in a lattice.
///
/// Uses depth-first search to enumerate paths from start to end.
/// Paths are yielded lazily as they are discovered.
///
/// # Warning
///
/// For large lattices with many paths, consider using beam search or
/// N-best extraction instead, as this iterator may enumerate an
/// exponential number of paths.
pub struct PathIterator<'a, W: Semiring, B: LatticeBackend> {
    lattice: &'a Lattice<W, B>,
    /// Stack of (current_node, edge_index, partial_path)
    stack: Vec<(NodeId, usize, LatticePath<W>)>,
}

impl<'a, W: Semiring, B: LatticeBackend> PathIterator<'a, W, B> {
    /// Create a new path iterator for the lattice.
    pub fn new(lattice: &'a Lattice<W, B>) -> Self {
        let start = lattice.start();
        let mut stack = Vec::new();

        // Initialize with start node
        if lattice.num_nodes() > 0 {
            stack.push((start, 0, LatticePath::new()));
        }

        Self { lattice, stack }
    }

    /// Create a path iterator with a maximum number of paths.
    pub fn with_limit(lattice: &'a Lattice<W, B>, limit: usize) -> LimitedPathIterator<'a, W, B> {
        LimitedPathIterator {
            inner: Self::new(lattice),
            remaining: limit,
        }
    }
}

impl<'a, W: Semiring, B: LatticeBackend> Iterator for PathIterator<'a, W, B> {
    type Item = LatticePath<W>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((node, edge_idx, path)) = self.stack.pop() {
            // Get outgoing edges
            let outgoing: SmallVec<[_; 8]> = self.lattice
                .outgoing_edges(node)
                .map(|e| (e.id, e.target, e.weight))
                .collect();

            if edge_idx < outgoing.len() {
                let (edge_id, target, weight) = outgoing[edge_idx];

                // Push state for next edge from this node
                if edge_idx + 1 < outgoing.len() {
                    self.stack.push((node, edge_idx + 1, path.clone()));
                }

                // Create extended path
                let mut new_path = path;
                new_path.extend(edge_id, weight);

                // Check if we reached the end
                if target == self.lattice.end() {
                    new_path.mark_complete();
                    return Some(new_path);
                }

                // Continue from target node
                self.stack.push((target, 0, new_path));
            }
        }

        None
    }
}

/// Iterator with a maximum path limit.
pub struct LimitedPathIterator<'a, W: Semiring, B: LatticeBackend> {
    inner: PathIterator<'a, W, B>,
    remaining: usize,
}

impl<'a, W: Semiring, B: LatticeBackend> Iterator for LimitedPathIterator<'a, W, B> {
    type Item = LatticePath<W>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        self.inner.next()
    }
}

/// Extension trait for lattices to provide path iteration.
pub trait LatticePathExt<W: Semiring, B: LatticeBackend> {
    /// Iterate over all paths in the lattice.
    fn paths(&self) -> PathIterator<'_, W, B>;

    /// Iterate over at most `limit` paths.
    fn paths_limited(&self, limit: usize) -> LimitedPathIterator<'_, W, B>;
}

impl<W: Semiring, B: LatticeBackend> LatticePathExt<W, B> for Lattice<W, B> {
    fn paths(&self) -> PathIterator<'_, W, B> {
        PathIterator::new(self)
    }

    fn paths_limited(&self, limit: usize) -> LimitedPathIterator<'_, W, B> {
        PathIterator::with_limit(self, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::builder::LatticeBuilder;
    use crate::lattice::types::EdgeMetadata;
    use crate::semiring::TropicalWeight;

    fn sample_lattice() -> Lattice<TropicalWeight, HashMapBackend> {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::default());
        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "quick", TropicalWeight::new(0.5), EdgeMetadata::default());
        builder.add_correction(1, 2, "slow", TropicalWeight::new(1.5), EdgeMetadata::default());

        builder.build(2)
    }

    #[test]
    fn test_lattice_path_new() {
        let path: LatticePath<TropicalWeight> = LatticePath::new();
        assert!(path.is_empty());
        assert_eq!(path.len(), 0);
        assert!(!path.is_complete);
        assert_eq!(path.weight, TropicalWeight::one());
    }

    #[test]
    fn test_lattice_path_extend() {
        let mut path: LatticePath<TropicalWeight> = LatticePath::new();
        path.extend(EdgeId::new(0), TropicalWeight::new(1.0));
        path.extend(EdgeId::new(1), TropicalWeight::new(2.0));

        assert_eq!(path.len(), 2);
        assert_eq!(path.weight.value(), 3.0); // TropicalWeight uses + for times
    }

    #[test]
    fn test_path_iterator_count() {
        let lattice = sample_lattice();
        let paths: Vec<_> = lattice.paths().collect();

        // 2 edges at position 0, 2 edges at position 1 = 4 paths
        assert_eq!(paths.len(), 4);
    }

    #[test]
    fn test_path_iterator_completeness() {
        let lattice = sample_lattice();

        for path in lattice.paths() {
            assert!(path.is_complete);
            assert_eq!(path.len(), 2); // Two edges per path
        }
    }

    #[test]
    fn test_path_to_words() {
        let lattice = sample_lattice();
        let mut word_paths: Vec<Vec<String>> = lattice
            .paths()
            .map(|p| p.to_words(&lattice))
            .collect();

        word_paths.sort();

        assert!(word_paths.contains(&vec!["the".to_string(), "quick".to_string()]));
        assert!(word_paths.contains(&vec!["the".to_string(), "slow".to_string()]));
        assert!(word_paths.contains(&vec!["a".to_string(), "quick".to_string()]));
        assert!(word_paths.contains(&vec!["a".to_string(), "slow".to_string()]));
    }

    #[test]
    fn test_path_weights() {
        let lattice = sample_lattice();
        let paths: Vec<_> = lattice.paths().collect();

        // Find the path with minimum weight
        let min_path = paths.iter()
            .min_by(|a, b| a.weight.value().partial_cmp(&b.weight.value()).unwrap())
            .unwrap();

        // "the" (0.5) + "quick" (0.5) = 1.0
        assert_eq!(min_path.weight.value(), 1.0);
    }

    #[test]
    fn test_limited_path_iterator() {
        let lattice = sample_lattice();
        let paths: Vec<_> = lattice.paths_limited(2).collect();

        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn test_empty_lattice_paths() {
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let lattice = builder.build(0);

        let paths: Vec<_> = lattice.paths().collect();
        assert!(paths.is_empty()); // No edges means no paths (empty path not yielded)
    }

    #[test]
    fn test_single_path_lattice() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "hello", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "world", TropicalWeight::new(2.0), EdgeMetadata::default());

        let lattice = builder.build(2);
        let paths: Vec<_> = lattice.paths().collect();

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].len(), 2);
        assert_eq!(paths[0].weight.value(), 3.0);

        let words = paths[0].to_words(&lattice);
        assert_eq!(words, vec!["hello", "world"]);
    }

    #[test]
    fn test_path_labels() {
        let lattice = sample_lattice();
        let paths: Vec<_> = lattice.paths().collect();

        for path in &paths {
            let labels: Vec<_> = path.labels(&lattice).collect();
            assert_eq!(labels.len(), 2);
        }
    }

    #[test]
    fn test_diamond_lattice_paths() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        // Diamond: 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3
        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 2, "b", TropicalWeight::new(2.0), EdgeMetadata::default());
        builder.add_correction(1, 3, "c", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(2, 3, "d", TropicalWeight::new(1.0), EdgeMetadata::default());

        let lattice = builder.build(3);
        let paths: Vec<_> = lattice.paths().collect();

        assert_eq!(paths.len(), 2);

        let word_paths: Vec<_> = paths.iter().map(|p| p.to_words(&lattice)).collect();
        assert!(word_paths.contains(&vec!["a".to_string(), "c".to_string()]));
        assert!(word_paths.contains(&vec!["b".to_string(), "d".to_string()]));
    }
}
