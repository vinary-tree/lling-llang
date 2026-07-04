//! Path extraction algorithms for lattices.
//!
//! This module provides efficient algorithms for finding optimal paths
//! through lattices:
//!
//! - **Viterbi**: Find the single best path using dynamic programming
//! - **N-best**: Extract the top-k paths using lazy enumeration
//! - **Beam search**: Approximate best paths with bounded memory
//!
//! # Algorithm Selection
//!
//! | Algorithm | Complexity | Memory | Use Case |
//! |-----------|------------|--------|----------|
//! | Viterbi | O(V + E) | O(V) | Single best path |
//! | N-best | O(V + E + P log P) | O(V + E + P × path_len) | Exact top-k paths |
//! | Beam | O(V + E + P) | O(V + E + beam_width × path_len) | Approximate top-k |
//!
//! # Example
//!
//! ```rust
//! use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
//! use lling_llang::backend::HashMapBackend;
//! use lling_llang::semiring::TropicalWeight;
//! use lling_llang::path::{viterbi, nbest, beam_search};
//!
//! let backend = HashMapBackend::new();
//! let mut builder = LatticeBuilder::new(backend);
//!
//! builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::default());
//! builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
//!
//! let mut lattice = builder.build(1);
//!
//! // Best path
//! let best = viterbi(&mut lattice);
//!
//! // Top 5 paths
//! let top5 = nbest(&mut lattice, 5);
//!
//! // Beam search with width 10
//! let approx = beam_search(&mut lattice, 10);
//! ```

mod adjacency;
mod beam;
mod nbest;
mod viterbi;

pub use beam::{beam_search, BeamSearchConfig};
pub use nbest::{nbest, NBestIterator};
pub use viterbi::{viterbi, ViterbiResult};

#[cfg(test)]
mod cross_algorithm_tests {
    use crate::test_utils::{arb_diamond_lattice, arb_tropical_lattice};
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Viterbi finds a path no heavier than any exact N-best path.
        #[test]
        fn viterbi_weight_is_no_greater_than_nbest_paths(
            mut lattice in arb_diamond_lattice(3)
        ) {
            let viterbi_result = super::viterbi::viterbi(&mut lattice);
            prop_assert!(viterbi_result.success);
            let viterbi_weight = viterbi_result.path.weight.value();

            let all_paths = super::nbest::nbest(&mut lattice, 100);
            for path in &all_paths {
                prop_assert!(
                    viterbi_weight <= path.weight.value() + 1e-9,
                    "Viterbi weight {} > path weight {}",
                    viterbi_weight,
                    path.weight.value()
                );
            }
        }

        /// N-best's first exact path agrees with Viterbi's best path.
        #[test]
        fn nbest_first_path_matches_viterbi(
            mut lattice in arb_tropical_lattice(3, 2)
        ) {
            let viterbi_result = super::viterbi::viterbi(&mut lattice);
            let nbest_paths = super::nbest::nbest(&mut lattice, 1);

            prop_assert!(viterbi_result.success);
            prop_assert_eq!(nbest_paths.len(), 1);

            let diff = (viterbi_result.path.weight.value() - nbest_paths[0].weight.value()).abs();
            prop_assert!(
                diff < 1e-9,
                "Weight mismatch: viterbi={}, nbest={}",
                viterbi_result.path.weight.value(),
                nbest_paths[0].weight.value()
            );
        }

        /// A sufficiently wide beam agrees with Viterbi on the best path.
        #[test]
        fn wide_beam_first_path_matches_viterbi(
            mut lattice in arb_diamond_lattice(3)
        ) {
            let viterbi_result = super::viterbi::viterbi(&mut lattice);
            let beam_paths = super::beam::beam_search(&mut lattice, 100);

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
    }
}
