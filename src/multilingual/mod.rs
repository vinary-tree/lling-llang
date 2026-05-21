//! Multilingual support including code-switching transducers.
//!
//! This module provides:
//! - **Code-Switching Transducers**: Model language switches in multilingual speech
//! - **Language Identification**: Detect language at word/phrase level
//! - **Language Models**: Per-language probability models
//!
//! ## Code-Switching
//!
//! Code-switching is the alternation between two or more languages within a single
//! conversation. This is common in multilingual communities and presents challenges
//! for ASR and NLP systems.
//!
//! The `CodeSwitchTransducer` models:
//! - Language-specific vocabularies
//! - Switch penalties between languages
//! - Language priors (expected frequency of each language)
//! - Per-language scoring
//!
//! ## Example
//!
//! ```rust,ignore
//! use lling_llang::multilingual::*;
//! use lling_llang::semiring::TropicalWeight;
//!
//! // Configure two languages
//! let english = LanguageConfig::new("en")
//!     .with_prior(0.7)
//!     .with_words(vec!["hello", "world", "the"]);
//!
//! let spanish = LanguageConfig::new("es")
//!     .with_prior(0.3)
//!     .with_words(vec!["hola", "mundo", "el"]);
//!
//! // Build code-switching transducer
//! let transducer: CodeSwitchTransducer<TropicalWeight> = CodeSwitchBuilder::new()
//!     .add_language(english)
//!     .add_language(spanish)
//!     .switch_penalty(2.0)
//!     .build();
//!
//! // Score a code-switched sequence
//! let score = transducer.score_sequence(&["hello", "mundo"]);
//! ```

mod code_switch;
mod language;

pub use code_switch::{
    CodeSwitchBuilder, CodeSwitchConfig, CodeSwitchPath, CodeSwitchTransducer, LanguageSpan,
    SwitchPoint,
};
pub use language::{
    DetectionResult, LanguageConfig, LanguageDetector, LanguageId, LanguageModel,
    SimpleLanguageModel, WordProbability,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_imports() {
        // Verify all public types are accessible
        let _id = LanguageId::new("en");
        let _config = LanguageConfig::new("es");
    }
}
