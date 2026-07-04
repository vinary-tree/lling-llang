//! N-gram transitions with pruning and back-off for differentiable training.
//!
//! This module provides efficient n-gram transition structures that scale
//! to large vocabularies through pruning and back-off mechanisms.
//!
//! ## Problem
//!
//! Dense n-gram transition graphs have complexity O(C^n) where:
//! - C = vocabulary size
//! - n = n-gram order
//!
//! For 1000 word pieces with bigrams: 1,000,000 states/transitions!
//!
//! ## Solution
//!
//! 1. **Pruning**: Only keep n-grams observed ≥ k times in training data
//! 2. **Back-off**: Missing n-grams fall back to (n-1)-gram probabilities
//!
//! ## Results
//!
//! Pruning rare n-grams and backing off sharply reduces per-epoch training cost for large
//! word-piece vocabularies, with negligible accuracy loss at a suitable threshold.
//!
//! ## Back-off Structure
//!
//! ```text
//! State for "ab":
//!   - If "abc" seen: direct transition to "bc" state
//!   - If "abc" unseen: ε-transition to "b" back-off state, then to "c"
//! ```
//!
//! ## References
//!
//! - Hannun et al., "Differentiable Weighted Finite-State Transducers" (ICML 2020, arXiv:2010.01003)
//! - Katz, "Estimation of probabilities from sparse data" (1987)

use std::collections::HashMap;

use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst};

/// Token identifier type.
pub type TokenId = u32;

/// N-gram order type.
pub type NgramOrder = usize;

/// Configuration for pruned n-gram construction.
#[derive(Clone, Debug)]
pub struct PrunedNgramConfig {
    /// N-gram order (2 = bigram, 3 = trigram, etc.).
    pub order: NgramOrder,
    /// Minimum count threshold for keeping an n-gram.
    pub min_count: usize,
    /// Whether to use back-off for unseen n-grams.
    pub use_backoff: bool,
    /// Back-off weight (log probability).
    pub backoff_weight: f64,
    /// Whether to smooth probabilities.
    pub smoothing: bool,
    /// Smoothing discount factor (for Kneser-Ney style smoothing).
    pub discount: f64,
}

impl Default for PrunedNgramConfig {
    fn default() -> Self {
        Self {
            order: 2,
            min_count: 1,
            use_backoff: true,
            backoff_weight: 0.0,
            smoothing: false,
            discount: 0.5,
        }
    }
}

/// N-gram counts for pruning decisions.
#[derive(Clone, Debug, Default)]
pub struct NgramCounts {
    /// Unigram counts: token -> count.
    pub unigrams: HashMap<TokenId, usize>,
    /// Bigram counts: (prev, curr) -> count.
    pub bigrams: HashMap<(TokenId, TokenId), usize>,
    /// Trigram counts: (prev2, prev1, curr) -> count.
    pub trigrams: HashMap<(TokenId, TokenId, TokenId), usize>,
    /// Total count of all tokens.
    pub total: usize,
}

impl NgramCounts {
    /// Create empty counts.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add counts from a token sequence.
    pub fn add_sequence(&mut self, tokens: &[TokenId]) {
        for &token in tokens {
            *self.unigrams.entry(token).or_insert(0) += 1;
            self.total += 1;
        }

        for window in tokens.windows(2) {
            let bigram = (window[0], window[1]);
            *self.bigrams.entry(bigram).or_insert(0) += 1;
        }

        for window in tokens.windows(3) {
            let trigram = (window[0], window[1], window[2]);
            *self.trigrams.entry(trigram).or_insert(0) += 1;
        }
    }

    /// Get unigram count.
    pub fn unigram_count(&self, token: TokenId) -> usize {
        self.unigrams.get(&token).copied().unwrap_or(0)
    }

    /// Get bigram count.
    pub fn bigram_count(&self, prev: TokenId, curr: TokenId) -> usize {
        self.bigrams.get(&(prev, curr)).copied().unwrap_or(0)
    }

    /// Get trigram count.
    pub fn trigram_count(&self, prev2: TokenId, prev1: TokenId, curr: TokenId) -> usize {
        self.trigrams
            .get(&(prev2, prev1, curr))
            .copied()
            .unwrap_or(0)
    }

    /// Compute unigram probability.
    pub fn unigram_prob(&self, token: TokenId) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.unigram_count(token) as f64 / self.total as f64
    }

    /// Compute bigram probability.
    pub fn bigram_prob(&self, prev: TokenId, curr: TokenId) -> f64 {
        let prev_count = self.unigram_count(prev);
        if prev_count == 0 {
            return 0.0;
        }
        self.bigram_count(prev, curr) as f64 / prev_count as f64
    }
}

fn vocab_tokens(vocab_size: usize) -> impl Iterator<Item = TokenId> {
    (0..vocab_size).map_while(|token| TokenId::try_from(token).ok())
}

fn token_in_vocab(token: TokenId, vocab_size: usize) -> bool {
    (token as usize) < vocab_size
}

fn sorted_kept_bigrams(
    counts: &NgramCounts,
    vocab_size: usize,
    min_count: usize,
) -> Vec<(TokenId, TokenId, usize)> {
    let mut bigrams: Vec<_> = counts
        .bigrams
        .iter()
        .filter_map(|(&(prev, curr), &count)| {
            (count >= min_count
                && token_in_vocab(prev, vocab_size)
                && token_in_vocab(curr, vocab_size))
            .then_some((prev, curr, count))
        })
        .collect();
    bigrams.sort_unstable();
    bigrams
}

fn sorted_bigram_arc_candidates(
    counts: &NgramCounts,
    vocab_size: usize,
    min_count: usize,
) -> Vec<(TokenId, TokenId, usize)> {
    if min_count > 0 {
        return sorted_kept_bigrams(counts, vocab_size, min_count);
    }

    let tokens: Vec<_> = vocab_tokens(vocab_size).collect();
    let mut bigrams = Vec::with_capacity(tokens.len().saturating_mul(tokens.len()));
    for &prev in &tokens {
        for &curr in &tokens {
            bigrams.push((prev, curr, counts.bigram_count(prev, curr)));
        }
    }
    bigrams
}

fn sorted_observed_trigrams(
    counts: &NgramCounts,
    vocab_size: usize,
    min_count: usize,
) -> Vec<(TokenId, TokenId, TokenId, usize)> {
    let mut trigrams: Vec<_> = counts
        .trigrams
        .iter()
        .filter_map(|(&(prev1, prev2, curr), &count)| {
            (count >= min_count
                && token_in_vocab(prev1, vocab_size)
                && token_in_vocab(prev2, vocab_size)
                && token_in_vocab(curr, vocab_size))
            .then_some((prev1, prev2, curr, count))
        })
        .collect();
    trigrams.sort_unstable();
    trigrams
}

fn sorted_trigram_arc_candidates(
    counts: &NgramCounts,
    vocab_size: usize,
    min_count: usize,
    kept_bigrams: &[(TokenId, TokenId, usize)],
) -> Vec<(TokenId, TokenId, TokenId, usize)> {
    if min_count > 0 {
        return sorted_observed_trigrams(counts, vocab_size, min_count);
    }

    let mut currs_by_prev: HashMap<TokenId, Vec<TokenId>> = HashMap::new();
    for &(prev, curr, _) in kept_bigrams {
        currs_by_prev.entry(prev).or_default().push(curr);
    }

    let mut trigrams = Vec::new();
    for &(prev1, prev2, _) in kept_bigrams {
        let Some(currs) = currs_by_prev.get(&prev2) else {
            continue;
        };
        for &curr in currs {
            trigrams.push((prev1, prev2, curr, counts.trigram_count(prev1, prev2, curr)));
        }
    }
    trigrams.sort_unstable();
    trigrams
}

fn direct_count_by_prev(bigrams: &[(TokenId, TokenId, usize)]) -> HashMap<TokenId, usize> {
    let mut counts = HashMap::new();
    for &(prev, _, _) in bigrams {
        *counts.entry(prev).or_insert(0) += 1;
    }
    counts
}

fn direct_count_by_history(
    trigrams: &[(TokenId, TokenId, TokenId, usize)],
    bigram_states: &HashMap<(TokenId, TokenId), StateId>,
) -> HashMap<(TokenId, TokenId), usize> {
    let mut counts = HashMap::new();
    for &(prev1, prev2, curr, _) in trigrams {
        if bigram_states.contains_key(&(prev1, prev2)) && bigram_states.contains_key(&(prev2, curr))
        {
            *counts.entry((prev1, prev2)).or_insert(0) += 1;
        }
    }
    counts
}

/// Build a pruned bigram transition graph.
///
/// # Arguments
///
/// * `vocab_size` - Number of tokens in vocabulary
/// * `counts` - N-gram counts for pruning
/// * `config` - Configuration options
///
/// # Returns
///
/// A WFST representing the pruned bigram transitions.
pub fn build_pruned_bigram_graph(
    vocab_size: usize,
    counts: &NgramCounts,
    config: &PrunedNgramConfig,
) -> VectorWfst<TokenId, LogWeight> {
    let tokens: Vec<_> = vocab_tokens(vocab_size).collect();
    let bigram_arcs = sorted_bigram_arc_candidates(counts, vocab_size, config.min_count);
    let direct_counts = direct_count_by_prev(&bigram_arcs);
    let mut fst = VectorWfst::with_capacity(1 + tokens.len() + usize::from(config.use_backoff));

    // Create start state
    let start = fst.add_state();
    fst.set_start(start);
    fst.set_final(start, LogWeight::one());
    fst.reserve_transitions(start, tokens.len());

    // Create state for each token (represents context)
    let mut token_states: HashMap<TokenId, StateId> = HashMap::with_capacity(tokens.len());
    for &token in &tokens {
        let state = fst.add_state();
        token_states.insert(token, state);
        fst.set_final(state, LogWeight::one());
        let outgoing =
            direct_counts.get(&token).copied().unwrap_or(0) + usize::from(config.use_backoff);
        fst.reserve_transitions(state, outgoing);
    }

    // Back-off state (if using back-off)
    let backoff_state = if config.use_backoff {
        let state = fst.add_state();
        fst.set_final(state, LogWeight::one());
        fst.reserve_transitions(state, tokens.len());
        Some(state)
    } else {
        None
    };

    // Add transitions from start to each token
    for &token in &tokens {
        let log_prob = if counts.total > 0 {
            let prob = counts.unigram_prob(token).max(1e-10);
            -prob.ln()
        } else {
            0.0
        };

        let to_state = token_states[&token];
        fst.add_arc(
            start,
            Some(token),
            Some(token),
            to_state,
            LogWeight::new(log_prob),
        );
    }

    // Add direct transitions for observed bigrams above threshold.
    for &(prev, curr, _) in &bigram_arcs {
        let from_state = token_states[&prev];
        let to_state = token_states[&curr];
        let log_prob = if config.smoothing {
            compute_smoothed_log_prob(counts, prev, curr, config)
        } else {
            let prob = counts.bigram_prob(prev, curr).max(1e-10);
            -prob.ln()
        };

        fst.add_arc(
            from_state,
            Some(curr),
            Some(curr),
            to_state,
            LogWeight::new(log_prob),
        );
    }

    // Add back-off transitions after direct arcs to preserve per-state arc order.
    if let Some(backoff) = backoff_state {
        for &prev in &tokens {
            let from_state = token_states[&prev];
            fst.add_arc(
                from_state,
                None,
                None,
                backoff,
                LogWeight::new(config.backoff_weight),
            );
        }
    }

    // From back-off state, allow all tokens with unigram probabilities
    if let Some(backoff) = backoff_state {
        for &token in &tokens {
            let log_prob = if counts.total > 0 {
                let prob = counts.unigram_prob(token).max(1e-10);
                -prob.ln()
            } else {
                0.0
            };

            let to_state = token_states[&token];
            fst.add_arc(
                backoff,
                Some(token),
                Some(token),
                to_state,
                LogWeight::new(log_prob),
            );
        }
    }

    fst
}

/// Compute smoothed log probability using simple discounting.
fn compute_smoothed_log_prob(
    counts: &NgramCounts,
    prev: TokenId,
    curr: TokenId,
    config: &PrunedNgramConfig,
) -> f64 {
    let bigram_count = counts.bigram_count(prev, curr) as f64;
    let prev_count = counts.unigram_count(prev) as f64;

    if prev_count == 0.0 {
        return 0.0;
    }

    // Simple discounting: (c - d) / total + lambda * P_backoff
    let discounted = (bigram_count - config.discount).max(0.0) / prev_count;
    let unigram_prob = counts.unigram_prob(curr);
    let lambda = config.discount / prev_count; // Normalization factor

    let prob = (discounted + lambda * unigram_prob).max(1e-10);
    -prob.ln()
}

/// Build a pruned trigram transition graph.
///
/// Uses a two-level structure: states encode the two-token history.
pub fn build_pruned_trigram_graph(
    vocab_size: usize,
    counts: &NgramCounts,
    config: &PrunedNgramConfig,
) -> VectorWfst<TokenId, LogWeight> {
    let tokens: Vec<_> = vocab_tokens(vocab_size).collect();
    let kept_bigrams = sorted_kept_bigrams(counts, vocab_size, config.min_count);
    let trigram_arcs =
        sorted_trigram_arc_candidates(counts, vocab_size, config.min_count, &kept_bigrams);
    let direct_bigram_counts = direct_count_by_prev(&kept_bigrams);
    let mut fst = VectorWfst::with_capacity(
        1 + tokens.len() + kept_bigrams.len() + usize::from(config.use_backoff),
    );

    // Create start state
    let start = fst.add_state();
    fst.set_start(start);
    fst.set_final(start, LogWeight::one());
    fst.reserve_transitions(start, tokens.len());

    // State for single-token history (used after start)
    let mut unigram_states: HashMap<TokenId, StateId> = HashMap::with_capacity(tokens.len());
    for &token in &tokens {
        let state = fst.add_state();
        unigram_states.insert(token, state);
        fst.set_final(state, LogWeight::one());
        let outgoing = direct_bigram_counts.get(&token).copied().unwrap_or(0)
            + usize::from(config.use_backoff);
        fst.reserve_transitions(state, outgoing);
    }

    // States for bigram history
    let mut bigram_states: HashMap<(TokenId, TokenId), StateId> =
        HashMap::with_capacity(kept_bigrams.len());
    for &(prev, curr, _) in &kept_bigrams {
        let state = fst.add_state();
        bigram_states.insert((prev, curr), state);
        fst.set_final(state, LogWeight::one());
    }

    let direct_trigram_counts = direct_count_by_history(&trigram_arcs, &bigram_states);
    for (&history, &state) in &bigram_states {
        let outgoing = direct_trigram_counts.get(&history).copied().unwrap_or(0)
            + usize::from(config.use_backoff);
        fst.reserve_transitions(state, outgoing);
    }

    // Back-off state for bigram level
    let bigram_backoff = if config.use_backoff {
        let state = fst.add_state();
        fst.set_final(state, LogWeight::one());
        fst.reserve_transitions(state, tokens.len());
        Some(state)
    } else {
        None
    };

    // Transitions from start -> unigram states
    for &token in &tokens {
        let log_prob = if counts.total > 0 {
            let prob = counts.unigram_prob(token).max(1e-10);
            -prob.ln()
        } else {
            0.0
        };
        let to_state = unigram_states[&token];
        fst.add_arc(
            start,
            Some(token),
            Some(token),
            to_state,
            LogWeight::new(log_prob),
        );
    }

    // Transitions from unigram -> bigram states
    for &(prev, curr, _) in &kept_bigrams {
        let from_state = unigram_states[&prev];
        let to_state = bigram_states[&(prev, curr)];
        let prob = counts.bigram_prob(prev, curr).max(1e-10);
        fst.add_arc(
            from_state,
            Some(curr),
            Some(curr),
            to_state,
            LogWeight::new(-prob.ln()),
        );
    }

    // Back-off from unigram states after direct arcs to preserve ordering.
    if let Some(backoff) = bigram_backoff {
        for &prev in &tokens {
            let from_state = unigram_states[&prev];
            fst.add_arc(
                from_state,
                None,
                None,
                backoff,
                LogWeight::new(config.backoff_weight),
            );
        }
    }

    // From bigram_backoff, allow all tokens with unigram probabilities
    if let Some(backoff) = bigram_backoff {
        for &token in &tokens {
            let prob = counts.unigram_prob(token).max(1e-10);
            // Go to unigram state after back-off
            let to_state = unigram_states[&token];
            fst.add_arc(
                backoff,
                Some(token),
                Some(token),
                to_state,
                LogWeight::new(-prob.ln()),
            );
        }
    }

    // Transitions from bigram -> bigram states (trigrams)
    for &(prev1, prev2, curr, count) in &trigram_arcs {
        let Some(&from_state) = bigram_states.get(&(prev1, prev2)) else {
            continue;
        };
        let Some(&to_state) = bigram_states.get(&(prev2, curr)) else {
            continue;
        };
        let denom = counts.bigram_count(prev1, prev2) as f64;
        let prob = if denom > 0.0 {
            (count as f64 / denom).max(1e-10)
        } else {
            1e-10
        };
        fst.add_arc(
            from_state,
            Some(curr),
            Some(curr),
            to_state,
            LogWeight::new(-prob.ln()),
        );
    }

    // Back-off from bigram states after direct trigram arcs.
    if let Some(backoff) = bigram_backoff {
        for &(prev, curr, _) in &kept_bigrams {
            let from_state = bigram_states[&(prev, curr)];
            fst.add_arc(
                from_state,
                None,
                None,
                backoff,
                LogWeight::new(config.backoff_weight),
            );
        }
    }

    fst
}

/// Statistics about the pruned n-gram graph.
#[derive(Clone, Debug, Default)]
pub struct PrunedNgramStats {
    /// Number of states in the graph.
    pub num_states: usize,
    /// Number of arcs in the graph.
    pub num_arcs: usize,
    /// Number of unique n-grams kept.
    pub ngrams_kept: usize,
    /// Number of unique n-grams pruned.
    pub ngrams_pruned: usize,
    /// Pruning ratio.
    pub pruning_ratio: f64,
    /// Comparison: dense graph would have this many arcs.
    pub dense_arcs: usize,
    /// Compression ratio.
    pub compression_ratio: f64,
}

impl PrunedNgramStats {
    /// Compute statistics for a pruned n-gram graph.
    pub fn from_bigram_graph<L: Clone + Send + Sync>(
        fst: &VectorWfst<L, LogWeight>,
        vocab_size: usize,
    ) -> Self {
        let num_states = fst.num_states();
        let num_arcs: usize = (0..num_states as StateId)
            .map(|s| fst.transitions(s).len())
            .sum();

        // Dense bigram graph would have vocab_size^2 arcs
        let dense_arcs = vocab_size * vocab_size;

        Self {
            num_states,
            num_arcs,
            ngrams_kept: 0, // Would need to track during construction
            ngrams_pruned: 0,
            pruning_ratio: 0.0,
            dense_arcs,
            compression_ratio: if num_arcs > 0 {
                dense_arcs as f64 / num_arcs as f64
            } else {
                0.0
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::NO_STATE;

    fn state_reached_from_start(fst: &VectorWfst<TokenId, LogWeight>, token: TokenId) -> StateId {
        fst.transitions(fst.start())
            .iter()
            .find(|transition| transition.input == Some(token))
            .map(|transition| transition.to)
            .expect("token should be reachable from start")
    }

    fn labeled_transition_count(fst: &VectorWfst<TokenId, LogWeight>, state: StateId) -> usize {
        fst.transitions(state)
            .iter()
            .filter(|transition| transition.input.is_some())
            .count()
    }

    #[test]
    fn test_pruned_ngram_config_default() {
        let config = PrunedNgramConfig::default();
        assert_eq!(config.order, 2);
        assert_eq!(config.min_count, 1);
        assert!(config.use_backoff);
    }

    #[test]
    fn test_ngram_counts_empty() {
        let counts = NgramCounts::new();
        assert_eq!(counts.total, 0);
        assert_eq!(counts.unigram_count(0), 0);
    }

    #[test]
    fn test_ngram_counts_add_sequence() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[1, 2, 3, 1, 2]);

        assert_eq!(counts.unigram_count(1), 2);
        assert_eq!(counts.unigram_count(2), 2);
        assert_eq!(counts.unigram_count(3), 1);
        assert_eq!(counts.total, 5);

        assert_eq!(counts.bigram_count(1, 2), 2);
        assert_eq!(counts.bigram_count(2, 3), 1);
        assert_eq!(counts.bigram_count(3, 1), 1);
    }

    #[test]
    fn test_ngram_counts_probabilities() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[0, 1, 0, 1]);

        // Unigram: 0 appears 2 times, 1 appears 2 times, total 4
        assert!((counts.unigram_prob(0) - 0.5).abs() < 1e-6);
        assert!((counts.unigram_prob(1) - 0.5).abs() < 1e-6);

        // Bigram: P(1|0) = 2/2 = 1.0 (0->1 appears 2 times, 0 appears 2 times)
        assert!((counts.bigram_prob(0, 1) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_build_pruned_bigram_graph() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[0, 1, 2, 0, 1]);

        let config = PrunedNgramConfig::default();
        let fst = build_pruned_bigram_graph(3, &counts, &config);

        assert!(fst.start() != NO_STATE);
        assert!(fst.num_states() > 0);
    }

    #[test]
    fn test_pruned_bigram_with_threshold() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[0, 1, 0, 1, 0, 1]); // 0->1 appears 3 times

        let config = PrunedNgramConfig {
            min_count: 2,
            ..Default::default()
        };
        let fst = build_pruned_bigram_graph(3, &counts, &config);

        // Should prune infrequent bigrams
        assert!(fst.num_states() > 0);
    }

    #[test]
    fn test_build_pruned_trigram_graph() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[0, 1, 2, 0, 1, 2]);

        let config = PrunedNgramConfig {
            order: 3,
            ..Default::default()
        };
        let fst = build_pruned_trigram_graph(3, &counts, &config);

        assert!(fst.start() != NO_STATE);
        assert!(fst.num_states() > 0);
    }

    #[test]
    fn test_pruned_ngram_stats() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[0, 1, 2]);

        let config = PrunedNgramConfig::default();
        let fst = build_pruned_bigram_graph(10, &counts, &config);

        let stats = PrunedNgramStats::from_bigram_graph(&fst, 10);
        assert!(stats.num_states > 0);
        assert!(stats.num_arcs > 0);
        assert_eq!(stats.dense_arcs, 100); // 10^2
    }

    #[test]
    fn test_backoff_disabled() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[0, 1]);

        let config = PrunedNgramConfig {
            use_backoff: false,
            ..Default::default()
        };
        let fst = build_pruned_bigram_graph(3, &counts, &config);

        // Should still work without back-off
        assert!(fst.start() != NO_STATE);
    }

    #[test]
    fn test_smoothing_enabled() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[0, 1, 2]);

        let config = PrunedNgramConfig {
            smoothing: true,
            discount: 0.5,
            ..Default::default()
        };
        let fst = build_pruned_bigram_graph(3, &counts, &config);

        assert!(fst.num_states() > 0);
    }

    #[test]
    fn test_sparse_bigram_builder_uses_observed_entries() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[9000, 42, 9000, 42, 9000, 7]);

        let config = PrunedNgramConfig {
            min_count: 2,
            use_backoff: false,
            ..Default::default()
        };
        let fst = build_pruned_bigram_graph(10_000, &counts, &config);

        let high_id_state = state_reached_from_start(&fst, 9000);
        let low_count_state = state_reached_from_start(&fst, 7);

        assert_eq!(labeled_transition_count(&fst, high_id_state), 1);
        assert_eq!(labeled_transition_count(&fst, low_count_state), 0);
        assert_eq!(fst.transitions(high_id_state)[0].input, Some(42));
    }

    #[test]
    fn test_sparse_trigram_builder_uses_observed_entries() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[9000, 42, 7, 9000, 42, 7]);

        let config = PrunedNgramConfig {
            order: 3,
            min_count: 2,
            use_backoff: false,
            ..Default::default()
        };
        let fst = build_pruned_trigram_graph(10_000, &counts, &config);

        let unigram_9000 = state_reached_from_start(&fst, 9000);
        let bigram_9000_42 = fst.transitions(unigram_9000)[0].to;

        assert_eq!(fst.transitions(unigram_9000)[0].input, Some(42));
        assert_eq!(labeled_transition_count(&fst, bigram_9000_42), 1);
        assert_eq!(fst.transitions(bigram_9000_42)[0].input, Some(7));
    }

    #[test]
    fn test_zero_min_count_bigram_keeps_dense_direct_arcs() {
        let counts = NgramCounts::new();
        let config = PrunedNgramConfig {
            min_count: 0,
            use_backoff: false,
            ..Default::default()
        };
        let fst = build_pruned_bigram_graph(3, &counts, &config);

        for token in 0..3 {
            let state = state_reached_from_start(&fst, token);
            assert_eq!(labeled_transition_count(&fst, state), 3);
        }
    }

    #[test]
    fn test_zero_min_count_trigram_keeps_context_continuations() {
        let mut counts = NgramCounts::new();
        counts.add_sequence(&[0, 1, 2, 0, 1]);

        let config = PrunedNgramConfig {
            order: 3,
            min_count: 0,
            use_backoff: false,
            ..Default::default()
        };
        let fst = build_pruned_trigram_graph(3, &counts, &config);

        let unigram_0 = state_reached_from_start(&fst, 0);
        let bigram_0_1 = fst.transitions(unigram_0)[0].to;

        assert_eq!(fst.transitions(unigram_0)[0].input, Some(1));
        assert_eq!(labeled_transition_count(&fst, bigram_0_1), 1);
        assert_eq!(fst.transitions(bigram_0_1)[0].input, Some(2));
    }

    #[test]
    fn test_compression_ratio() {
        let mut counts = NgramCounts::new();
        // Only observe a few bigrams
        counts.add_sequence(&[0, 1, 0, 1]);

        let config = PrunedNgramConfig {
            min_count: 2, // Only keep bigram (0,1)
            ..Default::default()
        };
        let fst = build_pruned_bigram_graph(100, &counts, &config);

        let stats = PrunedNgramStats::from_bigram_graph(&fst, 100);

        // Dense would have 10000 arcs, pruned should have far fewer
        assert!(stats.num_arcs < stats.dense_arcs);
        assert!(stats.compression_ratio > 1.0);
    }
}
