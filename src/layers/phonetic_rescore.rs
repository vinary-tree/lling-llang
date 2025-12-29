//! Phonetic rescoring layer for lattice path reranking.
//!
//! This layer rescores lattice paths based on phonetic similarity,
//! using liblevenshtein's phonetic processing capabilities.
//!
//! # Language Support
//!
//! By default, this layer uses Zompist English phonetic rules. For other
//! languages, provide custom rules using [`PhoneticRescoreLayer::with_rules`].
//! liblevenshtein's `.llev` file format can be used to define custom phonetic
//! rules for any language.
//!
//! # Feature Gate
//!
//! This module is only available when the `phonetic-rescore` feature is enabled.

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use liblevenshtein::phonetic::{
    OnlinePhoneticTransducerChar, RewriteRuleChar, zompist_rules_char,
};

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticeBuilder, NodeId};
use crate::semiring::{NumericalWeight, Semiring};

use super::traits::{CorrectionLayer, LayerError, LayerResult};

/// Default phonetic weight (50% phonetic, 50% original).
pub const DEFAULT_PHONETIC_WEIGHT: f64 = 0.5;

/// Default fuel for phonetic normalization.
pub const DEFAULT_PHONETIC_FUEL: usize = 1000;

/// Maximum context length for tracking word histories.
const MAX_CONTEXT_LEN: usize = 5;

/// Trait for providing reference words for phonetic comparison.
///
/// Implement this to provide the "intended" or "correct" words
/// for phonetic distance calculation.
pub trait PhoneticReference: Send + Sync {
    /// Get the reference word(s) for a position.
    ///
    /// Returns the expected correct word(s) at a given position
    /// for phonetic comparison.
    fn reference_at(&self, position: usize) -> Option<&[String]>;

    /// Check if a word is a known correct word.
    fn is_known(&self, word: &str) -> bool;
}

/// Simple reference implementation using a vocabulary set.
pub struct VocabularyReference {
    vocab: std::collections::HashSet<String>,
}

impl VocabularyReference {
    /// Create a new vocabulary reference.
    pub fn new(words: impl IntoIterator<Item = String>) -> Self {
        Self {
            vocab: words.into_iter().collect(),
        }
    }
}

impl PhoneticReference for VocabularyReference {
    fn reference_at(&self, _position: usize) -> Option<&[String]> {
        None // Position-based reference not supported
    }

    fn is_known(&self, word: &str) -> bool {
        self.vocab.contains(word)
    }
}

/// Reference implementation using expected word sequence.
pub struct SequenceReference {
    words: Vec<Vec<String>>,
}

impl SequenceReference {
    /// Create a new sequence reference with expected words at each position.
    pub fn new(words: Vec<Vec<String>>) -> Self {
        Self { words }
    }

    /// Create from a single word sequence.
    pub fn from_sequence(words: impl IntoIterator<Item = String>) -> Self {
        Self {
            words: words.into_iter().map(|w| vec![w]).collect(),
        }
    }
}

impl PhoneticReference for SequenceReference {
    fn reference_at(&self, position: usize) -> Option<&[String]> {
        self.words.get(position).map(|v| v.as_slice())
    }

    fn is_known(&self, word: &str) -> bool {
        self.words.iter().any(|ws| ws.iter().any(|w| w == word))
    }
}

/// Phonetic rescoring layer.
///
/// This layer adjusts lattice edge weights based on phonetic similarity
/// between the edge labels and reference words. By default it uses
/// liblevenshtein's Zompist phonetic rules for English spelling-to-pronunciation
/// normalization.
///
/// # Language Support
///
/// - **English (default)**: Uses Zompist rules automatically via `new()`
/// - **Other languages**: Use `with_rules()` to provide custom phonetic rules
///
/// Custom rules can be loaded from `.llev` files using liblevenshtein's
/// `RuleSetChar::from_llev()` API.
///
/// # Example
///
/// ```ignore
/// use lling_llang::layers::{PhoneticRescoreLayer, VocabularyReference};
/// use std::sync::Arc;
///
/// // English (using default Zompist rules)
/// let reference = VocabularyReference::new(["hello", "world"]);
/// let layer = PhoneticRescoreLayer::new(Arc::new(reference))
///     .with_weight(0.3);  // 30% phonetic, 70% original
/// let rescored = layer.apply(&lattice)?;
///
/// // Other language (provide custom rules)
/// use liblevenshtein::phonetic::llev::{parse_str, RuleSetChar};
/// let german_rules = RuleSetChar::from_llev(&parse_str("...")?)?;
/// let layer = PhoneticRescoreLayer::with_rules(Arc::new(reference), german_rules.rules);
/// ```
pub struct PhoneticRescoreLayer {
    /// Reference for phonetic comparison.
    reference: Arc<dyn PhoneticReference>,

    /// Phonetic normalization rules.
    rules: Vec<RewriteRuleChar>,

    /// Interpolation weight for phonetic scores (0.0 = ignore, 1.0 = only phonetic).
    weight: f64,

    /// Fuel for phonetic normalization (limits rewrite iterations).
    fuel: usize,

    /// Cache for phonetic normalizations.
    normalization_cache: DashMap<String, String>,

    /// Maximum cache size.
    max_cache_size: usize,
}

impl PhoneticRescoreLayer {
    /// Create a new phonetic rescore layer with default rules.
    ///
    /// Uses Zompist English phonetic rules for normalization.
    pub fn new(reference: Arc<dyn PhoneticReference>) -> Self {
        Self {
            reference,
            rules: zompist_rules_char(),
            weight: DEFAULT_PHONETIC_WEIGHT,
            fuel: DEFAULT_PHONETIC_FUEL,
            normalization_cache: DashMap::new(),
            max_cache_size: 10_000,
        }
    }

    /// Create with custom phonetic rules.
    pub fn with_rules(reference: Arc<dyn PhoneticReference>, rules: Vec<RewriteRuleChar>) -> Self {
        Self {
            reference,
            rules,
            weight: DEFAULT_PHONETIC_WEIGHT,
            fuel: DEFAULT_PHONETIC_FUEL,
            normalization_cache: DashMap::new(),
            max_cache_size: 10_000,
        }
    }

    /// Set the interpolation weight for phonetic scores.
    ///
    /// - 0.0: Ignore phonetic scores entirely
    /// - 0.5: Equal weight to phonetic and original scores (default)
    /// - 1.0: Use only phonetic scores
    pub fn with_weight(mut self, weight: f64) -> Self {
        self.weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Set the normalization fuel limit.
    pub fn with_fuel(mut self, fuel: usize) -> Self {
        self.fuel = fuel;
        self
    }

    /// Set the maximum cache size.
    pub fn with_cache_size(mut self, size: usize) -> Self {
        self.max_cache_size = size;
        self
    }

    /// Get the interpolation weight.
    pub fn weight(&self) -> f64 {
        self.weight
    }

    /// Normalize a word using phonetic rules.
    pub fn normalize(&self, word: &str) -> String {
        // Check cache first
        if let Some(cached) = self.normalization_cache.get(word) {
            return cached.clone();
        }

        // Normalize using streaming transducer
        // OnlinePhoneticTransducerChar takes ownership, so we clone the rules
        let mut transducer = OnlinePhoneticTransducerChar::new(self.rules.clone());
        let mut result = String::new();

        for ch in word.chars() {
            for normalized_ch in transducer.feed(ch) {
                result.push(normalized_ch);
            }
        }

        // Flush remaining output
        for ch in transducer.finish() {
            result.push(ch);
        }

        // Cache if under limit
        if self.normalization_cache.len() < self.max_cache_size {
            self.normalization_cache.insert(word.to_string(), result.clone());
        }

        result
    }

    /// Compute phonetic distance between two words.
    ///
    /// Returns a value in [0.0, 1.0] where 0.0 means identical phonetic forms.
    pub fn phonetic_distance(&self, word1: &str, word2: &str) -> f64 {
        let phone1 = self.normalize(word1);
        let phone2 = self.normalize(word2);

        if phone1 == phone2 {
            return 0.0;
        }

        // Compute normalized Levenshtein distance
        let dist = levenshtein_distance(&phone1, &phone2);
        let max_len = phone1.len().max(phone2.len());

        if max_len == 0 {
            0.0
        } else {
            dist as f64 / max_len as f64
        }
    }

    /// Compute phonetic similarity (1 - distance).
    pub fn phonetic_similarity(&self, word1: &str, word2: &str) -> f64 {
        1.0 - self.phonetic_distance(word1, word2)
    }

    /// Score a word based on phonetic similarity to reference.
    ///
    /// Returns a log-probability-like score (higher = better).
    fn score_word(&self, word: &str, position: usize) -> f64 {
        // If word is known, give it a good score
        if self.reference.is_known(word) {
            return -0.1; // High probability (low cost)
        }

        // Check for position-specific reference
        if let Some(refs) = self.reference.reference_at(position) {
            // Find best phonetic similarity to any reference word
            let best_sim = refs
                .iter()
                .map(|r| self.phonetic_similarity(word, r))
                .fold(0.0_f64, |a, b| a.max(b));

            // Convert similarity to log probability
            // sim=1.0 → -0.1 (good), sim=0.0 → -5.0 (bad)
            return (best_sim * 0.9 + 0.1).ln();
        }

        // Unknown word with no reference - moderate penalty
        -2.0
    }

    /// Interpolate original weight with phonetic score.
    #[inline]
    fn interpolate_weight<W>(&self, orig_weight: W, phonetic_log_prob: f64) -> W
    where
        W: NumericalWeight + From<f64>,
    {
        let orig_val = orig_weight.numerical_value();

        // Convert phonetic log prob to cost space (negate it)
        let phonetic_cost = -phonetic_log_prob;

        // Linear interpolation in cost space
        let interpolated = (1.0 - self.weight) * orig_val + self.weight * phonetic_cost;

        W::from(interpolated)
    }

    /// Compute forward contexts (word histories reaching each node).
    fn compute_forward_contexts<W, B>(
        &self,
        lattice: &mut Lattice<W, B>,
    ) -> LayerResult<HashMap<NodeId, Vec<(Vec<String>, usize)>>>
    where
        W: Semiring,
        B: LatticeBackend,
    {
        let mut context_map: HashMap<NodeId, Vec<(Vec<String>, usize)>> = HashMap::new();

        // Initialize start node with empty context at position 0
        context_map.insert(lattice.start(), vec![(vec![], 0)]);

        // Get topological order for forward processing
        let topo_order = lattice
            .topological_order()
            .ok_or_else(|| LayerError::Other("Lattice contains a cycle".to_string()))?
            .to_vec();

        // Process nodes in topological order
        for node_id in topo_order {
            let current_contexts: Vec<(Vec<String>, usize)> = context_map
                .get(&node_id)
                .cloned()
                .unwrap_or_default();

            if current_contexts.is_empty() {
                continue;
            }

            // Collect outgoing edge info
            let outgoing_info: Vec<(NodeId, Option<String>)> = lattice
                .outgoing_edges(node_id)
                .map(|edge| {
                    let word = lattice.edge_word(edge).map(|s| s.to_string());
                    (edge.target, word)
                })
                .collect();

            // Process each outgoing edge
            for (target, word) in outgoing_info {
                for (ctx, pos) in &current_contexts {
                    let mut new_ctx = ctx.clone();
                    let new_pos = if word.is_some() { pos + 1 } else { *pos };

                    if let Some(ref w) = word {
                        new_ctx.push(w.clone());

                        // Trim context to max length
                        while new_ctx.len() > MAX_CONTEXT_LEN {
                            new_ctx.remove(0);
                        }
                    }

                    context_map
                        .entry(target)
                        .or_default()
                        .push((new_ctx, new_pos));
                }
            }
        }

        Ok(context_map)
    }
}

impl<W, B> CorrectionLayer<W, B> for PhoneticRescoreLayer
where
    W: Semiring + NumericalWeight + From<f64>,
    B: LatticeBackend,
{
    fn name(&self) -> &str {
        "phonetic-rescore"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // Step 1: Compute forward contexts with positions
        let mut lattice_copy = lattice.clone();
        let context_map = self.compute_forward_contexts(&mut lattice_copy)?;

        // Step 2: Build new lattice with phonetic-adjusted weights
        let mut builder = LatticeBuilder::with_capacity(
            lattice.backend().clone(),
            lattice.num_nodes(),
            lattice.num_edges() / lattice.num_nodes().max(1) + 1,
        );

        // Step 3: Process each edge
        for edge in lattice.edges() {
            let word = match lattice.edge_word(edge) {
                Some(w) => w,
                None => {
                    // Epsilon transition - keep original weight
                    let source_pos = lattice
                        .node(edge.source)
                        .and_then(|n| n.position)
                        .unwrap_or(edge.source.0 as usize);
                    let target_pos = lattice
                        .node(edge.target)
                        .and_then(|n| n.position)
                        .unwrap_or(edge.target.0 as usize);

                    builder.add_correction_by_id(
                        source_pos,
                        target_pos,
                        edge.label,
                        edge.weight,
                        edge.metadata.clone(),
                    );
                    continue;
                }
            };

            // Get contexts at source node
            let contexts = context_map
                .get(&edge.source)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);

            // Compute phonetic score based on position
            let phonetic_log_prob = if contexts.is_empty() {
                self.score_word(word, 0)
            } else {
                // Average score over all contexts
                let mut total_score = 0.0;
                let mut count = 0;

                for (_ctx, pos) in contexts {
                    total_score += self.score_word(word, *pos);
                    count += 1;
                }

                if count > 0 {
                    total_score / count as f64
                } else {
                    self.score_word(word, 0)
                }
            };

            // Interpolate weights
            let adjusted_weight = self.interpolate_weight(edge.weight, phonetic_log_prob);

            // Get node positions
            let source_pos = lattice
                .node(edge.source)
                .and_then(|n| n.position)
                .unwrap_or(edge.source.0 as usize);
            let target_pos = lattice
                .node(edge.target)
                .and_then(|n| n.position)
                .unwrap_or(edge.target.0 as usize);

            // Add edge with adjusted weight
            builder.add_correction_by_id(
                source_pos,
                target_pos,
                edge.label,
                adjusted_weight,
                edge.metadata.clone(),
            );
        }

        // Build the rescored lattice
        let end_pos = lattice
            .node(lattice.end())
            .and_then(|n| n.position)
            .unwrap_or(lattice.end().0 as usize);

        Ok(builder.build(end_pos))
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        true
    }

    fn estimated_reduction(&self) -> f64 {
        // Phonetic rescoring doesn't reduce paths, it reweights them
        1.0
    }
}

/// Simple Levenshtein distance implementation for phonetic comparison.
fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let v1: Vec<char> = s1.chars().collect();
    let v2: Vec<char> = s2.chars().collect();
    let m = v1.len();
    let n = v2.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    // Use two-row optimization for space efficiency
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if v1[i - 1] == v2[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::{EdgeMetadata, LatticeBuilder};
    use crate::semiring::TropicalWeight;

    fn create_vocab_reference() -> Arc<dyn PhoneticReference> {
        Arc::new(VocabularyReference::new(
            ["hello", "world", "the", "quick", "brown", "fox"]
                .iter()
                .map(|s| s.to_string()),
        ))
    }

    fn create_sequence_reference() -> Arc<dyn PhoneticReference> {
        Arc::new(SequenceReference::from_sequence(
            ["the", "quick", "brown", "fox"]
                .iter()
                .map(|s| s.to_string()),
        ))
    }

    #[test]
    fn test_layer_creation() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference);

        assert!((layer.weight() - DEFAULT_PHONETIC_WEIGHT).abs() < 0.001);
    }

    #[test]
    fn test_layer_builder() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference)
            .with_weight(0.7)
            .with_fuel(500)
            .with_cache_size(5000);

        assert!((layer.weight() - 0.7).abs() < 0.001);
        assert_eq!(layer.fuel, 500);
        assert_eq!(layer.max_cache_size, 5000);
    }

    #[test]
    fn test_weight_clamping() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference).with_weight(1.5);

        assert!((layer.weight() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_phonetic_normalization() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference);

        // Test that normalization is deterministic
        let norm1 = layer.normalize("phone");
        let norm2 = layer.normalize("phone");
        assert_eq!(norm1, norm2);

        // Test caching
        assert!(layer.normalization_cache.contains_key("phone"));
    }

    #[test]
    fn test_phonetic_distance() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference);

        // Same word should have 0 distance
        let dist = layer.phonetic_distance("hello", "hello");
        assert!(dist.abs() < 0.001);

        // Similar sounding words should have lower distance
        let dist_similar = layer.phonetic_distance("knight", "night");
        let dist_different = layer.phonetic_distance("hello", "world");
        assert!(dist_similar < dist_different);
    }

    #[test]
    fn test_phonetic_similarity() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference);

        // Same word should have 1.0 similarity
        let sim = layer.phonetic_similarity("hello", "hello");
        assert!((sim - 1.0).abs() < 0.001);

        // Similarity should be in [0, 1]
        let sim = layer.phonetic_similarity("cat", "dog");
        assert!(sim >= 0.0 && sim <= 1.0);
    }

    #[test]
    fn test_layer_name() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference);
        let name = <PhoneticRescoreLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::name(
            &layer,
        );
        assert_eq!(name, "phonetic-rescore");
    }

    #[test]
    fn test_estimated_reduction() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference);
        let reduction = <PhoneticRescoreLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::estimated_reduction(&layer);
        assert!((reduction - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_apply_empty_lattice() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference);
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let lattice = builder.build(0);

        let result = <PhoneticRescoreLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(&layer, &lattice);
        assert!(result.is_ok());
        let rescored = result.expect("apply failed");
        assert!(rescored.is_empty());
    }

    #[test]
    fn test_apply_single_edge() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference).with_weight(0.5);

        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        builder.add_correction(0, 1, "hello", TropicalWeight::new(2.0), EdgeMetadata::default());
        let lattice = builder.build(1);

        let result = <PhoneticRescoreLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(&layer, &lattice);
        assert!(result.is_ok());
        let rescored = result.expect("apply failed");

        assert_eq!(rescored.num_edges(), 1);
    }

    #[test]
    fn test_apply_preserves_structure() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference);

        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        builder.add_correction(0, 1, "the", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "a", TropicalWeight::new(2.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "fox", TropicalWeight::new(1.5), EdgeMetadata::default());
        let lattice = builder.build(2);

        let result = <PhoneticRescoreLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(&layer, &lattice);
        assert!(result.is_ok());
        let rescored = result.expect("apply failed");

        assert_eq!(rescored.num_edges(), 3);
        assert_eq!(rescored.num_nodes(), lattice.num_nodes());
    }

    #[test]
    fn test_known_word_better_score() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference).with_weight(0.5);

        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        // "hello" is known, "xhello" is not
        builder.add_correction(0, 1, "hello", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "xhello", TropicalWeight::new(1.0), EdgeMetadata::default());
        let lattice = builder.build(1);

        let result = <PhoneticRescoreLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(&layer, &lattice);
        let rescored = result.expect("apply failed");

        let mut hello_weight = None;
        let mut xhello_weight = None;

        for edge in rescored.edges() {
            let word = rescored.edge_word(edge).unwrap_or("");
            if word == "hello" {
                hello_weight = Some(edge.weight.value());
            } else if word == "xhello" {
                xhello_weight = Some(edge.weight.value());
            }
        }

        assert!(hello_weight.is_some(), "hello edge not found");
        assert!(xhello_weight.is_some(), "xhello edge not found");
        assert!(
            hello_weight.expect("hello missing") < xhello_weight.expect("xhello missing"),
            "Expected hello ({:?}) < xhello ({:?})",
            hello_weight,
            xhello_weight
        );
    }

    #[test]
    fn test_lambda_zero_ignores_phonetic() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference).with_weight(0.0);

        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        builder.add_correction(0, 1, "word", TropicalWeight::new(5.0), EdgeMetadata::default());
        let lattice = builder.build(1);

        let result = <PhoneticRescoreLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(&layer, &lattice);
        let rescored = result.expect("apply failed");

        for edge in rescored.edges() {
            assert!(
                (edge.weight.value() - 5.0).abs() < 0.001,
                "Expected weight 5.0, got {}",
                edge.weight.value()
            );
        }
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("abc", ""), 3);
        assert_eq!(levenshtein_distance("", "abc"), 3);
        assert_eq!(levenshtein_distance("abc", "abc"), 0);
        assert_eq!(levenshtein_distance("abc", "abd"), 1);
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn test_vocabulary_reference() {
        let vocab = VocabularyReference::new(["hello", "world"].iter().map(|s| s.to_string()));

        assert!(vocab.is_known("hello"));
        assert!(vocab.is_known("world"));
        assert!(!vocab.is_known("foo"));
        assert!(vocab.reference_at(0).is_none());
    }

    #[test]
    fn test_sequence_reference() {
        let seq = SequenceReference::from_sequence(["hello", "world"].iter().map(|s| s.to_string()));

        assert!(seq.is_known("hello"));
        assert!(seq.is_known("world"));
        assert!(!seq.is_known("foo"));

        assert_eq!(seq.reference_at(0), Some(&["hello".to_string()][..]));
        assert_eq!(seq.reference_at(1), Some(&["world".to_string()][..]));
        assert!(seq.reference_at(2).is_none());
    }

    #[test]
    fn test_can_apply_always_true() {
        let reference = create_vocab_reference();
        let layer = PhoneticRescoreLayer::new(reference);
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let lattice = builder.build(0);

        let can_apply = <PhoneticRescoreLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::can_apply(&layer, &lattice);
        assert!(can_apply);
    }
}
