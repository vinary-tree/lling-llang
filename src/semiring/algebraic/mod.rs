//! Algebraic / compound semirings layered on top of the basic semirings.
//!
//! - [`GodelWeight`]: Gödel encoding of label sequences.
//! - [`PowerWeight`]: power-set / loss-augmented semiring.
//! - [`ExpectationWeight`]: expectation-rule semiring for E-step computations.
//! - [`LexicographicWeight`] (+ `Lexicographic3`, `Lexicographic4`, helpers):
//!   ordered tuple semirings.
//! - [`ProductWeight`]: independent product of two semirings.
//! - [`quantized`]: weight quantization helpers.

pub mod expectation;
pub mod godel;
pub mod lexicographic;
pub mod power;
pub mod product;
pub mod quantized;

pub use expectation::ExpectationWeight;
pub use godel::GodelWeight;
pub use lexicographic::{
    lexicographic3, lexicographic4, Lexicographic3, Lexicographic4, LexicographicWeight,
};
pub use power::PowerWeight;
pub use product::ProductWeight;
