//! Confusion transducer for character-level error modeling.
//!
//! Models character-level confusions from various sources:
//! - **Keyboard typos**: Adjacent key substitutions on QWERTY/Dvorak layouts
//! - **OCR errors**: Character misrecognition (0/O, l/1, rn/m)
//! - **Learned confusions**: Trained from aligned correct/observed pairs
//!
//! # Mathematical Model
//!
//! The confusion matrix encodes conditional probabilities P(observed | intended)
//! in log space (negative log probabilities). The transducer maps from intended
//! characters to observed characters with appropriate weights.
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::error_models::{ConfusionTransducer, qwerty_confusion_matrix};
//! use lling_llang::semiring::TropicalWeight;
//!
//! // Use pre-built QWERTY keyboard confusion matrix
//! let matrix = qwerty_confusion_matrix();
//! let transducer = ConfusionTransducer::<TropicalWeight>::from_matrix(matrix);
//! let fst = transducer.build();
//! ```

use std::collections::HashMap;
use std::marker::PhantomData;

use crate::semiring::{Semiring, TropicalWeight};
use crate::wfst::{MutableWfst, VectorWfst};

/// Configuration for building confusion transducers.
#[derive(Clone, Debug)]
pub struct ConfusionConfig {
    /// Cost for correct (identity) mappings
    pub identity_cost: f64,
    /// Default cost for unspecified substitutions
    pub default_substitution_cost: f64,
    /// Default cost for deletions
    pub default_deletion_cost: f64,
    /// Default cost for insertions
    pub default_insertion_cost: f64,
    /// Whether to include identity mappings in the transducer
    pub include_identity: bool,
    /// Maximum number of confusions per input character
    pub max_confusions_per_char: Option<usize>,
}

impl Default for ConfusionConfig {
    fn default() -> Self {
        ConfusionConfig {
            identity_cost: 0.0,
            default_substitution_cost: 2.0,
            default_deletion_cost: 1.5,
            default_insertion_cost: 1.5,
            include_identity: true,
            max_confusions_per_char: None,
        }
    }
}

/// Character-level confusion matrix.
///
/// Stores conditional probabilities P(observed | intended) in negative log space.
/// Lower costs indicate more likely confusions.
#[derive(Clone, Debug, Default)]
pub struct ConfusionMatrix {
    /// Substitution confusions: (intended, observed) -> cost
    /// P(observed | intended) = exp(-cost)
    substitutions: HashMap<(char, char), f64>,
    /// Deletion probabilities: intended -> cost of deleting
    deletions: HashMap<char, f64>,
    /// Insertion probabilities: observed -> cost of spurious insertion
    insertions: HashMap<char, f64>,
    /// Transposition pairs: (char1, char2) -> cost of swapping
    transpositions: HashMap<(char, char), f64>,
}

impl ConfusionMatrix {
    /// Create an empty confusion matrix.
    pub fn new() -> Self {
        ConfusionMatrix::default()
    }

    /// Add a substitution confusion: intended character -> observed character.
    ///
    /// The cost should be in negative log probability space (lower = more likely).
    pub fn add_substitution(&mut self, intended: char, observed: char, cost: f64) -> &mut Self {
        self.substitutions.insert((intended, observed), cost);
        self
    }

    /// Add symmetric substitution (both directions with same cost).
    pub fn add_symmetric_substitution(&mut self, a: char, b: char, cost: f64) -> &mut Self {
        self.substitutions.insert((a, b), cost);
        self.substitutions.insert((b, a), cost);
        self
    }

    /// Add a deletion cost for a character.
    pub fn add_deletion(&mut self, intended: char, cost: f64) -> &mut Self {
        self.deletions.insert(intended, cost);
        self
    }

    /// Add an insertion cost for a character.
    pub fn add_insertion(&mut self, observed: char, cost: f64) -> &mut Self {
        self.insertions.insert(observed, cost);
        self
    }

    /// Add a transposition cost for a character pair.
    pub fn add_transposition(&mut self, a: char, b: char, cost: f64) -> &mut Self {
        self.transpositions.insert((a, b), cost);
        self.transpositions.insert((b, a), cost);
        self
    }

    /// Get substitution cost, if defined.
    pub fn substitution_cost(&self, intended: char, observed: char) -> Option<f64> {
        self.substitutions.get(&(intended, observed)).copied()
    }

    /// Get deletion cost for a character.
    pub fn deletion_cost(&self, intended: char) -> Option<f64> {
        self.deletions.get(&intended).copied()
    }

    /// Get insertion cost for a character.
    pub fn insertion_cost(&self, observed: char) -> Option<f64> {
        self.insertions.get(&observed).copied()
    }

    /// Get transposition cost for a character pair.
    pub fn transposition_cost(&self, a: char, b: char) -> Option<f64> {
        self.transpositions.get(&(a, b)).copied()
    }

    /// Get all substitutions for a given intended character.
    pub fn substitutions_for(&self, intended: char) -> impl Iterator<Item = (char, f64)> + '_ {
        self.substitutions
            .iter()
            .filter(move |((i, _), _)| *i == intended)
            .map(|((_, o), c)| (*o, *c))
    }

    /// Get all characters that can substitute for the given intended character.
    pub fn confusable_with(&self, intended: char) -> Vec<(char, f64)> {
        self.substitutions_for(intended).collect()
    }

    /// Merge another confusion matrix into this one.
    /// On conflict, keeps the lower cost (more likely confusion).
    pub fn merge(&mut self, other: &ConfusionMatrix) {
        for ((i, o), cost) in &other.substitutions {
            self.substitutions
                .entry((*i, *o))
                .and_modify(|c| *c = c.min(*cost))
                .or_insert(*cost);
        }
        for (c, cost) in &other.deletions {
            self.deletions
                .entry(*c)
                .and_modify(|c| *c = c.min(*cost))
                .or_insert(*cost);
        }
        for (c, cost) in &other.insertions {
            self.insertions
                .entry(*c)
                .and_modify(|c| *c = c.min(*cost))
                .or_insert(*cost);
        }
        for ((a, b), cost) in &other.transpositions {
            self.transpositions
                .entry((*a, *b))
                .and_modify(|c| *c = c.min(*cost))
                .or_insert(*cost);
        }
    }

    /// Get the alphabet of all characters mentioned in the matrix.
    pub fn alphabet(&self) -> Vec<char> {
        let mut chars: Vec<char> = self
            .substitutions
            .keys()
            .flat_map(|(a, b)| [*a, *b])
            .chain(self.deletions.keys().copied())
            .chain(self.insertions.keys().copied())
            .chain(self.transpositions.keys().flat_map(|(a, b)| [*a, *b]))
            .collect();
        chars.sort();
        chars.dedup();
        chars
    }

    /// Number of substitution entries.
    pub fn num_substitutions(&self) -> usize {
        self.substitutions.len()
    }

    /// Number of deletion entries.
    pub fn num_deletions(&self) -> usize {
        self.deletions.len()
    }

    /// Number of insertion entries.
    pub fn num_insertions(&self) -> usize {
        self.insertions.len()
    }
}

/// Confusion transducer that maps intended characters to observed characters.
///
/// The transducer accepts input:output pairs where the input is the intended
/// (correct) text and the output is the observed (potentially erroneous) text.
#[derive(Clone, Debug)]
pub struct ConfusionTransducer<W: Semiring> {
    matrix: ConfusionMatrix,
    config: ConfusionConfig,
    _phantom: PhantomData<W>,
}

impl<W: Semiring> ConfusionTransducer<W> {
    /// Create a confusion transducer from a confusion matrix.
    pub fn from_matrix(matrix: ConfusionMatrix) -> Self {
        ConfusionTransducer {
            matrix,
            config: ConfusionConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(matrix: ConfusionMatrix, config: ConfusionConfig) -> Self {
        ConfusionTransducer {
            matrix,
            config,
            _phantom: PhantomData,
        }
    }

    /// Get the underlying confusion matrix.
    pub fn matrix(&self) -> &ConfusionMatrix {
        &self.matrix
    }

    /// Get the configuration.
    pub fn config(&self) -> &ConfusionConfig {
        &self.config
    }

    /// Build the confusion WFST.
    ///
    /// Creates a single-state transducer where:
    /// - Each arc maps input character to output character with appropriate weight
    /// - Identity mappings (correct) have the identity cost
    /// - Substitutions have their matrix-defined costs
    /// - Deletions map input to epsilon (represented as consuming input, no output)
    /// - Insertions map epsilon to output (represented as no input, producing output)
    pub fn build(&self) -> VectorWfst<char, W>
    where
        W: From<TropicalWeight>,
    {
        let mut fst = VectorWfst::new();
        let state = fst.add_state();
        fst.set_start(state);
        fst.set_final(state, W::one());

        let alphabet = self.matrix.alphabet();

        // Add identity and substitution arcs
        for &c in &alphabet {
            // Identity mapping
            if self.config.include_identity {
                let weight = W::from(TropicalWeight::new(self.config.identity_cost));
                fst.add_arc(state, Some(c), Some(c), state, weight);
            }

            // Substitutions for this character
            let mut subs: Vec<_> = self.matrix.substitutions_for(c).collect();

            // Sort by cost and optionally limit
            subs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            if let Some(max) = self.config.max_confusions_per_char {
                subs.truncate(max);
            }

            for (observed, cost) in subs {
                if observed != c {
                    // Skip identity since we handled it above
                    let weight = W::from(TropicalWeight::new(cost));
                    fst.add_arc(state, Some(c), Some(observed), state, weight);
                }
            }
        }

        fst
    }

    /// Build a transducer that also handles deletions and insertions.
    ///
    /// This creates a more complex structure to handle epsilon transitions.
    /// Deletions are modeled as transitions that consume input without producing output.
    /// Insertions are modeled as transitions that produce output without consuming input.
    pub fn build_with_indels(&self) -> VectorWfst<Option<char>, W>
    where
        W: From<TropicalWeight>,
    {
        let mut fst: VectorWfst<Option<char>, W> = VectorWfst::new();
        let state = fst.add_state();
        fst.set_start(state);
        fst.set_final(state, W::one());

        let alphabet = self.matrix.alphabet();

        // Add identity and substitution arcs
        for &c in &alphabet {
            // Identity mapping
            if self.config.include_identity {
                let weight = W::from(TropicalWeight::new(self.config.identity_cost));
                fst.add_arc(state, Some(Some(c)), Some(Some(c)), state, weight);
            }

            // Substitutions
            for (observed, cost) in self.matrix.substitutions_for(c) {
                if observed != c {
                    let weight = W::from(TropicalWeight::new(cost));
                    fst.add_arc(state, Some(Some(c)), Some(Some(observed)), state, weight);
                }
            }

            // Deletion: input -> epsilon
            let del_cost = self
                .matrix
                .deletion_cost(c)
                .unwrap_or(self.config.default_deletion_cost);
            let weight = W::from(TropicalWeight::new(del_cost));
            fst.add_arc(state, Some(Some(c)), Some(None), state, weight);
        }

        // Insertions: epsilon -> output
        for &c in &alphabet {
            let ins_cost = self
                .matrix
                .insertion_cost(c)
                .unwrap_or(self.config.default_insertion_cost);
            let weight = W::from(TropicalWeight::new(ins_cost));
            fst.add_arc(state, Some(None), Some(Some(c)), state, weight);
        }

        fst
    }
}

/// Train a confusion matrix from aligned pairs of (correct, observed) strings.
///
/// This uses simple counting to estimate P(observed | intended).
pub fn train_confusion_matrix(pairs: &[(String, String)], smoothing: f64) -> ConfusionMatrix {
    let mut counts: HashMap<(char, char), f64> = HashMap::new();
    let mut char_counts: HashMap<char, f64> = HashMap::new();

    for (correct, observed) in pairs {
        // Simple character-by-character alignment (assumes same length)
        // For variable-length, would need proper alignment algorithm
        for (c_char, o_char) in correct.chars().zip(observed.chars()) {
            *counts.entry((c_char, o_char)).or_default() += 1.0;
            *char_counts.entry(c_char).or_default() += 1.0;
        }
    }

    let mut matrix = ConfusionMatrix::new();

    // Convert counts to negative log probabilities
    for ((intended, observed), count) in counts {
        let total = char_counts.get(&intended).unwrap_or(&1.0);
        let prob = (count + smoothing) / (total + smoothing * 256.0);
        let cost = -prob.ln();

        if intended != observed {
            matrix.add_substitution(intended, observed, cost);
        }
    }

    matrix
}

// ============================================================================
// Pre-built Confusion Matrices
// ============================================================================

/// QWERTY keyboard layout for computing adjacency.
const QWERTY_ROWS: &[&str] = &[
    "1234567890-=",
    "qwertyuiop[]\\",
    "asdfghjkl;'",
    "zxcvbnm,./",
];

/// Dvorak keyboard layout.
const DVORAK_ROWS: &[&str] = &[
    "1234567890[]",
    "',.pyfgcrl/=\\",
    "aoeuidhtns-",
    ";qjkxbmwvz",
];

/// Build a keyboard confusion matrix from a layout.
fn keyboard_confusion_from_layout(
    rows: &[&str],
    base_cost: f64,
    diagonal_penalty: f64,
) -> ConfusionMatrix {
    let mut matrix = ConfusionMatrix::new();

    // Build position map
    let mut positions: HashMap<char, (usize, usize)> = HashMap::new();
    for (row_idx, row) in rows.iter().enumerate() {
        for (col_idx, c) in row.chars().enumerate() {
            positions.insert(c, (row_idx, col_idx));
            positions.insert(c.to_ascii_uppercase(), (row_idx, col_idx));
        }
    }

    // For each character, find adjacent characters
    for (row_idx, row) in rows.iter().enumerate() {
        for (col_idx, c) in row.chars().enumerate() {
            // Check all 8 directions
            let offsets: [(i32, i32); 8] = [
                (-1, -1),
                (-1, 0),
                (-1, 1),
                (0, -1),
                (0, 1),
                (1, -1),
                (1, 0),
                (1, 1),
            ];

            for (dr, dc) in offsets {
                let new_row = row_idx as i32 + dr;
                let new_col = col_idx as i32 + dc;

                if new_row >= 0 && new_row < rows.len() as i32 {
                    if let Some(adj_char) = rows[new_row as usize].chars().nth(new_col as usize) {
                        // Diagonal adjacency has higher cost
                        let cost = if dr != 0 && dc != 0 {
                            base_cost + diagonal_penalty
                        } else {
                            base_cost
                        };

                        matrix.add_symmetric_substitution(c, adj_char, cost);
                        // Also add uppercase variants
                        matrix.add_symmetric_substitution(
                            c.to_ascii_uppercase(),
                            adj_char.to_ascii_uppercase(),
                            cost,
                        );
                    }
                }
            }
        }
    }

    matrix
}

/// Create a QWERTY keyboard confusion matrix.
///
/// Adjacent keys have low substitution costs, reflecting typical typing errors.
/// The base cost is 0.5 for horizontally/vertically adjacent keys,
/// and 0.7 for diagonally adjacent keys.
pub fn qwerty_confusion_matrix() -> ConfusionMatrix {
    keyboard_confusion_from_layout(QWERTY_ROWS, 0.5, 0.2)
}

/// Create a Dvorak keyboard confusion matrix.
pub fn dvorak_confusion_matrix() -> ConfusionMatrix {
    keyboard_confusion_from_layout(DVORAK_ROWS, 0.5, 0.2)
}

/// Create an OCR confusion matrix for common recognition errors.
///
/// Includes confusions like:
/// - 0/O, 1/l/I, rn/m, cl/d, vv/w
/// - Similar-looking characters across fonts
pub fn ocr_confusion_matrix() -> ConfusionMatrix {
    let mut matrix = ConfusionMatrix::new();

    // Very common OCR confusions (low cost = high probability)
    let high_prob_confusions = [
        ('0', 'O'),
        ('O', '0'),
        ('0', 'o'),
        ('o', '0'),
        ('1', 'l'),
        ('l', '1'),
        ('1', 'I'),
        ('I', '1'),
        ('l', 'I'),
        ('I', 'l'),
        ('5', 'S'),
        ('S', '5'),
        ('8', 'B'),
        ('B', '8'),
        ('2', 'Z'),
        ('Z', '2'),
    ];

    for (a, b) in high_prob_confusions {
        matrix.add_substitution(a, b, 0.3);
    }

    // Common OCR confusions (medium probability)
    let medium_prob_confusions = [
        ('c', 'e'),
        ('e', 'c'),
        ('n', 'h'),
        ('h', 'n'),
        ('u', 'v'),
        ('v', 'u'),
        ('f', 't'),
        ('t', 'f'),
        ('i', 'j'),
        ('j', 'i'),
        ('m', 'n'),
        ('n', 'm'),
        ('a', 'o'),
        ('o', 'a'),
        ('g', 'q'),
        ('q', 'g'),
        ('p', 'P'),
        ('P', 'p'), // Case confusions
        ('k', 'K'),
        ('K', 'k'),
    ];

    for (a, b) in medium_prob_confusions {
        matrix.add_substitution(a, b, 0.7);
    }

    // Less common but possible confusions
    let low_prob_confusions = [
        ('b', 'd'),
        ('d', 'b'),
        ('p', 'q'),
        ('q', 'p'),
        ('6', 'G'),
        ('G', '6'),
        ('9', 'g'),
        ('g', '9'),
    ];

    for (a, b) in low_prob_confusions {
        matrix.add_substitution(a, b, 1.2);
    }

    // Multi-character confusions (rn -> m, etc.) would need special handling
    // in a more sophisticated model

    matrix
}

/// Create a combined confusion matrix with keyboard and OCR errors.
pub fn combined_confusion_matrix() -> ConfusionMatrix {
    let mut matrix = qwerty_confusion_matrix();
    matrix.merge(&ocr_confusion_matrix());
    matrix
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::Wfst;

    #[test]
    fn test_confusion_matrix_basic() {
        let mut matrix = ConfusionMatrix::new();
        matrix.add_substitution('a', 'e', 0.5);
        matrix.add_substitution('a', 'o', 0.8);
        matrix.add_deletion('x', 1.0);
        matrix.add_insertion('z', 1.2);

        assert_eq!(matrix.substitution_cost('a', 'e'), Some(0.5));
        assert_eq!(matrix.substitution_cost('a', 'o'), Some(0.8));
        assert_eq!(matrix.substitution_cost('b', 'c'), None);
        assert_eq!(matrix.deletion_cost('x'), Some(1.0));
        assert_eq!(matrix.insertion_cost('z'), Some(1.2));
    }

    #[test]
    fn test_symmetric_substitution() {
        let mut matrix = ConfusionMatrix::new();
        matrix.add_symmetric_substitution('a', 'e', 0.5);

        assert_eq!(matrix.substitution_cost('a', 'e'), Some(0.5));
        assert_eq!(matrix.substitution_cost('e', 'a'), Some(0.5));
    }

    #[test]
    fn test_confusable_with() {
        let mut matrix = ConfusionMatrix::new();
        matrix.add_substitution('a', 'e', 0.5);
        matrix.add_substitution('a', 'o', 0.8);
        matrix.add_substitution('a', 'i', 1.0);

        let confusable = matrix.confusable_with('a');
        assert_eq!(confusable.len(), 3);
        assert!(confusable.contains(&('e', 0.5)));
        assert!(confusable.contains(&('o', 0.8)));
    }

    #[test]
    fn test_alphabet() {
        let mut matrix = ConfusionMatrix::new();
        matrix.add_substitution('a', 'e', 0.5);
        matrix.add_substitution('b', 'c', 0.5);
        matrix.add_deletion('x', 1.0);

        let alphabet = matrix.alphabet();
        assert!(alphabet.contains(&'a'));
        assert!(alphabet.contains(&'e'));
        assert!(alphabet.contains(&'b'));
        assert!(alphabet.contains(&'c'));
        assert!(alphabet.contains(&'x'));
    }

    #[test]
    fn test_merge() {
        let mut matrix1 = ConfusionMatrix::new();
        matrix1.add_substitution('a', 'e', 0.8);
        matrix1.add_substitution('b', 'c', 0.5);

        let mut matrix2 = ConfusionMatrix::new();
        matrix2.add_substitution('a', 'e', 0.5); // Lower cost
        matrix2.add_substitution('d', 'f', 0.6);

        matrix1.merge(&matrix2);

        // Should take lower cost
        assert_eq!(matrix1.substitution_cost('a', 'e'), Some(0.5));
        // Should keep original
        assert_eq!(matrix1.substitution_cost('b', 'c'), Some(0.5));
        // Should add new
        assert_eq!(matrix1.substitution_cost('d', 'f'), Some(0.6));
    }

    #[test]
    fn test_build_transducer() {
        let mut matrix = ConfusionMatrix::new();
        matrix.add_substitution('a', 'e', 0.5);
        matrix.add_substitution('a', 'o', 0.8);

        let transducer = ConfusionTransducer::<TropicalWeight>::from_matrix(matrix);
        let fst = transducer.build();

        // Should have one state
        assert_eq!(fst.num_states(), 1);

        // Start state should be final
        let start = fst.start();
        assert!(fst.is_final(start));
    }

    #[test]
    fn test_qwerty_matrix() {
        let matrix = qwerty_confusion_matrix();

        // Adjacent keys should have confusions
        // 'q' and 'w' are adjacent
        assert!(matrix.substitution_cost('q', 'w').is_some());
        assert!(matrix.substitution_cost('w', 'q').is_some());

        // 'a' and 's' are adjacent
        assert!(matrix.substitution_cost('a', 's').is_some());
    }

    #[test]
    fn test_ocr_matrix() {
        let matrix = ocr_confusion_matrix();

        // Classic OCR confusions
        assert!(matrix.substitution_cost('0', 'O').is_some());
        assert!(matrix.substitution_cost('1', 'l').is_some());
        assert!(matrix.substitution_cost('l', 'I').is_some());
    }

    #[test]
    fn test_combined_matrix() {
        let matrix = combined_confusion_matrix();

        // Should have both keyboard and OCR confusions
        assert!(matrix.substitution_cost('q', 'w').is_some()); // keyboard
        assert!(matrix.substitution_cost('0', 'O').is_some()); // OCR
    }

    #[test]
    fn test_train_confusion_matrix() {
        let pairs = vec![
            ("hello".to_string(), "hallo".to_string()),
            ("hello".to_string(), "hella".to_string()),
            ("world".to_string(), "warld".to_string()),
        ];

        let matrix = train_confusion_matrix(&pairs, 0.1);

        // 'e' was confused with 'a' twice
        assert!(matrix.substitution_cost('e', 'a').is_some());
        // 'o' was confused with 'a' once
        assert!(matrix.substitution_cost('o', 'a').is_some());
    }

    #[test]
    fn test_config() {
        let matrix = qwerty_confusion_matrix();
        let config = ConfusionConfig {
            identity_cost: 0.1,
            include_identity: true,
            max_confusions_per_char: Some(3),
            ..Default::default()
        };

        let transducer = ConfusionTransducer::<TropicalWeight>::with_config(matrix, config);
        assert_eq!(transducer.config().identity_cost, 0.1);
        assert_eq!(transducer.config().max_confusions_per_char, Some(3));
    }

    #[test]
    fn test_build_with_indels() {
        let mut matrix = ConfusionMatrix::new();
        matrix.add_substitution('a', 'e', 0.5);
        matrix.add_deletion('x', 1.0);
        matrix.add_insertion('z', 1.2);

        let transducer = ConfusionTransducer::<TropicalWeight>::from_matrix(matrix);
        let fst = transducer.build_with_indels();

        // Should have one state
        assert_eq!(fst.num_states(), 1);

        let start = fst.start();
        assert!(fst.is_final(start));
    }
}
