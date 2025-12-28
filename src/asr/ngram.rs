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
use std::hash::Hash;
use std::fmt::Debug;

use crate::semiring::Semiring;
use crate::wfst::{VectorWfst, MutableWfst, Wfst, StateId};

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

            fst.add_arc(
                backoff_id,
                Some(word),
                Some(word),
                next_id,
                weight.clone(),
            );
        }

        // Add transitions for each history context
        for (history, word_weights) in &self.ngrams {
            let from_state = NgramState::with_history(history.clone());
            let from_id = self.get_or_create_state(&mut fst, &mut state_map, &from_state);

            // Add direct transitions for seen n-grams
            for (&word, weight) in word_weights {
                let next_state = from_state.extend(word, self.config.order - 1);
                let next_id = self.get_or_create_state(&mut fst, &mut state_map, &next_state);

                fst.add_arc(
                    from_id,
                    Some(word),
                    Some(word),
                    next_id,
                    weight.clone(),
                );
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
        let unigram_backoff_id = *state_map.get(&NgramState::backoff(Vec::new()))
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
            fst.add_arc(
                start_id,
                None,
                None,
                unigram_backoff_id,
                W::one(),
            );
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
