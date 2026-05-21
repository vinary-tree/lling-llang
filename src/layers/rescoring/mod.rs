//! Layers that rescore lattice edges (rather than filter them).

#[cfg(feature = "lm-rerank")]
pub mod lm_rerank;
#[cfg(feature = "lm-rerank")]
pub use lm_rerank::{LanguageModel, LanguageModelLayer};

#[cfg(feature = "phonetic-rescore")]
pub mod phonetic_rescore;
#[cfg(feature = "phonetic-rescore")]
pub use phonetic_rescore::{
    PhoneticReference, PhoneticRescoreLayer, SequenceReference, VocabularyReference,
    DEFAULT_PHONETIC_FUEL, DEFAULT_PHONETIC_WEIGHT,
};
