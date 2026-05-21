//! Confusion matrix correction layer for OCR and keyboard typo modeling.
//!
//! This layer models character-level confusions that commonly occur in OCR
//! (Optical Character Recognition) or keyboard typing. Unlike edit distance,
//! this layer uses empirical probability distributions to weight corrections.
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::layers::{ConfusionLayer, ConfusionMatrix};
//! use lling_llang::semiring::TropicalWeight;
//!
//! // Create a keyboard-based confusion layer
//! let layer = ConfusionLayer::<TropicalWeight>::qwerty_keyboard()
//!     .with_confusion_threshold(0.1);
//!
//! // Or load a custom confusion matrix
//! let matrix = ConfusionMatrix::from_pairs(&[
//!     (('m', 'n'), 0.3),  // 30% chance of m->n confusion
//!     (('0', 'O'), 0.2),  // 20% chance of 0->O confusion
//! ]);
//! let layer = ConfusionLayer::<TropicalWeight>::new(matrix);
//!
//! // Apply to a lattice
//! let corrected = layer.apply(&input_lattice)?;
//! ```
//!
//! # Pre-built Confusion Matrices
//!
//! - `qwerty_keyboard()`: Common QWERTY keyboard typos (adjacent keys)
//! - `dvorak_keyboard()`: Common Dvorak keyboard typos
//! - `ocr_confusion()`: Common OCR errors (0/O, 1/l/I, etc.)
//! - `mobile_keyboard()`: Common touchscreen keyboard errors

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticeBuilder};
use crate::layers::{CorrectionLayer, LayerResult};
use crate::semiring::{Semiring, TropicalWeight};

/// A confusion matrix mapping (observed, intended) character pairs to probabilities.
///
/// For each character pair (a, b), the matrix stores P(observed=a | intended=b),
/// i.e., the probability of seeing character `a` when the intended character was `b`.
#[derive(Clone, Debug)]
pub struct ConfusionMatrix {
    /// Confusion probabilities: (observed, intended) -> probability
    confusions: HashMap<(char, char), f64>,
    /// Deletion probabilities: intended char -> probability of deletion
    deletions: HashMap<char, f64>,
    /// Insertion probabilities: inserted char -> probability
    insertions: HashMap<char, f64>,
    /// Default confusion probability for unlisted pairs
    default_confusion: f64,
    /// Default deletion probability
    default_deletion: f64,
    /// Default insertion probability
    default_insertion: f64,
}

impl ConfusionMatrix {
    /// Create an empty confusion matrix.
    pub fn new() -> Self {
        Self {
            confusions: HashMap::new(),
            deletions: HashMap::new(),
            insertions: HashMap::new(),
            default_confusion: 0.001,
            default_deletion: 0.001,
            default_insertion: 0.001,
        }
    }

    /// Create from a list of (observed, intended, probability) tuples.
    pub fn from_pairs(pairs: &[((char, char), f64)]) -> Self {
        let mut matrix = Self::new();
        for &((observed, intended), prob) in pairs {
            matrix.confusions.insert((observed, intended), prob);
        }
        matrix
    }

    /// Add a confusion pair with probability.
    pub fn add_confusion(mut self, observed: char, intended: char, probability: f64) -> Self {
        self.confusions.insert((observed, intended), probability);
        self
    }

    /// Add a symmetric confusion (both directions).
    pub fn add_symmetric_confusion(mut self, char1: char, char2: char, probability: f64) -> Self {
        self.confusions.insert((char1, char2), probability);
        self.confusions.insert((char2, char1), probability);
        self
    }

    /// Add a deletion probability.
    pub fn add_deletion(mut self, char: char, probability: f64) -> Self {
        self.deletions.insert(char, probability);
        self
    }

    /// Add an insertion probability.
    pub fn add_insertion(mut self, char: char, probability: f64) -> Self {
        self.insertions.insert(char, probability);
        self
    }

    /// Set the default confusion probability.
    pub fn with_default_confusion(mut self, probability: f64) -> Self {
        self.default_confusion = probability;
        self
    }

    /// Get the confusion probability P(observed | intended).
    pub fn confusion_prob(&self, observed: char, intended: char) -> f64 {
        if observed == intended {
            // Identity: high probability of correct transcription
            1.0 - self.total_error_prob(intended)
        } else {
            *self
                .confusions
                .get(&(observed, intended))
                .unwrap_or(&self.default_confusion)
        }
    }

    /// Get the deletion probability for a character.
    pub fn deletion_prob(&self, char: char) -> f64 {
        *self.deletions.get(&char).unwrap_or(&self.default_deletion)
    }

    /// Get the insertion probability for a character.
    pub fn insertion_prob(&self, char: char) -> f64 {
        *self
            .insertions
            .get(&char)
            .unwrap_or(&self.default_insertion)
    }

    /// Get the total error probability for an intended character.
    fn total_error_prob(&self, intended: char) -> f64 {
        let confusion_sum: f64 = self
            .confusions
            .iter()
            .filter(|((_, i), _)| *i == intended)
            .map(|(_, &p)| p)
            .sum();

        let deletion = self.deletion_prob(intended);

        (confusion_sum + deletion).min(0.99) // Cap at 99% error rate
    }

    /// Get all possible confusions for an intended character.
    pub fn confusions_for(&self, intended: char) -> Vec<(char, f64)> {
        self.confusions
            .iter()
            .filter(|((_, i), _)| *i == intended)
            .map(|((o, _), &p)| (*o, p))
            .collect()
    }

    /// Get all intended characters that could produce an observed character.
    pub fn sources_for(&self, observed: char) -> Vec<(char, f64)> {
        self.confusions
            .iter()
            .filter(|((o, _), _)| *o == observed)
            .map(|((_, i), &p)| (*i, p))
            .collect()
    }

    /// Get the number of confusion pairs.
    pub fn num_confusions(&self) -> usize {
        self.confusions.len()
    }
}

impl Default for ConfusionMatrix {
    fn default() -> Self {
        Self::new()
    }
}

/// Pre-built QWERTY keyboard confusion matrix.
///
/// Models common typos from adjacent keys on a QWERTY keyboard.
pub fn qwerty_keyboard_matrix() -> ConfusionMatrix {
    let mut matrix = ConfusionMatrix::new();

    // Adjacent key pairs on QWERTY keyboard (horizontal neighbors)
    let adjacent_pairs = [
        // Row 1 (numbers)
        ('1', '2'),
        ('2', '3'),
        ('3', '4'),
        ('4', '5'),
        ('5', '6'),
        ('6', '7'),
        ('7', '8'),
        ('8', '9'),
        ('9', '0'),
        // Row 2 (qwerty)
        ('q', 'w'),
        ('w', 'e'),
        ('e', 'r'),
        ('r', 't'),
        ('t', 'y'),
        ('y', 'u'),
        ('u', 'i'),
        ('i', 'o'),
        ('o', 'p'),
        // Row 3 (asdf)
        ('a', 's'),
        ('s', 'd'),
        ('d', 'f'),
        ('f', 'g'),
        ('g', 'h'),
        ('h', 'j'),
        ('j', 'k'),
        ('k', 'l'),
        // Row 4 (zxcv)
        ('z', 'x'),
        ('x', 'c'),
        ('c', 'v'),
        ('v', 'b'),
        ('b', 'n'),
        ('n', 'm'),
    ];

    // Add horizontal adjacencies with moderate probability
    for &(a, b) in &adjacent_pairs {
        matrix = matrix.add_symmetric_confusion(a, b, 0.15);
        // Also add uppercase variants
        matrix =
            matrix.add_symmetric_confusion(a.to_ascii_uppercase(), b.to_ascii_uppercase(), 0.15);
    }

    // Vertical adjacencies (less common but still happen)
    let vertical_pairs = [
        // q row to a row
        ('q', 'a'),
        ('w', 's'),
        ('e', 'd'),
        ('r', 'f'),
        ('t', 'g'),
        ('y', 'h'),
        ('u', 'j'),
        ('i', 'k'),
        ('o', 'l'),
        // a row to z row
        ('a', 'z'),
        ('s', 'x'),
        ('d', 'c'),
        ('f', 'v'),
        ('g', 'b'),
        ('h', 'n'),
        ('j', 'm'),
    ];

    for &(a, b) in &vertical_pairs {
        matrix = matrix.add_symmetric_confusion(a, b, 0.08);
    }

    // Common double-tap errors (doubled letters)
    for c in 'a'..='z' {
        matrix = matrix.add_insertion(c, 0.02);
    }

    matrix
}

/// Pre-built Dvorak keyboard confusion matrix.
pub fn dvorak_keyboard_matrix() -> ConfusionMatrix {
    let mut matrix = ConfusionMatrix::new();

    // Dvorak layout adjacent pairs (top row: ' , . p y f g c r l)
    let adjacent_pairs = [
        ('\'', ','),
        (',', '.'),
        ('.', 'p'),
        ('p', 'y'),
        ('y', 'f'),
        ('f', 'g'),
        ('g', 'c'),
        ('c', 'r'),
        ('r', 'l'),
        // Home row: a o e u i d h t n s
        ('a', 'o'),
        ('o', 'e'),
        ('e', 'u'),
        ('u', 'i'),
        ('i', 'd'),
        ('d', 'h'),
        ('h', 't'),
        ('t', 'n'),
        ('n', 's'),
        // Bottom row: ; q j k x b m w v z
        (';', 'q'),
        ('q', 'j'),
        ('j', 'k'),
        ('k', 'x'),
        ('x', 'b'),
        ('b', 'm'),
        ('m', 'w'),
        ('w', 'v'),
        ('v', 'z'),
    ];

    for &(a, b) in &adjacent_pairs {
        matrix = matrix.add_symmetric_confusion(a, b, 0.15);
    }

    matrix
}

/// Pre-built OCR confusion matrix.
///
/// Models common OCR (Optical Character Recognition) errors.
pub fn ocr_confusion_matrix() -> ConfusionMatrix {
    let mut matrix = ConfusionMatrix::new();

    // Common OCR confusions
    let ocr_pairs = [
        // Numbers and letters
        (('0', 'O'), 0.25),
        (('O', '0'), 0.25),
        (('0', 'o'), 0.15),
        (('o', '0'), 0.15),
        (('1', 'l'), 0.30),
        (('l', '1'), 0.30),
        (('1', 'I'), 0.25),
        (('I', '1'), 0.25),
        (('1', 'i'), 0.15),
        (('i', '1'), 0.15),
        (('l', 'I'), 0.20),
        (('I', 'l'), 0.20),
        (('5', 'S'), 0.15),
        (('S', '5'), 0.15),
        (('6', 'G'), 0.10),
        (('G', '6'), 0.10),
        (('8', 'B'), 0.12),
        (('B', '8'), 0.12),
        (('2', 'Z'), 0.08),
        (('Z', '2'), 0.08),
        // Similar letters
        (('m', 'n'), 0.10),
        (('n', 'm'), 0.10),
        // Note: rn looks like m, but we can't represent multi-char confusions here
        (('c', 'e'), 0.08),
        (('e', 'c'), 0.08),
        (('c', 'o'), 0.06),
        (('o', 'c'), 0.06),
        (('h', 'n'), 0.07),
        (('n', 'h'), 0.07),
        (('u', 'v'), 0.10),
        (('v', 'u'), 0.10),
        // Note: w can look like vv, but we can't represent multi-char confusions here
        (('f', 't'), 0.08),
        (('t', 'f'), 0.08),
        // Punctuation
        (('.', ','), 0.20),
        ((',', '.'), 0.20),
        ((':', ';'), 0.25),
        ((';', ':'), 0.25),
        (('\'', '`'), 0.15),
        (('`', '\''), 0.15),
    ];

    for &((observed, intended), prob) in &ocr_pairs {
        if observed.len_utf8() == 1 && intended.len_utf8() == 1 {
            matrix = matrix.add_confusion(observed, intended, prob);
        }
    }

    // Common OCR deletions (characters that get "swallowed")
    for c in [' ', '.', ',', '-'] {
        matrix = matrix.add_deletion(c, 0.05);
    }

    matrix
}

/// Pre-built mobile/touchscreen keyboard confusion matrix.
pub fn mobile_keyboard_matrix() -> ConfusionMatrix {
    let mut matrix = qwerty_keyboard_matrix();

    // Mobile keyboards have larger touch targets, more autocorrect issues
    // and different error patterns

    // Increase adjacent key probabilities (fat finger syndrome)
    let mobile_adjacent = [
        ('a', 's', 0.20),
        ('s', 'd', 0.20),
        ('d', 'f', 0.20),
        ('q', 'w', 0.22),
        ('w', 'e', 0.22),
        ('e', 'r', 0.22),
        ('z', 'x', 0.18),
        ('x', 'c', 0.18),
        ('c', 'v', 0.18),
    ];

    for &(a, b, prob) in &mobile_adjacent {
        matrix = matrix.add_symmetric_confusion(a, b, prob);
    }

    // Common autocorrect-related issues (not really confusions, but similar effect)
    // These are characters that get auto-replaced incorrectly
    let autocorrect_pairs = [
        (('i', 'I'), 0.10), // Auto-capitalization
        (('u', 'I'), 0.05), // Common autocorrect error
        (('s', 'a'), 0.08), // Swipe keyboard error
    ];

    for &((a, b), prob) in &autocorrect_pairs {
        matrix = matrix.add_confusion(a, b, prob);
    }

    matrix
}

/// Configuration for the confusion layer.
#[derive(Clone, Debug)]
pub struct ConfusionLayerConfig {
    /// Minimum confusion probability to consider (default: 0.01)
    pub confusion_threshold: f64,
    /// Maximum number of corrections per word (default: 10)
    pub max_corrections_per_word: usize,
    /// Maximum edit distance for confusion-based corrections (default: 3)
    pub max_edit_distance: usize,
    /// Whether to keep original edges (default: true)
    pub keep_original: bool,
    /// Case-insensitive matching (default: true)
    pub case_insensitive: bool,
    /// Use log probabilities for scoring (default: true)
    pub use_log_probs: bool,
    /// Minimum word length to attempt correction (default: 2)
    pub min_word_length: usize,
}

impl Default for ConfusionLayerConfig {
    fn default() -> Self {
        Self {
            confusion_threshold: 0.01,
            max_corrections_per_word: 10,
            max_edit_distance: 3,
            keep_original: true,
            case_insensitive: true,
            use_log_probs: true,
            min_word_length: 2,
        }
    }
}

/// Confusion correction layer.
///
/// Uses a confusion matrix to model character-level substitution,
/// deletion, and insertion errors, producing weighted correction candidates.
pub struct ConfusionLayer<W: Semiring> {
    matrix: Arc<ConfusionMatrix>,
    dictionary: Option<Arc<HashSet<String>>>,
    config: ConfusionLayerConfig,
    _phantom: PhantomData<W>,
}

impl<W: Semiring> ConfusionLayer<W> {
    /// Create a new confusion layer with the given matrix.
    pub fn new(matrix: ConfusionMatrix) -> Self {
        Self {
            matrix: Arc::new(matrix),
            dictionary: None,
            config: ConfusionLayerConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create a confusion layer for QWERTY keyboard typos.
    pub fn qwerty_keyboard() -> Self {
        Self::new(qwerty_keyboard_matrix())
    }

    /// Create a confusion layer for Dvorak keyboard typos.
    pub fn dvorak_keyboard() -> Self {
        Self::new(dvorak_keyboard_matrix())
    }

    /// Create a confusion layer for OCR errors.
    pub fn ocr() -> Self {
        Self::new(ocr_confusion_matrix())
    }

    /// Create a confusion layer for mobile keyboard errors.
    pub fn mobile_keyboard() -> Self {
        Self::new(mobile_keyboard_matrix())
    }

    /// Add a dictionary for validation.
    pub fn with_dictionary<S: AsRef<str>>(mut self, words: impl IntoIterator<Item = S>) -> Self {
        let dict: HashSet<String> = words
            .into_iter()
            .map(|w| w.as_ref().to_lowercase())
            .collect();
        self.dictionary = Some(Arc::new(dict));
        self
    }

    /// Set the confusion threshold.
    pub fn with_confusion_threshold(mut self, threshold: f64) -> Self {
        self.config.confusion_threshold = threshold;
        self
    }

    /// Set the maximum corrections per word.
    pub fn with_max_corrections(mut self, max: usize) -> Self {
        self.config.max_corrections_per_word = max;
        self
    }

    /// Set the maximum edit distance.
    pub fn with_max_edit_distance(mut self, distance: usize) -> Self {
        self.config.max_edit_distance = distance;
        self
    }

    /// Set whether to keep original edges.
    pub fn with_keep_original(mut self, keep: bool) -> Self {
        self.config.keep_original = keep;
        self
    }

    /// Get the confusion matrix.
    pub fn matrix(&self) -> &ConfusionMatrix {
        &self.matrix
    }

    /// Get the configuration.
    pub fn config(&self) -> &ConfusionLayerConfig {
        &self.config
    }

    /// Generate confusion-based corrections for a word.
    ///
    /// Returns a list of (corrected_word, probability) pairs.
    pub fn generate_corrections(&self, word: &str) -> Vec<(String, f64)> {
        if word.len() < self.config.min_word_length {
            return vec![];
        }

        let chars: Vec<char> = word.chars().collect();
        let mut corrections: Vec<(String, f64)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // Generate single-character substitutions
        for (i, &c) in chars.iter().enumerate() {
            for (intended, prob) in self.matrix.sources_for(c) {
                if prob >= self.config.confusion_threshold {
                    let mut new_chars = chars.clone();
                    new_chars[i] = intended;
                    let new_word: String = new_chars.into_iter().collect();

                    if self.is_valid_correction(&new_word) && !seen.contains(&new_word) {
                        let log_prob = if self.config.use_log_probs {
                            prob.ln()
                        } else {
                            prob
                        };
                        corrections.push((new_word.clone(), log_prob));
                        seen.insert(new_word);
                    }
                }
            }
        }

        // Generate single-character deletions (character was inserted by error)
        for i in 0..chars.len() {
            let deletion_prob = self.matrix.insertion_prob(chars[i]);
            if deletion_prob >= self.config.confusion_threshold {
                let new_word: String = chars
                    .iter()
                    .enumerate()
                    .filter(|&(j, _)| j != i)
                    .map(|(_, &c)| c)
                    .collect();

                if self.is_valid_correction(&new_word) && !seen.contains(&new_word) {
                    let log_prob = if self.config.use_log_probs {
                        deletion_prob.ln()
                    } else {
                        deletion_prob
                    };
                    corrections.push((new_word.clone(), log_prob));
                    seen.insert(new_word);
                }
            }
        }

        // Generate single-character insertions (character was deleted by error)
        for char_to_insert in 'a'..='z' {
            let insertion_prob = self.matrix.deletion_prob(char_to_insert);
            if insertion_prob >= self.config.confusion_threshold {
                // Try inserting at each position
                for i in 0..=chars.len() {
                    let mut new_chars = chars.clone();
                    new_chars.insert(i, char_to_insert);
                    let new_word: String = new_chars.into_iter().collect();

                    if self.is_valid_correction(&new_word) && !seen.contains(&new_word) {
                        let log_prob = if self.config.use_log_probs {
                            insertion_prob.ln()
                        } else {
                            insertion_prob
                        };
                        corrections.push((new_word.clone(), log_prob));
                        seen.insert(new_word);
                    }
                }
            }
        }

        // Sort by probability (highest first) and limit
        corrections.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        corrections.truncate(self.config.max_corrections_per_word);

        corrections
    }

    /// Check if a correction is valid (in dictionary if provided).
    fn is_valid_correction(&self, word: &str) -> bool {
        match &self.dictionary {
            Some(dict) => {
                let lookup = if self.config.case_insensitive {
                    word.to_lowercase()
                } else {
                    word.to_string()
                };
                dict.contains(&lookup)
            }
            None => true, // No dictionary means accept all corrections
        }
    }

    /// Convert probability to weight cost.
    fn prob_to_cost(&self, log_prob: f64) -> f64 {
        // For tropical semiring, cost = -log_prob
        // Higher probability = lower cost
        if self.config.use_log_probs {
            -log_prob
        } else {
            -log_prob.ln()
        }
    }
}

impl<W, B> CorrectionLayer<W, B> for ConfusionLayer<W>
where
    W: Semiring + From<TropicalWeight>,
    B: LatticeBackend + Clone,
{
    fn name(&self) -> &str {
        "confusion"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        let backend = lattice.backend().clone();
        let mut builder = LatticeBuilder::new(backend);

        let mut added_edges: HashSet<(u32, u32, String)> = HashSet::new();

        for edge in lattice.edges() {
            let word = match lattice.word(edge.label) {
                Some(w) => w.to_string(),
                None => continue,
            };

            let source = edge.source.value();
            let target = edge.target.value();

            // Keep original edge if configured
            if self.config.keep_original {
                builder.add_correction(
                    source as usize,
                    target as usize,
                    &word,
                    edge.weight.clone(),
                    edge.metadata.clone(),
                );
                added_edges.insert((source, target, word.clone()));
            }

            // Generate confusion-based corrections
            let corrections = self.generate_corrections(&word);

            for (correction, log_prob) in corrections {
                if added_edges.contains(&(source, target, correction.clone())) {
                    continue;
                }

                let cost = self.prob_to_cost(log_prob);
                let edit_weight = W::from(TropicalWeight::new(cost));
                let new_weight = edge.weight.clone().times(&edit_weight);

                let mut metadata = edge.metadata.clone();
                metadata.is_original = false;

                builder.add_correction(
                    source as usize,
                    target as usize,
                    &correction,
                    new_weight,
                    metadata,
                );
                added_edges.insert((source, target, correction));
            }
        }

        let num_nodes = lattice.num_nodes();
        Ok(builder.build(num_nodes))
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        self.matrix.num_confusions() > 0
    }

    fn estimated_reduction(&self) -> f64 {
        // This layer typically increases paths
        1.0 + (self.config.max_corrections_per_word as f64 * 0.2)
    }
}

impl<W: Semiring> Clone for ConfusionLayer<W> {
    fn clone(&self) -> Self {
        Self {
            matrix: Arc::clone(&self.matrix),
            dictionary: self.dictionary.clone(),
            config: self.config.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<W: Semiring> Debug for ConfusionLayer<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfusionLayer")
            .field("config", &self.config)
            .field("num_confusions", &self.matrix.num_confusions())
            .field("has_dictionary", &self.dictionary.is_some())
            .finish()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::EdgeMetadata;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_confusion_matrix_creation() {
        let matrix = ConfusionMatrix::new()
            .add_confusion('a', 's', 0.1)
            .add_symmetric_confusion('m', 'n', 0.15);

        assert!((matrix.confusion_prob('a', 's') - 0.1).abs() < 0.001);
        assert!((matrix.confusion_prob('m', 'n') - 0.15).abs() < 0.001);
        assert!((matrix.confusion_prob('n', 'm') - 0.15).abs() < 0.001);
    }

    #[test]
    fn test_confusion_matrix_from_pairs() {
        let matrix = ConfusionMatrix::from_pairs(&[(('a', 'b'), 0.2), (('c', 'd'), 0.3)]);

        assert!((matrix.confusion_prob('a', 'b') - 0.2).abs() < 0.001);
        assert!((matrix.confusion_prob('c', 'd') - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_confusions_for() {
        let matrix = ConfusionMatrix::new()
            .add_confusion('a', 'q', 0.1)
            .add_confusion('s', 'q', 0.15)
            .add_confusion('z', 'q', 0.05);

        let confusions = matrix.confusions_for('q');
        assert_eq!(confusions.len(), 3);
    }

    #[test]
    fn test_sources_for() {
        let matrix = ConfusionMatrix::new()
            .add_confusion('a', 'q', 0.1)
            .add_confusion('a', 'w', 0.2)
            .add_confusion('a', 's', 0.15);

        let sources = matrix.sources_for('a');
        assert_eq!(sources.len(), 3);
    }

    #[test]
    fn test_qwerty_keyboard_matrix() {
        let matrix = qwerty_keyboard_matrix();

        // Adjacent keys should have confusion probability
        assert!(matrix.confusion_prob('w', 'q') > 0.0);
        assert!(matrix.confusion_prob('e', 'w') > 0.0);
        assert!(matrix.confusion_prob('s', 'a') > 0.0);
    }

    #[test]
    fn test_ocr_confusion_matrix() {
        let matrix = ocr_confusion_matrix();

        // Common OCR confusions
        assert!(matrix.confusion_prob('0', 'O') > 0.0);
        assert!(matrix.confusion_prob('1', 'l') > 0.0);
        assert!(matrix.confusion_prob('l', 'I') > 0.0);
    }

    #[test]
    fn test_confusion_layer_creation() {
        let layer = ConfusionLayer::<TropicalWeight>::qwerty_keyboard();
        assert!(layer.matrix().num_confusions() > 0);
    }

    #[test]
    fn test_confusion_layer_with_dictionary() {
        let layer = ConfusionLayer::<TropicalWeight>::qwerty_keyboard()
            .with_dictionary(vec!["hello", "world", "hallo"]);

        // Should only return valid dictionary words
        let corrections = layer.generate_corrections("hello");
        for (word, _) in &corrections {
            assert!(["hello", "world", "hallo"].contains(&word.as_str()));
        }
    }

    #[test]
    fn test_generate_corrections() {
        // Create a simple matrix
        let matrix = ConfusionMatrix::new()
            .add_confusion('a', 'e', 0.2) // 'a' was typed when 'e' intended
            .add_confusion('o', 'a', 0.15); // 'o' was typed when 'a' intended

        let layer = ConfusionLayer::<TropicalWeight>::new(matrix).with_confusion_threshold(0.01);

        let corrections = layer.generate_corrections("hallo");

        // Should find "hello" (a->e substitution)
        let words: Vec<&str> = corrections.iter().map(|(w, _)| w.as_str()).collect();
        assert!(words.contains(&"hello"));
    }

    #[test]
    fn test_layer_config() {
        let config = ConfusionLayerConfig::default();

        assert!((config.confusion_threshold - 0.01).abs() < 0.001);
        assert_eq!(config.max_corrections_per_word, 10);
        assert_eq!(config.max_edit_distance, 3);
        assert!(config.keep_original);
    }

    #[test]
    fn test_layer_apply() {
        let matrix = ConfusionMatrix::new().add_confusion('a', 'e', 0.2);

        let layer = ConfusionLayer::<TropicalWeight>::new(matrix).with_confusion_threshold(0.01);

        let mut backend = HashMapBackend::new();
        let hallo_id = backend.intern("hallo");

        let mut builder: LatticeBuilder<TropicalWeight, HashMapBackend> =
            LatticeBuilder::new(backend);
        builder.add_correction_by_id(
            0,
            1,
            hallo_id,
            TropicalWeight::one(),
            EdgeMetadata::default(),
        );
        let lattice = builder.build(1);

        let result = layer.apply(&lattice).expect("should apply");

        // Should have at least the original edge
        assert!(result.num_edges() >= 1);
    }

    #[test]
    fn test_layer_name() {
        let layer = ConfusionLayer::<TropicalWeight>::qwerty_keyboard();
        assert_eq!(
            CorrectionLayer::<TropicalWeight, HashMapBackend>::name(&layer),
            "confusion"
        );
    }

    #[test]
    fn test_layer_clone() {
        let layer = ConfusionLayer::<TropicalWeight>::ocr().with_confusion_threshold(0.05);

        let cloned = layer.clone();
        assert!((cloned.config().confusion_threshold - 0.05).abs() < 0.001);
    }

    #[test]
    fn test_layer_debug() {
        let layer = ConfusionLayer::<TropicalWeight>::qwerty_keyboard();
        let debug_str = format!("{:?}", layer);

        assert!(debug_str.contains("ConfusionLayer"));
        assert!(debug_str.contains("num_confusions"));
    }

    #[test]
    fn test_dvorak_keyboard_matrix() {
        let matrix = dvorak_keyboard_matrix();

        // Dvorak adjacent keys
        assert!(matrix.confusion_prob('e', 'o') > 0.0);
        assert!(matrix.confusion_prob('a', 'o') > 0.0);
    }

    #[test]
    fn test_mobile_keyboard_matrix() {
        let matrix = mobile_keyboard_matrix();

        // Should have higher probabilities than regular QWERTY
        assert!(matrix.num_confusions() > 0);
    }

    #[test]
    fn test_deletion_probability() {
        let matrix = ConfusionMatrix::new().add_deletion('e', 0.1);

        assert!((matrix.deletion_prob('e') - 0.1).abs() < 0.001);
        assert!(matrix.deletion_prob('x') < 0.01); // Default
    }

    #[test]
    fn test_insertion_probability() {
        let matrix = ConfusionMatrix::new().add_insertion('e', 0.05);

        assert!((matrix.insertion_prob('e') - 0.05).abs() < 0.001);
        assert!(matrix.insertion_prob('x') < 0.01); // Default
    }

    #[test]
    fn test_min_word_length() {
        let layer = ConfusionLayer::<TropicalWeight>::qwerty_keyboard();

        // Very short words should get no corrections
        let corrections = layer.generate_corrections("a");
        assert!(corrections.is_empty());
    }

    #[test]
    fn test_estimated_reduction() {
        let layer = ConfusionLayer::<TropicalWeight>::qwerty_keyboard().with_max_corrections(10);

        // Use the trait method with explicit type annotation
        let reduction = <ConfusionLayer<TropicalWeight> as CorrectionLayer<
            TropicalWeight,
            HashMapBackend,
        >>::estimated_reduction(&layer);
        assert!(reduction > 1.0);
    }

    #[test]
    fn test_can_apply() {
        let layer_with_confusions = ConfusionLayer::<TropicalWeight>::qwerty_keyboard();
        let layer_empty = ConfusionLayer::<TropicalWeight>::new(ConfusionMatrix::new());

        let backend = HashMapBackend::new();
        let lattice: Lattice<TropicalWeight, HashMapBackend> =
            LatticeBuilder::new(backend).build(0);

        assert!(layer_with_confusions.can_apply(&lattice));
        assert!(!layer_empty.can_apply(&lattice));
    }

    #[test]
    fn test_prob_to_cost() {
        let layer = ConfusionLayer::<TropicalWeight>::qwerty_keyboard();

        // Higher log prob (closer to 0) should have lower cost
        let cost_high = layer.prob_to_cost(-0.5); // High prob
        let cost_low = layer.prob_to_cost(-2.0); // Lower prob

        assert!(cost_high < cost_low);
    }

    #[test]
    fn test_identity_probability() {
        let matrix = ConfusionMatrix::new()
            .add_confusion('a', 'b', 0.1)
            .add_confusion('c', 'b', 0.05);

        // Identity should be high (1 - total error rate)
        let identity_prob = matrix.confusion_prob('b', 'b');
        assert!(identity_prob > 0.8);
    }
}
