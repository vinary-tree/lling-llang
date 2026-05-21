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
//! | [`CodeCorrectionLayer`] | `code-correction` | Pattern-aware code syntax recovery |

mod cfg_filter;
mod confusion;
mod disfluency;
mod edit_distance;
mod traits;

pub use cfg_filter::CfgFilterLayer;
pub use confusion::{
    dvorak_keyboard_matrix, mobile_keyboard_matrix, ocr_confusion_matrix, qwerty_keyboard_matrix,
    ConfusionLayer, ConfusionLayerConfig, ConfusionMatrix,
};
pub use disfluency::{
    DisfluencyLayer, DisfluencyLayerConfig, DisfluencyRuleBuilder, DisfluencySpan, DisfluencyType,
};
pub use edit_distance::{
    Dictionary, EditDistanceLayer, EditDistanceLayerConfig, InMemoryDictionary,
};
pub use traits::{
    CorrectionLayer, LayerError, LayerPipeline, LayerPipelineBuilder, LayerResult, LayerStats,
};

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
    PhoneticReference, PhoneticRescoreLayer, SequenceReference, VocabularyReference,
    DEFAULT_PHONETIC_FUEL, DEFAULT_PHONETIC_WEIGHT,
};

#[cfg(feature = "code-correction")]
pub mod code_correction;
#[cfg(feature = "code-correction")]
pub use code_correction::{
    CodeCorrectionConfig, CodeCorrectionLanguage, CodeCorrectionLayer, PatternAwareConfig,
    PatternAwareLayer, PatternBoost, RecoveryStrategy, SyntaxRecoveryConfig, SyntaxRecoveryLayer,
};

#[cfg(feature = "latex-syntax")]
pub mod latex;
#[cfg(feature = "latex-syntax")]
pub use latex::{
    IssueSeverity, LatexGrammar, LatexGrammarBuilder, LatexGrammarError, LatexSyntaxConfig,
    LatexSyntaxLayer, LatexValidator, RepairKind, RepairStrategy, RepairSuggestion,
    ValidationIssue, ValidationResult,
};

#[cfg(feature = "mathml-semantic")]
pub mod mathml;
#[cfg(feature = "mathml-semantic")]
pub use mathml::{
    Arity, DisambiguationDecision, DisambiguatorConfig, GlyphMeaning, HomoglyphDisambiguator,
    HomoglyphSet, MathContext, MathMLSemanticConfig, MathMLSemanticLayer, MathType,
    MathTypeChecker, SemanticIssue, SemanticIssueKind, SemanticResult, TypeCheckerConfig,
    TypeEnvironment, TypeError, TypeResult,
};
