//! Pronunciation variant transducer for ASR lexicon modeling.
//!
//! This module implements transducers that model multiple pronunciations per word,
//! including reduced forms common in conversational speech.
//!
//! # Features
//!
//! - Multiple pronunciation variants per word with probabilities
//! - Reduced form mappings (gonna -> going to, wanna -> want to)
//! - CMUdict format support
//! - Lexicon transducer construction
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::asr::pronunciation_variants::{PronunciationVariantTransducer, PronunciationEntry};
//! use lling_llang::semiring::LogWeight;
//!
//! let mut transducer = PronunciationVariantTransducer::<LogWeight>::new();
//!
//! // Add pronunciation variants
//! transducer.add_entry(PronunciationEntry {
//!     word: "the".to_string(),
//!     phonemes: vec![0, 1],  // /ðə/
//!     probability: 0.7,
//! });
//! transducer.add_entry(PronunciationEntry {
//!     word: "the".to_string(),
//!     phonemes: vec![0, 2],  // /ði/
//!     probability: 0.3,
//! });
//!
//! // Add reduced forms
//! transducer.add_reduced_form("gonna", "going to", 0.8);
//!
//! let lexicon_fst = transducer.build();
//! ```

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::marker::PhantomData;
use std::path::Path;

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition};

/// Phone identifier.
pub type PhoneId = u32;

/// Word identifier.
pub type WordId = u32;

/// A single pronunciation entry for a word.
#[derive(Debug, Clone)]
pub struct PronunciationEntry {
    /// The word (orthographic form).
    pub word: String,
    /// Phoneme sequence for this pronunciation.
    pub phonemes: Vec<PhoneId>,
    /// Prior probability of this variant (0.0 to 1.0).
    pub probability: f64,
    /// Optional variant tag (e.g., "1", "2" for numbered variants).
    pub variant_tag: Option<String>,
}

impl PronunciationEntry {
    /// Create a new pronunciation entry.
    pub fn new(word: impl Into<String>, phonemes: Vec<PhoneId>, probability: f64) -> Self {
        Self {
            word: word.into(),
            phonemes,
            probability,
            variant_tag: None,
        }
    }

    /// Create with a variant tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.variant_tag = Some(tag.into());
        self
    }
}

/// A reduced form mapping (e.g., "gonna" -> "going to").
#[derive(Debug, Clone)]
pub struct ReducedForm {
    /// The reduced form (e.g., "gonna").
    pub reduced: String,
    /// The full form (e.g., "going to").
    pub full: String,
    /// Probability of the reduced form being used.
    pub probability: f64,
}

impl ReducedForm {
    /// Create a new reduced form mapping.
    pub fn new(reduced: impl Into<String>, full: impl Into<String>, probability: f64) -> Self {
        Self {
            reduced: reduced.into(),
            full: full.into(),
            probability,
        }
    }
}

/// Configuration for the pronunciation variant transducer.
#[derive(Debug, Clone)]
pub struct PronunciationConfig {
    /// Whether to normalize probabilities per word.
    pub normalize_probabilities: bool,
    /// Default probability for entries without explicit probability.
    pub default_probability: f64,
    /// Whether to include reduced forms in the lexicon.
    pub include_reduced_forms: bool,
    /// Epsilon symbol ID (for word boundaries).
    pub epsilon_id: Option<PhoneId>,
    /// Word boundary symbol ID.
    pub word_boundary_id: Option<PhoneId>,
}

impl Default for PronunciationConfig {
    fn default() -> Self {
        Self {
            normalize_probabilities: true,
            default_probability: 1.0,
            include_reduced_forms: true,
            epsilon_id: None,
            word_boundary_id: None,
        }
    }
}

/// Pronunciation variant transducer for lexicon modeling.
///
/// Maps words to their phoneme sequences with associated probabilities.
#[derive(Debug)]
pub struct PronunciationVariantTransducer<W: Semiring> {
    /// Pronunciation entries grouped by word.
    entries: HashMap<String, Vec<PronunciationEntry>>,
    /// Reduced form mappings.
    reduced_forms: Vec<ReducedForm>,
    /// Phone symbol table (phone string -> PhoneId).
    phone_table: HashMap<String, PhoneId>,
    /// Reverse phone table (PhoneId -> phone string).
    phone_names: Vec<String>,
    /// Word symbol table (word -> WordId).
    word_table: HashMap<String, WordId>,
    /// Configuration.
    config: PronunciationConfig,
    /// Phantom data for weight type.
    _phantom: PhantomData<W>,
}

impl<W: Semiring> PronunciationVariantTransducer<W> {
    /// Create a new empty transducer.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            reduced_forms: Vec::new(),
            phone_table: HashMap::new(),
            phone_names: Vec::new(),
            word_table: HashMap::new(),
            config: PronunciationConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: PronunciationConfig) -> Self {
        Self {
            entries: HashMap::new(),
            reduced_forms: Vec::new(),
            phone_table: HashMap::new(),
            phone_names: Vec::new(),
            word_table: HashMap::new(),
            config,
            _phantom: PhantomData,
        }
    }

    /// Add a pronunciation entry.
    pub fn add_entry(&mut self, entry: PronunciationEntry) {
        self.entries
            .entry(entry.word.clone())
            .or_insert_with(Vec::new)
            .push(entry);
    }

    /// Add a reduced form mapping.
    pub fn add_reduced_form(&mut self, reduced: &str, full: &str, probability: f64) {
        self.reduced_forms
            .push(ReducedForm::new(reduced, full, probability));
    }

    /// Add multiple reduced forms at once.
    pub fn add_reduced_forms(&mut self, forms: &[(&str, &str, f64)]) {
        for (reduced, full, prob) in forms {
            self.add_reduced_form(reduced, full, *prob);
        }
    }

    /// Get or create a phone ID for a phone symbol.
    pub fn get_or_create_phone(&mut self, phone: &str) -> PhoneId {
        if let Some(&id) = self.phone_table.get(phone) {
            return id;
        }
        let id = self.phone_names.len() as PhoneId;
        self.phone_table.insert(phone.to_string(), id);
        self.phone_names.push(phone.to_string());
        id
    }

    /// Get or create a word ID.
    pub fn get_or_create_word(&mut self, word: &str) -> WordId {
        if let Some(&id) = self.word_table.get(word) {
            return id;
        }
        let id = self.word_table.len() as WordId;
        self.word_table.insert(word.to_string(), id);
        id
    }

    /// Get phone name by ID.
    pub fn phone_name(&self, id: PhoneId) -> Option<&str> {
        self.phone_names.get(id as usize).map(|s| s.as_str())
    }

    /// Get the number of phones in the vocabulary.
    pub fn num_phones(&self) -> usize {
        self.phone_names.len()
    }

    /// Get the number of words in the lexicon.
    pub fn num_words(&self) -> usize {
        self.entries.len()
    }

    /// Get all pronunciation variants for a word.
    pub fn get_pronunciations(&self, word: &str) -> Option<&[PronunciationEntry]> {
        self.entries.get(word).map(|v| v.as_slice())
    }

    /// Get configuration.
    pub fn config(&self) -> &PronunciationConfig {
        &self.config
    }

    /// Load from CMUdict format.
    ///
    /// CMUdict format: `WORD  PH1 PH2 PH3...` or `WORD(n)  PH1 PH2...` for variants.
    ///
    /// # Example CMUdict entries
    /// ```text
    /// THE  DH AH0
    /// THE(1)  DH IY1
    /// GOING  G OW1 IH0 NG
    /// ```
    pub fn from_cmudict<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut transducer = Self::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            // Skip comments and empty lines
            if line.is_empty() || line.starts_with(";;;") {
                continue;
            }

            // Parse the line
            if let Some((word_part, phones_part)) = line.split_once("  ") {
                let (word, variant_tag) = Self::parse_word_with_variant(word_part);
                let phones: Vec<&str> = phones_part.split_whitespace().collect();

                // Convert phones to IDs
                let phone_ids: Vec<PhoneId> = phones
                    .iter()
                    .map(|p| transducer.get_or_create_phone(p))
                    .collect();

                let mut entry =
                    PronunciationEntry::new(word, phone_ids, transducer.config.default_probability);

                if let Some(tag) = variant_tag {
                    entry = entry.with_tag(tag);
                }

                transducer.add_entry(entry);
            }
        }

        // Normalize probabilities if configured
        if transducer.config.normalize_probabilities {
            transducer.normalize_probabilities();
        }

        Ok(transducer)
    }

    /// Parse a word with optional variant number: "WORD" or "WORD(n)".
    fn parse_word_with_variant(word_part: &str) -> (String, Option<String>) {
        if let Some(paren_pos) = word_part.find('(') {
            let word = word_part[..paren_pos].to_string();
            let variant = word_part[paren_pos + 1..].trim_end_matches(')').to_string();
            (word, Some(variant))
        } else {
            (word_part.to_string(), None)
        }
    }

    /// Normalize probabilities so variants of each word sum to 1.0.
    fn normalize_probabilities(&mut self) {
        for variants in self.entries.values_mut() {
            let total: f64 = variants.iter().map(|e| e.probability).sum();
            if total > 0.0 {
                for entry in variants.iter_mut() {
                    entry.probability /= total;
                }
            }
        }
    }
}

impl<W: Semiring + From<f64> + Clone> PronunciationVariantTransducer<W> {
    /// Build the lexicon transducer.
    ///
    /// The transducer maps word IDs (input) to phone sequences (output).
    /// Each word can have multiple paths for its pronunciation variants.
    pub fn build(&self) -> VectorWfst<PhoneId, W> {
        let mut fst: VectorWfst<PhoneId, W> = VectorWfst::new();

        // Start state
        fst.add_state();
        fst.set_start(0);

        // Also make start state final (for empty input)
        fst.set_final(0, W::one());

        let mut next_state: StateId = 1;

        // Add each word's pronunciations
        for (word, variants) in &self.entries {
            let word_id = *self.word_table.get(word).unwrap_or(&0);

            for variant in variants {
                if variant.phonemes.is_empty() {
                    continue;
                }

                let weight = W::from(-variant.probability.ln()); // Convert to negative log

                // Create a path for this pronunciation
                let mut current_state: StateId = 0;

                for (i, &phone) in variant.phonemes.iter().enumerate() {
                    let is_last = i == variant.phonemes.len() - 1;

                    if is_last {
                        // Last phone goes back to start (or to a final state)
                        fst.add_state();
                        fst.add_transition(WeightedTransition {
                            from: current_state,
                            input: Some(phone),
                            output: Some(word_id),
                            to: next_state,
                            weight: if i == 0 { weight.clone() } else { W::one() },
                        });

                        // Make the end state final
                        fst.set_final(next_state, W::one());

                        // Add epsilon back to start for chaining
                        fst.add_transition(WeightedTransition {
                            from: next_state,
                            input: None,
                            output: None,
                            to: 0,
                            weight: W::one(),
                        });

                        next_state += 1;
                    } else {
                        // Intermediate phone
                        fst.add_state();
                        fst.add_transition(WeightedTransition {
                            from: current_state,
                            input: Some(phone),
                            output: if i == 0 { Some(word_id) } else { None },
                            to: next_state,
                            weight: if i == 0 { weight.clone() } else { W::one() },
                        });
                        current_state = next_state;
                        next_state += 1;
                    }
                }
            }
        }

        // Add reduced forms if configured
        if self.config.include_reduced_forms {
            for reduced_form in &self.reduced_forms {
                // Get IDs for reduced and full forms
                if let (Some(&_reduced_id), Some(full_entries)) = (
                    self.word_table.get(&reduced_form.reduced),
                    self.entries.get(&reduced_form.full),
                ) {
                    // Add alternative paths from reduced form's phones to full form's word
                    let weight = W::from(-reduced_form.probability.ln());

                    for full_entry in full_entries {
                        if !full_entry.phonemes.is_empty() {
                            let full_word_id = *self.word_table.get(&full_entry.word).unwrap_or(&0);

                            // Create path: reduced_phones -> full_word_id
                            if let Some(reduced_entries) = self.entries.get(&reduced_form.reduced) {
                                for reduced_entry in reduced_entries {
                                    let mut current_state: StateId = 0;

                                    for (i, &phone) in reduced_entry.phonemes.iter().enumerate() {
                                        let is_last = i == reduced_entry.phonemes.len() - 1;

                                        fst.add_state();
                                        fst.add_transition(WeightedTransition {
                                            from: current_state,
                                            input: Some(phone),
                                            output: if is_last { Some(full_word_id) } else { None },
                                            to: next_state,
                                            weight: if i == 0 { weight.clone() } else { W::one() },
                                        });

                                        if is_last {
                                            fst.set_final(next_state, W::one());
                                            fst.add_transition(WeightedTransition {
                                                from: next_state,
                                                input: None,
                                                output: None,
                                                to: 0,
                                                weight: W::one(),
                                            });
                                        }

                                        current_state = next_state;
                                        next_state += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        fst
    }

    /// Build an inverse lexicon transducer (phones -> words).
    ///
    /// Useful for decoding: given a phone sequence, find matching words.
    pub fn build_inverse(&self) -> VectorWfst<PhoneId, W> {
        let mut fst: VectorWfst<PhoneId, W> = VectorWfst::new();

        fst.add_state();
        fst.set_start(0);
        fst.set_final(0, W::one());

        let mut next_state: StateId = 1;

        for (word, variants) in &self.entries {
            let word_id = *self.word_table.get(word).unwrap_or(&0);

            for variant in variants {
                if variant.phonemes.is_empty() {
                    continue;
                }

                let weight = W::from(-variant.probability.ln());
                let mut current_state: StateId = 0;

                for (i, &phone) in variant.phonemes.iter().enumerate() {
                    let is_last = i == variant.phonemes.len() - 1;

                    fst.add_state();
                    fst.add_transition(WeightedTransition {
                        from: current_state,
                        // Inverse: input is word, output is phone
                        input: if i == 0 { Some(word_id) } else { None },
                        output: Some(phone),
                        to: next_state,
                        weight: if i == 0 { weight.clone() } else { W::one() },
                    });

                    if is_last {
                        fst.set_final(next_state, W::one());
                        fst.add_transition(WeightedTransition {
                            from: next_state,
                            input: None,
                            output: None,
                            to: 0,
                            weight: W::one(),
                        });
                    }

                    current_state = next_state;
                    next_state += 1;
                }
            }
        }

        fst
    }
}

impl<W: Semiring> Default for PronunciationVariantTransducer<W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Semiring> Clone for PronunciationVariantTransducer<W> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            reduced_forms: self.reduced_forms.clone(),
            phone_table: self.phone_table.clone(),
            phone_names: self.phone_names.clone(),
            word_table: self.word_table.clone(),
            config: self.config.clone(),
            _phantom: PhantomData,
        }
    }
}

/// Common English reduced forms.
pub fn common_english_reduced_forms() -> Vec<(&'static str, &'static str, f64)> {
    vec![
        ("gonna", "going to", 0.7),
        ("wanna", "want to", 0.6),
        ("gotta", "got to", 0.6),
        ("kinda", "kind of", 0.5),
        ("sorta", "sort of", 0.5),
        ("outta", "out of", 0.5),
        ("lotta", "lot of", 0.4),
        ("coulda", "could have", 0.5),
        ("woulda", "would have", 0.5),
        ("shoulda", "should have", 0.5),
        ("musta", "must have", 0.4),
        ("oughta", "ought to", 0.4),
        ("hafta", "have to", 0.5),
        ("useta", "used to", 0.4),
        ("lemme", "let me", 0.6),
        ("gimme", "give me", 0.6),
        ("dunno", "don't know", 0.5),
        ("whatcha", "what are you", 0.4),
        ("gotcha", "got you", 0.5),
        ("betcha", "bet you", 0.4),
        ("c'mon", "come on", 0.6),
        ("y'all", "you all", 0.7),
        ("ain't", "am not", 0.6),
        ("'cause", "because", 0.7),
        ("'bout", "about", 0.5),
        ("'em", "them", 0.6),
        ("'til", "until", 0.6),
    ]
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::Wfst;

    #[test]
    fn test_pronunciation_entry() {
        let entry = PronunciationEntry::new("hello", vec![1, 2, 3], 0.8);
        assert_eq!(entry.word, "hello");
        assert_eq!(entry.phonemes, vec![1, 2, 3]);
        assert!((entry.probability - 0.8).abs() < 0.001);
        assert!(entry.variant_tag.is_none());
    }

    #[test]
    fn test_pronunciation_entry_with_tag() {
        let entry = PronunciationEntry::new("the", vec![1, 2], 0.7).with_tag("1");
        assert_eq!(entry.variant_tag, Some("1".to_string()));
    }

    #[test]
    fn test_reduced_form() {
        let rf = ReducedForm::new("gonna", "going to", 0.7);
        assert_eq!(rf.reduced, "gonna");
        assert_eq!(rf.full, "going to");
        assert!((rf.probability - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_transducer_creation() {
        let transducer = PronunciationVariantTransducer::<TropicalWeight>::new();
        assert_eq!(transducer.num_phones(), 0);
        assert_eq!(transducer.num_words(), 0);
    }

    #[test]
    fn test_add_entry() {
        let mut transducer = PronunciationVariantTransducer::<TropicalWeight>::new();

        transducer.add_entry(PronunciationEntry::new("hello", vec![1, 2, 3], 1.0));
        transducer.add_entry(PronunciationEntry::new("world", vec![4, 5], 1.0));

        assert_eq!(transducer.num_words(), 2);
        assert!(transducer.get_pronunciations("hello").is_some());
        assert!(transducer.get_pronunciations("world").is_some());
        assert!(transducer.get_pronunciations("unknown").is_none());
    }

    #[test]
    fn test_multiple_variants() {
        let mut transducer = PronunciationVariantTransducer::<TropicalWeight>::new();

        transducer.add_entry(PronunciationEntry::new("the", vec![1, 2], 0.7));
        transducer.add_entry(PronunciationEntry::new("the", vec![1, 3], 0.3));

        let variants = transducer
            .get_pronunciations("the")
            .expect("asr/pronunciation_variants.rs: required value was None/Err");
        assert_eq!(variants.len(), 2);
    }

    #[test]
    fn test_phone_table() {
        let mut transducer = PronunciationVariantTransducer::<TropicalWeight>::new();

        let id1 = transducer.get_or_create_phone("AH");
        let id2 = transducer.get_or_create_phone("IY");
        let id3 = transducer.get_or_create_phone("AH"); // Duplicate

        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(id3, 0); // Same as first
        assert_eq!(transducer.num_phones(), 2);
        assert_eq!(transducer.phone_name(0), Some("AH"));
        assert_eq!(transducer.phone_name(1), Some("IY"));
    }

    #[test]
    fn test_add_reduced_forms() {
        let mut transducer = PronunciationVariantTransducer::<TropicalWeight>::new();

        transducer.add_reduced_forms(&[("gonna", "going to", 0.7), ("wanna", "want to", 0.6)]);

        assert_eq!(transducer.reduced_forms.len(), 2);
    }

    #[test]
    fn test_parse_word_with_variant() {
        let (word, tag) =
            PronunciationVariantTransducer::<TropicalWeight>::parse_word_with_variant("HELLO");
        assert_eq!(word, "HELLO");
        assert!(tag.is_none());

        let (word, tag) =
            PronunciationVariantTransducer::<TropicalWeight>::parse_word_with_variant("THE(1)");
        assert_eq!(word, "THE");
        assert_eq!(tag, Some("1".to_string()));

        let (word, tag) =
            PronunciationVariantTransducer::<TropicalWeight>::parse_word_with_variant("LIVE(2)");
        assert_eq!(word, "LIVE");
        assert_eq!(tag, Some("2".to_string()));
    }

    #[test]
    fn test_build_lexicon() {
        let mut transducer = PronunciationVariantTransducer::<TropicalWeight>::new();

        // Add entries with word IDs
        transducer.get_or_create_word("hello");
        transducer.get_or_create_word("world");

        transducer.add_entry(PronunciationEntry::new("hello", vec![1, 2, 3], 1.0));
        transducer.add_entry(PronunciationEntry::new("world", vec![4, 5], 1.0));

        let fst = transducer.build();

        assert!(fst.num_states() > 0);
        assert_eq!(fst.start(), 0);
        assert!(fst.is_final(0)); // Start is also final
    }

    #[test]
    fn test_build_inverse_lexicon() {
        let mut transducer = PronunciationVariantTransducer::<TropicalWeight>::new();

        transducer.get_or_create_word("hello");
        transducer.add_entry(PronunciationEntry::new("hello", vec![1, 2, 3], 1.0));

        let fst = transducer.build_inverse();

        assert!(fst.num_states() > 0);
        assert_eq!(fst.start(), 0);
    }

    #[test]
    fn test_common_reduced_forms() {
        let forms = common_english_reduced_forms();
        assert!(!forms.is_empty());

        // Check some known entries
        assert!(forms.iter().any(|(r, _, _)| *r == "gonna"));
        assert!(forms.iter().any(|(r, _, _)| *r == "wanna"));
        assert!(forms.iter().any(|(r, _, _)| *r == "gotta"));
    }

    #[test]
    fn test_config_default() {
        let config = PronunciationConfig::default();
        assert!(config.normalize_probabilities);
        assert!((config.default_probability - 1.0).abs() < 0.001);
        assert!(config.include_reduced_forms);
    }

    #[test]
    fn test_transducer_clone() {
        let mut transducer = PronunciationVariantTransducer::<TropicalWeight>::new();
        transducer.add_entry(PronunciationEntry::new("test", vec![1, 2], 1.0));

        let cloned = transducer.clone();
        assert_eq!(cloned.num_words(), 1);
    }

    #[test]
    fn test_normalize_probabilities() {
        let mut transducer = PronunciationVariantTransducer::<TropicalWeight>::new();

        transducer.add_entry(PronunciationEntry::new("the", vec![1, 2], 7.0));
        transducer.add_entry(PronunciationEntry::new("the", vec![1, 3], 3.0));

        transducer.normalize_probabilities();

        let variants = transducer
            .get_pronunciations("the")
            .expect("asr/pronunciation_variants.rs: required value was None/Err");
        let total: f64 = variants.iter().map(|e| e.probability).sum();
        assert!((total - 1.0).abs() < 0.001);
    }
}
