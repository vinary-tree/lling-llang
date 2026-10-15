//! Retained dictionary correction through the native edit-distance layer.

#[cfg(feature = "levenshtein")]
use std::sync::Arc;

use crate::layers::{
    dvorak_keyboard_matrix, mobile_keyboard_matrix, ocr_confusion_matrix, qwerty_keyboard_matrix,
    ConfusionLayer, ConfusionMatrix,
};
#[cfg(feature = "levenshtein")]
use crate::layers::{
    DictionaryError, EditDistanceLayer, EditDistanceLayerConfig, LayerError, ResourceDictionary,
    ResourceDictionaryNormalization,
};
use crate::semiring::TropicalWeight;
use vinary_tree_interop::VtResource;

use super::{boundary, required_mut, set_error, LlingLlangStatus};

/// Revision-1 policy for one retained Unicode dictionary correction layer.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingCorrectionConfig {
    /// Must be one.
    pub version: u32,
    /// Must be zero.
    pub reserved: u32,
    /// Maximum Unicode-scalar edit distance.
    pub max_distance: usize,
    /// Maximum corrections returned per word.
    pub max_corrections_per_word: usize,
    /// Skip words shorter than this many Unicode scalars.
    pub min_word_length: usize,
    /// Cost for each edit.
    pub cost_per_edit: f64,
    /// Cost multiplier for substitutions.
    pub substitution_multiplier: f64,
    /// Cost multiplier for adjacent transpositions.
    pub transposition_multiplier: f64,
    /// Subtract this cost from an exact dictionary match.
    pub exact_match_boost: f64,
    /// One to enable optimal-string-alignment transpositions.
    pub enable_transpositions: u8,
    /// One to lowercase the query before resource traversal.
    pub case_insensitive: u8,
    /// One to retain original lattice edges during layer application.
    pub keep_original: u8,
    /// Zero for exact stored keys, one for Unicode-lowercase stored keys.
    pub normalization: u8,
    /// Must be zero.
    pub reserved_flags: [u8; 4],
}

/// Owned snapshot-backed native correction layer.
pub struct LlingCorrectionDictionary {
    #[cfg(feature = "levenshtein")]
    layer: EditDistanceLayer<TropicalWeight>,
}

/// Owned, bounded corrections for one word.
pub struct LlingCorrectionResult {
    entries: Vec<(String, f64)>,
}

/// Revision-1 retained confusion-layer policy. Matrix kinds: QWERTY=0,
/// Dvorak=1, OCR=2, mobile=3, custom=4.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingConfusionConfig {
    /// Must be one.
    pub version: u32,
    /// QWERTY=0, Dvorak=1, OCR=2, mobile=3, custom=4.
    pub matrix_kind: u32,
    /// Maximum correction candidates returned by one query.
    pub max_corrections: usize,
    /// Minimum confusion probability to include in a query.
    pub confusion_threshold: f64,
}

/// One Unicode-scalar custom matrix entry. Operation 0 is a substitution
/// (observed, intended), 1 is an insertion (observed), and 2 is a deletion
/// (intended). Unused scalar fields and `reserved` must be zero.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingConfusionPair {
    /// Observed scalar for substitutions and insertions.
    pub observed: u32,
    /// Intended scalar for substitutions and deletions.
    pub intended: u32,
    /// Substitution=0, insertion=1, deletion=2.
    pub operation: u32,
    /// Must be zero.
    pub reserved: u32,
    /// Finite probability in the interval (0, 1].
    pub probability: f64,
}

/// Owned native confusion layer, shared by repeated queries.
pub struct LlingConfusionLayer {
    layer: ConfusionLayer<TropicalWeight>,
}

fn scalar(value: u32, name: &str) -> Result<char, LlingLlangStatus> {
    char::from_u32(value).ok_or_else(|| {
        set_error(format!("{name} is not a Unicode scalar"));
        LlingLlangStatus::InvalidArgument
    })
}

/// Construct a retained native confusion layer from a built-in or custom
/// matrix. Custom entries are copied. The output remains null on failure.
///
/// # Safety
/// `config` and `out_layer` must be valid pointers; `pairs` must cover
/// `pair_count` entries when nonzero. The returned handle is exclusive.
#[no_mangle]
pub unsafe extern "C" fn lling_confusion_layer_open(
    config: *const LlingConfusionConfig,
    pairs: *const LlingConfusionPair,
    pair_count: usize,
    out_layer: *mut *mut LlingConfusionLayer,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_layer, "out_layer")?;
        *output = std::ptr::null_mut();
        let config = *unsafe { config.as_ref() }.ok_or_else(|| {
            set_error("confusion config is null");
            LlingLlangStatus::NullPointer
        })?;
        if config.version != 1 || config.matrix_kind > 4 {
            set_error("invalid confusion config version or matrix kind");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if config.max_corrections > 4096 || pair_count > 65_536 {
            set_error("confusion matrix exceeds native materialization limits");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        if !config.confusion_threshold.is_finite()
            || !(0.0 < config.confusion_threshold && config.confusion_threshold <= 1.0)
        {
            set_error("confusion threshold must be in (0, 1]");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if (config.matrix_kind == 4) != (pair_count != 0) {
            set_error("custom confusion matrices require entries; built-ins take none");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if pair_count != 0 && pairs.is_null() {
            set_error("confusion pairs are null with nonzero count");
            return Err(LlingLlangStatus::NullPointer);
        }
        let matrix = match config.matrix_kind {
            0 => qwerty_keyboard_matrix(),
            1 => dvorak_keyboard_matrix(),
            2 => ocr_confusion_matrix(),
            3 => mobile_keyboard_matrix(),
            4 => {
                let mut matrix = ConfusionMatrix::new();
                for pair in unsafe { std::slice::from_raw_parts(pairs, pair_count) } {
                    if pair.reserved != 0
                        || !pair.probability.is_finite()
                        || !(0.0 < pair.probability && pair.probability <= 1.0)
                    {
                        set_error("invalid custom confusion probability or reserved field");
                        return Err(LlingLlangStatus::InvalidArgument);
                    }
                    matrix = match pair.operation {
                        0 => matrix.add_confusion(
                            scalar(pair.observed, "observed")?,
                            scalar(pair.intended, "intended")?,
                            pair.probability,
                        ),
                        1 if pair.intended == 0 => matrix
                            .add_insertion(scalar(pair.observed, "observed")?, pair.probability),
                        2 if pair.observed == 0 => matrix
                            .add_deletion(scalar(pair.intended, "intended")?, pair.probability),
                        _ => {
                            set_error("invalid custom confusion operation or unused scalar");
                            return Err(LlingLlangStatus::InvalidArgument);
                        }
                    };
                }
                matrix
            }
            _ => unreachable!("matrix kind checked above"),
        };
        let layer = ConfusionLayer::new(matrix)
            .with_confusion_threshold(config.confusion_threshold)
            .with_max_corrections(config.max_corrections);
        *output = Box::into_raw(Box::new(LlingConfusionLayer { layer }));
        Ok(())
    })
}

/// Return owned, probability-ordered confusion candidates with negative-log costs.
///
/// # Safety
/// `layer` must be live, `query` must contain `query_len` UTF-8 bytes when
/// nonempty, and `out_result` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_confusion_layer_query(
    layer: *const LlingConfusionLayer,
    query: *const u8,
    query_len: usize,
    out_result: *mut *mut LlingCorrectionResult,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_result, "out_result")?;
        *output = std::ptr::null_mut();
        let layer = unsafe { layer.as_ref() }.ok_or_else(|| {
            set_error("confusion layer is null");
            LlingLlangStatus::NullPointer
        })?;
        if query_len != 0 && query.is_null() {
            set_error("query is null with nonzero length");
            return Err(LlingLlangStatus::NullPointer);
        }
        if query_len > 1024 {
            set_error("confusion query exceeds the 1024-byte limit");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        let bytes = if query_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(query, query_len) }
        };
        let word = std::str::from_utf8(bytes).map_err(|_| {
            set_error("confusion query is not UTF-8");
            LlingLlangStatus::InvalidArgument
        })?;
        if word.chars().count() > 256 {
            set_error("confusion query exceeds the 256-scalar limit");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        let entries = layer
            .layer
            .generate_corrections(word)
            .into_iter()
            .map(|(candidate, log_probability)| (candidate, -log_probability))
            .collect::<Vec<_>>();
        if entries.iter().any(|(_, cost)| !cost.is_finite()) {
            set_error("confusion cost overflowed the finite scalar carrier");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        *output = Box::into_raw(Box::new(LlingCorrectionResult { entries }));
        Ok(())
    })
}

/// Release a retained confusion layer. Null is accepted.
///
/// # Safety
/// A non-null pointer must be a live handle and must not be released twice.
#[no_mangle]
pub unsafe extern "C" fn lling_confusion_layer_free(layer: *mut LlingConfusionLayer) {
    if !layer.is_null() {
        drop(Box::from_raw(layer));
    }
}

#[cfg(feature = "levenshtein")]
impl LlingCorrectionConfig {
    fn validated(self) -> Result<EditDistanceLayerConfig, LlingLlangStatus> {
        if self.version != 1
            || self.reserved != 0
            || self.reserved_flags != [0; 4]
            || self.enable_transpositions > 1
            || self.case_insensitive > 1
            || self.keep_original > 1
            || self.normalization > 1
        {
            set_error("invalid correction config version, flag, or reserved field");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if self.max_corrections_per_word > 1_048_576
            || self.max_distance > 1_048_576
            || self.min_word_length > 1_048_576
        {
            set_error("correction config exceeds native materialization limits");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        if !self.cost_per_edit.is_finite()
            || self.cost_per_edit < 0.0
            || !self.substitution_multiplier.is_finite()
            || self.substitution_multiplier < 0.0
            || !self.transposition_multiplier.is_finite()
            || self.transposition_multiplier < 0.0
            || !self.exact_match_boost.is_finite()
        {
            set_error("correction costs must be finite; multipliers cannot be negative");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if self.case_insensitive == 1 && self.normalization == 0 {
            set_error("case-insensitive queries require declared lowercase dictionary keys");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        Ok(EditDistanceLayerConfig {
            max_distance: self.max_distance,
            cost_per_edit: self.cost_per_edit,
            substitution_multiplier: self.substitution_multiplier,
            transposition_multiplier: self.transposition_multiplier,
            enable_transpositions: self.enable_transpositions != 0,
            max_corrections_per_word: self.max_corrections_per_word,
            min_word_length: self.min_word_length,
            case_insensitive: self.case_insensitive != 0,
            keep_original: self.keep_original != 0,
            exact_match_boost: self.exact_match_boost,
        })
    }
}

#[cfg(feature = "levenshtein")]
fn dictionary_error(error: DictionaryError) -> LlingLlangStatus {
    let status = if matches!(error, DictionaryError::UnsupportedNormalization(_)) {
        LlingLlangStatus::InvalidArgument
    } else {
        LlingLlangStatus::ProviderError
    };
    set_error(error.to_string());
    status
}

#[cfg(feature = "levenshtein")]
fn layer_error(error: LayerError) -> LlingLlangStatus {
    let status = if matches!(error, LayerError::ConfigError(_)) {
        LlingLlangStatus::InvalidArgument
    } else {
        LlingLlangStatus::ProviderError
    };
    set_error(error.to_string());
    status
}

/// Capture a Unicode-scalar dictionary and its current immutable revision.
///
/// The `normalization` declaration is a producer contract: value one means
/// every stored key is already Rust-Unicode-lowercase normalized. The native
/// layer retains its own snapshot after the caller releases `resource`.
///
/// # Safety
/// Input and output pointers must be valid. The resource must honor the
/// Vinary Tree retain/release contract until this call returns.
#[no_mangle]
pub unsafe extern "C" fn lling_correction_dictionary_open(
    resource: *const VtResource,
    config: *const LlingCorrectionConfig,
    out_dictionary: *mut *mut LlingCorrectionDictionary,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_dictionary, "out_dictionary")?;
        *output = std::ptr::null_mut();
        #[cfg(not(feature = "levenshtein"))]
        {
            let _ = (resource, config);
            set_error("dictionary correction requires the levenshtein feature");
            Err(LlingLlangStatus::Unsupported)
        }
        #[cfg(feature = "levenshtein")]
        {
            let resource = *unsafe { resource.as_ref() }.ok_or_else(|| {
                set_error("resource is null");
                LlingLlangStatus::NullPointer
            })?;
            let config = *unsafe { config.as_ref() }.ok_or_else(|| {
                set_error("config is null");
                LlingLlangStatus::NullPointer
            })?;
            let policy = config.validated()?;
            let normalization = if config.normalization == 0 {
                ResourceDictionaryNormalization::Exact
            } else {
                ResourceDictionaryNormalization::UnicodeLowercaseKeys
            };
            let dictionary = unsafe {
                ResourceDictionary::from_resource_with_normalization(resource, normalization)
            }
            .map_err(dictionary_error)?;
            let layer = EditDistanceLayer::with_dictionary_and_config(Arc::new(dictionary), policy);
            *output = Box::into_raw(Box::new(LlingCorrectionDictionary { layer }));
            Ok(())
        }
    })
}

/// Query one word through the retained native correction layer.
///
/// Result storage is independently owned. Query bytes must be UTF-8 and remain
/// valid until this call returns. No result is published on failure.
///
/// # Safety
/// Pointers must be valid for their declared lengths and outputs must be
/// writable. The dictionary must remain live for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_correction_dictionary_query(
    dictionary: *const LlingCorrectionDictionary,
    query: *const u8,
    query_len: usize,
    out_result: *mut *mut LlingCorrectionResult,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_result, "out_result")?;
        *output = std::ptr::null_mut();
        #[cfg(not(feature = "levenshtein"))]
        {
            let _ = (dictionary, query, query_len);
            set_error("dictionary correction requires the levenshtein feature");
            Err(LlingLlangStatus::Unsupported)
        }
        #[cfg(feature = "levenshtein")]
        {
            let dictionary = unsafe { dictionary.as_ref() }.ok_or_else(|| {
                set_error("dictionary is null");
                LlingLlangStatus::NullPointer
            })?;
            if query_len != 0 && query.is_null() {
                set_error("query is null with nonzero length");
                return Err(LlingLlangStatus::NullPointer);
            }
            if query_len > 1_048_576 {
                set_error("query exceeds the one-megabyte input limit");
                return Err(LlingLlangStatus::LimitExceeded);
            }
            let bytes = if query_len == 0 {
                &[][..]
            } else {
                unsafe { std::slice::from_raw_parts(query, query_len) }
            };
            let word = std::str::from_utf8(bytes).map_err(|_| {
                set_error("query is not UTF-8");
                LlingLlangStatus::InvalidArgument
            })?;
            let entries = dictionary
                .layer
                .find_corrections(word)
                .map_err(layer_error)?;
            if entries.iter().any(|(_, cost)| !cost.is_finite()) {
                set_error("correction cost overflowed the finite scalar carrier");
                return Err(LlingLlangStatus::LimitExceeded);
            }
            *output = Box::into_raw(Box::new(LlingCorrectionResult { entries }));
            Ok(())
        }
    })
}

/// Return the number of bounded correction results.
///
/// # Safety
/// `result` must be a live result and `out_count` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_correction_result_count(
    result: *const LlingCorrectionResult,
    out_count: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let result = unsafe { result.as_ref() }.ok_or_else(|| {
            set_error("result is null");
            LlingLlangStatus::NullPointer
        })?;
        *required_mut(out_count, "out_count")? = result.entries.len();
        Ok(())
    })
}

/// Copy one correction word into caller-owned UTF-8 storage.
///
/// Zero capacity queries the required byte length. A smaller buffer receives
/// a prefix and reports both written and required lengths.
///
/// # Safety
/// All output pointers must be valid; `out_word` must cover `capacity` bytes
/// when capacity is nonzero. The result must remain live for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_correction_result_item(
    result: *const LlingCorrectionResult,
    index: usize,
    out_word: *mut u8,
    capacity: usize,
    out_written: *mut usize,
    out_required: *mut usize,
    out_cost: *mut f64,
) -> LlingLlangStatus {
    boundary(|| {
        let result = unsafe { result.as_ref() }.ok_or_else(|| {
            set_error("result is null");
            LlingLlangStatus::NullPointer
        })?;
        let (word, cost) = result.entries.get(index).ok_or_else(|| {
            set_error("correction index is out of range");
            LlingLlangStatus::InvalidArgument
        })?;
        let written = required_mut(out_written, "out_written")?;
        let required = required_mut(out_required, "out_required")?;
        let output_cost = required_mut(out_cost, "out_cost")?;
        if capacity != 0 && out_word.is_null() {
            set_error("out_word is null with nonzero capacity");
            return Err(LlingLlangStatus::NullPointer);
        }
        *required = word.len();
        *written = capacity.min(*required);
        *output_cost = *cost;
        if *written != 0 {
            unsafe { std::ptr::copy_nonoverlapping(word.as_ptr(), out_word, *written) };
        }
        Ok(())
    })
}

/// Release a retained dictionary layer. Null is accepted.
///
/// # Safety
/// A non-null pointer must be a live handle and must not be released twice.
#[no_mangle]
pub unsafe extern "C" fn lling_correction_dictionary_free(
    dictionary: *mut LlingCorrectionDictionary,
) {
    if !dictionary.is_null() {
        drop(Box::from_raw(dictionary));
    }
}

/// Release copied correction results. Null is accepted.
///
/// # Safety
/// A non-null pointer must be a live result and must not be released twice.
#[no_mangle]
pub unsafe extern "C" fn lling_correction_result_free(result: *mut LlingCorrectionResult) {
    if !result.is_null() {
        drop(Box::from_raw(result));
    }
}
