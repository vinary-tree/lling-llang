//! Semiring algebra for WFST weight operations.
//!
//! A semiring (K, ⊕, ⊗, 0̄, 1̄) provides the algebraic structure for WFST weights.
//! Different semirings enable different optimization objectives:
//!
//! | Semiring | ⊕ | ⊗ | 0̄ | 1̄ | Use Case |
//! |----------|---|---|---|---|----------|
//! | Tropical | min | + | ∞ | 0 | Shortest path |
//! | Log | log-add | + | ∞ | 0 | Probabilities (log space) |
//! | Probability | + | × | 0 | 1 | Probabilities (direct) |
//! | Boolean | OR | AND | false | true | Unweighted |
//! | Product | component | component | (0̄,0̄) | (1̄,1̄) | Multi-objective |
//! | Lexicographic | lex-min | component | (0̄,0̄) | (1̄,1̄) | Multi-level priority |
//! | String | lcp/lcs | concat | ∞ | ε | Label accumulation |
//! | Expectation | + | product-rule | (0,0) | (1,0) | Expected values |
//!
//! Implementations are grouped by character into four sub-modules:
//! [`basic`] (numeric/boolean), [`string_kind`] (string- and set-typed),
//! [`algebraic`] (compound / loss-augmented), and [`signed`] (signed-weight
//! semirings with fallible star). The top-level prelude re-exports each
//! concrete weight type for back-compat.

pub mod algebraic;
pub mod basic;
pub mod signed;
pub mod string_kind;
pub mod traits;

pub use algebraic::{
    lexicographic3, lexicographic4, quantized, ExpectationWeight, GodelWeight, Lexicographic3,
    Lexicographic4, LexicographicWeight, PowerWeight, ProductWeight,
};
pub use basic::{BoolWeight, CountWeight, LogWeight, ProbabilityWeight, TropicalWeight};
pub use signed::{FallibleStarSemiring, SignedTropicalWeight, StarDivergenceError};
pub use string_kind::{
    EditOp, EditOpCounts, EditSequence, EditWeight, EditWeightBuilder, FeatureSetWeight,
    LeftStringWeight, RightStringWeight, SetWeight, StrSetWeight, StringSetWeight,
};
pub use traits::{
    CommutativeTimesSemiring, DivisibleSemiring, HashableSemiring, IdempotentSemiring,
    KClosedSemiring, NonnegativeSemiring, NumericalWeight, QuantizableSemiring, Semiring,
    StarSemiring, StochasticSemiring, TotallyOrderedSemiring, WeaklyLeftDivisibleSemiring,
    ZeroSumFreeSemiring,
};
