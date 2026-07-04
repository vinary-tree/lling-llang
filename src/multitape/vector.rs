//! Vector-based multi-tape WFST implementation.

use std::hash::Hash;

use super::label::MultiTapeLabel;
use super::traits::MultiTapeWfst;
use super::transition::MultiTapeTransition;
use crate::semiring::Semiring;
use crate::wfst::StateId;

/// State information for a multi-tape WFST.
#[derive(Debug, Clone)]
pub struct MultiTapeState<L, W: Semiring, const N: usize> {
    /// Whether this is a final state.
    pub is_final: bool,
    /// Final weight (only meaningful if is_final).
    pub final_weight: W,
    /// Outgoing transitions.
    pub transitions: Vec<MultiTapeTransition<L, W, N>>,
}

impl<L, W: Semiring, const N: usize> MultiTapeState<L, W, N> {
    /// Create a non-final state.
    pub fn non_final() -> Self {
        Self {
            is_final: false,
            final_weight: W::zero(),
            transitions: Vec::new(),
        }
    }

    /// Create a final state with the given weight.
    pub fn final_with_weight(weight: W) -> Self {
        Self {
            is_final: true,
            final_weight: weight,
            transitions: Vec::new(),
        }
    }
}

impl<L, W: Semiring, const N: usize> Default for MultiTapeState<L, W, N> {
    fn default() -> Self {
        Self::non_final()
    }
}

/// Vector-based implementation of a multi-tape WFST.
#[derive(Debug, Clone)]
pub struct VectorMultiTapeWfst<L, W: Semiring, const N: usize> {
    /// States indexed by ID.
    states: Vec<MultiTapeState<L, W, N>>,
    /// Initial state.
    start: StateId,
    /// Total number of transitions.
    num_transitions: usize,
}

impl<L: Clone + Eq + Hash, W: Semiring + Clone, const N: usize> VectorMultiTapeWfst<L, W, N> {
    /// Create a new empty multi-tape WFST.
    pub fn new() -> Self {
        Self {
            states: Vec::new(),
            start: 0,
            num_transitions: 0,
        }
    }

    /// Add a state and return its ID.
    pub fn add_state(&mut self) -> StateId {
        let id = self.states.len() as StateId;
        self.states.push(MultiTapeState::non_final());
        id
    }

    /// Add a final state with the given weight.
    pub fn add_final_state(&mut self, weight: W) -> StateId {
        let id = self.states.len() as StateId;
        self.states.push(MultiTapeState::final_with_weight(weight));
        id
    }

    /// Set the start state.
    pub fn set_start(&mut self, state: StateId) {
        self.start = state;
    }

    /// Make a state final.
    pub fn set_final(&mut self, state: StateId, weight: W) {
        if let Some(s) = self.states.get_mut(state as usize) {
            s.is_final = true;
            s.final_weight = weight;
        }
    }

    /// Remove final status from a state.
    pub fn unset_final(&mut self, state: StateId) {
        if let Some(s) = self.states.get_mut(state as usize) {
            s.is_final = false;
            s.final_weight = W::zero();
        }
    }

    /// Add a transition.
    pub fn add_transition(&mut self, transition: MultiTapeTransition<L, W, N>) {
        if let Some(s) = self.states.get_mut(transition.from as usize) {
            s.transitions.push(transition);
            self.num_transitions += 1;
        }
    }

    /// Add a transition with explicit parameters.
    pub fn add_transition_parts(
        &mut self,
        from: StateId,
        to: StateId,
        labels: MultiTapeLabel<L, N>,
        weight: W,
    ) {
        self.add_transition(MultiTapeTransition::new(from, labels, to, weight));
    }

    /// Add an epsilon transition.
    pub fn add_epsilon_transition(&mut self, from: StateId, to: StateId, weight: W)
    where
        L: Clone,
    {
        self.add_transition(MultiTapeTransition::epsilon(from, to, weight));
    }

    /// Get mutable access to a state.
    pub fn state_mut(&mut self, state: StateId) -> Option<&mut MultiTapeState<L, W, N>> {
        self.states.get_mut(state as usize)
    }

    /// Reserve capacity for states.
    pub fn reserve_states(&mut self, additional: usize) {
        self.states.reserve(additional);
    }
}

impl<L: Clone + Eq + Hash, W: Semiring> Default for VectorMultiTapeWfst<L, W, 2> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L, W, const N: usize> MultiTapeWfst<L, W, N> for VectorMultiTapeWfst<L, W, N>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    fn start(&self) -> StateId {
        self.start
    }

    fn is_final(&self, state: StateId) -> bool {
        self.states
            .get(state as usize)
            .map(|s| s.is_final)
            .unwrap_or(false)
    }

    fn final_weight(&self, state: StateId) -> W {
        self.states
            .get(state as usize)
            .map(|s| s.final_weight.clone())
            .unwrap_or_else(W::zero)
    }

    fn transitions(&self, state: StateId) -> &[MultiTapeTransition<L, W, N>] {
        self.states
            .get(state as usize)
            .map(|s| s.transitions.as_slice())
            .unwrap_or(&[])
    }

    fn num_states(&self) -> usize {
        self.states.len()
    }

    fn num_transitions(&self) -> usize {
        self.num_transitions
    }

    fn states(&self) -> impl Iterator<Item = StateId> {
        0..self.states.len() as StateId
    }

    fn final_states(&self) -> impl Iterator<Item = StateId> {
        self.states
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_final)
            .map(|(i, _)| i as StateId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_empty_wfst() {
        let mt: VectorMultiTapeWfst<char, TropicalWeight, 2> = VectorMultiTapeWfst::new();
        assert_eq!(mt.num_states(), 0);
        assert_eq!(mt.num_transitions(), 0);
        assert!(mt.is_empty());
    }

    #[test]
    fn test_add_states() {
        let mut mt: VectorMultiTapeWfst<char, TropicalWeight, 2> = VectorMultiTapeWfst::new();

        let s0 = mt.add_state();
        let s1 = mt.add_final_state(TropicalWeight::one());

        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(mt.num_states(), 2);
        assert!(!mt.is_final(s0));
        assert!(mt.is_final(s1));
    }

    #[test]
    fn test_set_start() {
        let mut mt: VectorMultiTapeWfst<char, TropicalWeight, 2> = VectorMultiTapeWfst::new();

        let s0 = mt.add_state();
        let s1 = mt.add_state();

        mt.set_start(s0);
        assert_eq!(mt.start(), s0);

        mt.set_start(s1);
        assert_eq!(mt.start(), s1);
    }

    #[test]
    fn test_set_final() {
        let mut mt: VectorMultiTapeWfst<char, TropicalWeight, 2> = VectorMultiTapeWfst::new();

        let s0 = mt.add_state();
        assert!(!mt.is_final(s0));

        mt.set_final(s0, TropicalWeight::new(2.0));
        assert!(mt.is_final(s0));
        assert_eq!(mt.final_weight(s0).value(), 2.0);

        mt.unset_final(s0);
        assert!(!mt.is_final(s0));
    }

    #[test]
    fn test_add_transitions() {
        let mut mt: VectorMultiTapeWfst<char, TropicalWeight, 2> = VectorMultiTapeWfst::new();

        let s0 = mt.add_state();
        let s1 = mt.add_state();

        mt.add_transition_parts(
            s0,
            s1,
            MultiTapeLabel::from_values(['a', 'x']),
            TropicalWeight::one(),
        );

        assert_eq!(mt.num_transitions(), 1);
        assert_eq!(mt.transitions(s0).len(), 1);
        assert_eq!(mt.transitions(s1).len(), 0);
    }

    #[test]
    fn test_epsilon_transition() {
        let mut mt: VectorMultiTapeWfst<char, TropicalWeight, 2> = VectorMultiTapeWfst::new();

        let s0 = mt.add_state();
        let s1 = mt.add_state();

        mt.add_epsilon_transition(s0, s1, TropicalWeight::one());

        let trans = &mt.transitions(s0)[0];
        assert!(trans.is_epsilon());
    }

    #[test]
    fn test_states_iterator() {
        let mut mt: VectorMultiTapeWfst<char, TropicalWeight, 2> = VectorMultiTapeWfst::new();

        mt.add_state();
        mt.add_state();
        mt.add_state();

        let states: Vec<_> = mt.states().collect();
        assert_eq!(states, vec![0, 1, 2]);
    }

    #[test]
    fn test_final_states_iterator() {
        let mut mt: VectorMultiTapeWfst<char, TropicalWeight, 2> = VectorMultiTapeWfst::new();

        mt.add_state(); // not final
        mt.add_final_state(TropicalWeight::one()); // final
        mt.add_state(); // not final
        mt.add_final_state(TropicalWeight::one()); // final

        let final_states: Vec<_> = mt.final_states().collect();
        assert_eq!(final_states, vec![1, 3]);
    }

    #[test]
    fn test_three_tape_wfst() {
        let mut mt: VectorMultiTapeWfst<char, TropicalWeight, 3> = VectorMultiTapeWfst::new();

        let s0 = mt.add_state();
        let s1 = mt.add_final_state(TropicalWeight::one());

        mt.set_start(s0);

        // Add transition on all three tapes
        mt.add_transition_parts(
            s0,
            s1,
            MultiTapeLabel::from_values(['a', 'b', 'c']),
            TropicalWeight::one(),
        );

        assert_eq!(mt.num_tapes(), 3);
        let trans = &mt.transitions(s0)[0];
        assert_eq!(trans.tape_label(0), Some(&'a'));
        assert_eq!(trans.tape_label(1), Some(&'b'));
        assert_eq!(trans.tape_label(2), Some(&'c'));
    }

    #[test]
    fn test_mixed_epsilon_tapes() {
        let mut mt: VectorMultiTapeWfst<char, TropicalWeight, 3> = VectorMultiTapeWfst::new();

        let s0 = mt.add_state();
        let s1 = mt.add_state();

        // Transition with tape 0 non-epsilon, tapes 1 and 2 epsilon
        mt.add_transition_parts(
            s0,
            s1,
            MultiTapeLabel::single(0, 'a'),
            TropicalWeight::one(),
        );

        let trans = &mt.transitions(s0)[0];
        assert!(!trans.is_epsilon());
        assert!(!trans.is_tape_epsilon(0));
        assert!(trans.is_tape_epsilon(1));
        assert!(trans.is_tape_epsilon(2));
    }
}
