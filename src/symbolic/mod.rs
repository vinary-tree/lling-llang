//! Symbolic-automata + algebra-tower core (hoisted from prattail, Task #21 / ADR-018).
//!
//! The reusable foundation for symbolic verification: effective Boolean algebras
//! (`BooleanAlgebra`), Symbolic Finite Automata/Transducers (SFA/SFT), the
//! Heyting/RejectSafe algebra tower (`Sat3`), `ConstraintTheory`/`TheoryAlgebra`
//! (the generic solver bridge), behavioral algebra (μ-calculus over an LTS), and
//! Presburger arithmetic. prattail re-exports these for source compatibility; pgmcp,
//! the constrained decoder, and the WFST sidecar depend on lling-llang for them.
//!
//! Hoisted verbatim from prattail; the prattail-grammar-specific glue
//! (`analyze_from_bundle`, `SymbolicCompiler`, the `SyntaxItemSpec`/`pipeline`
//! adapters) stays in prattail, which re-exports this core.
#![allow(missing_docs)] // hoisted verbatim; prattail's doc posture (no missing_docs gate) travels with it

pub mod algebra_tower;
pub mod any_algebra;
pub mod behavioral_algebra;
pub mod behavioral_pred;
pub mod bisimulation;
pub mod collection_algebra;
pub mod kat_algebra;
pub mod lattice_theory;
pub mod logict;
/// SMT-backed `ConstraintTheory` (Z3 library, in-process) — Task #22 §4-B. Makes
/// `TheoryAlgebra<Z3Theory>` a `BooleanAlgebra`, giving symbolic automata SMT-theory
/// guards (bool / linear integer arithmetic / bitvectors) with a sound `Sat3` channel
/// for solver `Unknown`.
pub mod logict_smt;
pub mod ordered_field;
pub mod presburger;
pub mod product_nary;
pub mod regex_sfa;
pub mod sfa;
pub mod sft;
pub mod string_algebra;
pub mod sym_tree;
pub mod sym_tree_transducer;

// Re-export the SFA core at the `symbolic` root so siblings and downstreams write
// `crate::symbolic::BooleanAlgebra` (resolving here), exactly as prattail did.
pub use sfa::*;

// The KAT→BooleanAlgebra adapter lived at the `symbolic` root in prattail's
// monolithic `symbolic.rs`; re-export it here so `crate::symbolic::KatBooleanAlgebra`
// (and `eval_test_public`) still resolve after the kat-adapter split.
pub use kat_algebra::{eval_test_public, KatBooleanAlgebra};
