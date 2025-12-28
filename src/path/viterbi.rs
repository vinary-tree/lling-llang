//! Viterbi algorithm for finding the best path.

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticePath, NodeId, EdgeId};
use crate::semiring::Semiring;

/// Result of Viterbi decoding.
#[derive(Clone, Debug)]
pub struct ViterbiResult<W: Semiring> {
    /// The best path through the lattice.
    pub path: LatticePath<W>,
    /// Whether a valid path was found.
    pub success: bool,
}

impl<W: Semiring> ViterbiResult<W> {
    /// Create a successful result.
    fn success(path: LatticePath<W>) -> Self {
        Self { path, success: true }
    }

    /// Create a failed result (no path found).
    fn failure() -> Self {
        Self {
            path: LatticePath::new(),
            success: false,
        }
    }
}

/// Find the best path through a lattice using the Viterbi algorithm.
///
/// Uses dynamic programming in topological order to find the path with
/// the optimal (minimum for TropicalWeight) total weight.
///
/// # Time Complexity
///
/// O(V + E) where V is the number of nodes and E is the number of edges.
///
/// # Space Complexity
///
/// O(V) for storing forward scores and backpointers.
///
/// # Example
///
/// ```rust
/// use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
/// use lling_llang::backend::HashMapBackend;
/// use lling_llang::semiring::TropicalWeight;
/// use lling_llang::path::viterbi;
///
/// let backend = HashMapBackend::new();
/// let mut builder = LatticeBuilder::new(backend);
///
/// builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::default());
/// builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
///
/// let mut lattice = builder.build(1);
/// let result = viterbi(&mut lattice);
///
/// if result.success {
///     let words = result.path.to_words(&lattice);
///     println!("Best path: {:?}", words);
/// }
/// ```
pub fn viterbi<W: Semiring, B: LatticeBackend>(lattice: &mut Lattice<W, B>) -> ViterbiResult<W> {
    // Handle empty lattice
    if lattice.is_empty() {
        // No edges, but start == end is a valid (empty) path
        if lattice.start() == lattice.end() {
            let mut path = LatticePath::new();
            path.mark_complete();
            return ViterbiResult::success(path);
        }
        return ViterbiResult::failure();
    }

    // Get topological order
    let topo_order = match lattice.topological_order() {
        Some(order) => order.to_vec(),
        None => return ViterbiResult::failure(), // Cycle detected
    };

    let n = lattice.num_nodes();
    let start = lattice.start();
    let end = lattice.end();

    // Forward pass: compute best scores
    // (score, backpointer edge, backpointer node)
    let mut best: Vec<Option<(W, EdgeId, NodeId)>> = vec![None; n];

    // Start node has zero cost (semiring one)
    // We use None for start to indicate "no edge taken yet"
    let _start_idx = start.0 as usize;

    // Process in topological order
    for &node_id in &topo_order {
        let node_idx = node_id.0 as usize;

        // Skip nodes that are not reachable from start
        // (except start itself, which we initialize with weight one)
        if node_id != start && best[node_idx].is_none() {
            // Check if this is an unreachable node
            // For start, we don't need a backpointer
        }

        // Get current best score to this node
        let current_score = if node_id == start {
            W::one()
        } else {
            match &best[node_idx] {
                Some((score, _, _)) => *score,
                None => continue, // Not reachable
            }
        };

        // Relax outgoing edges
        for edge in lattice.outgoing_edges(node_id) {
            let target_idx = edge.target.0 as usize;
            let new_score = current_score.times(&edge.weight);

            let update = match &best[target_idx] {
                None => true,
                Some((existing_score, _, _)) => {
                    // Use natural ordering if available, otherwise compare values
                    match new_score.natural_less(existing_score) {
                        Some(true) => true,
                        Some(false) => false,
                        None => {
                            // Fallback: for semirings where natural_less is not defined,
                            // we still need to pick one. Use times identity check.
                            new_score.is_zero() || existing_score.is_zero()
                        }
                    }
                }
            };

            if update {
                best[target_idx] = Some((new_score, edge.id, node_id));
            }
        }
    }

    // Check if end is reachable
    let end_idx = end.0 as usize;
    if end_idx >= n || (start != end && best[end_idx].is_none()) {
        return ViterbiResult::failure();
    }

    // Backward pass: reconstruct path
    let mut edges = Vec::new();
    let mut current = end;

    while current != start {
        let current_idx = current.0 as usize;
        match &best[current_idx] {
            Some((_, edge_id, prev_node)) => {
                edges.push(*edge_id);
                current = *prev_node;
            }
            None => return ViterbiResult::failure(), // Should not happen
        }
    }

    // Reverse to get forward order
    edges.reverse();

    // Compute total weight
    let final_weight = if edges.is_empty() {
        W::one()
    } else {
        best[end_idx].as_ref().map(|(w, _, _)| *w).unwrap_or_else(W::one)
    };

    let mut path = LatticePath::with_weight(final_weight);
    for edge_id in edges {
        path.edges.push(edge_id);
    }
    // Correct the weight (we computed it in best already)
    path.weight = final_weight;
    path.mark_complete();

    ViterbiResult::success(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::{LatticeBuilder, EdgeMetadata};
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_viterbi_simple() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::default());
        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());

        let mut lattice = builder.build(1);
        let result = viterbi(&mut lattice);

        assert!(result.success);
        assert_eq!(result.path.len(), 1);
        assert_eq!(result.path.weight.value(), 0.5); // "the" has lower weight

        let words = result.path.to_words(&lattice);
        assert_eq!(words, vec!["the"]);
    }

    #[test]
    fn test_viterbi_multi_position() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::default());
        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "quick", TropicalWeight::new(0.3), EdgeMetadata::default());
        builder.add_correction(1, 2, "slow", TropicalWeight::new(0.7), EdgeMetadata::default());

        let mut lattice = builder.build(2);
        let result = viterbi(&mut lattice);

        assert!(result.success);
        assert_eq!(result.path.len(), 2);
        assert_eq!(result.path.weight.value(), 0.8); // 0.5 + 0.3

        let words = result.path.to_words(&lattice);
        assert_eq!(words, vec!["the", "quick"]);
    }

    #[test]
    fn test_viterbi_empty_lattice() {
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let mut lattice = builder.build(0);

        let result = viterbi(&mut lattice);

        // Empty lattice with start == end is a valid empty path
        assert!(result.success);
        assert!(result.path.is_empty());
    }

    #[test]
    fn test_viterbi_single_path() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "hello", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "world", TropicalWeight::new(2.0), EdgeMetadata::default());

        let mut lattice = builder.build(2);
        let result = viterbi(&mut lattice);

        assert!(result.success);
        assert_eq!(result.path.len(), 2);
        assert_eq!(result.path.weight.value(), 3.0);
    }

    #[test]
    fn test_viterbi_diamond() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        // Diamond: 0 -> 1 -> 3, 0 -> 2 -> 3
        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 2, "b", TropicalWeight::new(2.0), EdgeMetadata::default());
        builder.add_correction(1, 3, "c", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(2, 3, "d", TropicalWeight::new(0.5), EdgeMetadata::default());

        let mut lattice = builder.build(3);
        let result = viterbi(&mut lattice);

        assert!(result.success);
        assert_eq!(result.path.len(), 2);
        // Best path: a (1.0) + c (1.0) = 2.0
        // Alternative: b (2.0) + d (0.5) = 2.5
        assert_eq!(result.path.weight.value(), 2.0);

        let words = result.path.to_words(&lattice);
        assert_eq!(words, vec!["a", "c"]);
    }

    #[test]
    fn test_viterbi_equal_weights() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "b", TropicalWeight::new(1.0), EdgeMetadata::default());

        let mut lattice = builder.build(1);
        let result = viterbi(&mut lattice);

        assert!(result.success);
        assert_eq!(result.path.len(), 1);
        assert_eq!(result.path.weight.value(), 1.0);
        // Either "a" or "b" is valid (depends on processing order)
    }

    #[test]
    fn test_viterbi_zero_weight() {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "zero", TropicalWeight::new(0.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "one", TropicalWeight::new(1.0), EdgeMetadata::default());

        let mut lattice = builder.build(1);
        let result = viterbi(&mut lattice);

        assert!(result.success);
        assert_eq!(result.path.weight.value(), 0.0);

        let words = result.path.to_words(&lattice);
        assert_eq!(words, vec!["zero"]);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::path::nbest;
    use crate::test_utils::{arb_tropical_lattice, arb_linear_lattice, arb_diamond_lattice};
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Viterbi on a linear lattice finds the only path.
        #[test]
        fn viterbi_linear_finds_only_path(
            mut lattice in arb_linear_lattice(4)
        ) {
            let result = viterbi(&mut lattice);
            prop_assert!(result.success);
            prop_assert_eq!(result.path.len(), 4);
        }

        /// Viterbi on a lattice always succeeds (we generate connected lattices).
        #[test]
        fn viterbi_always_succeeds_on_connected(
            mut lattice in arb_tropical_lattice(3, 2)
        ) {
            let result = viterbi(&mut lattice);
            prop_assert!(result.success);
        }

        /// Viterbi path length matches number of positions.
        #[test]
        fn viterbi_path_length_matches_positions(
            mut lattice in arb_tropical_lattice(4, 3)
        ) {
            let result = viterbi(&mut lattice);
            prop_assert!(result.success);
            prop_assert_eq!(result.path.len(), 4);
        }

        /// Viterbi finds optimal path (weight <= all other paths).
        #[test]
        fn viterbi_finds_optimal(
            mut lattice in arb_diamond_lattice(3)
        ) {
            let viterbi_result = viterbi(&mut lattice);
            prop_assert!(viterbi_result.success);
            let viterbi_weight = viterbi_result.path.weight.value();

            // Get all paths via n-best
            let all_paths = nbest(&mut lattice, 100);

            // Viterbi should find the minimum weight
            for path in &all_paths {
                prop_assert!(
                    viterbi_weight <= path.weight.value() + 1e-9,
                    "Viterbi weight {} > path weight {}",
                    viterbi_weight,
                    path.weight.value()
                );
            }
        }

        /// Viterbi path has non-negative weight (tropical).
        #[test]
        fn viterbi_weight_non_negative(
            mut lattice in arb_tropical_lattice(3, 2)
        ) {
            let result = viterbi(&mut lattice);
            prop_assert!(result.success);
            prop_assert!(result.path.weight.value() >= 0.0);
        }

        /// Viterbi path is marked complete.
        #[test]
        fn viterbi_path_is_complete(
            mut lattice in arb_tropical_lattice(2, 2)
        ) {
            let result = viterbi(&mut lattice);
            prop_assert!(result.success);
            prop_assert!(result.path.is_complete);
        }
    }
}
