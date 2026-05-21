//! Semirings whose carrier is a string- or set-like type (used for label
//! accumulation, edit-distance modelling, and feature aggregation).
//!
//! - [`LeftStringWeight`] / [`RightStringWeight`]: prefix/suffix string semirings.
//! - [`EditWeight`] + supporting types: edit-sequence semiring.
//! - [`SetWeight`], [`StrSetWeight`], [`StringSetWeight`], [`FeatureSetWeight`]:
//!   set semirings used for non-numeric weight algebras.

pub mod edit;
pub mod set;
pub mod string;

pub use edit::{EditOp, EditOpCounts, EditSequence, EditWeight, EditWeightBuilder};
pub use set::{FeatureSetWeight, SetWeight, StrSetWeight, StringSetWeight};
pub use string::{LeftStringWeight, RightStringWeight};
