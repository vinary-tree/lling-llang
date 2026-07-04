//! Trait definitions for multi-tape WFSTs.

use std::collections::HashSet;
use std::hash::Hash;

use super::label::MultiTapeLabel;
use super::transition::MultiTapeTransition;
use crate::semiring::Semiring;
use crate::wfst::StateId;

/// Trait for multi-tape weighted finite state transducers.
///
/// A multi-tape WFST has k tapes, each with its own alphabet. Transitions
/// can read/write symbols on any subset of tapes, with epsilon allowed
/// on individual tapes.
pub trait MultiTapeWfst<L, W, const N: usize>: Clone + Send + Sync
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring,
{
    /// Get the start state.
    fn start(&self) -> StateId;

    /// Check if a state is final.
    fn is_final(&self, state: StateId) -> bool;

    /// Get the final weight for a state.
    fn final_weight(&self, state: StateId) -> W;

    /// Get all transitions from a state.
    fn transitions(&self, state: StateId) -> &[MultiTapeTransition<L, W, N>];

    /// Get the number of states.
    fn num_states(&self) -> usize;

    /// Get the number of transitions.
    fn num_transitions(&self) -> usize;

    /// Get all states.
    fn states(&self) -> impl Iterator<Item = StateId>;

    /// Get all final states.
    fn final_states(&self) -> impl Iterator<Item = StateId>;

    /// Check if the transducer is empty (no states).
    fn is_empty(&self) -> bool {
        self.num_states() == 0
    }

    /// Get the number of tapes.
    fn num_tapes(&self) -> usize {
        N
    }
}

/// Extension trait for multi-tape WFST operations.
pub trait MultiTapeWfstOps<L, W, const N: usize>: MultiTapeWfst<L, W, N>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    /// Get transitions from a state that match a label on a specific tape.
    fn transitions_matching_tape(
        &self,
        state: StateId,
        tape: usize,
        label: &L,
    ) -> Vec<&MultiTapeTransition<L, W, N>> {
        self.transitions(state)
            .iter()
            .filter(|t| t.tape_label(tape) == Some(label))
            .collect()
    }

    /// Get epsilon transitions from a state (epsilon on all tapes).
    fn epsilon_transitions(&self, state: StateId) -> Vec<&MultiTapeTransition<L, W, N>> {
        self.transitions(state)
            .iter()
            .filter(|t| t.is_epsilon())
            .collect()
    }

    /// Get transitions that are epsilon on a specific tape.
    fn tape_epsilon_transitions(
        &self,
        state: StateId,
        tape: usize,
    ) -> Vec<&MultiTapeTransition<L, W, N>> {
        self.transitions(state)
            .iter()
            .filter(|t| t.is_tape_epsilon(tape))
            .collect()
    }

    /// Get transitions that are non-epsilon on a specific tape.
    fn tape_non_epsilon_transitions(
        &self,
        state: StateId,
        tape: usize,
    ) -> Vec<&MultiTapeTransition<L, W, N>> {
        self.transitions(state)
            .iter()
            .filter(|t| !t.is_tape_epsilon(tape))
            .collect()
    }

    /// Collect all labels that appear on a specific tape.
    fn tape_alphabet(&self, tape: usize) -> Vec<L> {
        let mut labels = std::collections::HashSet::new();
        for state in self.states() {
            for trans in self.transitions(state) {
                if let Some(label) = trans.tape_label(tape) {
                    labels.insert(label.clone());
                }
            }
        }
        labels.into_iter().collect()
    }

    /// Check if the transducer has any epsilon transitions.
    fn has_epsilon_transitions(&self) -> bool {
        self.states()
            .any(|s| self.transitions(s).iter().any(|t| t.is_epsilon()))
    }

    /// Check if a specific tape has any epsilon transitions.
    fn tape_has_epsilon(&self, tape: usize) -> bool {
        self.states()
            .any(|s| self.transitions(s).iter().any(|t| t.is_tape_epsilon(tape)))
    }

    /// Count transitions.
    fn count_transitions(&self) -> usize {
        self.states().map(|s| self.transitions(s).len()).sum()
    }

    /// Accept check: run the transducer on a sequence of multi-tape labels.
    fn accepts(&self, input: &[MultiTapeLabel<L, N>]) -> bool
    where
        L: PartialEq,
    {
        self.transduce(input).is_some()
    }

    /// Run the transducer on input and return an accepting path weight if one exists.
    fn transduce(&self, input: &[MultiTapeLabel<L, N>]) -> Option<W>
    where
        L: PartialEq,
    {
        let start = self.start();
        let mut agenda = vec![(start, 0usize, W::one())];
        let mut visited = HashSet::new();

        while let Some((state, input_pos, path_weight)) = agenda.pop() {
            if !visited.insert((state, input_pos)) {
                continue;
            }

            if input_pos == input.len() && self.is_final(state) {
                return Some(path_weight.times(&self.final_weight(state)));
            }

            let mut successors = Vec::new();

            if input_pos < input.len() {
                let label = &input[input_pos];
                for trans in self.transitions(state) {
                    if trans.labels == *label {
                        successors.push((
                            trans.to,
                            input_pos + 1,
                            path_weight.clone().times(&trans.weight),
                        ));
                    }
                }
            }

            for trans in self.transitions(state) {
                if trans.is_epsilon() {
                    successors.push((
                        trans.to,
                        input_pos,
                        path_weight.clone().times(&trans.weight),
                    ));
                }
            }

            agenda.extend(successors.into_iter().rev());
        }

        None
    }
}

// Blanket implementation
impl<T, L, W, const N: usize> MultiTapeWfstOps<L, W, N> for T
where
    T: MultiTapeWfst<L, W, N>,
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[derive(Clone)]
    struct TestMultiTape {
        start: StateId,
        final_weights: Vec<Option<TropicalWeight>>,
        transitions: Vec<Vec<MultiTapeTransition<char, TropicalWeight, 2>>>,
    }

    impl TestMultiTape {
        fn new(num_states: usize) -> Self {
            Self {
                start: 0,
                final_weights: vec![None; num_states],
                transitions: vec![Vec::new(); num_states],
            }
        }

        fn set_start(&mut self, state: StateId) {
            self.start = state;
        }

        fn set_final(&mut self, state: StateId, weight: TropicalWeight) {
            self.final_weights[state as usize] = Some(weight);
        }

        fn add_transition(
            &mut self,
            from: StateId,
            to: StateId,
            labels: MultiTapeLabel<char, 2>,
            weight: TropicalWeight,
        ) {
            self.transitions[from as usize]
                .push(MultiTapeTransition::new(from, labels, to, weight));
        }
    }

    impl MultiTapeWfst<char, TropicalWeight, 2> for TestMultiTape {
        fn start(&self) -> StateId {
            self.start
        }

        fn is_final(&self, state: StateId) -> bool {
            self.final_weights
                .get(state as usize)
                .is_some_and(Option::is_some)
        }

        fn final_weight(&self, state: StateId) -> TropicalWeight {
            self.final_weights
                .get(state as usize)
                .and_then(|weight| *weight)
                .unwrap_or_else(TropicalWeight::zero)
        }

        fn transitions(&self, state: StateId) -> &[MultiTapeTransition<char, TropicalWeight, 2>] {
            self.transitions
                .get(state as usize)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        }

        fn num_states(&self) -> usize {
            self.transitions.len()
        }

        fn num_transitions(&self) -> usize {
            self.transitions.iter().map(Vec::len).sum()
        }

        fn states(&self) -> impl Iterator<Item = StateId> {
            (0..self.num_states()).map(|state| state as StateId)
        }

        fn final_states(&self) -> impl Iterator<Item = StateId> {
            self.final_weights
                .iter()
                .enumerate()
                .filter_map(|(state, weight)| weight.is_some().then_some(state as StateId))
        }
    }

    fn make_simple_mt() -> TestMultiTape {
        let mut mt = TestMultiTape::new(2);
        mt.set_start(0);
        mt.set_final(1, TropicalWeight::one());
        mt.add_transition(
            0,
            1,
            MultiTapeLabel::from_values(['a', 'x']),
            TropicalWeight::one(),
        );
        mt
    }

    fn make_epsilon_cycle_mt() -> TestMultiTape {
        let mut mt = TestMultiTape::new(2);
        mt.set_start(0);
        mt.set_final(1, TropicalWeight::one());
        mt.add_transition(0, 0, MultiTapeLabel::epsilon(), TropicalWeight::one());
        mt.add_transition(
            0,
            1,
            MultiTapeLabel::from_values(['a', 'x']),
            TropicalWeight::one(),
        );
        mt
    }

    fn make_long_chain_mt(length: usize) -> TestMultiTape {
        let mut mt = TestMultiTape::new(length + 1);
        mt.set_start(0);

        for state in 0..length {
            mt.add_transition(
                state as StateId,
                state as StateId + 1,
                MultiTapeLabel::from_values(['a', 'x']),
                TropicalWeight::one(),
            );
        }

        mt.set_final(length as StateId, TropicalWeight::one());
        mt
    }

    #[test]
    fn test_basic_ops() {
        let mt = make_simple_mt();
        assert_eq!(mt.num_tapes(), 2);
        assert!(!mt.is_empty());
    }

    #[test]
    fn test_tape_alphabet() {
        let mt = make_simple_mt();
        let alphabet0 = mt.tape_alphabet(0);
        let alphabet1 = mt.tape_alphabet(1);

        assert!(alphabet0.contains(&'a'));
        assert!(alphabet1.contains(&'x'));
    }

    #[test]
    fn test_transduce() {
        let mt = make_simple_mt();

        // Should accept
        let input = vec![MultiTapeLabel::from_values(['a', 'x'])];
        assert!(mt.accepts(&input));

        // Should reject
        let input2 = vec![MultiTapeLabel::from_values(['b', 'y'])];
        assert!(!mt.accepts(&input2));
    }

    #[test]
    fn test_transduce_handles_epsilon_cycle() {
        let mt = make_epsilon_cycle_mt();
        let input = vec![MultiTapeLabel::from_values(['a', 'x'])];

        assert!(mt.accepts(&input));
    }

    #[test]
    fn test_transduce_long_chain_without_recursion() {
        let length = 2048;
        let mt = make_long_chain_mt(length);
        let input = vec![MultiTapeLabel::from_values(['a', 'x']); length];

        assert!(mt.accepts(&input));
    }
}
