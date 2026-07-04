//! Beam search for approximate path extraction.

use smallvec::SmallVec;

use rustc_hash::FxHashMap;

use super::adjacency::{
    best_suffix_distances, compare_weights, edge_adjacency, node_index, path_priority,
};
use crate::backend::{LatticeBackend, VocabId};
use crate::lattice::{EdgeId, Lattice, LatticePath, NodeId};
use crate::semiring::Semiring;

type WordSignature = SmallVec<[VocabId; 16]>;

fn bounded_capacity(requested: usize, available_items: usize) -> usize {
    requested.min(available_items.max(1)).max(1)
}

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
    /// Best possible complete-path priority through this node.
    priority: W,
}

impl<W: Semiring> Hypothesis<W> {
    fn new(start: NodeId, priority: W) -> Self {
        Self {
            node: start,
            edges: SmallVec::new(),
            weight: W::one(),
            priority,
        }
    }

    fn extend(&self, edge_id: EdgeId, target: NodeId, weight: W, priority: W) -> Self {
        let mut new_edges = self.edges.clone();
        new_edges.push(edge_id);
        Self {
            node: target,
            edges: new_edges,
            weight,
            priority,
        }
    }

    /// Extend by taking ownership (avoids clone for the last extension).
    fn extend_move(mut self, edge_id: EdgeId, target: NodeId, weight: W, priority: W) -> Self {
        self.edges.push(edge_id);
        self.node = target;
        self.weight = weight;
        self.priority = priority;
        self
    }

    fn into_lattice_path(self) -> LatticePath<W> {
        let mut path = LatticePath::with_weight(self.weight);
        path.edges = self.edges;
        path.mark_complete();
        path
    }
}

fn compare_hypotheses<W: Semiring>(a: &Hypothesis<W>, b: &Hypothesis<W>) -> std::cmp::Ordering {
    compare_weights(&a.priority, &b.priority)
        .then_with(|| compare_weights(&a.weight, &b.weight))
        .then_with(|| a.edges.cmp(&b.edges))
        .then_with(|| a.node.cmp(&b.node))
}

fn compare_paths<W: Semiring>(a: &LatticePath<W>, b: &LatticePath<W>) -> std::cmp::Ordering {
    compare_weights(&a.weight, &b.weight).then_with(|| a.edges.cmp(&b.edges))
}

fn retain_best_hypotheses<W: Semiring>(hypotheses: &mut Vec<Hypothesis<W>>, beam_width: usize) {
    if hypotheses.len() <= beam_width {
        return;
    }

    hypotheses.select_nth_unstable_by(beam_width, compare_hypotheses);
    hypotheses.truncate(beam_width);
}

fn truncate_best_paths<W: Semiring>(paths: &mut Vec<LatticePath<W>>, max_results: usize) {
    if paths.len() > max_results {
        paths.select_nth_unstable_by(max_results, compare_paths);
        paths.truncate(max_results);
    }
}

fn retain_best_paths<W: Semiring>(paths: &mut Vec<LatticePath<W>>, max_results: usize) {
    truncate_best_paths(paths, max_results);
    paths.sort_by(compare_paths);
}

fn word_signature<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
    edge_ids: &[EdgeId],
) -> WordSignature {
    let mut signature = SmallVec::with_capacity(edge_ids.len());
    for &edge_id in edge_ids {
        if let Some(edge) = lattice.edge(edge_id) {
            signature.push(edge.label);
        }
    }
    signature
}

fn rebuild_completed_signatures<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
    completed: &[LatticePath<W>],
    signatures: &mut FxHashMap<WordSignature, usize>,
) {
    signatures.clear();
    signatures.reserve(completed.len());
    for (index, path) in completed.iter().enumerate() {
        signatures.insert(word_signature(lattice, &path.edges), index);
    }
}

fn retain_best_completed<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
    completed: &mut Vec<LatticePath<W>>,
    completed_signatures: &mut FxHashMap<WordSignature, usize>,
    config: &BeamSearchConfig,
) {
    if completed.len() <= config.max_results {
        return;
    }

    truncate_best_paths(completed, config.max_results);
    if !config.allow_duplicates {
        rebuild_completed_signatures(lattice, completed, completed_signatures);
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
/// O(V + E + P) where P is the number of generated partial hypotheses.
/// Each beam-pruning step uses linear-time top-k selection rather than sorting
/// the full frontier. Acyclic lattices rank partial hypotheses by exact best
/// suffix cost, so pruning uses the best possible complete-path cost through
/// each hypothesis rather than only its prefix cost.
///
/// # Space Complexity
///
/// O(V + E + (beam_width + max_results) × path_length) for suffix costs,
/// adjacency, active hypotheses, and retained completed hypotheses.
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
    if config.beam_width == 0 || config.max_results == 0 {
        return Vec::new();
    }

    let start = lattice.start();
    let end = lattice.end();
    let lattice_work_bound = lattice.num_edges().saturating_add(1);
    if node_index(start, lattice.num_nodes()).is_none()
        || node_index(end, lattice.num_nodes()).is_none()
    {
        return Vec::new();
    }

    // Handle empty lattice
    if lattice.is_empty() {
        if start == end {
            let mut path = LatticePath::new();
            path.mark_complete();
            return vec![path];
        }
        return Vec::new();
    }

    let Some(adjacency) = edge_adjacency(lattice) else {
        return Vec::new();
    };
    let Some(suffix_best) = best_suffix_distances(lattice, &adjacency) else {
        return Vec::new(); // Cycle detected
    };
    let Some(start_priority) = path_priority(Some(&suffix_best), adjacency.len(), start, W::one())
    else {
        return Vec::new();
    };

    // Initialize beam with start node
    let beam_capacity = bounded_capacity(config.beam_width, lattice_work_bound);
    let result_capacity = bounded_capacity(config.max_results, lattice_work_bound);
    let mut current_beam: Vec<Hypothesis<W>> = Vec::with_capacity(1);
    current_beam.push(Hypothesis::new(start, start_priority));
    let mut completed: Vec<LatticePath<W>> = Vec::with_capacity(result_capacity);
    let mut completed_signatures: FxHashMap<WordSignature, usize> = FxHashMap::default();
    if !config.allow_duplicates {
        completed_signatures.reserve(result_capacity);
    }

    // Process until all hypotheses complete or the bounded beam is empty.  Do
    // not stop at max_results: a cheaper partial hypothesis can complete later.
    while !current_beam.is_empty() {
        let mut next_beam: Vec<Hypothesis<W>> = Vec::with_capacity(
            beam_capacity
                .saturating_mul(2)
                .min(lattice_work_bound.max(1)),
        );

        // Expand each hypothesis
        for hyp in current_beam.drain(..) {
            // Check if hypothesis reached the end
            if hyp.node == end {
                let path = hyp.into_lattice_path();
                if config.allow_duplicates {
                    completed.push(path);
                } else {
                    let signature = word_signature(lattice, &path.edges);
                    if let Some(&existing_index) = completed_signatures.get(&signature) {
                        if compare_weights(&path.weight, &completed[existing_index].weight).is_lt()
                        {
                            completed[existing_index] = path;
                        }
                    } else {
                        completed_signatures.insert(signature, completed.len());
                        completed.push(path);
                    }
                }
                retain_best_completed(lattice, &mut completed, &mut completed_signatures, &config);
                continue;
            }

            // Expand outgoing edges - use move for the last edge to avoid one clone
            let Some(edge_ids) =
                node_index(hyp.node, adjacency.len()).and_then(|node_idx| adjacency.get(node_idx))
            else {
                continue;
            };
            let mut edges_iter = edge_ids.iter().filter_map(|&edge_id| lattice.edge(edge_id));
            if let Some(first_edge) = edges_iter.next() {
                let mut last_edge = (first_edge.id, first_edge.target, first_edge.weight);

                for edge in edges_iter {
                    // Process the previous edge with clone (more edges follow)
                    let weight = hyp.weight.times(&last_edge.2);
                    if let Some(priority) =
                        path_priority(Some(&suffix_best), adjacency.len(), last_edge.1, weight)
                    {
                        let extended = hyp.extend(last_edge.0, last_edge.1, weight, priority);
                        next_beam.push(extended);
                    }
                    last_edge = (edge.id, edge.target, edge.weight);
                }

                // Process the last edge with move (no more edges)
                let weight = hyp.weight.times(&last_edge.2);
                if let Some(priority) =
                    path_priority(Some(&suffix_best), adjacency.len(), last_edge.1, weight)
                {
                    let extended = hyp.extend_move(last_edge.0, last_edge.1, weight, priority);
                    next_beam.push(extended);
                }
            }
        }

        // Prune beam to top beam_width hypotheses
        if next_beam.len() > config.beam_width {
            retain_best_hypotheses(&mut next_beam, config.beam_width);
        }

        current_beam = next_beam;
    }

    retain_best_paths(&mut completed, config.max_results);

    completed
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

    fn duplicate_word_lattice() -> Lattice<TropicalWeight, HashMapBackend> {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(2.0), EdgeMetadata::default());
        builder.add_correction(1, 3, "b", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 2, "a", TropicalWeight::new(0.5), EdgeMetadata::default());
        builder.add_correction(2, 3, "b", TropicalWeight::new(1.5), EdgeMetadata::default());

        builder.build(3)
    }

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
    fn test_beam_search_rejects_invalid_start_or_end() {
        let mut lattice = lattice_with_invalid_start();
        let paths = beam_search(&mut lattice, 10);

        assert!(paths.is_empty());
    }

    #[test]
    fn test_beam_search_rejects_malformed_target() {
        let mut lattice = lattice_with_malformed_target();
        let paths = beam_search(&mut lattice, 10);

        assert!(paths.is_empty());
    }

    #[test]
    fn test_beam_search_uses_edges_when_outgoing_cache_is_stale() {
        let mut lattice = lattice_with_stale_outgoing();
        let paths = beam_search(&mut lattice, 10);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].weight.value(), 1.0);
        assert_eq!(paths[0].to_words(&lattice), vec!["a"]);
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
    fn test_beam_search_exhausts_frontier_before_max_results_truncation() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(
            0,
            2,
            "expensive",
            TropicalWeight::new(10.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            0,
            1,
            "cheap",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            1,
            2,
            "finish",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );

        let mut lattice = builder.build(2);
        let config = BeamSearchConfig::new(2).with_max_results(1);
        let paths = beam_search_with_config(&mut lattice, config);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].to_words(&lattice), vec!["cheap", "finish"]);
        assert_eq!(paths[0].weight.value(), 0.0);
    }

    #[test]
    fn test_beam_search_completed_pruning_keeps_late_best_path() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        for index in 0..8 {
            builder.add_correction(
                0,
                2,
                &format!("early{index}"),
                TropicalWeight::new(10.0 + index as f64),
                EdgeMetadata::default(),
            );
        }
        builder.add_correction(
            0,
            1,
            "late",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            1,
            2,
            "best",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );

        let mut lattice = builder.build(2);
        let config = BeamSearchConfig::new(16).with_max_results(1);
        let paths = beam_search_with_config(&mut lattice, config);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].to_words(&lattice), vec!["late", "best"]);
        assert_eq!(paths[0].weight.value(), 0.0);
    }

    #[test]
    fn test_beam_search_uses_suffix_priority_for_negative_dag_edges() {
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
        let config = BeamSearchConfig::new(1).with_max_results(1);
        let paths = beam_search_with_config(&mut lattice, config);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].weight.value(), -10.0);
        assert_eq!(
            paths[0].to_words(&lattice),
            vec!["expensive-prefix", "large-credit"]
        );
    }

    #[test]
    fn test_beam_search_zero_limits_return_empty() {
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let mut empty_lattice = builder.build(0);

        let no_results = BeamSearchConfig::new(10).with_max_results(0);
        assert!(beam_search_with_config(&mut empty_lattice, no_results).is_empty());

        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        builder.add_correction(
            0,
            1,
            "hello",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );
        let mut lattice = builder.build(1);

        assert!(beam_search(&mut lattice, 0).is_empty());
    }

    #[test]
    fn test_beam_search_large_config_does_not_overallocate() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        builder.add_correction(
            0,
            1,
            "hello",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );
        let mut lattice = builder.build(1);
        let config = BeamSearchConfig::new(usize::MAX).with_max_results(usize::MAX);

        let paths = beam_search_with_config(&mut lattice, config);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].to_words(&lattice), vec!["hello"]);
    }

    #[test]
    fn test_beam_search_deduplicates_word_sequences_by_default() {
        let mut lattice = duplicate_word_lattice();

        let paths = beam_search(&mut lattice, 10);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].to_words(&lattice), vec!["a", "b"]);
        assert_eq!(paths[0].weight.value(), 2.0);
    }

    #[test]
    fn test_beam_search_allows_duplicate_word_sequences_when_configured() {
        let mut lattice = duplicate_word_lattice();
        let config = BeamSearchConfig::new(10).with_duplicates(true);

        let paths = beam_search_with_config(&mut lattice, config);

        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].to_words(&lattice), vec!["a", "b"]);
        assert_eq!(paths[1].to_words(&lattice), vec!["a", "b"]);
        assert_eq!(paths[0].weight.value(), 2.0);
        assert_eq!(paths[1].weight.value(), 3.0);
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

        #[test]
        fn beam_wide_matches_viterbi(
            mut lattice in arb_diamond_lattice(3)
        ) {
            let viterbi_result = super::super::viterbi::viterbi(&mut lattice);
            let config = BeamSearchConfig::new(100).with_duplicates(true);
            let beam_paths = beam_search_with_config(&mut lattice, config);

            prop_assert!(viterbi_result.success);
            prop_assert!(!beam_paths.is_empty());

            let diff = (viterbi_result.path.weight.value() - beam_paths[0].weight.value()).abs();
            prop_assert!(
                diff < 1e-9,
                "Beam first {} != Viterbi {}",
                beam_paths[0].weight.value(),
                viterbi_result.path.weight.value()
            );
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
