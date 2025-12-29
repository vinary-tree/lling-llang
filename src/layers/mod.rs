//! Correction layer infrastructure for text normalization pipelines.
//!
//! This module provides an extensible layer architecture for building
//! correction pipelines. Each layer receives a lattice and returns a
//! (typically smaller) lattice with paths filtered or reweighted.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Correction Layer Stack                           │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Layer N: [User-Defined]           ← Implement CorrectionLayer trait    │
//! │     ↑                                                                   │
//! │  Layer 3: CFG Grammar              ← Syntactic filtering                │
//! │     ↑                                                                   │
//! │  Layer 1: Lexical Correction       ← Levenshtein + phonetic candidates  │
//! │     ↑                                                                   │
//! │  [Input Lattice]                                                        │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use lling_llang::layers::{LayerPipeline, CfgFilterLayer};
//! use lling_llang::cfg::Grammar;
//!
//! let grammar = Grammar::from_file("grammar.cfg")?;
//! let mut pipeline = LayerPipeline::new();
//! pipeline.add_layer(CfgFilterLayer::new(&grammar));
//!
//! let filtered = pipeline.apply(&input_lattice)?;
//! ```
//!
//! # Built-in Layers
//!
//! | Layer | Description |
//! |-------|-------------|
//! | [`CfgFilterLayer`] | Filters paths that don't parse with a CFG |
//!
//! # Feature-Gated Layers
//!
//! | Layer | Feature | Description |
//! |-------|---------|-------------|
//! | `PosTaggingLayer` | `pos-tagging` | POS-based filtering |
//! | `LanguageModelLayer` | `lm-rerank` | LM-based reranking |
//! | `PhoneticRescoreLayer` | `phonetic-rescore` | Phonetic similarity rescoring |
//! | `MeTTaILTypeLayer` | `f1r3fly` | MeTTaIL semantic type filtering |

mod traits;
mod cfg_filter;

pub use traits::{
    CorrectionLayer, LayerError, LayerPipeline, LayerPipelineBuilder,
    LayerStats, LayerResult,
};
pub use cfg_filter::CfgFilterLayer;

// Feature-gated layers
#[cfg(feature = "pos-tagging")]
mod pos_tagging;
#[cfg(feature = "pos-tagging")]
pub use pos_tagging::PosTaggingLayer;

#[cfg(feature = "lm-rerank")]
mod lm_rerank;
#[cfg(feature = "lm-rerank")]
pub use lm_rerank::{LanguageModel, LanguageModelLayer};

#[cfg(feature = "f1r3fly")]
mod mettail_type;
#[cfg(feature = "f1r3fly")]
pub use mettail_type::MeTTaILTypeLayer;

#[cfg(feature = "phonetic-rescore")]
mod phonetic_rescore;
#[cfg(feature = "phonetic-rescore")]
pub use phonetic_rescore::{
    PhoneticRescoreLayer, PhoneticReference, VocabularyReference, SequenceReference,
    DEFAULT_PHONETIC_WEIGHT, DEFAULT_PHONETIC_FUEL,
};
