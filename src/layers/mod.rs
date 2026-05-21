//! Correction layer infrastructure for text normalization pipelines.
//!
//! Each layer receives a lattice and returns a (typically smaller) lattice
//! with paths filtered or reweighted.
//!
//! Layers are grouped by their pipeline role:
//!
//! - [`filtering`]: in-place path filtering / pruning (CFG, edit-distance,
//!   confusion, disfluency).
//! - [`rescoring`]: edge-weight rescoring (language models, phonetic).
//! - [`syntactic`]: syntactic / semantic typing layers (POS tagging,
//!   MeTTaIL types).
//! - [`code_correction`] / [`latex`] / [`mathml`]: domain-specific structured
//!   layers, each feature-gated.
//!
//! All concrete layer types are re-exported from this module for back-compat.
//!
//! # Built-in Layers
//!
//! | Layer | Description |
//! |-------|-------------|
//! | [`CfgFilterLayer`] | Filters paths that don't parse with a CFG |
//! | [`ConfusionLayer`] | Expands lattice with confusion-matrix substitutions |
//! | [`DisfluencyLayer`] | Rewrites filler/repetition tokens |
//! | [`EditDistanceLayer`] | Filters paths by Levenshtein distance to dictionary |
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

pub mod filtering;
pub mod rescoring;
pub mod syntactic;
pub mod traits;

pub use filtering::{
    damerau_levenshtein_distance, dvorak_keyboard_matrix, mobile_keyboard_matrix,
    ocr_confusion_matrix, qwerty_keyboard_matrix, CfgFilterLayer, ConfusionLayer,
    ConfusionLayerConfig, ConfusionMatrix, Dictionary, DisfluencyLayer, DisfluencyLayerConfig,
    DisfluencyRuleBuilder, DisfluencySpan, DisfluencyType, EditDistanceLayer,
    EditDistanceLayerConfig, InMemoryDictionary,
};
pub use traits::{
    CorrectionLayer, LayerError, LayerPipeline, LayerPipelineBuilder, LayerResult, LayerStats,
};

#[cfg(feature = "pos-tagging")]
pub use syntactic::PosTaggingLayer;

#[cfg(feature = "lm-rerank")]
pub use rescoring::{LanguageModel, LanguageModelLayer};

#[cfg(feature = "f1r3fly")]
pub use syntactic::MeTTaILTypeLayer;

#[cfg(feature = "phonetic-rescore")]
pub use rescoring::{
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
