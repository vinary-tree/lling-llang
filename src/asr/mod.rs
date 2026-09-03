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
//! - **$`\pi`$**: erasing operation (auxiliary symbols to $`\varepsilon`$).
//!
//! ## Module Organization
//!
//! - [`ContextDependencyBuilder`]: context-dependency transducers (triphone,
//!   tetraphone)
//! - [`NgramBuilder`]: n-gram language-model transducers with backoff
//! - [`CascadeBuilder`]: ASR transducer-cascade construction
//! - [`chain_factor`]: chain factoring for compact representation
//! - [`rescore_lattice`]: lattice rescoring for multi-pass recognition
//! - [`SubwordLexiconBuilder`]: subword lexicons with BPE/boundary markers
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

mod cascade;
mod context;
mod dysfluency;
mod factoring;
mod ngram;
mod pronunciation_variants;
mod rescoring;
mod subword_lexicon;

pub use context::{
    ContextDependencyBuilder, ContextDependencyConfig, ContextDependencyError, ContextState,
    PhoneId, TetraploneBuilder, TriphoneBuilder, EPSILON,
};

pub use ngram::{
    BackoffState, NgramBuilder, NgramConfig, NgramOrder, NgramTransducer, NgramWeight,
    NGRAM_EPSILON,
};

pub use cascade::{AsrCascade, AuxiliarySymbol, CascadeBuilder, CascadeConfig, LexiconEntry};

pub use factoring::{
    chain_factor, find_chains, Chain, ChainFactorConfig, ChainFactorResult, ChainId,
};

pub use rescoring::{
    multi_pass_rescore, rescore_lattice, LatticeGrammar, RescoreConfig, RescorePass, RescoreResult,
};

pub use subword_lexicon::{MarkingStyle, SubwordEntry, SubwordLexiconBuilder, SubwordPosition};

pub use dysfluency::{
    DysfluencyConfig, DysfluencyDetector, DysfluencyPattern, DysfluencySpan,
    SyllableRepetitionBuilder, WordRepetitionBuilder,
};

pub use pronunciation_variants::{
    common_english_reduced_forms, PronunciationConfig, PronunciationEntry,
    PronunciationVariantTransducer, ReducedForm,
};

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_structure() {
        // Basic module import test
        // Detailed tests are in individual submodules
    }
}
