//! Piecewise subsequential transducers.
//!
//! This module provides an optimal representation for non-subsequential functions
//! by decomposing them into a finite union of subsequential pieces.
//!
//! # Background
//!
//! A transducer is **subsequential** if it is:
//! 1. Deterministic on the input (each state has at most one transition per input)
//! 2. Has a unique final output string per final state
//!
//! Subsequential transducers are efficient because they process input left-to-right
//! without backtracking. However, many useful functions (like morphological analysis)
//! are not subsequential.
//!
//! # Piecewise Subsequential Decomposition
//!
//! Any finite-state transducer that computes a function can be decomposed into
//! a finite union of subsequential transducers:
//!
//! ```text
//! T = T₁ ∪ T₂ ∪ ... ∪ Tₖ
//! ```
//!
//! where each Tᵢ is subsequential. The minimum k is called the **degree of ambiguity**.
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::subsequential::*;
//!
//! // A non-subsequential transducer
//! let transducer = build_morphology_transducer();
//!
//! // Decompose into subsequential pieces
//! let piecewise = PiecewiseSubsequential::decompose(&transducer);
//!
//! // Apply efficiently (each piece is O(n) in input length)
//! let outputs = piecewise.apply(&input);
//! ```
//!
//! # References
//!
//! - Schützenberger (1977): "Sur une variante des fonctions séquentielles"
//! - Roche & Schabes (1997): "Deterministic Part-of-Speech Tagging with FSTs"
//! - Mohri (2000): "Minimization Algorithms for Sequential Transducers"

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst, NO_STATE};

/// A subsequential (deterministic) transducer.
///
/// In a subsequential transducer:
/// - Each state has at most one transition per input symbol
/// - Each final state has a unique final output string
/// - The transducer computes a (partial) function from input to output
#[derive(Debug, Clone)]
pub struct SubsequentialTransducer<L, W>
where
    L: Clone + Eq + Hash,
    W: Semiring,
{
    /// The underlying WFST (guaranteed to be deterministic on input).
    wfst: VectorWfst<L, W>,
    /// Final outputs indexed by final state.
    final_outputs: HashMap<StateId, Vec<L>>,
    /// Piece identifier (for tracking in decomposition).
    piece_id: usize,
}

impl<L, W> SubsequentialTransducer<L, W>
where
    L: Clone + Eq + Hash + Send + Sync + 'static,
    W: Semiring,
{
    /// Create a new subsequential transducer from a WFST.
    ///
    /// Returns `None` if the WFST is not subsequential (not deterministic on input).
    pub fn from_wfst(wfst: VectorWfst<L, W>) -> Option<Self> {
        if !Self::is_subsequential(&wfst) {
            return None;
        }

        let final_outputs = HashMap::new();
        Some(Self {
            wfst,
            final_outputs,
            piece_id: 0,
        })
    }

    /// Check if a WFST is subsequential (deterministic on input).
    fn is_subsequential(wfst: &VectorWfst<L, W>) -> bool {
        // Check that each state has at most one transition per input symbol
        for state_id in 0..wfst.num_states() as StateId {
            let mut seen_inputs: HashMap<Option<L>, bool> = HashMap::new();

            for trans in wfst.transitions(state_id) {
                if seen_inputs.contains_key(&trans.input) {
                    return false; // Non-deterministic
                }
                seen_inputs.insert(trans.input.clone(), true);
            }
        }
        true
    }

    /// Apply the transducer to an input sequence.
    ///
    /// Returns `None` if the input is not accepted.
    pub fn apply(&self, input: &[L]) -> Option<(Vec<L>, W)> {
        let start = self.wfst.start();
        if start == NO_STATE {
            return None;
        }

        let mut state = start;
        let mut output = Vec::new();
        let mut weight = W::one();

        for symbol in input {
            let mut found = false;
            for trans in self.wfst.transitions(state) {
                if trans.input.as_ref() == Some(symbol) {
                    if let Some(out) = &trans.output {
                        output.push(out.clone());
                    }
                    weight = weight.times(&trans.weight);
                    state = trans.to;
                    found = true;
                    break;
                }
            }
            if !found {
                return None; // Input not accepted
            }
        }

        // Check if final state and add final output
        if self.wfst.is_final(state) {
            let final_weight = self.wfst.final_weight(state);
            weight = weight.times(&final_weight);

            if let Some(final_out) = self.final_outputs.get(&state) {
                output.extend(final_out.iter().cloned());
            }

            Some((output, weight))
        } else {
            None
        }
    }

    /// Get the underlying WFST.
    pub fn wfst(&self) -> &VectorWfst<L, W> {
        &self.wfst
    }

    /// Get the piece identifier.
    pub fn piece_id(&self) -> usize {
        self.piece_id
    }

    /// Set a final output for a state.
    pub fn set_final_output(&mut self, state: StateId, output: Vec<L>) {
        self.final_outputs.insert(state, output);
    }
}

/// A piecewise subsequential transducer.
///
/// Represents a non-subsequential transducer as a union of subsequential pieces.
/// This allows efficient processing where each piece runs in linear time.
#[derive(Debug, Clone)]
pub struct PiecewiseSubsequential<L, W>
where
    L: Clone + Eq + Hash,
    W: Semiring,
{
    /// The subsequential pieces.
    pieces: Vec<SubsequentialTransducer<L, W>>,
    /// Statistics about the decomposition.
    stats: DecompositionStats,
}

/// Statistics about piecewise decomposition.
#[derive(Debug, Clone, Default)]
pub struct DecompositionStats {
    /// Number of subsequential pieces.
    pub num_pieces: usize,
    /// Total number of states across all pieces.
    pub total_states: usize,
    /// Total number of transitions across all pieces.
    pub total_transitions: usize,
    /// Whether every accepted source path is represented by a materialized piece.
    pub is_exact: bool,
}

#[derive(Debug, Clone)]
struct AmbiguitySite<L> {
    state: StateId,
    input: Option<L>,
    alternatives: usize,
}

impl<L, W> PiecewiseSubsequential<L, W>
where
    L: Clone + Eq + Hash + Send + Sync + 'static,
    W: Semiring,
{
    /// Create a new piecewise subsequential transducer from pieces.
    pub fn new(pieces: Vec<SubsequentialTransducer<L, W>>) -> Self {
        Self::from_pieces_with_exactness(pieces, true)
    }

    fn from_pieces_with_exactness(
        pieces: Vec<SubsequentialTransducer<L, W>>,
        is_exact: bool,
    ) -> Self {
        let stats = DecompositionStats {
            num_pieces: pieces.len(),
            total_states: pieces.iter().map(|p| p.wfst.num_states()).sum(),
            total_transitions: pieces
                .iter()
                .map(|p| {
                    (0..p.wfst.num_states() as StateId)
                        .map(|s| p.wfst.transitions(s).len())
                        .sum::<usize>()
                })
                .sum(),
            is_exact,
        };

        Self { pieces, stats }
    }

    /// Decompose a non-subsequential WFST into subsequential pieces.
    ///
    /// Uses the subset construction algorithm to create deterministic pieces,
    /// then splits ambiguous paths into separate pieces.
    pub fn decompose(wfst: &VectorWfst<L, W>) -> Self
    where
        W: Clone,
    {
        // If already subsequential, return single piece
        if SubsequentialTransducer::is_subsequential(wfst) {
            let piece = SubsequentialTransducer {
                wfst: wfst.clone(),
                final_outputs: HashMap::new(),
                piece_id: 0,
            };
            return Self::new(vec![piece]);
        }

        let (pieces, is_exact) = Self::build_pieces(wfst);
        Self::from_pieces_with_exactness(pieces, is_exact)
    }

    /// Build subsequential pieces using subset construction with output disambiguation.
    fn build_pieces(wfst: &VectorWfst<L, W>) -> (Vec<SubsequentialTransducer<L, W>>, bool)
    where
        W: Clone,
    {
        let start = wfst.start();
        if start == NO_STATE {
            return (vec![], true);
        }

        let ambiguity_sites = Self::find_ambiguity_sites(wfst);

        if ambiguity_sites.is_empty() {
            let piece = SubsequentialTransducer {
                wfst: wfst.clone(),
                final_outputs: HashMap::new(),
                piece_id: 0,
            };
            return (vec![piece], true);
        }

        let is_exact = !Self::has_repeating_ambiguity(wfst, &ambiguity_sites);
        let piece_count = match Self::piece_count(&ambiguity_sites) {
            Some(count) => count,
            None => {
                let choice_map = Self::choice_map_for_piece(&ambiguity_sites, 0);
                let mut piece_wfst = VectorWfst::new();
                Self::copy_with_disambiguation(wfst, &mut piece_wfst, &choice_map);
                return (
                    vec![SubsequentialTransducer {
                        wfst: piece_wfst,
                        final_outputs: HashMap::new(),
                        piece_id: 0,
                    }],
                    false,
                );
            }
        };

        let mut result = Vec::with_capacity(piece_count);
        for i in 0..piece_count {
            let choice_map = Self::choice_map_for_piece(&ambiguity_sites, i);
            let mut piece_wfst = VectorWfst::new();
            Self::copy_with_disambiguation(wfst, &mut piece_wfst, &choice_map);

            result.push(SubsequentialTransducer {
                wfst: piece_wfst,
                final_outputs: HashMap::new(),
                piece_id: i,
            });
        }

        (result, is_exact)
    }

    fn piece_count(ambiguity_sites: &[AmbiguitySite<L>]) -> Option<usize> {
        let mut count = 1usize;
        for site in ambiguity_sites {
            count = count.checked_mul(site.alternatives)?;
        }
        Some(count)
    }

    fn choice_map_for_piece(
        ambiguity_sites: &[AmbiguitySite<L>],
        piece_idx: usize,
    ) -> HashMap<(StateId, Option<L>), usize> {
        let mut residue = piece_idx;
        let mut choices = HashMap::with_capacity(ambiguity_sites.len());

        for site in ambiguity_sites {
            let choice = residue % site.alternatives;
            residue /= site.alternatives;
            choices.insert((site.state, site.input.clone()), choice);
        }

        choices
    }

    /// Find states where the transducer is ambiguous (multiple transitions with same input).
    #[cfg(test)]
    fn find_ambiguity_points(wfst: &VectorWfst<L, W>) -> Vec<(StateId, usize)> {
        let mut by_state: HashMap<StateId, usize> = HashMap::new();

        for site in Self::find_ambiguity_sites(wfst) {
            by_state
                .entry(site.state)
                .and_modify(|count| *count = (*count).max(site.alternatives))
                .or_insert(site.alternatives);
        }

        let mut ambiguous: Vec<_> = by_state.into_iter().collect();
        ambiguous.sort_by_key(|(state, _)| *state);
        ambiguous
    }

    fn find_ambiguity_sites(wfst: &VectorWfst<L, W>) -> Vec<AmbiguitySite<L>> {
        let mut ambiguous = Vec::new();

        for state_id in Self::reachable_states(wfst) {
            for (input, count) in Self::input_group_counts(wfst, state_id) {
                if count > 1 {
                    ambiguous.push(AmbiguitySite {
                        state: state_id,
                        input,
                        alternatives: count,
                    });
                }
            }
        }

        ambiguous
    }

    fn input_group_counts(wfst: &VectorWfst<L, W>, state_id: StateId) -> Vec<(Option<L>, usize)> {
        let mut group_indices: HashMap<Option<L>, usize> = HashMap::new();
        let mut groups: Vec<(Option<L>, usize)> = Vec::new();

        for trans in wfst.transitions(state_id) {
            let key = trans.input.clone();
            if let Some(idx) = group_indices.get(&key).copied() {
                groups[idx].1 += 1;
            } else {
                let idx = groups.len();
                group_indices.insert(key.clone(), idx);
                groups.push((key, 1));
            }
        }

        groups
    }

    fn transition_groups<'a>(
        wfst: &'a VectorWfst<L, W>,
        state_id: StateId,
    ) -> Vec<(Option<L>, Vec<&'a WeightedTransition<L, W>>)> {
        let mut group_indices: HashMap<Option<L>, usize> = HashMap::new();
        let mut groups: Vec<(Option<L>, Vec<&'a WeightedTransition<L, W>>)> = Vec::new();

        for trans in wfst.transitions(state_id) {
            let key = trans.input.clone();
            if let Some(idx) = group_indices.get(&key).copied() {
                groups[idx].1.push(trans);
            } else {
                let idx = groups.len();
                group_indices.insert(key.clone(), idx);
                groups.push((key, vec![trans]));
            }
        }

        groups
    }

    fn reachable_states(wfst: &VectorWfst<L, W>) -> Vec<StateId> {
        let start = wfst.start();
        if start == NO_STATE {
            return Vec::new();
        }

        let mut seen = HashSet::new();
        let mut queue = VecDeque::new();
        seen.insert(start);
        queue.push_back(start);

        while let Some(state) = queue.pop_front() {
            for trans in wfst.transitions(state) {
                if seen.insert(trans.to) {
                    queue.push_back(trans.to);
                }
            }
        }

        let mut states: Vec<_> = seen.into_iter().collect();
        states.sort_unstable();
        states
    }

    fn has_repeating_ambiguity(
        wfst: &VectorWfst<L, W>,
        ambiguity_sites: &[AmbiguitySite<L>],
    ) -> bool {
        let mut checked = HashSet::new();

        for site in ambiguity_sites {
            if checked.insert(site.state) && Self::state_has_cycle(wfst, site.state) {
                return true;
            }
        }

        false
    }

    fn state_has_cycle(wfst: &VectorWfst<L, W>, state: StateId) -> bool {
        let mut seen = HashSet::new();
        let mut queue = VecDeque::new();

        for trans in wfst.transitions(state) {
            if trans.to == state {
                return true;
            }
            if seen.insert(trans.to) {
                queue.push_back(trans.to);
            }
        }

        while let Some(current) = queue.pop_front() {
            for trans in wfst.transitions(current) {
                if trans.to == state {
                    return true;
                }
                if seen.insert(trans.to) {
                    queue.push_back(trans.to);
                }
            }
        }

        false
    }

    /// Copy WFST structure with one selected alternative per ambiguous input group.
    fn copy_with_disambiguation(
        source: &VectorWfst<L, W>,
        target: &mut VectorWfst<L, W>,
        choices: &HashMap<(StateId, Option<L>), usize>,
    ) where
        W: Clone,
    {
        let start = source.start();
        if start == NO_STATE {
            return;
        }

        // State mapping from source to target
        let mut state_map: HashMap<StateId, StateId> = HashMap::new();
        let target_start = if target.start() == NO_STATE {
            let s = target.add_state();
            target.set_start(s);
            s
        } else {
            target.start()
        };
        state_map.insert(start, target_start);

        // BFS to copy states and transitions
        let mut queue = VecDeque::new();
        queue.push_back(start);

        while let Some(src_state) = queue.pop_front() {
            let Some(tgt_state) = state_map.get(&src_state).copied() else {
                continue;
            };

            // Copy final weight
            if source.is_final(src_state) {
                target.set_final(tgt_state, source.final_weight(src_state));
            }

            // Copy transitions, disambiguating at ambiguous points
            for (input, trans_list) in Self::transition_groups(source, src_state) {
                let trans = if trans_list.len() > 1 {
                    let choice = choices
                        .get(&(src_state, input.clone()))
                        .copied()
                        .unwrap_or(0)
                        % trans_list.len();
                    trans_list.get(choice)
                } else {
                    trans_list.first()
                };

                if let Some(trans) = trans {
                    // Get or create target state
                    let to_state = *state_map.entry(trans.to).or_insert_with(|| {
                        let new_state = target.add_state();
                        queue.push_back(trans.to);
                        new_state
                    });

                    target.add_transition(WeightedTransition::new(
                        tgt_state,
                        trans.input.clone(),
                        trans.output.clone(),
                        to_state,
                        trans.weight.clone(),
                    ));
                }
            }
        }
    }

    /// Apply the piecewise transducer to an input sequence.
    ///
    /// Returns all possible outputs from all pieces (may contain duplicates).
    pub fn apply(&self, input: &[L]) -> Vec<(Vec<L>, W)> {
        let mut results = Vec::new();

        for piece in &self.pieces {
            if let Some(result) = piece.apply(input) {
                results.push(result);
            }
        }

        results
    }

    /// Apply and deduplicate results.
    pub fn apply_unique(&self, input: &[L]) -> Vec<(Vec<L>, W)>
    where
        L: Ord,
    {
        let mut results = self.apply(input);

        // Sort and deduplicate by output
        results.sort_by(|a, b| a.0.cmp(&b.0));
        results.dedup_by(|a, b| a.0 == b.0);

        results
    }

    /// Get the number of pieces.
    pub fn num_pieces(&self) -> usize {
        self.pieces.len()
    }

    /// Get the decomposition statistics.
    pub fn stats(&self) -> &DecompositionStats {
        &self.stats
    }

    /// Get the pieces.
    pub fn pieces(&self) -> &[SubsequentialTransducer<L, W>] {
        &self.pieces
    }

    /// Get a mutable reference to the pieces.
    pub fn pieces_mut(&mut self) -> &mut Vec<SubsequentialTransducer<L, W>> {
        &mut self.pieces
    }

    /// Check if the decomposition is trivial (single piece).
    pub fn is_trivial(&self) -> bool {
        self.pieces.len() == 1
    }

    /// Get the degree of ambiguity (number of pieces).
    pub fn degree(&self) -> usize {
        self.pieces.len()
    }
}

/// Builder for piecewise subsequential transducers.
#[derive(Debug, Clone)]
pub struct PiecewiseBuilder<L, W>
where
    L: Clone + Eq + Hash,
    W: Semiring,
{
    pieces: Vec<SubsequentialTransducer<L, W>>,
}

impl<L, W> PiecewiseBuilder<L, W>
where
    L: Clone + Eq + Hash + Send + Sync + 'static,
    W: Semiring,
{
    /// Create a new builder.
    pub fn new() -> Self {
        Self { pieces: Vec::new() }
    }

    /// Add a subsequential piece.
    pub fn add_piece(mut self, piece: SubsequentialTransducer<L, W>) -> Self {
        self.pieces.push(piece);
        self
    }

    /// Add a WFST as a piece (must be subsequential).
    pub fn add_wfst(mut self, wfst: VectorWfst<L, W>) -> Option<Self> {
        let piece = SubsequentialTransducer::from_wfst(wfst)?;
        self.pieces.push(piece);
        Some(self)
    }

    /// Build the piecewise transducer.
    pub fn build(self) -> PiecewiseSubsequential<L, W> {
        PiecewiseSubsequential::new(self.pieces)
    }
}

impl<L, W> Default for PiecewiseBuilder<L, W>
where
    L: Clone + Eq + Hash + Send + Sync + 'static,
    W: Semiring,
{
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    fn make_simple_fst() -> VectorWfst<char, TropicalWeight> {
        let mut fst = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();

        fst.set_start(s0);
        fst.set_final(s2, TropicalWeight::one());

        // Simple path: a -> A -> b -> B
        fst.add_transition(WeightedTransition::new(
            s0,
            Some('a'),
            Some('A'),
            s1,
            TropicalWeight::one(),
        ));
        fst.add_transition(WeightedTransition::new(
            s1,
            Some('b'),
            Some('B'),
            s2,
            TropicalWeight::one(),
        ));

        fst
    }

    fn make_ambiguous_fst() -> VectorWfst<char, TropicalWeight> {
        let mut fst = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        let s3 = fst.add_state();

        fst.set_start(s0);
        fst.set_final(s2, TropicalWeight::one());
        fst.set_final(s3, TropicalWeight::one());

        // Ambiguous: 'a' can output 'X' or 'Y'
        fst.add_transition(WeightedTransition::new(
            s0,
            Some('a'),
            Some('X'),
            s1,
            TropicalWeight::new(1.0),
        ));
        fst.add_transition(WeightedTransition::new(
            s0,
            Some('a'),
            Some('Y'),
            s1,
            TropicalWeight::new(2.0),
        ));
        fst.add_transition(WeightedTransition::new(
            s1,
            Some('b'),
            Some('B'),
            s2,
            TropicalWeight::one(),
        ));
        fst.add_transition(WeightedTransition::new(
            s1,
            Some('c'),
            Some('C'),
            s3,
            TropicalWeight::one(),
        ));

        fst
    }

    fn make_independent_ambiguities_fst() -> VectorWfst<char, TropicalWeight> {
        let mut fst = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();

        fst.set_start(s0);
        fst.set_final(s2, TropicalWeight::one());

        fst.add_transition(WeightedTransition::new(
            s0,
            Some('a'),
            Some('X'),
            s1,
            TropicalWeight::one(),
        ));
        fst.add_transition(WeightedTransition::new(
            s0,
            Some('a'),
            Some('Y'),
            s1,
            TropicalWeight::one(),
        ));
        fst.add_transition(WeightedTransition::new(
            s1,
            Some('b'),
            Some('M'),
            s2,
            TropicalWeight::one(),
        ));
        fst.add_transition(WeightedTransition::new(
            s1,
            Some('b'),
            Some('N'),
            s2,
            TropicalWeight::one(),
        ));

        fst
    }

    fn make_repeating_ambiguity_fst() -> VectorWfst<char, TropicalWeight> {
        let mut fst = VectorWfst::new();
        let s0 = fst.add_state();

        fst.set_start(s0);
        fst.set_final(s0, TropicalWeight::one());

        fst.add_transition(WeightedTransition::new(
            s0,
            Some('a'),
            Some('X'),
            s0,
            TropicalWeight::one(),
        ));
        fst.add_transition(WeightedTransition::new(
            s0,
            Some('a'),
            Some('Y'),
            s0,
            TropicalWeight::one(),
        ));

        fst
    }

    #[test]
    fn test_subsequential_check() {
        let simple = make_simple_fst();
        assert!(SubsequentialTransducer::<char, TropicalWeight>::is_subsequential(&simple));

        let ambiguous = make_ambiguous_fst();
        assert!(!SubsequentialTransducer::<char, TropicalWeight>::is_subsequential(&ambiguous));
    }

    #[test]
    fn test_subsequential_from_wfst() {
        let simple = make_simple_fst();
        let subseq = SubsequentialTransducer::from_wfst(simple);
        assert!(subseq.is_some());

        let ambiguous = make_ambiguous_fst();
        let subseq = SubsequentialTransducer::from_wfst(ambiguous);
        assert!(subseq.is_none());
    }

    #[test]
    fn test_subsequential_apply() {
        let fst = make_simple_fst();
        let subseq = SubsequentialTransducer::from_wfst(fst).expect("Should be subsequential");

        let result = subseq.apply(&['a', 'b']);
        assert!(result.is_some());

        let (output, _weight) = result.expect("subsequential/mod.rs: required value was None/Err");
        assert_eq!(output, vec!['A', 'B']);
    }

    #[test]
    fn test_subsequential_apply_not_accepted() {
        let fst = make_simple_fst();
        let subseq = SubsequentialTransducer::from_wfst(fst).expect("Should be subsequential");

        // 'a' alone is not accepted (s1 is not final)
        let result = subseq.apply(&['a']);
        assert!(result.is_none());

        // 'x' is not in the alphabet
        let result = subseq.apply(&['x']);
        assert!(result.is_none());
    }

    #[test]
    fn test_decompose_subsequential() {
        let fst = make_simple_fst();
        let piecewise = PiecewiseSubsequential::decompose(&fst);

        assert!(piecewise.is_trivial());
        assert_eq!(piecewise.num_pieces(), 1);
    }

    #[test]
    fn test_decompose_ambiguous() {
        let fst = make_ambiguous_fst();
        let piecewise = PiecewiseSubsequential::decompose(&fst);

        assert_eq!(piecewise.num_pieces(), 2);
        assert!(piecewise.stats().is_exact);

        for piece in piecewise.pieces() {
            assert!(
                SubsequentialTransducer::<char, TropicalWeight>::is_subsequential(piece.wfst())
            );
        }
    }

    #[test]
    fn test_decompose_ambiguous_outputs_all_choices() {
        let fst = make_ambiguous_fst();
        let piecewise = PiecewiseSubsequential::decompose(&fst);

        let mut outputs: Vec<_> = piecewise
            .apply(&['a', 'b'])
            .into_iter()
            .map(|(output, _)| output)
            .collect();
        outputs.sort();

        assert_eq!(outputs, vec![vec!['X', 'B'], vec!['Y', 'B']]);
    }

    #[test]
    fn test_decompose_independent_ambiguities_uses_cartesian_product() {
        let fst = make_independent_ambiguities_fst();
        let piecewise = PiecewiseSubsequential::decompose(&fst);

        assert_eq!(piecewise.num_pieces(), 4);
        assert!(piecewise.stats().is_exact);

        let mut outputs: Vec<_> = piecewise
            .apply(&['a', 'b'])
            .into_iter()
            .map(|(output, _)| output)
            .collect();
        outputs.sort();

        assert_eq!(
            outputs,
            vec![
                vec!['X', 'M'],
                vec!['X', 'N'],
                vec!['Y', 'M'],
                vec!['Y', 'N'],
            ]
        );
    }

    #[test]
    fn test_repeating_ambiguity_reports_non_exact_materialization() {
        let fst = make_repeating_ambiguity_fst();
        let piecewise = PiecewiseSubsequential::decompose(&fst);

        assert_eq!(piecewise.num_pieces(), 2);
        assert!(!piecewise.stats().is_exact);

        for piece in piecewise.pieces() {
            assert!(
                SubsequentialTransducer::<char, TropicalWeight>::is_subsequential(piece.wfst())
            );
        }
    }

    #[test]
    fn test_piecewise_apply() {
        let fst = make_simple_fst();
        let piecewise = PiecewiseSubsequential::decompose(&fst);

        let results = piecewise.apply(&['a', 'b']);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, vec!['A', 'B']);
    }

    #[test]
    fn test_piecewise_stats() {
        let fst = make_simple_fst();
        let piecewise = PiecewiseSubsequential::decompose(&fst);

        let stats = piecewise.stats();
        assert_eq!(stats.num_pieces, 1);
        assert!(stats.total_states > 0);
    }

    #[test]
    fn test_builder() {
        let fst = make_simple_fst();
        let piece = SubsequentialTransducer::from_wfst(fst).expect("Should be subsequential");

        let piecewise = PiecewiseBuilder::new().add_piece(piece).build();

        assert_eq!(piecewise.num_pieces(), 1);
    }

    #[test]
    fn test_builder_add_wfst() {
        let fst = make_simple_fst();

        let builder = PiecewiseBuilder::<char, TropicalWeight>::new().add_wfst(fst);

        assert!(builder.is_some());
        let piecewise = builder
            .expect("subsequential/mod.rs: required value was None/Err")
            .build();
        assert_eq!(piecewise.num_pieces(), 1);
    }

    #[test]
    fn test_builder_reject_ambiguous() {
        let fst = make_ambiguous_fst();

        let builder = PiecewiseBuilder::<char, TropicalWeight>::new().add_wfst(fst);

        assert!(builder.is_none()); // Should fail because FST is not subsequential
    }

    #[test]
    fn test_degree() {
        let fst = make_simple_fst();
        let piecewise = PiecewiseSubsequential::decompose(&fst);
        assert_eq!(piecewise.degree(), 1);
    }

    #[test]
    fn test_piece_id() {
        let fst = make_simple_fst();
        let piece = SubsequentialTransducer::from_wfst(fst).expect("Should be subsequential");
        assert_eq!(piece.piece_id(), 0);
    }

    #[test]
    fn test_empty_fst() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        let piecewise = PiecewiseSubsequential::decompose(&fst);

        let results = piecewise.apply(&['a']);
        assert!(results.is_empty());
    }

    #[test]
    fn test_set_final_output() {
        let fst = make_simple_fst();
        let mut subseq = SubsequentialTransducer::from_wfst(fst).expect("Should be subsequential");

        subseq.set_final_output(2, vec!['!']);

        let result = subseq.apply(&['a', 'b']);
        assert!(result.is_some());

        let (output, _) = result.expect("subsequential/mod.rs: required value was None/Err");
        assert_eq!(output, vec!['A', 'B', '!']);
    }

    #[test]
    fn test_apply_unique() {
        let fst = make_simple_fst();
        let piecewise = PiecewiseSubsequential::decompose(&fst);

        let results = piecewise.apply_unique(&['a', 'b']);
        assert!(!results.is_empty());

        // Should be deduplicated
        let mut seen = std::collections::HashSet::new();
        for (output, _) in &results {
            assert!(seen.insert(output.clone()), "Duplicate output found");
        }
    }

    #[test]
    fn test_find_ambiguity_points() {
        let fst = make_ambiguous_fst();
        let ambiguous = PiecewiseSubsequential::<char, TropicalWeight>::find_ambiguity_points(&fst);

        assert!(!ambiguous.is_empty());
        // State 0 has ambiguity on 'a'
        assert!(ambiguous
            .iter()
            .any(|(state, count)| *state == 0 && *count == 2));
    }

    #[test]
    fn test_decomposition_stats() {
        let fst = make_ambiguous_fst();
        let piecewise = PiecewiseSubsequential::decompose(&fst);

        let stats = piecewise.stats();
        assert!(stats.num_pieces >= 1);
        assert!(stats.total_states > 0);
        assert!(stats.total_transitions > 0);
    }
}
