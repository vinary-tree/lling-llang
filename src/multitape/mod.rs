//! Multi-Tape Weighted Finite State Transducers.
//!
//! This module implements multi-tape WFSTs for synchronized transduction
//! over multiple input/output streams. Multi-tape transducers generalize
//! standard two-tape transducers to k tapes.
//!
//! # Mathematical Definition
//!
//! A k-tape weighted transducer is a tuple T = (Q, Σ₁,...,Σₖ, q₀, F, E, ρ) where:
//! - Q: Finite set of states
//! - Σ₁,...,Σₖ: Tape alphabets (each tape has its own alphabet)
//! - q₀: Initial state
//! - F ⊆ Q: Final states
//! - E: Transitions of the form (q, (a₁,...,aₖ), w, q') where aᵢ ∈ Σᵢ ∪ {ε}
//! - ρ: Final weight function
//!
//! # Transition Format
//!
//! ```text
//! (state, [label₁, label₂, ..., labelₖ], weight, next_state)
//! ```
//!
//! Each label can be `Some(symbol)` or `None` (epsilon).
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::multitape::{MultiTapeWfstBuilder, MultiTapeLabel};
//! use lling_llang::semiring::TropicalWeight;
//!
//! // Create a 3-tape transducer for word alignment
//! let mut builder = MultiTapeWfstBuilder::<char, TropicalWeight, 3>::new();
//!
//! let s0 = builder.add_state();
//! builder.set_start(s0);
//! builder.set_final(s0, TropicalWeight::one());
//!
//! // Transition: source word, target word, alignment tag
//! builder.add_transition(s0, s0, ['h', 'h', 'A'], TropicalWeight::one());
//! builder.add_transition(s0, s0, ['e', 'e', 'A'], TropicalWeight::one());
//! builder.add_transition(s0, s0, ['l', 'l', 'A'], TropicalWeight::one());
//!
//! let mt = builder.build();
//! ```
//!
//! # Applications
//!
//! - **Word alignment**: (source, target, alignment tape)
//! - **Multi-stream ASR**: (audio features, visual features, text)
//! - **Morphological analysis**: (surface form, lemma, morphological tags)
//! - **Parallel corpus processing**: Multiple language tapes

mod builder;
mod label;
mod project;
mod synchronize;
mod traits;
mod transition;
mod vector;

pub use builder::MultiTapeWfstBuilder;
pub use label::MultiTapeLabel;
pub use project::{ProjectSource, ProjectedWfst};
pub use synchronize::{SyncConfig, SynchronizedMultiTape, TapeDelay};
pub use traits::MultiTapeWfst;
pub use transition::MultiTapeTransition;
pub use vector::{MultiTapeState, VectorMultiTapeWfst};

// StateId re-exported from wfst

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_module_imports() {
        // Verify module structure compiles
        let _builder: MultiTapeWfstBuilder<char, TropicalWeight, 2> = MultiTapeWfstBuilder::new();
    }
}
