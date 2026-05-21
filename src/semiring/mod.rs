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

mod boolean;
mod count;
mod edit;
mod expectation;
mod godel;
mod lexicographic;
mod log;
mod power;
mod probability;
mod product;
pub mod quantized;
mod set;
mod signed_tropical;
mod string;
mod traits;
mod tropical;

pub use boolean::BoolWeight;
pub use count::CountWeight;
pub use edit::{EditOp, EditOpCounts, EditSequence, EditWeight, EditWeightBuilder};
pub use expectation::ExpectationWeight;
pub use godel::GodelWeight;
pub use lexicographic::{
    lexicographic3, lexicographic4, Lexicographic3, Lexicographic4, LexicographicWeight,
};
pub use log::LogWeight;
pub use power::PowerWeight;
pub use probability::ProbabilityWeight;
pub use product::ProductWeight;
pub use set::{FeatureSetWeight, SetWeight, StrSetWeight, StringSetWeight};
pub use signed_tropical::{FallibleStarSemiring, SignedTropicalWeight, StarDivergenceError};
pub use string::{LeftStringWeight, RightStringWeight};
pub use traits::{
    CommutativeTimesSemiring, DivisibleSemiring, IdempotentSemiring, KClosedSemiring,
    NonnegativeSemiring, NumericalWeight, QuantizableSemiring, Semiring, StarSemiring,
    StochasticSemiring, TotallyOrderedSemiring, WeaklyLeftDivisibleSemiring, ZeroSumFreeSemiring,
};
pub use tropical::TropicalWeight;
