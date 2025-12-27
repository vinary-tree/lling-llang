//! Semiring algebra for WFST weight operations.
//!
//! A semiring (K, ⊕, ⊗, 0̄, 1̄) provides the algebraic structure for WFST weights.
//! Different semirings enable different optimization objectives:
//!
//! | Semiring | ⊕ | ⊗ | 0̄ | 1̄ | Use Case |
//! |----------|---|---|---|---|----------|
//! | Tropical | min | + | ∞ | 0 | Shortest path |
//! | Log | log-add | + | -∞ | 0 | Probabilities |
//! | Boolean | OR | AND | false | true | Unweighted |
//! | Product | component | component | (0̄,0̄) | (1̄,1̄) | Multi-objective |

mod traits;
mod tropical;
mod log;
mod boolean;
mod product;

pub use traits::{Semiring, DivisibleSemiring, StarSemiring};
pub use tropical::TropicalWeight;
pub use log::LogWeight;
pub use boolean::BoolWeight;
pub use product::ProductWeight;
