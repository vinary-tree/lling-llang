//! Test utilities for lling-llang.
//!
//! This module provides testing infrastructure for property-based testing
//! using `proptest`, custom assertions, common fixtures, and language
//! equivalence checking.
//!
//! # Modules
//!
//! - [`arbitrary`]: `proptest` strategies for generating WFSTs, lattices, and weights
//! - [`assertions`]: Custom assertion helpers for approximate equality and WFST properties
//! - [`fixtures`]: Pre-built test WFSTs and lattices for common test scenarios
//! - [`language`]: Language equivalence checking for WFSTs
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::test_utils::arbitrary::arb_wfst;
//! use lling_llang::test_utils::assertions::approx_eq;
//! use proptest::prelude::*;
//!
//! proptest! {
//!     #[test]
//!     fn test_wfst_property(fst in arb_wfst::<char, TropicalWeight>(10, 5)) {
//!         // Test properties of randomly generated WFSTs
//!     }
//! }
//! ```

pub mod arbitrary;
pub mod assertions;
pub mod fixtures;
pub mod language;

// Re-export commonly used items
pub use arbitrary::{
    arb_acyclic_wfst,
    arb_acyclic_wfst_tropical,
    arb_deterministic_wfst,
    arb_deterministic_wfst_tropical,
    arb_diamond_lattice,
    arb_label,
    arb_linear_lattice,
    arb_log_weight,
    arb_log_wfst,
    arb_probability_weight,
    // Lattice strategies
    arb_tropical_lattice,
    arb_tropical_weight,
    arb_tropical_wfst,
    arb_wfst,
};
pub use assertions::{
    approx_eq, assert_has_no_epsilon, assert_is_acyclic, assert_is_deterministic,
    assert_wfst_invariants, wfst_approx_eq,
};
pub use fixtures::{
    branching_wfst, cyclic_wfst, diamond_wfst, epsilon_wfst, linear_wfst, single_state_wfst,
};
pub use language::{accepts_string, enumerate_paths, language_eq, path_weights_eq, Path};
