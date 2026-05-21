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

mod lazy;
pub mod rational;
mod state;
pub mod synchronize;
mod traits;
mod transition;
pub mod unary;
mod vector;

pub use lazy::{LazyState, LazyWfstWrapper, StateSource};
pub use state::WfstState;
pub use traits::{CachePolicy, LazyWfst, MutableWfst, Wfst};
pub use transition::WeightedTransition;
pub use vector::{VectorWfst, VectorWfstBuilder};

// Rational operations (Union, Concatenation, Closure)
pub use rational::{
    closure, closure_plus, concat, union, ClosureSource, ClosureWfst, ConcatSource, ConcatWfst,
    UnionSource, UnionWfst,
};

// Unary operations (Invert, Project, Reverse)
pub use unary::{
    invert, project_input, project_output, reverse, InvertSource, InvertWfst, ProjectInputWfst,
    ProjectOutputWfst, ProjectSource,
};

// Synchronization operation
pub use synchronize::{
    compute_max_delay, has_bounded_delay, synchronize, synchronize_bounded, MutableSyncSource,
    StringDelay, SyncSource, SyncState, SyncWfst,
};

/// State identifier for WFST states.
///
/// Uses `u32` for compact storage while supporting millions of states.
pub type StateId = u32;

/// Sentinel value indicating no state (similar to null).
pub const NO_STATE: StateId = StateId::MAX;
