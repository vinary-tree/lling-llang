//! liblevenshtein integration for lling-llang.
//!
//! This module provides a bridge between liblevenshtein's Levenshtein automata
//! and lling-llang's WFST framework, enabling efficient fuzzy string matching
//! in correction pipelines.
//!
//! # Overview
//!
//! The integration provides:
//!
//! - **Dictionary Types**: Access to liblevenshtein's dictionary implementations
//! - **Edit Semiring Adapters**: Convert between `TropicalWeight` and `EditWeight`
//! - **Fuzzy Lookup**: High-level APIs for dictionary-based correction
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────┐
//! │                    lling-llang WFST Framework                       │
//! ├────────────────────────────────────────────────────────────────────┤
//! │  fuzzy_lookup()  → liblevenshtein Transducer × Dictionary          │
//! │       │                                                            │
//! │       ↓                                                            │
//! │  EditWeight → Explainable corrections with operations              │
//! └────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::integration::prelude::*;
//!
//! // Build a dictionary
//! let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
//!
//! // Fuzzy lookup with edit operations tracking
//! let results = fuzzy_lookup_with_edits(&dict, "helo", FuzzyConfig::new(2));
//!
//! for (correction, edit_weight) in results {
//!     println!("{}: cost={}, operations={:?}",
//!         correction,
//!         edit_weight.cost(),
//!         edit_weight.describe());
//! }
//! ```
//!
//! # Feature Gate
//!
//! This module requires the `levenshtein` feature:
//!
//! ```toml
//! [dependencies]
//! lling-llang = { version = "0.1", features = ["levenshtein"] }
//! ```

mod liblevenshtein_bridge;

pub use liblevenshtein_bridge::*;

/// Prelude for convenient imports.
pub mod prelude {
    // Re-export dictionary types
    pub use liblevenshtein::prelude::{
        Algorithm, Candidate, Dictionary, DictionaryBackend as DictBackend, DictionaryContainer,
        DictionaryFactory, DictionaryNode, DoubleArrayTrie, DynamicDawg, QueryBuilder,
        SuffixAutomaton, Transducer, TransducerBuilder,
    };

    // Re-export UTF-8 dictionary types
    pub use libdictenstein::double_array_trie::char::DoubleArrayTrieChar;
    pub use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    pub use libdictenstein::suffix_automaton::char::SuffixAutomatonChar;

    // Re-export PathMap dictionary when available
    #[cfg(feature = "pathmap-backend")]
    pub use liblevenshtein::prelude::PathMapDictionary;

    // Re-export WallBreaker for large error bounds
    pub use libdictenstein::scdawg::Scdawg;
    pub use libdictenstein::scdawg::char::ScdawgChar;
    pub use liblevenshtein::wallbreaker::{
        PatternPiece, PatternSplitter, WallBreaker, WallBreakerQuery, WallBreakerResult,
    };

    // Re-export our bridge types
    pub use super::{
        fuzzy_lookup, fuzzy_lookup_parallel, fuzzy_lookup_with_edits, EditCosts, EditTracker,
        EditTrackerBuilder, FuzzyConfig, FuzzyResult,
    };
}
