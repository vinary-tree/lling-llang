//! Builder for multi-tape WFSTs.

use std::hash::Hash;

use super::label::MultiTapeLabel;
use super::traits::MultiTapeWfst;
use super::transition::MultiTapeTransition;
use super::vector::VectorMultiTapeWfst;
use crate::semiring::Semiring;
use crate::wfst::StateId;

/// Builder for constructing multi-tape WFSTs.
#[derive(Debug, Clone)]
pub struct MultiTapeWfstBuilder<L, W: Semiring, const N: usize> {
    /// The WFST being built.
    wfst: VectorMultiTapeWfst<L, W, N>,
}

impl<L, W, const N: usize> MultiTapeWfstBuilder<L, W, N>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            wfst: VectorMultiTapeWfst::new(),
        }
    }

    /// Add a new state and return its ID.
    pub fn add_state(&mut self) -> StateId {
        self.wfst.add_state()
    }

    /// Add a new final state with the given weight.
    pub fn add_final_state(&mut self, weight: W) -> StateId {
        self.wfst.add_final_state(weight)
    }

    /// Set the start state.
    pub fn set_start(&mut self, state: StateId) -> &mut Self {
        self.wfst.set_start(state);
        self
    }

    /// Make a state final with the given weight.
    pub fn set_final(&mut self, state: StateId, weight: W) -> &mut Self {
        self.wfst.set_final(state, weight);
        self
    }

    /// Add a transition.
    pub fn add_transition(
        &mut self,
        from: StateId,
        to: StateId,
        labels: MultiTapeLabel<L, N>,
        weight: W,
    ) -> &mut Self {
        self.wfst
            .add_transition(MultiTapeTransition::new(from, labels, to, weight));
        self
    }

    /// Add an epsilon transition.
    pub fn add_epsilon_transition(&mut self, from: StateId, to: StateId, weight: W) -> &mut Self {
        self.wfst.add_epsilon_transition(from, to, weight);
        self
    }

    /// Add a transition with explicit label values (all tapes non-epsilon).
    pub fn add_full_transition(
        &mut self,
        from: StateId,
        to: StateId,
        labels: [L; N],
        weight: W,
    ) -> &mut Self {
        self.add_transition(from, to, MultiTapeLabel::from_values(labels), weight)
    }

    /// Add a transition with a single non-epsilon tape.
    pub fn add_single_tape_transition(
        &mut self,
        from: StateId,
        to: StateId,
        tape: usize,
        label: L,
        weight: W,
    ) -> &mut Self {
        self.add_transition(from, to, MultiTapeLabel::single(tape, label), weight)
    }

    /// Add a transition with two non-epsilon tapes.
    pub fn add_two_tape_transition(
        &mut self,
        from: StateId,
        to: StateId,
        tape1: usize,
        label1: L,
        tape2: usize,
        label2: L,
        weight: W,
    ) -> &mut Self {
        self.add_transition(
            from,
            to,
            MultiTapeLabel::pair(tape1, label1, tape2, label2),
            weight,
        )
    }

    /// Get the number of states added.
    pub fn num_states(&self) -> usize {
        self.wfst.num_states()
    }

    /// Get the number of transitions added.
    pub fn num_transitions(&self) -> usize {
        self.wfst.num_transitions()
    }

    /// Build and return the multi-tape WFST.
    pub fn build(self) -> VectorMultiTapeWfst<L, W, N> {
        self.wfst
    }
}

impl<L, W, const N: usize> Default for MultiTapeWfstBuilder<L, W, N>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to create a 2-tape WFST (standard transducer).
pub fn two_tape_transducer<L, W>() -> MultiTapeWfstBuilder<L, W, 2>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    MultiTapeWfstBuilder::new()
}

/// Convenience function to create a 3-tape WFST.
pub fn three_tape_transducer<L, W>() -> MultiTapeWfstBuilder<L, W, 3>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    MultiTapeWfstBuilder::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_builder_creation() {
        let builder: MultiTapeWfstBuilder<char, TropicalWeight, 2> = MultiTapeWfstBuilder::new();
        assert_eq!(builder.num_states(), 0);
        assert_eq!(builder.num_transitions(), 0);
    }

    #[test]
    fn test_builder_add_states() {
        let mut builder: MultiTapeWfstBuilder<char, TropicalWeight, 2> =
            MultiTapeWfstBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_final_state(TropicalWeight::one());

        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(builder.num_states(), 2);
    }

    #[test]
    fn test_builder_set_start_final() {
        let mut builder: MultiTapeWfstBuilder<char, TropicalWeight, 2> =
            MultiTapeWfstBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.set_start(s0);
        builder.set_final(s1, TropicalWeight::new(2.0));

        let wfst = builder.build();

        assert_eq!(wfst.start(), s0);
        assert!(!wfst.is_final(s0));
        assert!(wfst.is_final(s1));
        assert_eq!(wfst.final_weight(s1).value(), 2.0);
    }

    #[test]
    fn test_builder_add_transitions() {
        let mut builder: MultiTapeWfstBuilder<char, TropicalWeight, 2> =
            MultiTapeWfstBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_final_state(TropicalWeight::one());

        builder.set_start(s0);
        builder.add_transition(
            s0,
            s1,
            MultiTapeLabel::from_values(['a', 'x']),
            TropicalWeight::one(),
        );

        assert_eq!(builder.num_transitions(), 1);

        let wfst = builder.build();
        let trans = &wfst.transitions(s0)[0];
        assert_eq!(trans.tape_label(0), Some(&'a'));
        assert_eq!(trans.tape_label(1), Some(&'x'));
    }

    #[test]
    fn test_builder_full_transition() {
        let mut builder: MultiTapeWfstBuilder<char, TropicalWeight, 3> =
            MultiTapeWfstBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.add_full_transition(s0, s1, ['a', 'b', 'c'], TropicalWeight::one());

        let wfst = builder.build();
        let trans = &wfst.transitions(s0)[0];
        assert_eq!(trans.tape_label(0), Some(&'a'));
        assert_eq!(trans.tape_label(1), Some(&'b'));
        assert_eq!(trans.tape_label(2), Some(&'c'));
    }

    #[test]
    fn test_builder_single_tape_transition() {
        let mut builder: MultiTapeWfstBuilder<char, TropicalWeight, 3> =
            MultiTapeWfstBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.add_single_tape_transition(s0, s1, 1, 'x', TropicalWeight::one());

        let wfst = builder.build();
        let trans = &wfst.transitions(s0)[0];
        assert!(trans.is_tape_epsilon(0));
        assert!(!trans.is_tape_epsilon(1));
        assert!(trans.is_tape_epsilon(2));
    }

    #[test]
    fn test_builder_two_tape_transition() {
        let mut builder: MultiTapeWfstBuilder<char, TropicalWeight, 3> =
            MultiTapeWfstBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.add_two_tape_transition(s0, s1, 0, 'a', 2, 'c', TropicalWeight::one());

        let wfst = builder.build();
        let trans = &wfst.transitions(s0)[0];
        assert_eq!(trans.tape_label(0), Some(&'a'));
        assert!(trans.is_tape_epsilon(1));
        assert_eq!(trans.tape_label(2), Some(&'c'));
    }

    #[test]
    fn test_builder_epsilon_transition() {
        let mut builder: MultiTapeWfstBuilder<char, TropicalWeight, 2> =
            MultiTapeWfstBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.add_epsilon_transition(s0, s1, TropicalWeight::new(0.5));

        let wfst = builder.build();
        let trans = &wfst.transitions(s0)[0];
        assert!(trans.is_epsilon());
    }

    #[test]
    fn test_builder_chaining() {
        let mut builder: MultiTapeWfstBuilder<char, TropicalWeight, 2> =
            MultiTapeWfstBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder
            .set_start(s0)
            .set_final(s1, TropicalWeight::one())
            .add_transition(
                s0,
                s1,
                MultiTapeLabel::from_values(['a', 'x']),
                TropicalWeight::one(),
            );

        let wfst = builder.build();
        assert_eq!(wfst.start(), s0);
        assert!(wfst.is_final(s1));
        assert_eq!(wfst.num_transitions(), 1);
    }

    #[test]
    fn test_two_tape_convenience() {
        let mut builder = two_tape_transducer::<char, TropicalWeight>();

        let s0 = builder.add_state();
        builder.set_start(s0);
        builder.set_final(s0, TropicalWeight::one());

        let wfst = builder.build();
        assert_eq!(wfst.num_tapes(), 2);
    }

    #[test]
    fn test_three_tape_convenience() {
        let mut builder = three_tape_transducer::<char, TropicalWeight>();

        let s0 = builder.add_state();
        builder.set_start(s0);
        builder.set_final(s0, TropicalWeight::one());

        let wfst = builder.build();
        assert_eq!(wfst.num_tapes(), 3);
    }

    #[test]
    fn test_complex_three_tape() {
        // Build a word alignment transducer
        let mut builder: MultiTapeWfstBuilder<&str, TropicalWeight, 3> =
            MultiTapeWfstBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();
        let s2 = builder.add_state();
        let s3 = builder.add_final_state(TropicalWeight::one());

        builder.set_start(s0);

        // Tape 0: source word
        // Tape 1: target word
        // Tape 2: alignment tag

        // Aligned words
        builder.add_full_transition(s0, s1, ["the", "le", "A"], TropicalWeight::new(1.0));
        builder.add_full_transition(s1, s2, ["cat", "chat", "A"], TropicalWeight::new(1.0));
        builder.add_full_transition(s2, s3, ["sleeps", "dort", "A"], TropicalWeight::new(1.0));

        let wfst = builder.build();

        assert_eq!(wfst.num_states(), 4);
        assert_eq!(wfst.num_transitions(), 3);
    }
}
