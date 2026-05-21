//! Path extraction algorithms for lattices.
//!
//! This module provides efficient algorithms for finding optimal paths
//! through lattices:
//!
//! - **Viterbi**: Find the single best path using dynamic programming
//! - **N-best**: Extract the top-k paths using lazy enumeration
//! - **Beam search**: Approximate best paths with bounded memory
//!
//! # Algorithm Selection
//!
//! | Algorithm | Complexity | Memory | Use Case |
//! |-----------|------------|--------|----------|
//! | Viterbi | O(V + E) | O(V) | Single best path |
//! | N-best | O(k log k) | O(k × path_len) | Exact top-k paths |
//! | Beam | O(V × beam_width) | O(beam_width) | Approximate top-k |
//!
//! # Example
//!
//! ```rust
//! use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
//! use lling_llang::backend::HashMapBackend;
//! use lling_llang::semiring::TropicalWeight;
//! use lling_llang::path::{viterbi, nbest, beam_search};
//!
//! let backend = HashMapBackend::new();
//! let mut builder = LatticeBuilder::new(backend);
//!
//! builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::default());
//! builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
//!
//! let mut lattice = builder.build(1);
//!
//! // Best path
//! let best = viterbi(&mut lattice);
//!
//! // Top 5 paths
//! let top5 = nbest(&mut lattice, 5);
//!
//! // Beam search with width 10
//! let approx = beam_search(&mut lattice, 10);
//! ```

mod beam;
mod nbest;
mod viterbi;

pub use beam::{beam_search, BeamSearchConfig};
pub use nbest::{nbest, NBestIterator};
pub use viterbi::{viterbi, ViterbiResult};
