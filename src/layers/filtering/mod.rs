//! Layers that filter or rewrite lattice paths in-place.
//!
//! - [`CfgFilterLayer`]: rejects paths that don't parse with a CFG.
//! - [`ConfusionLayer`]: expands a lattice with confusion-matrix substitutions.
//! - [`DisfluencyLayer`]: rewrites filler/repetition tokens to canonical forms.
//! - [`EditDistanceLayer`]: filters paths by their Levenshtein distance to a
//!   reference dictionary.

pub mod cfg_filter;
pub mod confusion;
pub mod disfluency;
pub mod edit_distance;

pub use cfg_filter::CfgFilterLayer;
pub use confusion::{
    dvorak_keyboard_matrix, mobile_keyboard_matrix, ocr_confusion_matrix, qwerty_keyboard_matrix,
    ConfusionLayer, ConfusionLayerConfig, ConfusionMatrix,
};
pub use disfluency::{
    DisfluencyLayer, DisfluencyLayerConfig, DisfluencyRuleBuilder, DisfluencySpan, DisfluencyType,
};
pub use edit_distance::{
    damerau_levenshtein_distance, Dictionary, EditDistanceLayer, EditDistanceLayerConfig,
    InMemoryDictionary,
};
