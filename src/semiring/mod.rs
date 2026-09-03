//! Semiring algebra for WFST weight operations.
//!
//! A semiring $`(K,\oplus,\otimes,\bar{0},\bar{1})`$ provides the
//! algebraic structure for WFST weights.
//! Different semirings enable different optimization objectives:
//!
//! | Semiring | $`\oplus`$ | $`\otimes`$ | $`\bar{0}`$ | $`\bar{1}`$ | Use Case |
//! |----------|---|---|---|---|----------|
//! | Arctic | max | + | $`-\infty`$ | 0 | Maximum-score path |
//! | Tropical | min | + | $`+\infty`$ | 0 | Shortest path |
//! | Log | log-add | + | $`+\infty`$ | 0 | Probabilities (log space) |
//! | Probability | + | $`\times`$ | 0 | 1 | Probabilities (direct) |
//! | Boolean | OR | AND | false | true | Unweighted |
//! | Product | component | component | $`(\bar{0},\bar{0})`$ | $`(\bar{1},\bar{1})`$ | Multi-objective |
//! | Lexicographic | lex-min | component | $`(\bar{0},\bar{0})`$ | $`(\bar{1},\bar{1})`$ | Multi-level priority |
//! | String | lcp/lcs | concat | $`+\infty`$ | $`\varepsilon`$ | Label accumulation |
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
pub use basic::{
    ArcticWeight, BoolWeight, CountWeight, LogWeight, ProbabilityWeight, TropicalWeight,
};
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
