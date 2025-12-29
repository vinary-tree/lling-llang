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
//! | String | lcp/lcs | concat | ∞ | ε | Label accumulation |
//! | Expectation | + | product-rule | (0,0) | (1,0) | Expected values |

mod traits;
mod tropical;
mod log;
mod boolean;
mod product;
mod probability;
mod string;
mod expectation;
mod power;

pub use traits::{Semiring, DivisibleSemiring, StarSemiring, NumericalWeight};
pub use tropical::TropicalWeight;
pub use log::LogWeight;
pub use boolean::BoolWeight;
pub use product::ProductWeight;
pub use probability::ProbabilityWeight;
pub use string::{LeftStringWeight, RightStringWeight};
pub use expectation::ExpectationWeight;
pub use power::PowerWeight;
