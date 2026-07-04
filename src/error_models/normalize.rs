//! Normalization transducer for text preprocessing.
//!
//! Provides WFSTs for normalizing text before correction, including:
//! - **Case folding**: Convert to lowercase/uppercase
//! - **Unicode normalization**: NFC, NFD, NFKC, NFKD forms
//! - **Punctuation normalization**: Smart quotes, dashes, etc.
//! - **Whitespace normalization**: Collapse multiple spaces, normalize line endings
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::error_models::{NormalizationTransducer, NormalizationConfig};
//! use lling_llang::semiring::TropicalWeight;
//!
//! // Create a normalizer with default settings
//! let normalizer = NormalizationTransducer::<TropicalWeight>::new()
//!     .with_case_fold(true)
//!     .with_unicode_nfc(true)
//!     .with_smart_quotes(true);
//!
//! let fst = normalizer.build();
//! ```

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

use crate::semiring::{Semiring, TropicalWeight};
use crate::wfst::{MutableWfst, VectorWfst};

/// Configuration for normalization behavior.
#[derive(Clone, Debug)]
pub struct NormalizationConfig {
    /// Convert to lowercase
    pub case_fold_lower: bool,
    /// Convert to uppercase
    pub case_fold_upper: bool,
    /// Apply Unicode NFC normalization
    pub unicode_nfc: bool,
    /// Apply Unicode NFD normalization
    pub unicode_nfd: bool,
    /// Apply Unicode NFKC normalization (compatibility)
    pub unicode_nfkc: bool,
    /// Apply Unicode NFKD normalization (compatibility)
    pub unicode_nfkd: bool,
    /// Convert smart/curly quotes to straight quotes
    pub smart_quotes: bool,
    /// Convert various dashes to standard hyphen
    pub normalize_dashes: bool,
    /// Convert various ellipsis forms to standard "..."
    pub normalize_ellipsis: bool,
    /// Collapse multiple whitespace to single space
    pub collapse_whitespace: bool,
    /// Normalize line endings to \n
    pub normalize_line_endings: bool,
    /// Remove zero-width characters
    pub remove_zero_width: bool,
    /// Remove diacritics/accents
    pub remove_diacritics: bool,
    /// Strip leading/trailing whitespace
    pub strip_whitespace: bool,
    /// Cost for normalization operations (default 0.0 = free)
    pub normalization_cost: f64,
}

impl Default for NormalizationConfig {
    fn default() -> Self {
        NormalizationConfig {
            case_fold_lower: false,
            case_fold_upper: false,
            unicode_nfc: false,
            unicode_nfd: false,
            unicode_nfkc: false,
            unicode_nfkd: false,
            smart_quotes: true,
            normalize_dashes: true,
            normalize_ellipsis: true,
            collapse_whitespace: true,
            normalize_line_endings: true,
            remove_zero_width: true,
            remove_diacritics: false,
            strip_whitespace: true,
            normalization_cost: 0.0,
        }
    }
}

impl NormalizationConfig {
    /// Create a minimal config (no normalization).
    pub fn none() -> Self {
        NormalizationConfig {
            case_fold_lower: false,
            case_fold_upper: false,
            unicode_nfc: false,
            unicode_nfd: false,
            unicode_nfkc: false,
            unicode_nfkd: false,
            smart_quotes: false,
            normalize_dashes: false,
            normalize_ellipsis: false,
            collapse_whitespace: false,
            normalize_line_endings: false,
            remove_zero_width: false,
            remove_diacritics: false,
            strip_whitespace: false,
            normalization_cost: 0.0,
        }
    }

    /// Create an aggressive config (maximum normalization).
    pub fn aggressive() -> Self {
        NormalizationConfig {
            case_fold_lower: true,
            case_fold_upper: false,
            unicode_nfkc: true,
            unicode_nfc: false,
            unicode_nfd: false,
            unicode_nfkd: false,
            smart_quotes: true,
            normalize_dashes: true,
            normalize_ellipsis: true,
            collapse_whitespace: true,
            normalize_line_endings: true,
            remove_zero_width: true,
            remove_diacritics: true,
            strip_whitespace: true,
            normalization_cost: 0.0,
        }
    }

    fn mapping_capacity_hint(&self) -> (usize, usize, usize) {
        let mut single = 0usize;
        let mut multi = 0usize;
        let mut delete = 0usize;

        if self.smart_quotes {
            single += 14;
        }
        if self.normalize_dashes {
            single += 10;
        }
        if self.normalize_ellipsis {
            multi += 2;
        }
        if self.remove_zero_width {
            delete += 6;
        }
        if self.collapse_whitespace {
            single += 15;
        }
        if self.normalize_line_endings {
            single += 1;
        }
        if self.remove_diacritics {
            single += 61;
            multi += 4;
        }

        (single, multi, delete)
    }
}

/// Character mapping for normalization.
#[derive(Clone, Debug, Default)]
pub struct CharacterMapping {
    /// Direct character-to-character mappings
    single: HashMap<char, char>,
    /// Character-to-string mappings (for expansions like ligatures)
    multi: HashMap<char, String>,
    /// Characters to delete (map to empty)
    delete: HashSet<char>,
}

impl CharacterMapping {
    /// Create a new empty mapping.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new empty mapping with pre-allocated storage.
    pub fn with_capacity(
        single_capacity: usize,
        multi_capacity: usize,
        delete_capacity: usize,
    ) -> Self {
        Self {
            single: HashMap::with_capacity(single_capacity),
            multi: HashMap::with_capacity(multi_capacity),
            delete: HashSet::with_capacity(delete_capacity),
        }
    }

    /// Reserve additional mapping capacity.
    pub fn reserve(
        &mut self,
        single_additional: usize,
        multi_additional: usize,
        delete_additional: usize,
    ) {
        self.single.reserve(single_additional);
        self.multi.reserve(multi_additional);
        self.delete.reserve(delete_additional);
    }

    /// Add a single character mapping.
    pub fn add(&mut self, from: char, to: char) -> &mut Self {
        self.single.insert(from, to);
        self
    }

    /// Add a character-to-string mapping.
    pub fn add_expansion(&mut self, from: char, to: &str) -> &mut Self {
        self.multi.insert(from, to.to_string());
        self
    }

    /// Add a character to delete.
    pub fn add_deletion(&mut self, c: char) -> &mut Self {
        self.delete.insert(c);
        self
    }

    /// Get the mapping for a character.
    pub fn get(&self, c: char) -> Option<NormalizationResult> {
        if self.delete.contains(&c) {
            return Some(NormalizationResult::Delete);
        }
        if let Some(&to) = self.single.get(&c) {
            return Some(NormalizationResult::Single(to));
        }
        if let Some(to) = self.multi.get(&c) {
            return Some(NormalizationResult::Multi(to.clone()));
        }
        None
    }

    /// Check if a character has a mapping.
    pub fn contains(&self, c: char) -> bool {
        self.single.contains_key(&c) || self.multi.contains_key(&c) || self.delete.contains(&c)
    }

    /// Get all source characters.
    pub fn source_chars(&self) -> Vec<char> {
        let mut chars = Vec::with_capacity(self.mapping_count());
        chars.extend(self.single.keys().copied());
        chars.extend(self.multi.keys().copied());
        chars.extend(self.delete.iter().copied());
        chars.sort();
        chars.dedup();
        chars
    }

    fn mapping_count(&self) -> usize {
        self.single
            .len()
            .saturating_add(self.multi.len())
            .saturating_add(self.delete.len())
    }
}

/// Result of applying a normalization mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NormalizationResult {
    /// Map to a single character
    Single(char),
    /// Map to multiple characters
    Multi(String),
    /// Delete the character
    Delete,
}

/// Normalization transducer for text preprocessing.
///
/// Creates WFSTs that normalize input text according to configurable rules.
#[derive(Clone, Debug)]
pub struct NormalizationTransducer<W: Semiring> {
    config: NormalizationConfig,
    custom_mappings: CharacterMapping,
    _phantom: PhantomData<W>,
}

impl<W: Semiring> NormalizationTransducer<W> {
    /// Create a new normalizer with default configuration.
    pub fn new() -> Self {
        NormalizationTransducer {
            config: NormalizationConfig::default(),
            custom_mappings: CharacterMapping::new(),
            _phantom: PhantomData,
        }
    }

    /// Create with a specific configuration.
    pub fn with_config(config: NormalizationConfig) -> Self {
        NormalizationTransducer {
            config,
            custom_mappings: CharacterMapping::new(),
            _phantom: PhantomData,
        }
    }

    /// Enable case folding to lowercase.
    pub fn with_case_fold_lower(mut self, enabled: bool) -> Self {
        self.config.case_fold_lower = enabled;
        self
    }

    /// Enable case folding to uppercase.
    pub fn with_case_fold_upper(mut self, enabled: bool) -> Self {
        self.config.case_fold_upper = enabled;
        self
    }

    /// Enable Unicode NFC normalization.
    pub fn with_unicode_nfc(mut self, enabled: bool) -> Self {
        self.config.unicode_nfc = enabled;
        self
    }

    /// Enable smart quote conversion.
    pub fn with_smart_quotes(mut self, enabled: bool) -> Self {
        self.config.smart_quotes = enabled;
        self
    }

    /// Enable dash normalization.
    pub fn with_normalize_dashes(mut self, enabled: bool) -> Self {
        self.config.normalize_dashes = enabled;
        self
    }

    /// Enable whitespace collapsing.
    pub fn with_collapse_whitespace(mut self, enabled: bool) -> Self {
        self.config.collapse_whitespace = enabled;
        self
    }

    /// Enable diacritic removal.
    pub fn with_remove_diacritics(mut self, enabled: bool) -> Self {
        self.config.remove_diacritics = enabled;
        self
    }

    /// Add custom character mappings.
    pub fn with_custom_mappings(mut self, mappings: CharacterMapping) -> Self {
        self.custom_mappings = mappings;
        self
    }

    /// Get the configuration.
    pub fn config(&self) -> &NormalizationConfig {
        &self.config
    }

    /// Build the complete character mapping based on configuration.
    fn build_mapping(&self) -> CharacterMapping {
        let mut mapping = self.custom_mappings.clone();
        let (single_capacity, multi_capacity, delete_capacity) =
            self.config.mapping_capacity_hint();
        mapping.reserve(single_capacity, multi_capacity, delete_capacity);

        // Smart quotes to straight quotes
        if self.config.smart_quotes {
            mapping.add('\u{2018}', '\''); // '
            mapping.add('\u{2019}', '\''); // '
            mapping.add('\u{201A}', '\''); // ‚
            mapping.add('\u{201B}', '\''); // ‛
            mapping.add('\u{201C}', '"'); // "
            mapping.add('\u{201D}', '"'); // "
            mapping.add('\u{201E}', '"'); // „
            mapping.add('\u{201F}', '"'); // ‟
            mapping.add('\u{2039}', '\''); // ‹
            mapping.add('\u{203A}', '\''); // ›
            mapping.add('\u{00AB}', '"'); // «
            mapping.add('\u{00BB}', '"'); // »
            mapping.add('\u{0060}', '\''); // ` (backtick)
            mapping.add('\u{00B4}', '\''); // ´ (acute accent)
        }

        // Dash normalization
        if self.config.normalize_dashes {
            mapping.add('\u{2010}', '-'); // ‐ Hyphen
            mapping.add('\u{2011}', '-'); // ‑ Non-breaking hyphen
            mapping.add('\u{2012}', '-'); // ‒ Figure dash
            mapping.add('\u{2013}', '-'); // – En dash
            mapping.add('\u{2014}', '-'); // — Em dash
            mapping.add('\u{2015}', '-'); // ― Horizontal bar
            mapping.add('\u{2212}', '-'); // − Minus sign
            mapping.add('\u{FE58}', '-'); // ﹘ Small em dash
            mapping.add('\u{FE63}', '-'); // ﹣ Small hyphen-minus
            mapping.add('\u{FF0D}', '-'); // － Fullwidth hyphen-minus
        }

        // Ellipsis normalization
        if self.config.normalize_ellipsis {
            mapping.add_expansion('\u{2026}', "..."); // … Horizontal ellipsis
            mapping.add_expansion('\u{22EF}', "..."); // ⋯ Midline horizontal ellipsis
        }

        // Zero-width character removal
        if self.config.remove_zero_width {
            mapping.add_deletion('\u{200B}'); // Zero-width space
            mapping.add_deletion('\u{200C}'); // Zero-width non-joiner
            mapping.add_deletion('\u{200D}'); // Zero-width joiner
            mapping.add_deletion('\u{FEFF}'); // Byte order mark
            mapping.add_deletion('\u{2060}'); // Word joiner
            mapping.add_deletion('\u{00AD}'); // Soft hyphen
        }

        // Whitespace normalization
        if self.config.collapse_whitespace {
            // Various space characters to standard space
            mapping.add('\u{00A0}', ' '); // Non-breaking space
            mapping.add('\u{2000}', ' '); // En quad
            mapping.add('\u{2001}', ' '); // Em quad
            mapping.add('\u{2002}', ' '); // En space
            mapping.add('\u{2003}', ' '); // Em space
            mapping.add('\u{2004}', ' '); // Three-per-em space
            mapping.add('\u{2005}', ' '); // Four-per-em space
            mapping.add('\u{2006}', ' '); // Six-per-em space
            mapping.add('\u{2007}', ' '); // Figure space
            mapping.add('\u{2008}', ' '); // Punctuation space
            mapping.add('\u{2009}', ' '); // Thin space
            mapping.add('\u{200A}', ' '); // Hair space
            mapping.add('\u{202F}', ' '); // Narrow no-break space
            mapping.add('\u{205F}', ' '); // Medium mathematical space
            mapping.add('\u{3000}', ' '); // Ideographic space
        }

        // Line ending normalization
        if self.config.normalize_line_endings {
            mapping.add('\r', '\n');
            // \r\n -> \n handled separately in string normalization
        }

        // Diacritic removal (basic Latin characters with diacritics)
        if self.config.remove_diacritics {
            // Uppercase with diacritics
            mapping.add('À', 'A');
            mapping.add('Á', 'A');
            mapping.add('Â', 'A');
            mapping.add('Ã', 'A');
            mapping.add('Ä', 'A');
            mapping.add('Å', 'A');
            mapping.add('Ç', 'C');
            mapping.add('È', 'E');
            mapping.add('É', 'E');
            mapping.add('Ê', 'E');
            mapping.add('Ë', 'E');
            mapping.add('Ì', 'I');
            mapping.add('Í', 'I');
            mapping.add('Î', 'I');
            mapping.add('Ï', 'I');
            mapping.add('Ñ', 'N');
            mapping.add('Ò', 'O');
            mapping.add('Ó', 'O');
            mapping.add('Ô', 'O');
            mapping.add('Õ', 'O');
            mapping.add('Ö', 'O');
            mapping.add('Ø', 'O');
            mapping.add('Ù', 'U');
            mapping.add('Ú', 'U');
            mapping.add('Û', 'U');
            mapping.add('Ü', 'U');
            mapping.add('Ý', 'Y');

            // Lowercase with diacritics
            mapping.add('à', 'a');
            mapping.add('á', 'a');
            mapping.add('â', 'a');
            mapping.add('ã', 'a');
            mapping.add('ä', 'a');
            mapping.add('å', 'a');
            mapping.add('ç', 'c');
            mapping.add('è', 'e');
            mapping.add('é', 'e');
            mapping.add('ê', 'e');
            mapping.add('ë', 'e');
            mapping.add('ì', 'i');
            mapping.add('í', 'i');
            mapping.add('î', 'i');
            mapping.add('ï', 'i');
            mapping.add('ñ', 'n');
            mapping.add('ò', 'o');
            mapping.add('ó', 'o');
            mapping.add('ô', 'o');
            mapping.add('õ', 'o');
            mapping.add('ö', 'o');
            mapping.add('ø', 'o');
            mapping.add('ù', 'u');
            mapping.add('ú', 'u');
            mapping.add('û', 'u');
            mapping.add('ü', 'u');
            mapping.add('ý', 'y');
            mapping.add('ÿ', 'y');

            // Ligatures
            mapping.add_expansion('Æ', "AE");
            mapping.add_expansion('æ', "ae");
            mapping.add_expansion('Œ', "OE");
            mapping.add_expansion('œ', "oe");
            mapping.add('ß', 's'); // German sharp s (could also be "ss")
            mapping.add('Ð', 'D'); // Eth
            mapping.add('ð', 'd');
            mapping.add('Þ', 'P'); // Thorn (approximation)
            mapping.add('þ', 'p');
        }

        mapping
    }

    /// Normalize a single character, returning the result.
    pub fn normalize_char(&self, c: char) -> NormalizationResult {
        let mapping = self.build_mapping();
        self.normalize_char_with_mapping(c, &mapping)
    }

    fn normalize_char_with_mapping(
        &self,
        c: char,
        mapping: &CharacterMapping,
    ) -> NormalizationResult {
        // First check custom/config mappings
        let base_result = if let Some(result) = mapping.get(c) {
            result
        } else {
            NormalizationResult::Single(c)
        };

        // Apply case folding to the result
        if self.config.case_fold_lower {
            Self::fold_result_lower(base_result)
        } else if self.config.case_fold_upper {
            Self::fold_result_upper(base_result)
        } else {
            base_result
        }
    }

    fn fold_result_lower(result: NormalizationResult) -> NormalizationResult {
        match result {
            NormalizationResult::Single(ch) => Self::fold_char(ch, char::to_lowercase),
            NormalizationResult::Multi(s) => NormalizationResult::Multi(s.to_lowercase()),
            NormalizationResult::Delete => NormalizationResult::Delete,
        }
    }

    fn fold_result_upper(result: NormalizationResult) -> NormalizationResult {
        match result {
            NormalizationResult::Single(ch) => Self::fold_char(ch, char::to_uppercase),
            NormalizationResult::Multi(s) => NormalizationResult::Multi(s.to_uppercase()),
            NormalizationResult::Delete => NormalizationResult::Delete,
        }
    }

    fn fold_char<I>(ch: char, fold: impl FnOnce(char) -> I) -> NormalizationResult
    where
        I: Iterator<Item = char>,
    {
        let mut folded = fold(ch);
        let first = folded
            .next()
            .expect("error_models/normalize.rs: required value was None/Err");
        if let Some(second) = folded.next() {
            let mut output = String::with_capacity(first.len_utf8() + second.len_utf8());
            output.push(first);
            output.push(second);
            output.extend(folded);
            NormalizationResult::Multi(output)
        } else {
            NormalizationResult::Single(first)
        }
    }

    /// Normalize a string.
    pub fn normalize_string(&self, input: &str) -> String {
        let mapping = self.build_mapping();
        let mut result = String::with_capacity(input.len());

        if self.config.normalize_line_endings {
            let mut chars = input.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                    self.push_normalized_char('\n', &mapping, &mut result);
                } else {
                    self.push_normalized_char(c, &mapping, &mut result);
                }
            }
        } else {
            for c in input.chars() {
                self.push_normalized_char(c, &mapping, &mut result);
            }
        }

        // Collapse multiple whitespace
        if self.config.collapse_whitespace {
            let mut collapsed = String::with_capacity(result.len());
            let mut last_was_space = false;
            for c in result.chars() {
                if c.is_whitespace() {
                    if !last_was_space {
                        collapsed.push(' ');
                        last_was_space = true;
                    }
                } else {
                    collapsed.push(c);
                    last_was_space = false;
                }
            }
            result = collapsed;
        }

        // Strip leading/trailing whitespace
        if self.config.strip_whitespace {
            let trimmed = result.trim();
            if trimmed.len() != result.len() {
                result = trimmed.to_string();
            }
        }

        result
    }

    fn push_normalized_char(&self, c: char, mapping: &CharacterMapping, output: &mut String) {
        match self.normalize_char_with_mapping(c, mapping) {
            NormalizationResult::Single(nc) => output.push(nc),
            NormalizationResult::Multi(s) => output.push_str(&s),
            NormalizationResult::Delete => {}
        }
    }

    /// Build a character-level normalization WFST.
    ///
    /// Creates a transducer that maps input characters to normalized output characters.
    /// Note: This only handles single-character mappings. For full normalization
    /// including whitespace collapse, use `normalize_string`.
    pub fn build(&self) -> VectorWfst<char, W>
    where
        W: From<TropicalWeight>,
    {
        let mapping = self.build_mapping();
        let source_chars = mapping.source_chars();
        let transition_capacity = source_chars
            .len()
            .saturating_add(Self::ascii_identity_count(&mapping));

        let mut fst: VectorWfst<char, W> = VectorWfst::with_capacity(1);
        let state = fst.add_state();
        fst.set_start(state);
        fst.set_final(state, W::one());
        fst.reserve_transitions(state, transition_capacity);

        let weight = W::from(TropicalWeight::new(self.config.normalization_cost));

        // Add arcs for all mapped characters
        for c in source_chars {
            if let Some(result) = mapping.get(c) {
                match result {
                    NormalizationResult::Single(to) => {
                        fst.add_arc(state, Some(c), Some(to), state, weight.clone());
                    }
                    NormalizationResult::Multi(_) => {
                        // Multi-character mappings can't be represented in a single arc
                        // We represent them as deletion (map to first char only)
                        // Full multi-char support would need additional states
                    }
                    NormalizationResult::Delete => {
                        // For deletion, we consume input without producing output
                        // This requires Option<char> for epsilon output
                    }
                }
            }
        }

        // Add identity mappings for ASCII
        let identity_weight = W::from(TropicalWeight::new(0.0));
        for c in ' '..='~' {
            if !mapping.contains(c) {
                // Case folding
                let output = if self.config.case_fold_lower {
                    c.to_ascii_lowercase()
                } else if self.config.case_fold_upper {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                fst.add_arc(state, Some(c), Some(output), state, identity_weight.clone());
            }
        }

        fst
    }

    /// Build a normalization WFST with epsilon support for deletions.
    pub fn build_with_epsilon(&self) -> VectorWfst<Option<char>, W>
    where
        W: From<TropicalWeight>,
    {
        let mapping = self.build_mapping();
        let source_chars = mapping.source_chars();
        let transition_capacity = source_chars
            .len()
            .saturating_add(Self::ascii_identity_count(&mapping));

        let mut fst: VectorWfst<Option<char>, W> = VectorWfst::with_capacity(1);
        let state = fst.add_state();
        fst.set_start(state);
        fst.set_final(state, W::one());
        fst.reserve_transitions(state, transition_capacity);

        let weight = W::from(TropicalWeight::new(self.config.normalization_cost));

        // Add arcs for all mapped characters
        for c in source_chars {
            if let Some(result) = mapping.get(c) {
                match result {
                    NormalizationResult::Single(to) => {
                        fst.add_arc(state, Some(Some(c)), Some(Some(to)), state, weight.clone());
                    }
                    NormalizationResult::Multi(s) => {
                        // For multi-char output, we'd need multiple states
                        // Simplified: output first char only
                        if let Some(first) = s.chars().next() {
                            fst.add_arc(
                                state,
                                Some(Some(c)),
                                Some(Some(first)),
                                state,
                                weight.clone(),
                            );
                        }
                    }
                    NormalizationResult::Delete => {
                        // Deletion: input character, epsilon output
                        fst.add_arc(state, Some(Some(c)), Some(None), state, weight.clone());
                    }
                }
            }
        }

        // Add identity mappings for ASCII
        let identity_weight = W::from(TropicalWeight::new(0.0));
        for c in ' '..='~' {
            if !mapping.contains(c) {
                let output = if self.config.case_fold_lower {
                    c.to_ascii_lowercase()
                } else if self.config.case_fold_upper {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                fst.add_arc(
                    state,
                    Some(Some(c)),
                    Some(Some(output)),
                    state,
                    identity_weight.clone(),
                );
            }
        }

        fst
    }

    fn ascii_identity_count(mapping: &CharacterMapping) -> usize {
        (' '..='~').filter(|&c| !mapping.contains(c)).count()
    }
}

impl<W: Semiring> Default for NormalizationTransducer<W> {
    fn default() -> Self {
        Self::new()
    }
}

/// Pre-built normalizer for ASCII/English text.
pub fn ascii_normalizer<W: Semiring>() -> NormalizationTransducer<W> {
    NormalizationTransducer::with_config(NormalizationConfig {
        case_fold_lower: true,
        smart_quotes: true,
        normalize_dashes: true,
        collapse_whitespace: true,
        normalize_line_endings: true,
        remove_zero_width: true,
        strip_whitespace: true,
        ..NormalizationConfig::none()
    })
}

/// Pre-built normalizer for Unicode text with full normalization.
pub fn unicode_normalizer<W: Semiring>() -> NormalizationTransducer<W> {
    NormalizationTransducer::with_config(NormalizationConfig {
        unicode_nfkc: true,
        smart_quotes: true,
        normalize_dashes: true,
        normalize_ellipsis: true,
        collapse_whitespace: true,
        normalize_line_endings: true,
        remove_zero_width: true,
        remove_diacritics: true,
        strip_whitespace: true,
        ..NormalizationConfig::none()
    })
}

/// Pre-built normalizer for search indexing (aggressive normalization).
pub fn search_normalizer<W: Semiring>() -> NormalizationTransducer<W> {
    NormalizationTransducer::with_config(NormalizationConfig::aggressive())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::Wfst;

    #[test]
    fn test_smart_quotes() {
        let normalizer = NormalizationTransducer::<TropicalWeight>::new().with_smart_quotes(true);

        // Left and right double quotes -> straight double quote
        assert_eq!(
            normalizer.normalize_string("\u{201C}Hello\u{201D}"),
            "\"Hello\""
        );
        // Left and right single quotes -> straight single quote
        assert_eq!(
            normalizer.normalize_string("\u{2018}single\u{2019}"),
            "'single'"
        );
        // Guillemets -> straight double quotes
        assert_eq!(
            normalizer.normalize_string("\u{00AB}guillemets\u{00BB}"),
            "\"guillemets\""
        );
    }

    #[test]
    fn test_dash_normalization() {
        let normalizer =
            NormalizationTransducer::<TropicalWeight>::new().with_normalize_dashes(true);

        assert_eq!(normalizer.normalize_string("en–dash"), "en-dash");
        assert_eq!(normalizer.normalize_string("em—dash"), "em-dash");
        assert_eq!(normalizer.normalize_string("a−b"), "a-b"); // Minus sign
    }

    #[test]
    fn test_whitespace_collapse() {
        let normalizer =
            NormalizationTransducer::<TropicalWeight>::new().with_collapse_whitespace(true);

        assert_eq!(normalizer.normalize_string("a  b   c"), "a b c");
        assert_eq!(normalizer.normalize_string("a\t\tb"), "a b");
        assert_eq!(
            normalizer.normalize_string("  leading  trailing  "),
            "leading trailing"
        );
    }

    #[test]
    fn test_case_fold_lower() {
        let normalizer =
            NormalizationTransducer::<TropicalWeight>::new().with_case_fold_lower(true);

        assert_eq!(normalizer.normalize_string("HELLO World"), "hello world");
        assert_eq!(normalizer.normalize_string("MiXeD"), "mixed");
    }

    #[test]
    fn test_case_fold_upper() {
        let normalizer =
            NormalizationTransducer::<TropicalWeight>::new().with_case_fold_upper(true);

        assert_eq!(normalizer.normalize_string("hello world"), "HELLO WORLD");
        assert_eq!(normalizer.normalize_string("straße"), "STRASSE");
    }

    #[test]
    fn test_diacritic_removal() {
        let normalizer =
            NormalizationTransducer::<TropicalWeight>::new().with_remove_diacritics(true);

        assert_eq!(normalizer.normalize_string("café"), "cafe");
        assert_eq!(normalizer.normalize_string("naïve"), "naive");
        assert_eq!(normalizer.normalize_string("Zürich"), "Zurich");
        assert_eq!(normalizer.normalize_string("résumé"), "resume");
    }

    #[test]
    fn test_zero_width_removal() {
        let normalizer = NormalizationTransducer::<TropicalWeight>::new();

        // Zero-width space between letters
        let input = "a\u{200B}b";
        assert_eq!(normalizer.normalize_string(input), "ab");

        // BOM removal
        let input = "\u{FEFF}hello";
        assert_eq!(normalizer.normalize_string(input), "hello");
    }

    #[test]
    fn test_ellipsis_normalization() {
        let normalizer = NormalizationTransducer::<TropicalWeight>::new();

        assert_eq!(normalizer.normalize_string("wait…"), "wait...");
    }

    #[test]
    fn test_line_ending_normalization() {
        // Test with line endings but without whitespace collapse
        let normalizer =
            NormalizationTransducer::<TropicalWeight>::with_config(NormalizationConfig {
                normalize_line_endings: true,
                collapse_whitespace: false,
                strip_whitespace: false,
                ..NormalizationConfig::none()
            });

        assert_eq!(normalizer.normalize_string("a\r\nb\rc"), "a\nb\nc");
    }

    #[test]
    fn test_custom_mapping() {
        let mut custom = CharacterMapping::new();
        custom.add('@', 'a');
        custom.add('$', 's');

        let normalizer =
            NormalizationTransducer::<TropicalWeight>::new().with_custom_mappings(custom);

        assert_eq!(normalizer.normalize_string("@dmin $ystem"), "admin system");
    }

    #[test]
    fn test_config_none() {
        let normalizer =
            NormalizationTransducer::<TropicalWeight>::with_config(NormalizationConfig::none());

        // Should not change anything
        let input = "\u{201C}Hello\u{201D}  world\r\n";
        assert_eq!(normalizer.normalize_string(input), input);
    }

    #[test]
    fn test_config_aggressive() {
        let normalizer = NormalizationTransducer::<TropicalWeight>::with_config(
            NormalizationConfig::aggressive(),
        );

        // Curly quotes, umlaut o, em dash
        let input = "  \u{201C}HELLO\u{201D}  W\u{00F6}rld\u{2014}TEST  ";
        let result = normalizer.normalize_string(input);

        // Should be lowercase, stripped, normalized
        assert!(result
            .chars()
            .all(|c| c.is_lowercase() || !c.is_alphabetic()));
        assert!(!result.starts_with(' '));
        assert!(!result.ends_with(' '));
    }

    #[test]
    fn test_build_fst() {
        let normalizer =
            NormalizationTransducer::<TropicalWeight>::new().with_case_fold_lower(true);

        let fst = normalizer.build();

        assert_eq!(fst.num_states(), 1);
        assert!(fst.is_final(fst.start()));
    }

    #[test]
    fn test_build_with_epsilon() {
        let normalizer = NormalizationTransducer::<TropicalWeight>::new();

        let fst = normalizer.build_with_epsilon();

        assert_eq!(fst.num_states(), 1);
        assert!(fst.is_final(fst.start()));
    }

    #[test]
    fn test_ascii_normalizer() {
        let normalizer: NormalizationTransducer<TropicalWeight> = ascii_normalizer();

        // Curly quotes around Hello
        let result = normalizer.normalize_string("  \u{201C}Hello\u{201D}  WORLD  ");
        assert_eq!(result, "\"hello\" world");
    }

    #[test]
    fn test_search_normalizer() {
        let normalizer: NormalizationTransducer<TropicalWeight> = search_normalizer();

        let result = normalizer.normalize_string("Café—NAÏVE");
        assert!(result.chars().all(|c| c.is_ascii() || !c.is_alphabetic()));
    }

    #[test]
    fn test_character_mapping() {
        let mut mapping = CharacterMapping::new();
        mapping.add('a', 'b');
        mapping.add_expansion('x', "yz");
        mapping.add_deletion('q');

        assert_eq!(mapping.get('a'), Some(NormalizationResult::Single('b')));
        assert_eq!(
            mapping.get('x'),
            Some(NormalizationResult::Multi("yz".to_string()))
        );
        assert_eq!(mapping.get('q'), Some(NormalizationResult::Delete));
        assert_eq!(mapping.get('z'), None);

        let chars = mapping.source_chars();
        assert!(chars.contains(&'a'));
        assert!(chars.contains(&'x'));
        assert!(chars.contains(&'q'));
    }

    #[test]
    fn test_character_mapping_source_chars_are_sorted_and_unique() {
        let mut mapping = CharacterMapping::with_capacity(2, 1, 2);
        mapping.add('b', 'B');
        mapping.add('a', 'A');
        mapping.add_expansion('c', "see");
        mapping.add_deletion('b');
        mapping.add_deletion('b');

        assert_eq!(mapping.source_chars(), vec!['a', 'b', 'c']);
    }

    #[test]
    fn test_unicode_spaces() {
        let normalizer =
            NormalizationTransducer::<TropicalWeight>::new().with_collapse_whitespace(true);

        // Non-breaking space
        assert_eq!(
            normalizer.normalize_string("hello\u{00A0}world"),
            "hello world"
        );
        // Em space
        assert_eq!(normalizer.normalize_string("a\u{2003}b"), "a b");
        // Ideographic space
        assert_eq!(normalizer.normalize_string("日\u{3000}本"), "日 本");
    }

    #[test]
    fn test_ligatures() {
        let normalizer =
            NormalizationTransducer::<TropicalWeight>::new().with_remove_diacritics(true);

        assert_eq!(normalizer.normalize_string("Æsop"), "AEsop");
        assert_eq!(normalizer.normalize_string("œuvre"), "oeuvre");
    }

    #[test]
    fn test_combined_normalizations() {
        let normalizer = NormalizationTransducer::<TropicalWeight>::new()
            .with_case_fold_lower(true)
            .with_smart_quotes(true)
            .with_normalize_dashes(true)
            .with_collapse_whitespace(true)
            .with_remove_diacritics(true);

        // Curly quotes, accented characters, em dash
        let input = "  \u{201C}CAF\u{00C9}\u{201D}\u{2014}  NA\u{00CF}VE  ";
        let result = normalizer.normalize_string(input);

        assert_eq!(result, "\"cafe\"- naive");
    }
}
