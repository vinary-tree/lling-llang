//! Backend adapters for lattice storage and vocabulary management.
//!
//! This module provides a trait-based abstraction over different dictionary
//! and storage backends. The design supports:
//!
//! - **Generic backends**: Work with any hashmap-like storage
//! - **PathMap backends**: Optimized for structural sharing (via `f1r3fly` feature)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                     LatticeBackend Trait                                │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │                                                                         │
//! │  ┌──────────────────────┐    ┌──────────────────────────────────────┐  │
//! │  │  HashMapBackend      │    │  PathMapBackend (#[cfg(f1r3fly)])    │  │
//! │  │  - Simple vocabulary │    │  - Structural sharing                │  │
//! │  │  - No sharing        │    │  - Copy-on-write                     │  │
//! │  └──────────────────────┘    │  - S-expression paths                │  │
//! │                              └──────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! For most use cases, use [`HashMapBackend`] which provides simple vocabulary
//! interning without external dependencies:
//!
//! ```rust
//! use lling_llang::backend::{LatticeBackend, HashMapBackend};
//!
//! let mut backend = HashMapBackend::new();
//! let id1 = backend.intern("hello");
//! let id2 = backend.intern("world");
//! let id3 = backend.intern("hello"); // Returns same id as id1
//!
//! assert_eq!(id1, id3);
//! assert_eq!(backend.lookup(id1), Some("hello"));
//! ```

mod hashmap;
mod traits;

pub use hashmap::HashMapBackend;
pub use traits::{LatticeBackend, SharingBackend, VocabId};

#[cfg(feature = "f1r3fly")]
mod pathmap;

#[cfg(feature = "f1r3fly")]
pub use pathmap::{PathId, PathMapBackend, PathMapSharingBackend};
