//! Basic numeric/boolean semirings used by most WFST algorithms.
//!
//! - [`BoolWeight`]: OR/AND, unweighted recognition.
//! - [`CountWeight`]: integer multiplicity.
//! - [`TropicalWeight`]: min/+, shortest path.
//! - [`LogWeight`]: log-add/+, probabilities in log space.
//! - [`ProbabilityWeight`]: +/×, probabilities in direct space.

pub mod boolean;
pub mod count;
pub mod log;
pub mod probability;
pub mod tropical;

pub use boolean::BoolWeight;
pub use count::CountWeight;
pub use log::LogWeight;
pub use probability::ProbabilityWeight;
pub use tropical::TropicalWeight;
