//! Automatic Speech Recognition (ASR) WFST components.
//!
//! This module provides WFST-based components for building speech recognition systems,
//! following the architecture described in Mohri et al.'s work on WFSTs in speech recognition.
//!
//! ## ASR Transducer Cascade
//!
//! The standard ASR pipeline constructs a recognition network as:
//!
//! ```text
//! N = π(min(det(H̃ ∘ det(C̃ ∘ det(L̃ ∘ G)))))
//! ```
//!
//! Where:
//! - **G**: Word-level grammar (n-gram language model)
//! - **L̃**: Pronunciation lexicon with auxiliary symbols
//! - **C̃**: Context-dependency transducer (triphone/tetraphone)
//! - **H̃**: HMM transducer with auxiliary distribution symbols
//! - **π**: Erasing operation (auxiliary symbols → ε)
//!
//! ## Module Organization
//!
//! - [`context`]: Context-dependency transducers (triphone, tetraphone)
//! - [`ngram`]: N-gram language model transducers with backoff
//! - [`cascade`]: ASR transducer cascade construction
//! - [`factoring`]: Chain factoring for compact representation
//! - [`rescoring`]: Lattice rescoring for multi-pass recognition
//! - [`subword_lexicon`]: Subword lexicon with BPE/boundary marker support
//!
//! ## Example
//!
//! ```ignore
//! use lling_llang::asr::{TriphoneBuilder, NgramBuilder, CascadeBuilder};
//! use lling_llang::semiring::LogWeight;
//!
//! // Build context-dependency transducer
//! let phones = vec!["a", "b", "c"];
//! let context = TriphoneBuilder::new(&phones).build();
//!
//! // Build n-gram language model transducer
//! let ngram = NgramBuilder::<LogWeight>::new()
//!     .add_unigram("hello", LogWeight::new(1.0))
//!     .add_bigram("hello", "world", LogWeight::new(0.5))
//!     .build();
//!
//! // Compose into full cascade
//! let cascade = CascadeBuilder::new()
//!     .grammar(ngram)
//!     .context_dependency(context)
//!     .build();
//! ```
//!
//! ## References
//!
//! - Mohri, M., Pereira, F., & Riley, M. (2002). "WFSTs in Speech Recognition"
//! - Mohri, M., Pereira, F., & Riley, M. (2008). "Speech Recognition with WFSTs"

mod context;
mod ngram;
mod cascade;
mod factoring;
mod rescoring;
mod subword_lexicon;

pub use context::{
    ContextDependencyBuilder, TriphoneBuilder, TetraploneBuilder,
    ContextDependencyConfig, ContextState, PhoneId,
};

pub use ngram::{
    NgramBuilder, NgramTransducer, NgramConfig,
    BackoffState, NgramOrder, NgramWeight,
};

pub use cascade::{
    CascadeBuilder, AsrCascade, CascadeConfig,
    LexiconEntry, AuxiliarySymbol,
};

pub use factoring::{
    chain_factor, ChainFactorConfig, ChainFactorResult,
    Chain, ChainId,
};

pub use rescoring::{
    rescore_lattice, RescoreConfig, RescoreResult,
    LatticeGrammar, RescorePass,
};

pub use subword_lexicon::{
    SubwordLexiconBuilder, SubwordEntry, SubwordPosition, MarkingStyle,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_structure() {
        // Basic module import test
        // Detailed tests are in individual submodules
    }
}
