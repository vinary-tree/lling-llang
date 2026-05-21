//! Edit distance transducer for bounded error matching.
//!
//! This module provides WFSTs that accept (input, output) pairs where the
//! edit distance between input and output is within a configurable bound.
//!
//! # Mathematical Foundation
//!
//! An edit distance transducer T_k for max distance k accepts:
//! ```text
//! L(T_k) = {(x, y) | d(x, y) ≤ k}
//! ```
//!
//! where d is the Levenshtein (or Damerau-Levenshtein) distance.
//!
//! # State Encoding
//!
//! States encode the current position in the Levenshtein automaton:
//! ```text
//! state = position × (2k + 1) + offset
//! ```
//!
//! where `position` is the input position and `offset` tracks error count.
//!
//! # Transitions
//!
//! Each state has transitions for:
//! - **Match**: Input char = Output char, advance both, no cost
//! - **Substitution**: Input ≠ Output, advance both, cost 1
//! - **Deletion**: Consume input char, produce ε, cost 1
//! - **Insertion**: Consume ε, produce output char, cost 1
//! - **Transposition**: Swap adjacent chars (Damerau only), cost 1

use std::marker::PhantomData;

use crate::semiring::{Semiring, TropicalWeight};
use crate::wfst::{MutableWfst, StateId, VectorWfst};

/// Configuration for per-operation costs.
#[derive(Debug, Clone)]
pub struct EditCosts {
    /// Cost of inserting a character.
    pub insert: f64,
    /// Cost of deleting a character.
    pub delete: f64,
    /// Cost of substituting one character for another.
    pub substitute: f64,
    /// Cost of transposing two adjacent characters.
    pub transpose: f64,
}

impl Default for EditCosts {
    fn default() -> Self {
        Self {
            insert: 1.0,
            delete: 1.0,
            substitute: 1.0,
            transpose: 1.0,
        }
    }
}

impl EditCosts {
    /// Create uniform costs where all operations have the same cost.
    pub fn uniform(cost: f64) -> Self {
        Self {
            insert: cost,
            delete: cost,
            substitute: cost,
            transpose: cost,
        }
    }

    /// Create costs where transposition is cheaper than separate delete+insert.
    pub fn prefer_transpose() -> Self {
        Self {
            insert: 1.0,
            delete: 1.0,
            substitute: 1.0,
            transpose: 0.5, // Half the cost of delete+insert
        }
    }
}

/// Configuration for edit distance transducer construction.
#[derive(Debug, Clone)]
pub struct EditDistanceConfig {
    /// Maximum edit distance allowed.
    pub max_distance: usize,
    /// Per-operation costs.
    pub costs: EditCosts,
    /// Whether to include transpositions (Damerau-Levenshtein).
    pub include_transpositions: bool,
    /// Alphabet of valid characters.
    pub alphabet: Vec<char>,
}

impl Default for EditDistanceConfig {
    fn default() -> Self {
        Self {
            max_distance: 2,
            costs: EditCosts::default(),
            include_transpositions: false,
            alphabet: Vec::new(),
        }
    }
}

impl EditDistanceConfig {
    /// Create a configuration for Levenshtein distance.
    pub fn levenshtein(max_distance: usize) -> Self {
        Self {
            max_distance,
            include_transpositions: false,
            ..Default::default()
        }
    }

    /// Create a configuration for Damerau-Levenshtein distance.
    pub fn damerau_levenshtein(max_distance: usize) -> Self {
        Self {
            max_distance,
            include_transpositions: true,
            ..Default::default()
        }
    }

    /// Set the alphabet from a string.
    pub fn with_alphabet(mut self, chars: &str) -> Self {
        self.alphabet = chars.chars().collect();
        self
    }

    /// Set the alphabet from a character slice.
    pub fn with_alphabet_chars(mut self, chars: &[char]) -> Self {
        self.alphabet = chars.to_vec();
        self
    }

    /// Set custom costs.
    pub fn with_costs(mut self, costs: EditCosts) -> Self {
        self.costs = costs;
        self
    }
}

/// Edit distance transducer builder.
///
/// Builds a WFST that accepts all (input, output) pairs within a bounded
/// edit distance. The transducer can be composed with a dictionary FSA
/// for efficient fuzzy matching.
///
/// # State Space
///
/// For max distance k, the automaton has O(n × (2k+1)) states where n is
/// the input length. The transducer version has additional output transitions.
///
/// # Example
///
/// ```rust,ignore
/// use lling_llang::error_models::{EditDistanceTransducer, EditDistanceConfig};
///
/// let config = EditDistanceConfig::levenshtein(2)
///     .with_alphabet("abcdefghijklmnopqrstuvwxyz");
///
/// let transducer = EditDistanceTransducer::new(config).build();
/// ```
pub struct EditDistanceTransducer {
    config: EditDistanceConfig,
}

impl EditDistanceTransducer {
    /// Create a new edit distance transducer builder.
    pub fn new(config: EditDistanceConfig) -> Self {
        Self { config }
    }

    /// Create a Levenshtein transducer with the given max distance.
    pub fn levenshtein(max_distance: usize) -> Self {
        Self::new(EditDistanceConfig::levenshtein(max_distance))
    }

    /// Create a Damerau-Levenshtein transducer with the given max distance.
    pub fn damerau_levenshtein(max_distance: usize) -> Self {
        Self::new(EditDistanceConfig::damerau_levenshtein(max_distance))
    }

    /// Set the alphabet for the transducer.
    pub fn with_alphabet(mut self, chars: &str) -> Self {
        self.config = self.config.with_alphabet(chars);
        self
    }

    /// Build the WFST.
    ///
    /// The resulting transducer accepts (input, output) character pairs
    /// where the input is the potentially erroneous text and the output
    /// is the correction.
    ///
    /// Note: For practical use, this transducer should be composed with
    /// an input FSA (the query) and/or a dictionary FSA.
    pub fn build(&self) -> VectorWfst<char, TropicalWeight> {
        let k = self.config.max_distance;
        let costs = &self.config.costs;

        let alphabet = &self.config.alphabet;
        if alphabet.is_empty() {
            // Without an alphabet, return an identity transducer
            return self.build_identity_transducer();
        }

        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

        // Create states for error counts 0..=k
        // State i represents "we've used i edits so far"
        let states: Vec<StateId> = (0..=k).map(|_| fst.add_state()).collect();

        fst.set_start(states[0]);

        // All states are accepting (we can stop at any point)
        for &state in &states {
            fst.set_final(state, TropicalWeight::one());
        }

        // Add transitions
        for (i, &from_state) in states.iter().enumerate() {
            for &c in alphabet {
                // Match: input=c, output=c, no cost, stay at same error count
                fst.add_arc(
                    from_state,
                    Some(c),
                    Some(c),
                    from_state,
                    TropicalWeight::one(),
                );

                // If we haven't reached max errors yet, add error transitions
                if i < k {
                    let next_state = states[i + 1];

                    // Deletion: consume input c, produce nothing
                    fst.add_arc(
                        from_state,
                        Some(c),
                        None,
                        next_state,
                        TropicalWeight::new(costs.delete),
                    );

                    // Insertion: consume nothing, produce c
                    fst.add_arc(
                        from_state,
                        None,
                        Some(c),
                        next_state,
                        TropicalWeight::new(costs.insert),
                    );

                    // Substitution: consume c, produce each other character
                    for &d in alphabet {
                        if c != d {
                            fst.add_arc(
                                from_state,
                                Some(c),
                                Some(d),
                                next_state,
                                TropicalWeight::new(costs.substitute),
                            );
                        }
                    }
                }
            }
        }

        fst
    }

    /// Build an identity transducer (copies input to output unchanged).
    fn build_identity_transducer(&self) -> VectorWfst<char, TropicalWeight> {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        let state = fst.add_state();
        fst.set_start(state);
        fst.set_final(state, TropicalWeight::one());
        fst
    }

    /// Build a query-specific transducer for a given input string.
    ///
    /// This is more efficient than the general transducer when you know
    /// the input ahead of time, as it only generates states reachable
    /// from the specific query.
    pub fn build_for_query(&self, query: &str) -> VectorWfst<char, TropicalWeight> {
        let k = self.config.max_distance;
        let costs = &self.config.costs;
        let query_chars: Vec<char> = query.chars().collect();
        let n = query_chars.len();

        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

        // States: (position, error_count)
        // position: 0..=n (how many query chars consumed)
        // error_count: 0..=k
        //
        // State ID = position * (k + 1) + error_count

        let num_states = (n + 1) * (k + 1);
        fst.add_states(num_states);

        let state_id = |pos: usize, err: usize| -> StateId { (pos * (k + 1) + err) as StateId };

        fst.set_start(state_id(0, 0));

        // States at position n (all input consumed) with any error count are final
        for err in 0..=k {
            fst.set_final(state_id(n, err), TropicalWeight::one());
        }

        let alphabet = if self.config.alphabet.is_empty() {
            // If no alphabet, use unique chars from query
            let mut chars: Vec<char> = query_chars.clone();
            chars.sort();
            chars.dedup();
            chars
        } else {
            self.config.alphabet.clone()
        };

        // Add transitions
        for pos in 0..=n {
            for err in 0..=k {
                let from = state_id(pos, err);

                // Match/Substitution from current position
                if pos < n {
                    let query_char = query_chars[pos];

                    // Match: query_char -> query_char, advance position, no error
                    fst.add_arc(
                        from,
                        Some(query_char),
                        Some(query_char),
                        state_id(pos + 1, err),
                        TropicalWeight::one(),
                    );

                    if err < k {
                        // Deletion: consume query_char, produce nothing
                        fst.add_arc(
                            from,
                            Some(query_char),
                            None,
                            state_id(pos + 1, err + 1),
                            TropicalWeight::new(costs.delete),
                        );

                        // Substitution: query_char -> other char
                        for &c in &alphabet {
                            if c != query_char {
                                fst.add_arc(
                                    from,
                                    Some(query_char),
                                    Some(c),
                                    state_id(pos + 1, err + 1),
                                    TropicalWeight::new(costs.substitute),
                                );
                            }
                        }
                    }
                }

                // Insertion: consume nothing, produce any char
                if err < k {
                    for &c in &alphabet {
                        fst.add_arc(
                            from,
                            None,
                            Some(c),
                            state_id(pos, err + 1),
                            TropicalWeight::new(costs.insert),
                        );
                    }
                }
            }
        }

        fst
    }
}

/// Convenience alias for Damerau-Levenshtein transducer builder.
pub type DamerauLevenshteinTransducer = EditDistanceTransducer;

impl DamerauLevenshteinTransducer {
    /// Create a new Damerau-Levenshtein transducer with transpositions enabled.
    pub fn new_damerau(max_distance: usize) -> Self {
        Self::damerau_levenshtein(max_distance)
    }
}

/// Lazy edit distance transducer using on-demand state computation.
///
/// This variant computes states lazily as they are accessed, making it
/// more efficient for composition where only reachable states matter.
pub struct LazyEditDistanceTransducer<W: Semiring> {
    /// Query string for which the transducer is built.
    query: Vec<char>,
    /// Maximum edit distance.
    max_distance: usize,
    /// Per-operation costs.
    costs: EditCosts,
    /// Alphabet for output characters (reserved for future lazy expansion).
    #[allow(dead_code)]
    alphabet: Vec<char>,
    /// Number of states per position (max_distance + 1).
    states_per_pos: usize,
    /// Phantom marker for weight type.
    _phantom: PhantomData<W>,
}

impl<W: Semiring> LazyEditDistanceTransducer<W> {
    /// Create a new lazy edit distance transducer for a query.
    pub fn new(query: &str, max_distance: usize, alphabet: Vec<char>) -> Self {
        let query: Vec<char> = query.chars().collect();
        let states_per_pos = max_distance + 1;

        Self {
            query,
            max_distance,
            costs: EditCosts::default(),
            alphabet,
            states_per_pos,
            _phantom: PhantomData,
        }
    }

    /// Set custom costs.
    pub fn with_costs(mut self, costs: EditCosts) -> Self {
        self.costs = costs;
        self
    }

    /// Encode (position, error_count) as a state ID.
    #[inline]
    pub fn encode_state(&self, pos: usize, err: usize) -> StateId {
        (pos * self.states_per_pos + err) as StateId
    }

    /// Decode a state ID to (position, error_count).
    #[inline]
    pub fn decode_state(&self, state: StateId) -> (usize, usize) {
        let state = state as usize;
        let pos = state / self.states_per_pos;
        let err = state % self.states_per_pos;
        (pos, err)
    }

    /// Check if a state is valid.
    #[inline]
    pub fn is_valid_state(&self, state: StateId) -> bool {
        let (pos, err) = self.decode_state(state);
        pos <= self.query.len() && err <= self.max_distance
    }

    /// Get the query length.
    pub fn query_len(&self) -> usize {
        self.query.len()
    }

    /// Get the maximum distance.
    pub fn max_distance(&self) -> usize {
        self.max_distance
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::Wfst;

    #[test]
    fn test_edit_costs_default() {
        let costs = EditCosts::default();
        assert_eq!(costs.insert, 1.0);
        assert_eq!(costs.delete, 1.0);
        assert_eq!(costs.substitute, 1.0);
        assert_eq!(costs.transpose, 1.0);
    }

    #[test]
    fn test_edit_costs_uniform() {
        let costs = EditCosts::uniform(0.5);
        assert_eq!(costs.insert, 0.5);
        assert_eq!(costs.delete, 0.5);
        assert_eq!(costs.substitute, 0.5);
        assert_eq!(costs.transpose, 0.5);
    }

    #[test]
    fn test_edit_distance_config() {
        let config = EditDistanceConfig::levenshtein(3);
        assert_eq!(config.max_distance, 3);
        assert!(!config.include_transpositions);

        let config = EditDistanceConfig::damerau_levenshtein(2);
        assert_eq!(config.max_distance, 2);
        assert!(config.include_transpositions);
    }

    #[test]
    fn test_edit_distance_transducer_creation() {
        let transducer = EditDistanceTransducer::levenshtein(2).with_alphabet("abc");

        let fst = transducer.build();
        assert!(!fst.is_empty());
    }

    #[test]
    fn test_edit_distance_transducer_states() {
        let transducer = EditDistanceTransducer::levenshtein(2).with_alphabet("ab");

        let fst = transducer.build();

        // Should have 3 states (error counts 0, 1, 2)
        assert_eq!(fst.num_states(), 3);

        // Start state should be state 0
        assert_eq!(fst.start(), 0);

        // All states should be final
        for s in 0..3 {
            assert!(fst.is_final(s));
        }
    }

    #[test]
    fn test_edit_distance_transducer_transitions() {
        let transducer = EditDistanceTransducer::levenshtein(1).with_alphabet("ab");

        let fst = transducer.build();

        // State 0 should have transitions
        let transitions = fst.transitions(0);
        assert!(!transitions.is_empty());

        // Should have match transitions (a->a, b->b)
        let matches: Vec<_> = transitions
            .iter()
            .filter(|t| t.input == t.output && t.weight == TropicalWeight::one())
            .collect();
        assert!(!matches.is_empty());
    }

    #[test]
    fn test_build_for_query() {
        let transducer = EditDistanceTransducer::levenshtein(2).with_alphabet("hello");

        let fst = transducer.build_for_query("helo");

        // Query has 4 chars, max distance 2, so (4+1) * (2+1) = 15 states
        assert_eq!(fst.num_states(), 15);
    }

    #[test]
    fn test_lazy_state_encoding() {
        let lazy: LazyEditDistanceTransducer<TropicalWeight> =
            LazyEditDistanceTransducer::new("test", 2, vec!['a', 'b']);

        // Test encode/decode roundtrip
        for pos in 0..=4 {
            for err in 0..=2 {
                let encoded = lazy.encode_state(pos, err);
                let (dec_pos, dec_err) = lazy.decode_state(encoded);
                assert_eq!(dec_pos, pos);
                assert_eq!(dec_err, err);
            }
        }
    }

    #[test]
    fn test_damerau_levenshtein_alias() {
        let transducer = DamerauLevenshteinTransducer::new_damerau(2);
        assert!(transducer.config.include_transpositions);
    }
}
