//! Composition operators for WFSTs.
//!
//! This module provides lazy composition operators that compute product states
//! on-demand during traversal, avoiding state explosion.
//!
//! # Composition Types
//!
//! | Operator | Description | Use Case |
//! |----------|-------------|----------|
//! | FST ∘ FST | WFST composition | Cascaded transducers |
//! | NFA ∩ FST | NFA intersection | Phonetic matching |
//! | CFG × FST | CFG filtering | Grammar constraints |
//!
//! # Lazy Evaluation
//!
//! All composition operators use lazy evaluation:
//! - Product states computed on first access
//! - Configurable cache policy (CacheAll, Lru, NoCache)
//! - Memory bounded by actual traversal, not theoretical maximum
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::composition::compose;
//! use lling_llang::wfst::VectorWfst;
//!
//! let composed = compose(fst1, fst2);
//! // States computed lazily during iteration
//! for path in composed.accepting_paths() {
//!     println!("{:?}", path);
//! }
//! ```
//!
//! # Materialization
//!
//! For eager access to the full composed FST, use [`materialize`]:
//!
//! ```rust,ignore
//! use lling_llang::composition::{compose, materialize};
//!
//! let lazy = compose(fst1, fst2);
//! let eager: VectorWfst<_, _> = materialize(lazy);
//! ```

mod filter;
mod fst_fst;
mod cfg_fst;
mod materialize;

pub use filter::{EpsilonFilter, EpsilonFilterType, FilterState};
pub use fst_fst::{compose, LazyComposition, ComposedPath, ProductStateId};
pub use cfg_fst::{
    LazyCfgComposition, FilteredLattice, ValidPathIterator,
    ParseState, CompositionStats,
};
pub use materialize::materialize;
