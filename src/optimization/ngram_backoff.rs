//! N-gram language model with back-off structure.
//!
//! This module provides compact representation of n-gram language models
//! as WFSTs, using back-off states to avoid O(|V|²) transitions.
//!
//! ## Problem
//!
//! Naively representing an n-gram LM as a WFST requires:
//! - O(|V|^{n-1}) states for (n-1)-gram contexts
//! - O(|V|^n) arcs for all n-gram transitions
//!
//! For large vocabularies, this becomes intractable.
//!
//! ## Solution: Back-off Structure
//!
//! Instead of explicit transitions for unseen n-grams, use:
//! - **Back-off state b** reachable via ε-transition
//! - **Seen bigram w₁w₂**: Direct transition from state w₁ to w₂
//! - **Unseen bigram w₁w₃**: ε-transition from w₁ to b with weight -log(β(w₁)),
//!   then transition from b to w₃ with weight -log(p̂(w₃))
//!
//! ## Benefits
//!
//! - Linear space in number of *observed* n-grams (not all possible)
//! - Back-off ε-transitions enable graceful degradation
//! - Compatible with standard WFST operations (composition, determinization)
//! - Large reduction in training cost for big word-piece vocabularies via pruning
//!
//! ## References
//!
//! - Mohri et al. "Speech Recognition with WFSTs" (2002)
//! - Hannun et al. "Differentiable WFSTs" (ICML 2020, arXiv:2010.01003)

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{MutableWfst, StateId, VectorWfst};

/// Vocabulary ID type for language model.
pub type VocabId = u32;

/// Unknown word ID (out-of-vocabulary).
pub const UNK_ID: VocabId = 0;
/// Begin of sentence marker ID.
pub const BOS_ID: VocabId = 1;
/// End of sentence marker ID.
pub const EOS_ID: VocabId = 2;

/// N-gram entry with back-off weight.
#[derive(Clone, Debug)]
pub struct NgramEntry {
    /// The n-gram context (e.g., [w1, w2] for trigram ending in w3).
    pub context: SmallVec<[VocabId; 4]>,
    /// The word this n-gram predicts.
    pub word: VocabId,
    /// Log probability: -log P(word | context).
    pub log_prob: f64,
}

/// Back-off weight for a context.
#[derive(Clone, Debug)]
pub struct BackoffWeight {
    /// The context for which this back-off applies.
    pub context: SmallVec<[VocabId; 4]>,
    /// Back-off weight: -log β(context).
    pub weight: f64,
}

/// Configuration for N-gram LM construction.
#[derive(Clone, Debug)]
pub struct NgramLmConfig {
    /// Maximum n-gram order (e.g., 3 for trigram).
    pub order: usize,
    /// Whether to use special backoff symbol (vs epsilon).
    /// Using a special symbol prevents epsilon removal from expanding the graph.
    pub use_backoff_symbol: bool,
    /// Vocabulary size (for bounds checking).
    pub vocab_size: usize,
    /// Pruning threshold: omit n-grams with prob < threshold.
    pub prune_threshold: Option<f64>,
}

impl Default for NgramLmConfig {
    fn default() -> Self {
        Self {
            order: 3,
            use_backoff_symbol: true,
            vocab_size: 0, // Will be set based on data
            prune_threshold: None,
        }
    }
}

/// Builder for N-gram language model WFST.
///
/// Constructs a compact WFST representation of an n-gram LM using
/// back-off states to avoid O(|V|²) transitions.
pub struct NgramLmBuilder {
    config: NgramLmConfig,
    /// Map from context to state ID.
    context_to_state: FxHashMap<SmallVec<[VocabId; 4]>, StateId>,
    /// Back-off weights indexed by context.
    backoff_weights: FxHashMap<SmallVec<[VocabId; 4]>, f64>,
    /// N-gram entries.
    ngrams: Vec<NgramEntry>,
    /// Vocabulary words seen.
    vocab: FxHashMap<VocabId, bool>,
}

impl NgramLmBuilder {
    /// Create a new N-gram LM builder.
    pub fn new(config: NgramLmConfig) -> Self {
        Self {
            config,
            context_to_state: FxHashMap::default(),
            backoff_weights: FxHashMap::default(),
            ngrams: Vec::new(),
            vocab: FxHashMap::default(),
        }
    }

    /// Add an n-gram to the model.
    ///
    /// # Arguments
    ///
    /// * `context` - The conditioning context (may be empty for unigrams)
    /// * `word` - The word being predicted
    /// * `log_prob` - Log probability -log P(word | context)
    pub fn add_ngram(&mut self, context: &[VocabId], word: VocabId, log_prob: f64) {
        // Skip if below pruning threshold
        if let Some(threshold) = self.config.prune_threshold {
            if log_prob > threshold {
                return; // Low probability, skip
            }
        }

        self.vocab.insert(word, true);
        for &w in context {
            self.vocab.insert(w, true);
        }

        self.ngrams.push(NgramEntry {
            context: SmallVec::from_slice(context),
            word,
            log_prob,
        });
    }

    /// Add a back-off weight for a context.
    ///
    /// # Arguments
    ///
    /// * `context` - The context for back-off
    /// * `weight` - Back-off weight -log β(context)
    pub fn add_backoff(&mut self, context: &[VocabId], weight: f64) {
        self.backoff_weights
            .insert(SmallVec::from_slice(context), weight);
    }

    /// Get or create a state for a context.
    fn get_or_create_state<L: Clone + Send + Sync>(
        &mut self,
        fst: &mut VectorWfst<L, LogWeight>,
        context: &[VocabId],
    ) -> StateId {
        let key: SmallVec<[VocabId; 4]> = SmallVec::from_slice(context);
        if let Some(&state) = self.context_to_state.get(&key) {
            return state;
        }

        let state = fst.add_state();
        self.context_to_state.insert(key, state);
        state
    }

    /// Get the back-off context (one less word).
    fn backoff_context(context: &[VocabId]) -> SmallVec<[VocabId; 4]> {
        if context.is_empty() {
            SmallVec::new()
        } else {
            SmallVec::from_slice(&context[1..])
        }
    }

    /// Build the N-gram LM as a WFST.
    ///
    /// The resulting WFST has:
    /// - States for each observed context
    /// - Direct arcs for observed n-grams
    /// - Back-off arcs (ε or special symbol) to shorter contexts
    pub fn build(mut self) -> VectorWfst<VocabId, LogWeight> {
        let mut fst: VectorWfst<VocabId, LogWeight> = VectorWfst::new();

        // Create initial state (empty context)
        let initial = fst.add_state();
        fst.set_start(initial);
        self.context_to_state.insert(SmallVec::new(), initial);

        // Clone ngrams to avoid borrow conflict
        let ngrams = self.ngrams.clone();

        // First pass: collect all contexts we need to create
        let mut all_contexts: Vec<SmallVec<[VocabId; 4]>> = Vec::new();
        for ngram in &ngrams {
            // Context for this n-gram
            all_contexts.push(ngram.context.clone());

            // Target context after seeing this word
            let mut new_context = ngram.context.clone();
            new_context.push(ngram.word);
            if new_context.len() > self.config.order - 1 {
                new_context.remove(0);
            }
            all_contexts.push(new_context);
        }

        // Create states for all contexts
        for context in &all_contexts {
            let _state = self.get_or_create_state(&mut fst, context);
        }

        // Create back-off states for intermediate contexts
        let contexts: Vec<_> = self.context_to_state.keys().cloned().collect();
        for context in &contexts {
            if !context.is_empty() {
                let backoff = Self::backoff_context(context);
                let _backoff_state = self.get_or_create_state(&mut fst, &backoff);
            }
        }

        // Second pass: add n-gram arcs
        for ngram in &ngrams {
            let source = *self
                .context_to_state
                .get(&ngram.context)
                .expect("context exists");

            // Target context after seeing this word
            let mut new_context = ngram.context.clone();
            new_context.push(ngram.word);
            if new_context.len() > self.config.order - 1 {
                new_context.remove(0);
            }
            let target = *self
                .context_to_state
                .get(&new_context)
                .expect("target exists");

            fst.add_arc(
                source,
                Some(ngram.word),
                Some(ngram.word),
                target,
                LogWeight::new(ngram.log_prob),
            );
        }

        // Clone context_to_state for iteration
        let context_states: Vec<_> = self
            .context_to_state
            .iter()
            .map(|(k, &v)| (k.clone(), v))
            .collect();

        // Third pass: add back-off arcs
        for (context, state) in &context_states {
            if context.is_empty() {
                continue; // No back-off from unigram state
            }

            let backoff_context = Self::backoff_context(context);
            let backoff_state = *self
                .context_to_state
                .get(&backoff_context)
                .expect("backoff context exists");

            // Back-off weight
            let backoff_weight = self.backoff_weights.get(context).copied().unwrap_or(0.0); // Default: no penalty for back-off

            // Add back-off arc
            if self.config.use_backoff_symbol {
                // Use special backoff symbol (prevents ε-removal expansion)
                // We use VocabId::MAX as the backoff symbol
                fst.add_arc(
                    *state,
                    None, // Epsilon input (matches any)
                    None, // Epsilon output
                    backoff_state,
                    LogWeight::new(backoff_weight),
                );
            } else {
                // Use epsilon (may be expanded during ε-removal)
                fst.add_arc(
                    *state,
                    None,
                    None,
                    backoff_state,
                    LogWeight::new(backoff_weight),
                );
            }
        }

        // Set final weights for states that can end sentences
        // Typically states containing EOS get final weight
        for (ctx, &state) in self
            .context_to_state
            .iter()
            .filter(|(ctx, _)| ctx.last() == Some(&EOS_ID))
        {
            let _ = ctx; // ctx is used in filter
            fst.set_final(state, LogWeight::one());
        }

        // Also make initial state final (for empty sequences)
        fst.set_final(initial, LogWeight::one());

        fst
    }

    /// Get statistics about the model.
    pub fn stats(&self) -> NgramStats {
        let mut order_counts = [0usize; 8];
        for ngram in &self.ngrams {
            let order = ngram.context.len() + 1;
            if order < order_counts.len() {
                order_counts[order] += 1;
            }
        }

        NgramStats {
            num_ngrams: self.ngrams.len(),
            num_contexts: self.context_to_state.len(),
            num_backoffs: self.backoff_weights.len(),
            vocab_size: self.vocab.len(),
            order_counts,
        }
    }
}

/// Statistics about an N-gram LM.
#[derive(Clone, Debug, Default)]
pub struct NgramStats {
    /// Total number of n-grams.
    pub num_ngrams: usize,
    /// Number of unique contexts.
    pub num_contexts: usize,
    /// Number of back-off weights.
    pub num_backoffs: usize,
    /// Vocabulary size.
    pub vocab_size: usize,
    /// Count of n-grams by order (index 1 = unigrams, etc.)
    pub order_counts: [usize; 8],
}

/// Compact representation of bigram LM.
///
/// Specialized structure for bigrams with efficient lookup.
pub struct BigramLm {
    /// Unigram log probabilities: P(w).
    unigram_probs: Vec<f64>,
    /// Bigram log probabilities: P(w2 | w1).
    /// Stored as (w1, w2) -> log_prob.
    bigram_probs: FxHashMap<(VocabId, VocabId), f64>,
    /// Back-off weights for each word.
    backoff_weights: Vec<f64>,
    /// Vocabulary size.
    vocab_size: usize,
}

impl BigramLm {
    /// Create a new bigram LM.
    pub fn new(vocab_size: usize) -> Self {
        Self {
            unigram_probs: vec![f64::INFINITY; vocab_size], // -log(0) = infinity
            bigram_probs: FxHashMap::default(),
            backoff_weights: vec![0.0; vocab_size], // No penalty by default
            vocab_size,
        }
    }

    /// Set unigram probability.
    pub fn set_unigram(&mut self, word: VocabId, log_prob: f64) {
        if (word as usize) < self.vocab_size {
            self.unigram_probs[word as usize] = log_prob;
        }
    }

    /// Set bigram probability.
    pub fn set_bigram(&mut self, w1: VocabId, w2: VocabId, log_prob: f64) {
        self.bigram_probs.insert((w1, w2), log_prob);
    }

    /// Set back-off weight for a word.
    pub fn set_backoff(&mut self, word: VocabId, weight: f64) {
        if (word as usize) < self.vocab_size {
            self.backoff_weights[word as usize] = weight;
        }
    }

    /// Get probability P(w2 | w1).
    ///
    /// Uses back-off if bigram not observed:
    /// P(w2 | w1) = P(w2) * β(w1) if (w1, w2) not seen
    pub fn prob(&self, w1: VocabId, w2: VocabId) -> f64 {
        // Try bigram first
        if let Some(&log_prob) = self.bigram_probs.get(&(w1, w2)) {
            return log_prob;
        }

        // Back-off to unigram
        let unigram = self
            .unigram_probs
            .get(w2 as usize)
            .copied()
            .unwrap_or(f64::INFINITY);
        let backoff = self
            .backoff_weights
            .get(w1 as usize)
            .copied()
            .unwrap_or(0.0);

        // In log space: log(P(w2) * β(w1)) = log(P(w2)) + log(β(w1))
        // But we store -log, so: -log(P(w2) * β(w1)) = -log(P(w2)) - log(β(w1))
        // Since backoff is stored as -log(β), this is: unigram + backoff
        unigram + backoff
    }

    /// Convert to WFST representation.
    pub fn to_wfst(&self) -> VectorWfst<VocabId, LogWeight> {
        let mut fst: VectorWfst<VocabId, LogWeight> = VectorWfst::new();

        // Create states: one per word + one backoff state
        let backoff_state = fst.add_state();
        fst.set_start(backoff_state);

        let mut word_states: Vec<StateId> = Vec::with_capacity(self.vocab_size);
        for _ in 0..self.vocab_size {
            word_states.push(fst.add_state());
        }

        // Unigram arcs from backoff state
        for (w, &log_prob) in self.unigram_probs.iter().enumerate() {
            if log_prob < f64::INFINITY {
                fst.add_arc(
                    backoff_state,
                    Some(w as VocabId),
                    Some(w as VocabId),
                    word_states[w],
                    LogWeight::new(log_prob),
                );
            }
        }

        // Bigram arcs
        for (&(w1, w2), &log_prob) in &self.bigram_probs {
            if (w1 as usize) < self.vocab_size && (w2 as usize) < self.vocab_size {
                fst.add_arc(
                    word_states[w1 as usize],
                    Some(w2),
                    Some(w2),
                    word_states[w2 as usize],
                    LogWeight::new(log_prob),
                );
            }
        }

        // Back-off arcs from word states to backoff state
        for (w, &backoff_weight) in self.backoff_weights.iter().enumerate() {
            fst.add_arc(
                word_states[w],
                None, // Epsilon
                None,
                backoff_state,
                LogWeight::new(backoff_weight),
            );
        }

        // All states are final
        fst.set_final(backoff_state, LogWeight::one());
        for &state in &word_states {
            fst.set_final(state, LogWeight::one());
        }

        fst
    }

    /// Get statistics.
    pub fn stats(&self) -> BigramStats {
        let num_unigrams = self
            .unigram_probs
            .iter()
            .filter(|&&p| p < f64::INFINITY)
            .count();

        BigramStats {
            vocab_size: self.vocab_size,
            num_unigrams,
            num_bigrams: self.bigram_probs.len(),
            sparsity: 1.0
                - (self.bigram_probs.len() as f64 / (self.vocab_size * self.vocab_size) as f64),
        }
    }
}

/// Statistics for bigram LM.
#[derive(Clone, Debug)]
pub struct BigramStats {
    /// Vocabulary size.
    pub vocab_size: usize,
    /// Number of non-zero unigrams.
    pub num_unigrams: usize,
    /// Number of observed bigrams.
    pub num_bigrams: usize,
    /// Sparsity ratio (1 - density).
    pub sparsity: f64,
}

/// Pruning strategy for n-gram models.
#[derive(Clone, Debug)]
pub enum PruningStrategy {
    /// No pruning.
    None,
    /// Count-based: keep n-grams seen at least N times.
    CountThreshold(usize),
    /// Probability-based: keep n-grams with -log(P) < threshold.
    ProbabilityThreshold(f64),
    /// Entropy-based: prune n-grams that add little information.
    EntropyThreshold(f64),
}

/// Helper to compute graph size reduction from back-off.
pub fn compute_size_reduction(
    vocab_size: usize,
    num_observed: usize,
    order: usize,
) -> SizeReduction {
    // Dense representation
    let dense_states = vocab_size.pow((order - 1) as u32);
    let dense_arcs = vocab_size.pow(order as u32);

    // Sparse representation with back-off
    // Approximate: states = num_contexts, arcs = num_ngrams + back-off arcs
    let sparse_states = num_observed / vocab_size + 1; // Approximate contexts
    let sparse_arcs = num_observed + sparse_states; // N-grams + back-off arcs

    SizeReduction {
        dense_states,
        dense_arcs,
        sparse_states,
        sparse_arcs,
        state_reduction: if dense_states > 0 {
            1.0 - (sparse_states as f64 / dense_states as f64)
        } else {
            0.0
        },
        arc_reduction: if dense_arcs > 0 {
            1.0 - (sparse_arcs as f64 / dense_arcs as f64)
        } else {
            0.0
        },
    }
}

/// Size reduction from using back-off structure.
#[derive(Clone, Debug)]
pub struct SizeReduction {
    /// States in dense representation.
    pub dense_states: usize,
    /// Arcs in dense representation.
    pub dense_arcs: usize,
    /// States in sparse representation.
    pub sparse_states: usize,
    /// Arcs in sparse representation.
    pub sparse_arcs: usize,
    /// State reduction ratio.
    pub state_reduction: f64,
    /// Arc reduction ratio.
    pub arc_reduction: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::Wfst;

    #[test]
    fn test_bigram_lm_basic() {
        let mut lm = BigramLm::new(5);

        // Set unigrams
        lm.set_unigram(0, 2.0); // -log(P(0))
        lm.set_unigram(1, 1.5);
        lm.set_unigram(2, 1.0);

        // Set bigrams
        lm.set_bigram(0, 1, 0.5); // P(1|0) is high
        lm.set_bigram(1, 2, 0.3);

        // Set back-off
        lm.set_backoff(0, 0.1);

        // Query probabilities
        assert!((lm.prob(0, 1) - 0.5).abs() < 1e-10); // Direct bigram
        assert!((lm.prob(0, 2) - (1.0 + 0.1)).abs() < 1e-10); // Back-off
    }

    #[test]
    fn test_bigram_lm_to_wfst() {
        let mut lm = BigramLm::new(3);
        lm.set_unigram(0, 1.0);
        lm.set_unigram(1, 2.0);
        lm.set_bigram(0, 1, 0.5);

        let fst = lm.to_wfst();

        // Should have 4 states: backoff + 3 word states
        assert_eq!(fst.num_states(), 4);
    }

    #[test]
    fn test_ngram_builder_basic() {
        let config = NgramLmConfig {
            order: 2,
            use_backoff_symbol: true,
            vocab_size: 5,
            prune_threshold: None,
        };

        let mut builder = NgramLmBuilder::new(config);

        // Add unigrams
        builder.add_ngram(&[], 0, 2.0);
        builder.add_ngram(&[], 1, 1.5);
        builder.add_ngram(&[], 2, 1.0);

        // Add bigrams
        builder.add_ngram(&[0], 1, 0.5);
        builder.add_ngram(&[1], 2, 0.3);

        // Add back-off weights
        builder.add_backoff(&[0], 0.1);
        builder.add_backoff(&[1], 0.2);

        let stats = builder.stats();
        assert_eq!(stats.num_ngrams, 5);
        assert_eq!(stats.vocab_size, 3);

        let _fst = builder.build();
    }

    #[test]
    fn test_ngram_builder_with_pruning() {
        let config = NgramLmConfig {
            order: 2,
            use_backoff_symbol: true,
            vocab_size: 5,
            prune_threshold: Some(1.0), // Prune n-grams with -log(P) > 1.0
        };

        let mut builder = NgramLmBuilder::new(config);

        // Add n-grams with varying probabilities
        builder.add_ngram(&[], 0, 0.5); // Keep (< 1.0)
        builder.add_ngram(&[], 1, 1.5); // Prune (> 1.0)
        builder.add_ngram(&[], 2, 2.0); // Prune (> 1.0)

        let stats = builder.stats();
        assert_eq!(stats.num_ngrams, 1); // Only one kept
    }

    #[test]
    fn test_size_reduction() {
        // Bigram with vocab size 1000
        let reduction = compute_size_reduction(1000, 50000, 2);

        // Dense: 1000 states, 1M arcs
        assert_eq!(reduction.dense_states, 1000);
        assert_eq!(reduction.dense_arcs, 1_000_000);

        // Sparse should be much smaller
        assert!(reduction.sparse_arcs < reduction.dense_arcs);
        assert!(reduction.arc_reduction > 0.9); // >90% reduction
    }

    #[test]
    fn test_trigram_builder() {
        let config = NgramLmConfig {
            order: 3,
            use_backoff_symbol: true,
            vocab_size: 10,
            prune_threshold: None,
        };

        let mut builder = NgramLmBuilder::new(config);

        // Add trigrams
        builder.add_ngram(&[0, 1], 2, 0.5);
        builder.add_ngram(&[1, 2], 3, 0.3);

        // Add bigrams
        builder.add_ngram(&[0], 1, 0.8);
        builder.add_ngram(&[1], 2, 0.6);

        // Add unigrams
        builder.add_ngram(&[], 0, 2.0);
        builder.add_ngram(&[], 1, 1.8);
        builder.add_ngram(&[], 2, 1.5);
        builder.add_ngram(&[], 3, 1.2);

        builder.add_backoff(&[0, 1], 0.1);
        builder.add_backoff(&[1, 2], 0.1);
        builder.add_backoff(&[0], 0.2);
        builder.add_backoff(&[1], 0.2);

        let stats = builder.stats();
        assert_eq!(stats.order_counts[1], 4); // 4 unigrams
        assert_eq!(stats.order_counts[2], 2); // 2 bigrams
        assert_eq!(stats.order_counts[3], 2); // 2 trigrams

        let _fst = builder.build();
    }

    #[test]
    fn test_backoff_context() {
        let ctx: SmallVec<[VocabId; 4]> = SmallVec::from_slice(&[1, 2, 3]);
        let backoff = NgramLmBuilder::backoff_context(&ctx);
        assert_eq!(backoff.as_slice(), &[2, 3]);

        let empty: SmallVec<[VocabId; 4]> = SmallVec::new();
        let backoff_empty = NgramLmBuilder::backoff_context(&empty);
        assert!(backoff_empty.is_empty());
    }

    #[test]
    fn test_bigram_stats() {
        let mut lm = BigramLm::new(100);

        // Add some unigrams
        for i in 0..50 {
            lm.set_unigram(i, 1.0);
        }

        // Add sparse bigrams
        for i in 0..100 {
            lm.set_bigram(i, (i + 1) % 100, 0.5);
        }

        let stats = lm.stats();
        assert_eq!(stats.vocab_size, 100);
        assert_eq!(stats.num_unigrams, 50);
        assert_eq!(stats.num_bigrams, 100);
        assert!(stats.sparsity >= 0.99); // Very sparse (100/10000 = 1% density)
    }
}
