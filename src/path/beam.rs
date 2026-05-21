//! Beam search for approximate path extraction.

use smallvec::SmallVec;

use crate::backend::LatticeBackend;
use crate::lattice::{EdgeId, Lattice, LatticePath, NodeId};
use crate::semiring::Semiring;

/// Configuration for beam search.
#[derive(Clone, Debug)]
pub struct BeamSearchConfig {
    /// Maximum number of hypotheses to keep at each step.
    pub beam_width: usize,
    /// Maximum number of paths to return.
    pub max_results: usize,
    /// Whether to allow duplicate paths (same word sequence).
    pub allow_duplicates: bool,
}

impl Default for BeamSearchConfig {
    fn default() -> Self {
        Self {
            beam_width: 10,
            max_results: 10,
            allow_duplicates: false,
        }
    }
}

impl BeamSearchConfig {
    /// Create a new configuration with the given beam width.
    pub fn new(beam_width: usize) -> Self {
        Self {
            beam_width,
            ..Default::default()
        }
    }

    /// Set the maximum number of results.
    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.max_results = max_results;
        self
    }

    /// Set whether to allow duplicate paths.
    pub fn with_duplicates(mut self, allow: bool) -> Self {
        self.allow_duplicates = allow;
        self
    }
}

/// A hypothesis (partial path) in beam search.
#[derive(Clone, Debug)]
struct Hypothesis<W: Semiring> {
    /// Current node.
    node: NodeId,
    /// Edges traversed.
    edges: SmallVec<[EdgeId; 16]>,
    /// Accumulated weight.
    weight: W,
}

impl<W: Semiring> Hypothesis<W> {
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

/// Perform beam search on a lattice.
///
/// Beam search is an approximate algorithm that keeps only the top
/// `beam_width` hypotheses at each step. This provides bounded memory
/// usage at the cost of potentially missing optimal paths.
///
/// # Time Complexity
///
/// O(V × beam_width × avg_out_degree) where V is the number of nodes.
///
/// # Space Complexity
///
/// O(beam_width × path_length) for storing hypotheses.
///
/// # Example
///
/// ```rust
/// use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
/// use lling_llang::backend::HashMapBackend;
/// use lling_llang::semiring::TropicalWeight;
/// use lling_llang::path::beam_search;
///
/// let backend = HashMapBackend::new();
/// let mut builder = LatticeBuilder::new(backend);
///
/// builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::default());
/// builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
///
/// let mut lattice = builder.build(1);
/// let paths = beam_search(&mut lattice, 5);
///
/// for path in &paths {
///     println!("{:?}", path.to_words(&lattice));
/// }
/// ```
pub fn beam_search<W: Semiring, B: LatticeBackend>(
    lattice: &mut Lattice<W, B>,
    beam_width: usize,
) -> Vec<LatticePath<W>> {
    beam_search_with_config(lattice, BeamSearchConfig::new(beam_width))
}

/// Perform beam search with custom configuration.
pub fn beam_search_with_config<W: Semiring, B: LatticeBackend>(
    lattice: &mut Lattice<W, B>,
    config: BeamSearchConfig,
) -> Vec<LatticePath<W>> {
    let start = lattice.start();
    let end = lattice.end();

    // Handle empty lattice
    if lattice.is_empty() {
        if start == end {
            let mut path = LatticePath::new();
            path.mark_complete();
            return vec![path];
        }
        return Vec::new();
    }

    // Get topological order
    let _topo_order = match lattice.topological_order() {
        Some(order) => order.to_vec(),
        None => return Vec::new(), // Cycle detected
    };

    // Initialize beam with start node
    let mut current_beam: Vec<Hypothesis<W>> = vec![Hypothesis::new(start)];
    let mut completed: Vec<LatticePath<W>> = Vec::new();

    // Process until all hypotheses complete or beam is empty
    while !current_beam.is_empty() && completed.len() < config.max_results {
        let mut next_beam: Vec<Hypothesis<W>> = Vec::new();

        // Expand each hypothesis
        for hyp in current_beam.drain(..) {
            // Check if hypothesis reached the end
            if hyp.node == end {
                completed.push(hyp.into_lattice_path());
                continue;
            }

            // Expand outgoing edges - use move for the last edge to avoid one clone
            let mut edges_iter = lattice.outgoing_edges(hyp.node);
            if let Some(first_edge) = edges_iter.next() {
                let mut last_edge = (first_edge.id, first_edge.target, first_edge.weight);

                for edge in edges_iter {
                    // Process the previous edge with clone (more edges follow)
                    let extended = hyp.extend(last_edge.0, last_edge.1, last_edge.2);
                    next_beam.push(extended);
                    last_edge = (edge.id, edge.target, edge.weight);
                }

                // Process the last edge with move (no more edges)
                let extended = hyp.extend_move(last_edge.0, last_edge.1, last_edge.2);
                next_beam.push(extended);
            }
        }

        // Prune beam to top beam_width hypotheses
        if next_beam.len() > config.beam_width {
            // Sort by weight (ascending for TropicalWeight)
            next_beam.sort_by(|a, b| match a.weight.natural_less(&b.weight) {
                Some(true) => std::cmp::Ordering::Less,
                Some(false) => std::cmp::Ordering::Greater,
                None => std::cmp::Ordering::Equal,
            });
            next_beam.truncate(config.beam_width);
        }

        current_beam = next_beam;
    }

    // Sort by weight
    completed.sort_by(|a, b| match a.weight.natural_less(&b.weight) {
        Some(true) => std::cmp::Ordering::Less,
        Some(false) => std::cmp::Ordering::Greater,
        None => std::cmp::Ordering::Equal,
    });

    // Limit results
    completed.truncate(config.max_results);

    completed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::{EdgeMetadata, LatticeBuilder};
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_beam_search_simple() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "b", TropicalWeight::new(2.0), EdgeMetadata::default());

        let mut lattice = builder.build(1);
        let paths = beam_search(&mut lattice, 10);

        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].weight.value(), 1.0);
        assert_eq!(paths[1].weight.value(), 2.0);
    }

    #[test]
    fn test_beam_search_multi_position() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "b", TropicalWeight::new(2.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "c", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "d", TropicalWeight::new(2.0), EdgeMetadata::default());

        let mut lattice = builder.build(2);
        let paths = beam_search(&mut lattice, 10);

        assert_eq!(paths.len(), 4);
        assert_eq!(paths[0].weight.value(), 2.0); // a + c
    }

    #[test]
    fn test_beam_search_pruning() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        // Create many alternatives
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

        // With beam width 3, only top 3 should be kept
        let paths = beam_search(&mut lattice, 3);

        assert!(paths.len() <= 3);
        // Best paths should be preserved
        assert_eq!(paths[0].weight.value(), 0.0);
    }

    #[test]
    fn test_beam_search_empty_lattice() {
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let mut lattice = builder.build(0);

        let paths = beam_search(&mut lattice, 10);

        assert_eq!(paths.len(), 1);
        assert!(paths[0].is_empty());
    }

    #[test]
    fn test_beam_search_config() {
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

        let config = BeamSearchConfig::new(10).with_max_results(3);
        let paths = beam_search_with_config(&mut lattice, config);

        assert_eq!(paths.len(), 3);
    }

    #[test]
    fn test_beam_search_diamond() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 2, "b", TropicalWeight::new(2.0), EdgeMetadata::default());
        builder.add_correction(1, 3, "c", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(2, 3, "d", TropicalWeight::new(0.5), EdgeMetadata::default());

        let mut lattice = builder.build(3);
        let paths = beam_search(&mut lattice, 10);

        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].weight.value(), 2.0); // a + c
        assert_eq!(paths[1].weight.value(), 2.5); // b + d
    }

    #[test]
    fn test_beam_search_single_path() {
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
        let paths = beam_search(&mut lattice, 10);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].weight.value(), 3.0);

        let words = paths[0].to_words(&lattice);
        assert_eq!(words, vec!["hello", "world"]);
    }

    #[test]
    fn test_beam_search_narrow_beam() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        // Many paths that diverge early
        for i in 0..10 {
            builder.add_correction(
                0,
                1,
                &format!("a{}", i),
                TropicalWeight::new(i as f64),
                EdgeMetadata::default(),
            );
            builder.add_correction(
                1,
                2,
                &format!("b{}", i),
                TropicalWeight::new(i as f64),
                EdgeMetadata::default(),
            );
        }

        let mut lattice = builder.build(2);

        // With beam width 1, only the best path should survive
        let paths = beam_search(&mut lattice, 1);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].weight.value(), 0.0); // Best path: a0 + b0
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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Beam search on a linear lattice returns exactly 1 path.
        #[test]
        fn beam_linear_returns_one(
            mut lattice in arb_linear_lattice(4)
        ) {
            let paths = beam_search(&mut lattice, 100);
            prop_assert_eq!(paths.len(), 1);
        }

        /// Beam search returns paths in sorted order.
        #[test]
        fn beam_returns_sorted(
            mut lattice in arb_tropical_lattice(3, 3)
        ) {
            let paths = beam_search(&mut lattice, 50);

            for i in 1..paths.len() {
                prop_assert!(
                    paths[i - 1].weight.value() <= paths[i].weight.value() + 1e-9,
                    "Path {} (weight {}) > Path {} (weight {})",
                    i - 1, paths[i - 1].weight.value(),
                    i, paths[i].weight.value()
                );
            }
        }

        /// Beam search returns at least one path for non-empty lattice.
        #[test]
        fn beam_returns_at_least_one(
            mut lattice in arb_tropical_lattice(2, 2)
        ) {
            let paths = beam_search(&mut lattice, 10);
            prop_assert!(!paths.is_empty());
        }

        /// Wide beam search finds optimal path.
        #[test]
        fn beam_wide_finds_optimal(
            mut lattice in arb_diamond_lattice(3)
        ) {
            use crate::path::viterbi;

            let viterbi_result = viterbi(&mut lattice);
            // Use very wide beam to ensure optimal is found
            let beam_paths = beam_search(&mut lattice, 100);

            prop_assert!(viterbi_result.success);
            prop_assert!(!beam_paths.is_empty());

            // First beam path should match Viterbi
            let diff = (viterbi_result.path.weight.value() - beam_paths[0].weight.value()).abs();
            prop_assert!(diff < 1e-9, "Beam first {} != Viterbi {}",
                         beam_paths[0].weight.value(), viterbi_result.path.weight.value());
        }

        /// Beam search respects max_results config.
        #[test]
        fn beam_respects_max_results(
            mut lattice in arb_diamond_lattice(4),  // 2^4 = 16 paths
            max_results in 1usize..10
        ) {
            let config = BeamSearchConfig::new(100).with_max_results(max_results);
            let paths = beam_search_with_config(&mut lattice, config);
            prop_assert!(paths.len() <= max_results);
        }

        /// All beam paths are complete.
        #[test]
        fn beam_paths_complete(
            mut lattice in arb_tropical_lattice(3, 2)
        ) {
            let paths = beam_search(&mut lattice, 10);
            for path in &paths {
                prop_assert!(path.is_complete);
            }
        }

        /// All beam paths have correct length.
        #[test]
        fn beam_paths_correct_length(
            mut lattice in arb_tropical_lattice(4, 2)
        ) {
            let paths = beam_search(&mut lattice, 20);
            for path in &paths {
                prop_assert_eq!(path.len(), 4);
            }
        }

        /// Narrow beam still finds a valid path.
        #[test]
        fn beam_narrow_finds_valid(
            mut lattice in arb_tropical_lattice(3, 2)
        ) {
            let paths = beam_search(&mut lattice, 1);
            prop_assert!(!paths.is_empty());
            prop_assert!(paths[0].is_complete);
        }
    }
}
