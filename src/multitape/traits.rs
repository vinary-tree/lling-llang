//! Trait definitions for multi-tape WFSTs.

use std::hash::Hash;

use super::{MultiTapeLabel, MultiTapeTransition};
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

    /// Run the transducer on input and return the total weight if accepting.
    fn transduce(&self, input: &[MultiTapeLabel<L, N>]) -> Option<W>
    where
        L: PartialEq,
    {
        // Simple recursive implementation for now
        fn transduce_from<L, W, const N: usize, T>(
            wfst: &T,
            state: StateId,
            input: &[MultiTapeLabel<L, N>],
        ) -> Option<W>
        where
            L: Clone + Eq + Hash + Send + Sync + PartialEq,
            W: Semiring + Clone,
            T: MultiTapeWfst<L, W, N>,
        {
            if input.is_empty() {
                if wfst.is_final(state) {
                    return Some(wfst.final_weight(state));
                }
                // Try epsilon transitions
                for trans in wfst.transitions(state) {
                    if trans.is_epsilon() {
                        if let Some(w) = transduce_from(wfst, trans.to, input) {
                            return Some(trans.weight.clone().times(&w));
                        }
                    }
                }
                return None;
            }

            let label = &input[0];
            let rest = &input[1..];

            // Try matching transitions
            for trans in wfst.transitions(state) {
                if trans.labels == *label {
                    if let Some(w) = transduce_from(wfst, trans.to, rest) {
                        return Some(trans.weight.clone().times(&w));
                    }
                }
            }

            // Try epsilon transitions
            for trans in wfst.transitions(state) {
                if trans.is_epsilon() {
                    if let Some(w) = transduce_from(wfst, trans.to, input) {
                        return Some(trans.weight.clone().times(&w));
                    }
                }
            }

            None
        }

        transduce_from(self, self.start(), input)
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
    use crate::multitape::VectorMultiTapeWfst;
    use crate::semiring::TropicalWeight;

    fn make_simple_mt() -> VectorMultiTapeWfst<char, TropicalWeight, 2> {
        use crate::multitape::MultiTapeWfstBuilder;

        let mut builder = MultiTapeWfstBuilder::<char, TropicalWeight, 2>::new();
        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.set_start(s0);
        builder.set_final(s1, TropicalWeight::one());

        builder.add_transition(
            s0,
            s1,
            MultiTapeLabel::from_values(['a', 'x']),
            TropicalWeight::one(),
        );

        builder.build()
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
}
