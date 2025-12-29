//! Lattice data structures for representing correction alternatives.
//!
//! A lattice is a weighted directed acyclic graph (DAG) where:
//! - Nodes represent positions in the input sequence
//! - Edges represent token alternatives with weights
//! - Paths from start to end represent complete correction sequences
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         Lattice Structure                               │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │                                                                         │
//! │  Input: "teh quik fox"                                                  │
//! │                                                                         │
//! │                    ┌───the(0.5)───┐                                     │
//! │         start ────►│              ├───quick(0.5)───►fox(0.0)──►end     │
//! │                    └───teh(0.0)───┤               ▲                     │
//! │                                   └───quik(0.0)───┘                     │
//! │                                                                         │
//! │  Best path: "the quick fox" (weight: 1.0)                              │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust
//! use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
//! use lling_llang::backend::HashMapBackend;
//! use lling_llang::semiring::TropicalWeight;
//!
//! let backend = HashMapBackend::new();
//! let mut builder = LatticeBuilder::<TropicalWeight, _>::new(backend);
//!
//! // Add correction alternatives
//! builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::default());
//! builder.add_correction(0, 1, "teh", TropicalWeight::new(0.0), EdgeMetadata::original());
//!
//! let lattice = builder.build(1);
//! ```

mod types;
mod lattice;
mod builder;
mod algorithms;
mod iterator;

pub use types::{NodeId, EdgeId, Node, Edge, EdgeMetadata};
pub use lattice::Lattice;
pub use builder::LatticeBuilder;
pub use iterator::{PathIterator, LatticePath, LatticePathExt};
