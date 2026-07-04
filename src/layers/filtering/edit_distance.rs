//! Edit distance correction layer for spelling correction.
//!
//! This layer adds alternative correction edges to the lattice based on
//! edit distance (Levenshtein or Damerau-Levenshtein) from a reference dictionary.
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::layers::{EditDistanceLayer, EditDistanceLayerConfig};
//! use lling_llang::semiring::TropicalWeight;
//!
//! // Create a dictionary-based correction layer
//! let dictionary = vec!["hello", "world", "help", "held"];
//! let layer = EditDistanceLayer::<TropicalWeight>::new(dictionary)
//!     .with_max_distance(2)
//!     .with_cost_per_edit(1.0);
//!
//! // Apply to a lattice
//! let corrected = pipeline.apply(&input_lattice)?;
//! ```

use std::collections::HashSet;
use std::marker::PhantomData;
use std::sync::Arc;

use super::super::traits::{CorrectionLayer, LayerResult};
use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticeBuilder};
use crate::semiring::{Semiring, TropicalWeight};

/// Configuration for the edit distance correction layer.
#[derive(Clone, Debug)]
pub struct EditDistanceLayerConfig {
    /// Maximum edit distance to consider (default: 2)
    pub max_distance: usize,
    /// Cost per edit operation (default: 1.0)
    pub cost_per_edit: f64,
    /// Cost multiplier for substitutions vs insert/delete (default: 1.0)
    pub substitution_multiplier: f64,
    /// Cost multiplier for transpositions (Damerau-Levenshtein) (default: 1.0)
    pub transposition_multiplier: f64,
    /// Enable Damerau-Levenshtein (transpositions) (default: true)
    pub enable_transpositions: bool,
    /// Maximum number of corrections to generate per input word (default: 10)
    pub max_corrections_per_word: usize,
    /// Minimum word length to attempt correction (default: 2)
    pub min_word_length: usize,
    /// Case-insensitive matching (default: true)
    pub case_insensitive: bool,
    /// Keep original edges even when corrections are found (default: true)
    pub keep_original: bool,
    /// Weight boost for exact dictionary matches (default: 0.0 = no boost)
    pub exact_match_boost: f64,
}

impl Default for EditDistanceLayerConfig {
    fn default() -> Self {
        EditDistanceLayerConfig {
            max_distance: 2,
            cost_per_edit: 1.0,
            substitution_multiplier: 1.0,
            transposition_multiplier: 1.0,
            enable_transpositions: true,
            max_corrections_per_word: 10,
            min_word_length: 2,
            case_insensitive: true,
            keep_original: true,
            exact_match_boost: 0.0,
        }
    }
}

/// Dictionary for edit distance lookups.
pub trait Dictionary: Send + Sync {
    /// Check if a word exists in the dictionary.
    fn contains(&self, word: &str) -> bool;

    /// Get all words within edit distance of the query.
    fn find_within_distance(&self, query: &str, max_distance: usize) -> Vec<(String, usize)>;

    /// Get the size of the dictionary.
    fn len(&self) -> usize;

    /// Check if the dictionary is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Simple in-memory dictionary implementation.
#[derive(Clone, Debug)]
pub struct InMemoryDictionary {
    words: HashSet<String>,
    words_lower: HashSet<String>,
    case_insensitive: bool,
}

impl InMemoryDictionary {
    /// Create a new dictionary from a list of words.
    pub fn new<S: AsRef<str>>(words: &[S], case_insensitive: bool) -> Self {
        let words_set: HashSet<String> = words.iter().map(|w| w.as_ref().to_string()).collect();
        let words_lower: HashSet<String> = if case_insensitive {
            words_set.iter().map(|w| w.to_lowercase()).collect()
        } else {
            HashSet::new()
        };

        InMemoryDictionary {
            words: words_set,
            words_lower,
            case_insensitive,
        }
    }

    /// Add a word to the dictionary.
    pub fn add(&mut self, word: &str) {
        self.words.insert(word.to_string());
        if self.case_insensitive {
            self.words_lower.insert(word.to_lowercase());
        }
    }
}

impl Dictionary for InMemoryDictionary {
    fn contains(&self, word: &str) -> bool {
        if self.case_insensitive {
            self.words_lower.contains(&word.to_lowercase())
        } else {
            self.words.contains(word)
        }
    }

    fn find_within_distance(&self, query: &str, max_distance: usize) -> Vec<(String, usize)> {
        let query_normalized = if self.case_insensitive {
            query.to_lowercase()
        } else {
            query.to_string()
        };

        let mut results = Vec::new();

        for word in &self.words {
            let word_normalized = if self.case_insensitive {
                word.to_lowercase()
            } else {
                word.clone()
            };

            let distance = levenshtein_distance(&query_normalized, &word_normalized);
            if distance <= max_distance {
                results.push((word.clone(), distance));
            }
        }

        // Sort by distance, then alphabetically
        results.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        results
    }

    fn len(&self) -> usize {
        self.words.len()
    }
}

/// Compute Levenshtein edit distance between two strings.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    // Use two rows for space efficiency
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;

        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };

            curr[j] = (prev[j] + 1) // deletion
                .min(curr[j - 1] + 1) // insertion
                .min(prev[j - 1] + cost); // substitution
        }

        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Compute Damerau-Levenshtein edit distance (includes transpositions).
///
/// Exposed for benchmarking and external comparison against `levenshtein_distance`.
pub fn damerau_levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    // Full matrix needed for transposition lookback
    let mut dp = vec![vec![0usize; n + 1]; m + 1];

    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };

            dp[i][j] = (dp[i - 1][j] + 1) // deletion
                .min(dp[i][j - 1] + 1) // insertion
                .min(dp[i - 1][j - 1] + cost); // substitution

            // Transposition
            if i > 1
                && j > 1
                && a_chars[i - 1] == b_chars[j - 2]
                && a_chars[i - 2] == b_chars[j - 1]
            {
                dp[i][j] = dp[i][j].min(dp[i - 2][j - 2] + cost);
            }
        }
    }

    dp[m][n]
}

/// Edit distance correction layer.
///
/// Adds correction edges to the lattice for words within a specified
/// edit distance of dictionary entries.
pub struct EditDistanceLayer<W: Semiring> {
    dictionary: Arc<dyn Dictionary>,
    config: EditDistanceLayerConfig,
    _phantom: PhantomData<W>,
}

impl<W: Semiring> EditDistanceLayer<W> {
    /// Create a new edit distance layer with an in-memory dictionary.
    pub fn new<S: AsRef<str>>(words: &[S]) -> Self {
        let config = EditDistanceLayerConfig::default();
        let dictionary = InMemoryDictionary::new(words, config.case_insensitive);
        EditDistanceLayer {
            dictionary: Arc::new(dictionary),
            config,
            _phantom: PhantomData,
        }
    }

    /// Create with a custom dictionary implementation.
    pub fn with_dictionary(dictionary: Arc<dyn Dictionary>) -> Self {
        EditDistanceLayer {
            dictionary,
            config: EditDistanceLayerConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create with custom configuration.
    pub fn with_config<S: AsRef<str>>(words: &[S], config: EditDistanceLayerConfig) -> Self {
        let dictionary = InMemoryDictionary::new(words, config.case_insensitive);
        EditDistanceLayer {
            dictionary: Arc::new(dictionary),
            config,
            _phantom: PhantomData,
        }
    }

    /// Set maximum edit distance.
    pub fn with_max_distance(mut self, distance: usize) -> Self {
        self.config.max_distance = distance;
        self
    }

    /// Set cost per edit operation.
    pub fn with_cost_per_edit(mut self, cost: f64) -> Self {
        self.config.cost_per_edit = cost;
        self
    }

    /// Enable or disable transpositions.
    pub fn with_transpositions(mut self, enabled: bool) -> Self {
        self.config.enable_transpositions = enabled;
        self
    }

    /// Set maximum corrections per word.
    pub fn with_max_corrections(mut self, max: usize) -> Self {
        self.config.max_corrections_per_word = max;
        self
    }

    /// Get the configuration.
    pub fn config(&self) -> &EditDistanceLayerConfig {
        &self.config
    }

    /// Get the dictionary.
    pub fn dictionary(&self) -> &dyn Dictionary {
        self.dictionary.as_ref()
    }

    /// Get the layer name (inherent method, doesn't require backend type).
    pub fn layer_name(&self) -> &str {
        "edit-distance"
    }

    /// Get estimated reduction factor (inherent method, doesn't require backend type).
    ///
    /// This layer typically increases paths, so returns > 1.0.
    pub fn estimated_reduction_factor(&self) -> f64 {
        1.0 + (self.config.max_corrections_per_word as f64 * 0.3)
    }

    /// Find corrections for a word.
    pub fn find_corrections(&self, word: &str) -> Vec<(String, f64)> {
        if word.len() < self.config.min_word_length {
            return vec![];
        }

        let candidates = self
            .dictionary
            .find_within_distance(word, self.config.max_distance);

        let mut corrections: Vec<(String, f64)> = candidates
            .into_iter()
            .take(self.config.max_corrections_per_word)
            .map(|(correction, distance)| {
                let cost = self.compute_cost(distance);
                (correction, cost)
            })
            .collect();

        // Apply exact match boost if applicable
        if self.config.exact_match_boost != 0.0 {
            for (correction, cost) in &mut corrections {
                if correction.eq_ignore_ascii_case(word) {
                    *cost -= self.config.exact_match_boost;
                }
            }
        }

        corrections
    }

    /// Compute the cost for a given edit distance.
    fn compute_cost(&self, distance: usize) -> f64 {
        distance as f64 * self.config.cost_per_edit
    }
}

impl<W, B> CorrectionLayer<W, B> for EditDistanceLayer<W>
where
    W: Semiring + From<TropicalWeight>,
    B: LatticeBackend + Clone,
{
    fn name(&self) -> &str {
        "edit-distance"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        // Clone the backend for the new lattice
        let backend = lattice.backend().clone();
        let mut builder = LatticeBuilder::new(backend);

        // Track which edges we've added to avoid duplicates
        let mut added_edges: HashSet<(u32, u32, String)> = HashSet::new();

        // Process each edge in the lattice
        for edge in lattice.edges() {
            let word = match lattice.word(edge.label) {
                Some(w) => w.to_string(),
                None => continue, // Skip edges with unknown labels
            };

            let source = edge.source.value();
            let target = edge.target.value();

            // Always keep original edge if configured
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

            // Find corrections
            let corrections = self.find_corrections(&word);

            for (correction, cost) in corrections {
                // Skip if this would duplicate the original
                if added_edges.contains(&(source, target, correction.clone())) {
                    continue;
                }

                // Compute new weight by adding edit cost
                let edit_weight = W::from(TropicalWeight::new(cost));
                let new_weight = edge.weight.clone().times(&edit_weight);

                // Create metadata indicating this is a correction
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

        // Build the new lattice with the original node count
        let num_nodes = lattice.num_nodes();
        Ok(builder.build(num_nodes))
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        !self.dictionary.is_empty()
    }

    fn estimated_reduction(&self) -> f64 {
        // This layer typically increases paths, so return > 1.0
        // Estimate based on average corrections per word
        1.0 + (self.config.max_corrections_per_word as f64 * 0.3)
    }
}

// Clone implementation requires W: Clone, which Semiring implies
impl<W: Semiring> Clone for EditDistanceLayer<W> {
    fn clone(&self) -> Self {
        EditDistanceLayer {
            dictionary: Arc::clone(&self.dictionary),
            config: self.config.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<W: Semiring> std::fmt::Debug for EditDistanceLayer<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditDistanceLayer")
            .field("config", &self.config)
            .field("dictionary_size", &self.dictionary.len())
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
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("abc", ""), 3);
        assert_eq!(levenshtein_distance("", "abc"), 3);
        assert_eq!(levenshtein_distance("abc", "abc"), 0);
        assert_eq!(levenshtein_distance("abc", "ab"), 1);
        assert_eq!(levenshtein_distance("abc", "abcd"), 1);
        assert_eq!(levenshtein_distance("abc", "adc"), 1);
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn test_damerau_levenshtein_distance() {
        assert_eq!(damerau_levenshtein_distance("abc", "abc"), 0);
        assert_eq!(damerau_levenshtein_distance("ab", "ba"), 1); // transposition
        assert_eq!(damerau_levenshtein_distance("abc", "acb"), 1); // transposition
        assert_eq!(damerau_levenshtein_distance("abc", "bac"), 1); // transposition at start
    }

    #[test]
    fn test_in_memory_dictionary() {
        let words = vec!["hello", "world", "help", "held", "helm"];
        let dict = InMemoryDictionary::new(&words, true);

        assert!(dict.contains("hello"));
        assert!(dict.contains("HELLO")); // case insensitive
        assert!(!dict.contains("missing"));
        assert_eq!(dict.len(), 5);
    }

    #[test]
    fn test_dictionary_find_within_distance() {
        let words = vec!["hello", "hallo", "help", "held", "world"];
        let dict = InMemoryDictionary::new(&words, false);

        let results = dict.find_within_distance("hello", 1);
        assert!(results.iter().any(|(w, _)| w == "hello")); // exact match
        assert!(results.iter().any(|(w, _)| w == "hallo")); // 1 edit

        let results = dict.find_within_distance("hello", 2);
        assert!(results.iter().any(|(w, _)| w == "help")); // 2 edits
        assert!(results.iter().any(|(w, _)| w == "held")); // 2 edits
    }

    #[test]
    fn test_edit_distance_layer_creation() {
        let words = vec!["hello", "world"];
        let layer = EditDistanceLayer::<TropicalWeight>::new(&words)
            .with_max_distance(2)
            .with_cost_per_edit(0.5);

        assert_eq!(layer.config().max_distance, 2);
        assert!((layer.config().cost_per_edit - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_find_corrections() {
        let words = vec!["hello", "hallo", "help", "world"];
        let layer = EditDistanceLayer::<TropicalWeight>::new(&words).with_max_distance(2);

        let corrections = layer.find_corrections("helo");

        // Should find hello and hallo (both 1 edit away)
        let words_found: Vec<&str> = corrections.iter().map(|(w, _)| w.as_str()).collect();
        assert!(words_found.contains(&"hello"));
        assert!(words_found.contains(&"hallo"));
    }

    #[test]
    fn test_find_corrections_respects_max() {
        let words: Vec<String> = (0..100).map(|i| format!("word{}", i)).collect();
        let layer = EditDistanceLayer::<TropicalWeight>::new(&words).with_max_corrections(5);

        // Even with many potential matches, should limit to max
        let corrections = layer.find_corrections("word0");
        assert!(corrections.len() <= 5);
    }

    #[test]
    fn test_layer_apply() {
        let words = vec!["hello", "hallo", "help"];
        let layer = EditDistanceLayer::<TropicalWeight>::new(&words)
            .with_max_distance(2)
            .with_cost_per_edit(1.0);

        // Build a simple lattice with "helo" (misspelled)
        let mut backend = HashMapBackend::new();
        let helo_id = backend.intern("helo");

        let mut builder: LatticeBuilder<TropicalWeight, HashMapBackend> =
            LatticeBuilder::new(backend);
        builder.add_correction_by_id(
            0,
            1,
            helo_id,
            TropicalWeight::one(),
            EdgeMetadata::default(),
        );
        let lattice = builder.build(1);

        // Apply the layer
        let result = layer.apply(&lattice).expect("should apply");

        // Should have more edges now (original + corrections)
        assert!(result.num_edges() >= 1);
    }

    #[test]
    fn test_layer_name() {
        let layer = EditDistanceLayer::<TropicalWeight>::new(&["test"]);
        assert_eq!(layer.layer_name(), "edit-distance");
    }

    #[test]
    fn test_layer_can_apply() {
        let layer_with_dict = EditDistanceLayer::<TropicalWeight>::new(&["test"]);
        let layer_empty = EditDistanceLayer::<TropicalWeight>::new::<&str>(&[]);

        let backend = HashMapBackend::new();
        let lattice: Lattice<TropicalWeight, HashMapBackend> =
            LatticeBuilder::new(backend).build(0);

        assert!(layer_with_dict.can_apply(&lattice));
        assert!(!layer_empty.can_apply(&lattice)); // Empty dictionary can't apply
    }

    #[test]
    fn test_config_default() {
        let config = EditDistanceLayerConfig::default();

        assert_eq!(config.max_distance, 2);
        assert!((config.cost_per_edit - 1.0).abs() < 0.001);
        assert!(config.enable_transpositions);
        assert!(config.case_insensitive);
        assert!(config.keep_original);
    }

    #[test]
    fn test_cost_computation() {
        let layer = EditDistanceLayer::<TropicalWeight>::new(&["test"]).with_cost_per_edit(0.5);

        assert!((layer.compute_cost(0) - 0.0).abs() < 0.001);
        assert!((layer.compute_cost(1) - 0.5).abs() < 0.001);
        assert!((layer.compute_cost(2) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_case_insensitive_corrections() {
        let words = vec!["Hello", "WORLD"];
        let layer = EditDistanceLayer::<TropicalWeight>::new(&words);

        // Should find corrections regardless of case
        let corrections = layer.find_corrections("hello");
        assert!(!corrections.is_empty());

        let corrections = layer.find_corrections("HELLO");
        assert!(!corrections.is_empty());
    }

    #[test]
    fn test_min_word_length() {
        let words = vec!["a", "ab", "abc", "abcd"];
        let config = EditDistanceLayerConfig {
            min_word_length: 3,
            ..Default::default()
        };
        let layer = EditDistanceLayer::<TropicalWeight>::with_config(&words, config);

        // Short words should get no corrections
        let corrections = layer.find_corrections("a");
        assert!(corrections.is_empty());

        let corrections = layer.find_corrections("ab");
        assert!(corrections.is_empty());

        // Longer words should work
        let corrections = layer.find_corrections("abc");
        assert!(!corrections.is_empty());
    }

    #[test]
    fn test_estimated_reduction() {
        let layer = EditDistanceLayer::<TropicalWeight>::new(&["test"]).with_max_corrections(5);

        // Should return > 1.0 since this layer adds paths
        assert!(layer.estimated_reduction_factor() > 1.0);
    }

    #[test]
    fn test_layer_clone() {
        let layer = EditDistanceLayer::<TropicalWeight>::new(&["test"]).with_max_distance(3);

        let cloned = layer.clone();
        assert_eq!(cloned.config().max_distance, 3);
    }

    #[test]
    fn test_layer_debug() {
        let layer = EditDistanceLayer::<TropicalWeight>::new(&["hello", "world"]);
        let debug_str = format!("{:?}", layer);

        assert!(debug_str.contains("EditDistanceLayer"));
        assert!(debug_str.contains("dictionary_size"));
    }
}
