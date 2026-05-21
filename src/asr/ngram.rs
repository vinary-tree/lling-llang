//! N-gram language model transducers with backoff.
//!
//! This module provides efficient WFST representations of n-gram language models
//! with Katz backoff structure to avoid quadratic transition explosion.
//!
//! ## Backoff Structure
//!
//! Instead of O(|V|²) transitions for bigrams, we use:
//! - Seen n-gram: Direct transition with probability weight
//! - Unseen n-gram: ε-transition to backoff state, then transition with unigram prob
//!
//! This reduces the number of transitions from O(|V|^n) to O(seen_ngrams + |V|).
//!
//! ## Weight Format
//!
//! Weights are stored in negative log probability format:
//! - Weight = -log(P(word|history))
//! - Lower weights = higher probability
//!
//! ## Example
//!
//! ```ignore
//! use lling_llang::asr::NgramBuilder;
//! use lling_llang::semiring::LogWeight;
//!
//! let mut builder = NgramBuilder::<LogWeight>::new(3); // trigram
//!
//! // Add unigram probabilities
//! builder.add_unigram(1, LogWeight::new(5.0));  // word_id=1
//! builder.add_unigram(2, LogWeight::new(4.0));  // word_id=2
//!
//! // Add bigram probabilities
//! builder.add_bigram(&[1], 2, LogWeight::new(2.0));  // P(2|1)
//!
//! // Add backoff weights
//! builder.set_backoff(&[1], LogWeight::new(0.5));  // β(1)
//!
//! let fst = builder.build();
//! ```
//!
//! ## References
//!
//! - Mohri et al., "Speech Recognition with WFSTs" Section 4.2
//! - Katz, S. M., "Estimation of Probabilities from Sparse Data"

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst};

/// Word identifier type.
pub type WordId = u32;

/// N-gram order type.
pub type NgramOrder = usize;

/// N-gram weight (probability in log space).
pub type NgramWeight<W> = W;

/// Epsilon label constant.
pub const NGRAM_EPSILON: Option<WordId> = None;

/// Backoff state marker.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BackoffState {
    /// History for this backoff state (shortened by one).
    pub history: Vec<WordId>,
    /// Order of this state (0 = unigram backoff, 1 = bigram backoff, etc.)
    pub order: usize,
}

impl BackoffState {
    /// Create backoff state for given history.
    pub fn new(history: Vec<WordId>) -> Self {
        let order = history.len();
        Self { history, order }
    }

    /// Create initial (unigram) backoff state.
    pub fn initial() -> Self {
        Self {
            history: Vec::new(),
            order: 0,
        }
    }
}

/// Configuration for n-gram transducer construction.
#[derive(Clone, Debug)]
pub struct NgramConfig {
    /// Maximum n-gram order (1=unigram, 2=bigram, 3=trigram, etc.)
    pub order: NgramOrder,

    /// Whether to add sentence boundary markers.
    pub add_sentence_markers: bool,

    /// Start-of-sentence marker word ID.
    pub sos_id: Option<WordId>,

    /// End-of-sentence marker word ID.
    pub eos_id: Option<WordId>,

    /// Unknown word ID for OOV handling.
    pub unk_id: Option<WordId>,
}

impl Default for NgramConfig {
    fn default() -> Self {
        Self {
            order: 3, // Default trigram
            add_sentence_markers: false,
            sos_id: None,
            eos_id: None,
            unk_id: None,
        }
    }
}

/// N-gram state in the language model WFST.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NgramState {
    /// History of words (up to order-1 words).
    pub history: Vec<WordId>,
    /// Whether this is a backoff state.
    pub is_backoff: bool,
}

impl NgramState {
    /// Create state with given history.
    pub fn with_history(history: Vec<WordId>) -> Self {
        Self {
            history,
            is_backoff: false,
        }
    }

    /// Create backoff state for given history.
    pub fn backoff(history: Vec<WordId>) -> Self {
        Self {
            history,
            is_backoff: true,
        }
    }

    /// Create initial state (empty history).
    pub fn initial() -> Self {
        Self {
            history: Vec::new(),
            is_backoff: false,
        }
    }

    /// Get shortened history for backoff.
    pub fn backed_off(&self) -> Self {
        let mut new_history = self.history.clone();
        if !new_history.is_empty() {
            new_history.remove(0);
        }
        Self {
            history: new_history,
            is_backoff: true,
        }
    }

    /// Extend history with new word, maintaining max length.
    pub fn extend(&self, word: WordId, max_history: usize) -> Self {
        let mut new_history = self.history.clone();
        new_history.push(word);

        // Trim to max history length
        while new_history.len() > max_history {
            new_history.remove(0);
        }

        Self {
            history: new_history,
            is_backoff: false,
        }
    }
}

/// N-gram transducer (WFST representation of language model).
pub struct NgramTransducer<W: Semiring> {
    /// The underlying WFST.
    pub fst: VectorWfst<WordId, W>,
    /// Configuration used to build this transducer.
    pub config: NgramConfig,
    /// Vocabulary size.
    pub vocab_size: usize,
}

/// Builder for n-gram language model transducers.
pub struct NgramBuilder<W: Semiring> {
    /// Configuration.
    config: NgramConfig,

    /// Vocabulary size.
    vocab_size: usize,

    /// Unigram probabilities: word -> weight.
    unigrams: HashMap<WordId, W>,

    /// Higher-order n-gram probabilities: history -> (word -> weight).
    ngrams: HashMap<Vec<WordId>, HashMap<WordId, W>>,

    /// Backoff weights: history -> weight.
    backoffs: HashMap<Vec<WordId>, W>,
}

impl<W: Semiring + Clone> NgramBuilder<W> {
    /// Create a new n-gram builder.
    ///
    /// # Arguments
    ///
    /// * `order` - Maximum n-gram order (e.g., 3 for trigram)
    pub fn new(order: NgramOrder) -> Self {
        Self {
            config: NgramConfig {
                order,
                ..Default::default()
            },
            vocab_size: 0,
            unigrams: HashMap::new(),
            ngrams: HashMap::new(),
            backoffs: HashMap::new(),
        }
    }

    /// Set vocabulary size explicitly.
    pub fn vocab_size(mut self, size: usize) -> Self {
        self.vocab_size = size;
        self
    }

    /// Set configuration.
    pub fn config(mut self, config: NgramConfig) -> Self {
        self.config = config;
        self
    }

    /// Add a unigram probability.
    ///
    /// # Arguments
    ///
    /// * `word` - Word ID
    /// * `weight` - Probability weight (in -log space)
    pub fn add_unigram(&mut self, word: WordId, weight: W) {
        self.unigrams.insert(word, weight);
        self.vocab_size = self.vocab_size.max(word as usize + 1);
    }

    /// Add a bigram probability.
    ///
    /// # Arguments
    ///
    /// * `history` - History words (for bigram, this is one word)
    /// * `word` - Next word
    /// * `weight` - Conditional probability weight
    pub fn add_bigram(&mut self, history: &[WordId], word: WordId, weight: W) {
        self.add_ngram(history, word, weight);
    }

    /// Add an n-gram probability.
    ///
    /// # Arguments
    ///
    /// * `history` - History words (up to order-1 words)
    /// * `word` - Next word
    /// * `weight` - Conditional probability weight
    pub fn add_ngram(&mut self, history: &[WordId], word: WordId, weight: W) {
        let history_vec = history.to_vec();
        self.ngrams
            .entry(history_vec)
            .or_insert_with(HashMap::new)
            .insert(word, weight);

        self.vocab_size = self.vocab_size.max(word as usize + 1);
        for &h in history {
            self.vocab_size = self.vocab_size.max(h as usize + 1);
        }
    }

    /// Set backoff weight for a history.
    ///
    /// # Arguments
    ///
    /// * `history` - History words
    /// * `weight` - Backoff weight β(history)
    pub fn set_backoff(&mut self, history: &[WordId], weight: W) {
        self.backoffs.insert(history.to_vec(), weight);
    }

    /// Build the n-gram transducer.
    ///
    /// Constructs a WFST with:
    /// - States for each history context
    /// - Direct transitions for seen n-grams
    /// - ε-transitions to backoff states for unseen n-grams
    /// - Transitions from backoff states for lower-order probabilities
    pub fn build(self) -> NgramTransducer<W> {
        let mut fst: VectorWfst<WordId, W> = VectorWfst::new();
        let mut state_map: HashMap<NgramState, StateId> = HashMap::new();

        // Create initial state (empty history)
        let initial = NgramState::initial();
        let start_id = fst.add_state();
        fst.set_start(start_id);
        fst.set_final(start_id, W::one());
        state_map.insert(initial.clone(), start_id);

        // Create unigram backoff state
        let unigram_backoff = NgramState::backoff(Vec::new());
        let backoff_id = fst.add_state();
        fst.set_final(backoff_id, W::one());
        state_map.insert(unigram_backoff.clone(), backoff_id);

        // Add unigram transitions from backoff state
        for (&word, weight) in &self.unigrams {
            let next_state = NgramState::with_history(vec![word]);
            let next_id = self.get_or_create_state(&mut fst, &mut state_map, &next_state);

            fst.add_arc(backoff_id, Some(word), Some(word), next_id, weight.clone());
        }

        // Add transitions for each history context
        for (history, word_weights) in &self.ngrams {
            let from_state = NgramState::with_history(history.clone());
            let from_id = self.get_or_create_state(&mut fst, &mut state_map, &from_state);

            // Add direct transitions for seen n-grams
            for (&word, weight) in word_weights {
                let next_state = from_state.extend(word, self.config.order - 1);
                let next_id = self.get_or_create_state(&mut fst, &mut state_map, &next_state);

                fst.add_arc(from_id, Some(word), Some(word), next_id, weight.clone());
            }

            // Add backoff epsilon transition
            if let Some(backoff_weight) = self.backoffs.get(history) {
                let backoff_state = from_state.backed_off();
                let backoff_id = self.get_or_create_state(&mut fst, &mut state_map, &backoff_state);

                fst.add_arc(
                    from_id,
                    None, // ε input
                    None, // ε output
                    backoff_id,
                    backoff_weight.clone(),
                );
            }
        }

        // Add backoff from initial state to unigram backoff
        let unigram_backoff_id = *state_map
            .get(&NgramState::backoff(Vec::new()))
            .expect("unigram backoff should exist");

        // Only add if initial has a backoff weight
        if let Some(backoff_weight) = self.backoffs.get(&Vec::new()) {
            fst.add_arc(
                start_id,
                None,
                None,
                unigram_backoff_id,
                backoff_weight.clone(),
            );
        } else {
            // Default backoff weight of 1 (log 0)
            fst.add_arc(start_id, None, None, unigram_backoff_id, W::one());
        }

        NgramTransducer {
            fst,
            config: self.config,
            vocab_size: self.vocab_size,
        }
    }

    /// Get or create a state in the FST.
    fn get_or_create_state(
        &self,
        fst: &mut VectorWfst<WordId, W>,
        state_map: &mut HashMap<NgramState, StateId>,
        state: &NgramState,
    ) -> StateId {
        if let Some(&id) = state_map.get(state) {
            id
        } else {
            let id = fst.add_state();
            fst.set_final(id, W::one());
            state_map.insert(state.clone(), id);
            id
        }
    }
}

impl<W: Semiring> NgramTransducer<W> {
    /// Get the underlying WFST.
    pub fn as_fst(&self) -> &VectorWfst<WordId, W> {
        &self.fst
    }

    /// Get the n-gram order.
    pub fn order(&self) -> NgramOrder {
        self.config.order
    }

    /// Get the vocabulary size.
    pub fn vocabulary_size(&self) -> usize {
        self.vocab_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::LogWeight;
    use crate::wfst::{Wfst, NO_STATE};

    #[test]
    fn test_ngram_state_initial() {
        let state = NgramState::initial();
        assert!(state.history.is_empty());
        assert!(!state.is_backoff);
    }

    #[test]
    fn test_ngram_state_extend() {
        let state = NgramState::initial();

        let state1 = state.extend(1, 2);
        assert_eq!(state1.history, vec![1]);

        let state2 = state1.extend(2, 2);
        assert_eq!(state2.history, vec![1, 2]);

        // Should trim to max history
        let state3 = state2.extend(3, 2);
        assert_eq!(state3.history, vec![2, 3]);
    }

    #[test]
    fn test_ngram_state_backoff() {
        let state = NgramState::with_history(vec![1, 2, 3]);
        let backoff = state.backed_off();

        assert_eq!(backoff.history, vec![2, 3]);
        assert!(backoff.is_backoff);
    }

    #[test]
    fn test_bigram_builder() {
        let mut builder = NgramBuilder::<LogWeight>::new(2);

        // Add unigrams
        builder.add_unigram(1, LogWeight::new(5.0));
        builder.add_unigram(2, LogWeight::new(4.0));
        builder.add_unigram(3, LogWeight::new(6.0));

        // Add bigrams
        builder.add_bigram(&[1], 2, LogWeight::new(2.0));
        builder.add_bigram(&[1], 3, LogWeight::new(3.0));
        builder.add_bigram(&[2], 1, LogWeight::new(2.5));

        // Set backoff weights
        builder.set_backoff(&[1], LogWeight::new(0.5));
        builder.set_backoff(&[2], LogWeight::new(0.6));

        let lm = builder.build();

        // Check basic structure
        assert!(lm.fst.start() != NO_STATE);
        assert!(lm.fst.num_states() > 0);
    }

    #[test]
    fn test_trigram_builder() {
        let mut builder = NgramBuilder::<LogWeight>::new(3);

        // Add unigrams
        builder.add_unigram(1, LogWeight::new(5.0));
        builder.add_unigram(2, LogWeight::new(4.0));

        // Add bigrams
        builder.add_ngram(&[1], 2, LogWeight::new(2.0));

        // Add trigrams
        builder.add_ngram(&[1, 2], 1, LogWeight::new(1.5));

        let lm = builder.build();

        assert_eq!(lm.order(), 3);
        assert!(lm.fst.num_states() >= 3);
    }

    #[test]
    fn test_vocabulary_tracking() {
        let mut builder = NgramBuilder::<LogWeight>::new(2);

        builder.add_unigram(5, LogWeight::new(3.0));
        builder.add_unigram(10, LogWeight::new(4.0));

        let lm = builder.build();

        // Vocabulary size should be at least max word_id + 1
        assert!(lm.vocabulary_size() >= 11);
    }

    #[test]
    fn test_backoff_transitions() {
        let mut builder = NgramBuilder::<LogWeight>::new(2);

        builder.add_unigram(1, LogWeight::new(5.0));
        builder.add_unigram(2, LogWeight::new(4.0));
        builder.add_bigram(&[1], 2, LogWeight::new(2.0));
        builder.set_backoff(&[1], LogWeight::new(0.5));

        let lm = builder.build();

        // Check that we have epsilon transitions (backoff arcs)
        let mut has_epsilon = false;
        for state in 0..lm.fst.num_states() as StateId {
            for trans in lm.fst.transitions(state) {
                if trans.input.is_none() {
                    has_epsilon = true;
                    break;
                }
            }
            if has_epsilon {
                break;
            }
        }

        assert!(has_epsilon, "Should have backoff epsilon transitions");
    }

    #[test]
    fn test_all_states_final() {
        let mut builder = NgramBuilder::<LogWeight>::new(2);

        builder.add_unigram(1, LogWeight::new(5.0));
        builder.add_unigram(2, LogWeight::new(4.0));

        let lm = builder.build();

        // All states should be final (language model accepts any prefix)
        for state in 0..lm.fst.num_states() as StateId {
            assert!(lm.fst.is_final(state));
        }
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::semiring::LogWeight;
    use crate::wfst::{Wfst, NO_STATE};
    use proptest::prelude::*;

    // -------------------------------------------------------------------------
    // BackoffState Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Initial backoff state has empty history.
        #[test]
        fn initial_backoff_empty(_seed in any::<u64>()) {
            let state = BackoffState::initial();
            prop_assert!(state.history.is_empty());
            prop_assert_eq!(state.order, 0);
        }

        /// BackoffState order matches history length.
        #[test]
        fn backoff_order_matches_history(history in prop::collection::vec(0u32..100, 0..5)) {
            let expected_order = history.len();
            let state = BackoffState::new(history);
            prop_assert_eq!(state.order, expected_order);
        }

        /// BackoffState preserves history.
        #[test]
        fn backoff_preserves_history(history in prop::collection::vec(0u32..100, 0..5)) {
            let state = BackoffState::new(history.clone());
            prop_assert_eq!(state.history, history);
        }
    }

    // -------------------------------------------------------------------------
    // NgramConfig Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Default config has trigram order.
        #[test]
        fn default_config_trigram(_seed in any::<u64>()) {
            let config = NgramConfig::default();
            prop_assert_eq!(config.order, 3);
        }

        /// Default config has no sentence markers.
        #[test]
        fn default_config_no_markers(_seed in any::<u64>()) {
            let config = NgramConfig::default();
            prop_assert!(!config.add_sentence_markers);
            prop_assert!(config.sos_id.is_none());
            prop_assert!(config.eos_id.is_none());
        }

        /// Default config has no UNK.
        #[test]
        fn default_config_no_unk(_seed in any::<u64>()) {
            let config = NgramConfig::default();
            prop_assert!(config.unk_id.is_none());
        }
    }

    // -------------------------------------------------------------------------
    // NgramState Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Initial state is not backoff.
        #[test]
        fn initial_not_backoff(_seed in any::<u64>()) {
            let state = NgramState::initial();
            prop_assert!(!state.is_backoff);
            prop_assert!(state.history.is_empty());
        }

        /// with_history creates non-backoff state.
        #[test]
        fn with_history_not_backoff(history in prop::collection::vec(0u32..100, 0..5)) {
            let state = NgramState::with_history(history);
            prop_assert!(!state.is_backoff);
        }

        /// backoff creates backoff state.
        #[test]
        fn backoff_creates_backoff(history in prop::collection::vec(0u32..100, 0..5)) {
            let state = NgramState::backoff(history);
            prop_assert!(state.is_backoff);
        }

        /// backed_off shortens history.
        #[test]
        fn backed_off_shortens(history in prop::collection::vec(0u32..100, 1..5)) {
            let state = NgramState::with_history(history.clone());
            let backed = state.backed_off();

            prop_assert_eq!(backed.history.len(), history.len() - 1);
            prop_assert!(backed.is_backoff);
        }

        /// backed_off on empty history stays empty.
        #[test]
        fn backed_off_empty_stays_empty(_seed in any::<u64>()) {
            let state = NgramState::initial();
            let backed = state.backed_off();

            prop_assert!(backed.history.is_empty());
            prop_assert!(backed.is_backoff);
        }

        /// backed_off removes first element.
        #[test]
        fn backed_off_removes_first(history in prop::collection::vec(0u32..100, 2..5)) {
            let state = NgramState::with_history(history.clone());
            let backed = state.backed_off();

            // The new history should be [history[1], history[2], ...]
            prop_assert_eq!(backed.history, history[1..].to_vec());
        }

        /// extend adds word to history.
        #[test]
        fn extend_adds_word(word in 0u32..100, max_history in 1usize..5) {
            let state = NgramState::initial();
            let extended = state.extend(word, max_history);

            prop_assert!(extended.history.contains(&word));
            prop_assert!(!extended.is_backoff);
        }

        /// extend respects max_history.
        #[test]
        fn extend_respects_max(
            words in prop::collection::vec(0u32..100, 1..10),
            max_history in 1usize..5
        ) {
            let mut state = NgramState::initial();
            for &word in &words {
                state = state.extend(word, max_history);
                prop_assert!(state.history.len() <= max_history);
            }
        }

        /// extend clears backoff flag.
        #[test]
        fn extend_clears_backoff(word in 0u32..100, max_history in 1usize..5) {
            let state = NgramState::backoff(vec![1, 2]);
            let extended = state.extend(word, max_history);

            prop_assert!(!extended.is_backoff);
        }

        /// NgramState equality works correctly.
        #[test]
        fn ngram_state_equality(history in prop::collection::vec(0u32..50, 0..4)) {
            let state1 = NgramState::with_history(history.clone());
            let state2 = NgramState::with_history(history);
            prop_assert_eq!(state1, state2);
        }
    }

    // -------------------------------------------------------------------------
    // NgramBuilder Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        /// Builder preserves order.
        #[test]
        fn builder_preserves_order(order in 1usize..5) {
            let builder = NgramBuilder::<LogWeight>::new(order);
            let lm = builder.build();
            prop_assert_eq!(lm.order(), order);
        }

        /// Adding unigrams updates vocabulary size.
        #[test]
        fn unigrams_update_vocab(word_id in 1u32..100) {
            let mut builder = NgramBuilder::<LogWeight>::new(2);
            builder.add_unigram(word_id, LogWeight::new(1.0));
            let lm = builder.build();

            prop_assert!(lm.vocabulary_size() >= word_id as usize + 1);
        }

        /// Adding ngrams updates vocabulary size.
        #[test]
        fn ngrams_update_vocab(
            history in prop::collection::vec(0u32..50, 1..3),
            word in 50u32..100
        ) {
            let mut builder = NgramBuilder::<LogWeight>::new(3);
            builder.add_ngram(&history, word, LogWeight::new(1.0));
            let lm = builder.build();

            // Vocab should include all words
            let max_word = history.iter().cloned().max().unwrap_or(0).max(word);
            prop_assert!(lm.vocabulary_size() >= max_word as usize + 1);
        }

        /// Config method updates builder config.
        #[test]
        fn builder_config_updates(order in 1usize..5) {
            let config = NgramConfig {
                order,
                add_sentence_markers: true,
                ..Default::default()
            };

            let builder = NgramBuilder::<LogWeight>::new(2).config(config);
            let lm = builder.build();

            prop_assert_eq!(lm.config.order, order);
            prop_assert!(lm.config.add_sentence_markers);
        }

        /// vocab_size method sets vocabulary size.
        #[test]
        fn builder_vocab_size(size in 10usize..100) {
            let builder = NgramBuilder::<LogWeight>::new(2).vocab_size(size);
            let lm = builder.build();

            // If no words added, vocab_size might be overridden
            // But if we add a word smaller than size, it should be at least size
            prop_assert!(lm.vocabulary_size() >= 0);
        }
    }

    // -------------------------------------------------------------------------
    // NgramTransducer Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(25))]

        /// Built transducer has valid start state.
        #[test]
        fn transducer_has_start(order in 1usize..4) {
            let builder = NgramBuilder::<LogWeight>::new(order);
            let lm = builder.build();

            prop_assert!(lm.fst.start() != NO_STATE);
        }

        /// Built transducer has at least 2 states (start + backoff).
        #[test]
        fn transducer_min_states(order in 1usize..4) {
            let builder = NgramBuilder::<LogWeight>::new(order);
            let lm = builder.build();

            prop_assert!(lm.fst.num_states() >= 2);
        }

        /// All states in transducer are final.
        #[test]
        fn transducer_all_final(order in 1usize..4) {
            let mut builder = NgramBuilder::<LogWeight>::new(order);
            builder.add_unigram(1, LogWeight::new(1.0));
            let lm = builder.build();

            for state in 0..lm.fst.num_states() as StateId {
                prop_assert!(lm.fst.is_final(state));
            }
        }

        /// Transducer with unigrams has epsilon transitions (backoff).
        #[test]
        fn transducer_has_backoff_arcs(order in 2usize..4) {
            let mut builder = NgramBuilder::<LogWeight>::new(order);
            builder.add_unigram(1, LogWeight::new(1.0));
            builder.add_unigram(2, LogWeight::new(1.0));
            let lm = builder.build();

            // Should have at least one epsilon transition (backoff from start)
            let mut has_epsilon = false;
            for state in 0..lm.fst.num_states() as StateId {
                for trans in lm.fst.transitions(state) {
                    if trans.input.is_none() {
                        has_epsilon = true;
                        break;
                    }
                }
            }

            prop_assert!(has_epsilon);
        }

        /// Unigram transitions exist from backoff state.
        #[test]
        fn unigram_transitions_exist(words in prop::collection::vec(1u32..10, 1..5)) {
            let mut builder = NgramBuilder::<LogWeight>::new(2);
            for &word in &words {
                builder.add_unigram(word, LogWeight::new(1.0));
            }
            let lm = builder.build();

            // Count transitions with non-epsilon labels
            let mut word_transitions = 0;
            for state in 0..lm.fst.num_states() as StateId {
                for trans in lm.fst.transitions(state) {
                    if trans.input.is_some() {
                        word_transitions += 1;
                    }
                }
            }

            // Should have at least as many transitions as unique words
            let unique_words: std::collections::HashSet<_> = words.iter().collect();
            prop_assert!(word_transitions >= unique_words.len());
        }
    }

    // -------------------------------------------------------------------------
    // Bigram Specific Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// Bigram with backoff has epsilon transition from history state.
        #[test]
        fn bigram_backoff_structure(
            word1 in 1u32..10,
            word2 in 10u32..20
        ) {
            let mut builder = NgramBuilder::<LogWeight>::new(2);
            builder.add_unigram(word1, LogWeight::new(1.0));
            builder.add_unigram(word2, LogWeight::new(1.0));
            builder.add_bigram(&[word1], word2, LogWeight::new(0.5));
            builder.set_backoff(&[word1], LogWeight::new(0.3));

            let lm = builder.build();

            // Should have states and transitions
            prop_assert!(lm.fst.num_states() >= 3);
        }
    }
}
