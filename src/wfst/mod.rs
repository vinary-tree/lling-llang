//! Weighted Finite State Transducer (WFST) core types and traits.
//!
//! This module provides the fundamental building blocks for WFSTs:
//!
//! - [`StateId`]: Identifier for transducer states
//! - [`WeightedTransition`]: A transition with input/output labels and weight
//! - [`WfstState`]: A state with its transitions and final weight
//! - [`Wfst`]: Core trait for immutable WFST access
//! - [`MutableWfst`]: Trait for constructing WFSTs
//! - [`LazyWfst`]: Trait for on-demand state expansion (avoids state explosion)
//! - [`VectorWfst`]: Eager implementation storing all states in memory
//!
//! # Lazy Evaluation
//!
//! The lazy WFST design is critical for composition operations where the
//! product state space can explode exponentially. Instead of computing all
//! states upfront, lazy WFSTs compute states on-demand during traversal.

mod transition;
mod state;
mod traits;
mod vector;
mod lazy;
pub mod rational;
pub mod unary;
pub mod synchronize;

pub use transition::WeightedTransition;
pub use state::WfstState;
pub use traits::{Wfst, MutableWfst, LazyWfst, CachePolicy};
pub use vector::{VectorWfst, VectorWfstBuilder};
pub use lazy::{LazyState, StateSource, LazyWfstWrapper};

// Rational operations (Union, Concatenation, Closure)
pub use rational::{
    UnionSource, ConcatSource, ClosureSource,
    UnionWfst, ConcatWfst, ClosureWfst,
    union, concat, closure, closure_plus,
};

// Unary operations (Invert, Project, Reverse)
pub use unary::{
    InvertSource, ProjectSource,
    InvertWfst, ProjectInputWfst, ProjectOutputWfst,
    invert, project_input, project_output, reverse,
};

// Synchronization operation
pub use synchronize::{
    StringDelay, SyncState, SyncSource, MutableSyncSource, SyncWfst,
    synchronize, synchronize_bounded, has_bounded_delay, compute_max_delay,
};

/// State identifier for WFST states.
///
/// Uses `u32` for compact storage while supporting millions of states.
pub type StateId = u32;

/// Sentinel value indicating no state (similar to null).
pub const NO_STATE: StateId = StateId::MAX;
