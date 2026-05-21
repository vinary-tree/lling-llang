//! Error model transducers for correction pipelines.
//!
//! This module provides WFSTs that model various types of errors in text,
//! enabling systematic correction through composition with dictionaries
//! and language models.
//!
//! # Available Error Models
//!
//! - **Edit Distance**: Levenshtein/Damerau-Levenshtein transducers for spelling correction
//! - **Confusion**: Character-level confusion matrices (OCR, keyboard typos)
//! - **Homophone**: Sound-alike word mappings for spoken language errors
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::error_models::EditDistanceTransducer;
//! use lling_llang::composition::compose;
//!
//! // Build an edit distance transducer with max distance 2
//! let edit_fst = EditDistanceTransducer::new(2)
//!     .with_alphabet("abcdefghijklmnopqrstuvwxyz")
//!     .build();
//!
//! // Compose with a dictionary for fuzzy lookup
//! let fuzzy = compose(&edit_fst, &dictionary_fst);
//! ```

pub mod confusion;
pub mod edit_distance;
pub mod homophone;
pub mod normalize;

pub use edit_distance::{
    DamerauLevenshteinTransducer, EditCosts, EditDistanceConfig, EditDistanceTransducer,
};

pub use confusion::{
    combined_confusion_matrix, dvorak_confusion_matrix, ocr_confusion_matrix,
    qwerty_confusion_matrix, train_confusion_matrix, ConfusionConfig, ConfusionMatrix,
    ConfusionTransducer,
};

pub use homophone::{
    common_english_homophones, english_homophone_transducer, HomophoneConfig, HomophoneEntry,
    HomophoneTransducer, PhoneticAlgorithm, PhoneticCode, PhoneticEncoder,
};

pub use normalize::{
    ascii_normalizer, search_normalizer, unicode_normalizer, CharacterMapping, NormalizationConfig,
    NormalizationResult, NormalizationTransducer,
};
