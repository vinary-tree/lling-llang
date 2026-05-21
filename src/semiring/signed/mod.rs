//! Signed semirings (semirings whose star operation can diverge on negative
//! weights), plus the fallible-star trait that distinguishes them from
//! conventional [`StarSemiring`]s.

pub mod signed_tropical;

pub use signed_tropical::{FallibleStarSemiring, SignedTropicalWeight, StarDivergenceError};
