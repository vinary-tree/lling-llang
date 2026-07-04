//! Multi-tape transitions.

use std::fmt::{self, Debug};
use std::hash::Hash;

use super::label::MultiTapeLabel;
use crate::semiring::Semiring;
use crate::wfst::StateId;

/// A transition in a multi-tape WFST.
#[derive(Clone, PartialEq)]
pub struct MultiTapeTransition<L, W: Semiring, const N: usize> {
    /// Source state.
    pub from: StateId,
    /// Labels on each tape.
    pub labels: MultiTapeLabel<L, N>,
    /// Target state.
    pub to: StateId,
    /// Transition weight.
    pub weight: W,
}

impl<L, W: Semiring, const N: usize> MultiTapeTransition<L, W, N> {
    /// Create a new transition.
    pub fn new(from: StateId, labels: MultiTapeLabel<L, N>, to: StateId, weight: W) -> Self {
        Self {
            from,
            labels,
            to,
            weight,
        }
    }

    /// Create an epsilon transition (all tapes epsilon).
    pub fn epsilon(from: StateId, to: StateId, weight: W) -> Self
    where
        L: Clone,
    {
        Self {
            from,
            labels: MultiTapeLabel::epsilon(),
            to,
            weight,
        }
    }

    /// Check if this is an epsilon transition on all tapes.
    pub fn is_epsilon(&self) -> bool {
        self.labels.is_epsilon()
    }

    /// Check if this transition is an epsilon on a specific tape.
    pub fn is_tape_epsilon(&self, tape: usize) -> bool {
        self.labels.is_tape_epsilon(tape)
    }

    /// Get the label on a specific tape.
    pub fn tape_label(&self, tape: usize) -> Option<&L> {
        self.labels.tape(tape)
    }

    /// Get the source state.
    pub fn source(&self) -> StateId {
        self.from
    }

    /// Get the target state.
    pub fn target(&self) -> StateId {
        self.to
    }
}

impl<L: Clone, W: Semiring + Clone, const N: usize> MultiTapeTransition<L, W, N> {
    /// Create a transition with a single non-epsilon tape.
    pub fn single_tape(from: StateId, tape: usize, label: L, to: StateId, weight: W) -> Self {
        Self {
            from,
            labels: MultiTapeLabel::single(tape, label),
            to,
            weight,
        }
    }

    /// Create a transition with two non-epsilon tapes.
    pub fn two_tape(
        from: StateId,
        tape1: usize,
        label1: L,
        tape2: usize,
        label2: L,
        to: StateId,
        weight: W,
    ) -> Self {
        Self {
            from,
            labels: MultiTapeLabel::pair(tape1, label1, tape2, label2),
            to,
            weight,
        }
    }

    /// Map labels using a function.
    pub fn map_labels<F, M>(&self, f: F) -> MultiTapeTransition<M, W, N>
    where
        F: Fn(&L) -> M,
    {
        MultiTapeTransition {
            from: self.from,
            labels: self.labels.map(f),
            to: self.to,
            weight: self.weight.clone(),
        }
    }
}

impl<L: Debug, W: Semiring + Debug, const N: usize> Debug for MultiTapeTransition<L, W, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Transition {{ {} --{:?}-- {} (w={:?}) }}",
            self.from, self.labels, self.to, self.weight
        )
    }
}

impl<L: Eq + Hash, W: Semiring, const N: usize> Eq for MultiTapeTransition<L, W, N> where
    W: PartialEq
{
}

impl<L: Hash, W: Semiring + Hash, const N: usize> Hash for MultiTapeTransition<L, W, N> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.from.hash(state);
        self.labels.hash(state);
        self.to.hash(state);
        self.weight.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_transition_creation() {
        let labels = MultiTapeLabel::from_values(['a', 'b']);
        let trans: MultiTapeTransition<char, TropicalWeight, 2> =
            MultiTapeTransition::new(0, labels, 1, TropicalWeight::one());

        assert_eq!(trans.from, 0);
        assert_eq!(trans.to, 1);
        assert!(!trans.is_epsilon());
    }

    #[test]
    fn test_epsilon_transition() {
        let trans: MultiTapeTransition<char, TropicalWeight, 3> =
            MultiTapeTransition::epsilon(0, 1, TropicalWeight::new(0.5));

        assert!(trans.is_epsilon());
        assert!(trans.is_tape_epsilon(0));
        assert!(trans.is_tape_epsilon(1));
        assert!(trans.is_tape_epsilon(2));
    }

    #[test]
    fn test_single_tape_transition() {
        let trans: MultiTapeTransition<char, TropicalWeight, 3> =
            MultiTapeTransition::single_tape(0, 1, 'x', 1, TropicalWeight::one());

        assert!(!trans.is_epsilon());
        assert!(trans.is_tape_epsilon(0));
        assert!(!trans.is_tape_epsilon(1));
        assert!(trans.is_tape_epsilon(2));
        assert_eq!(trans.tape_label(1), Some(&'x'));
    }

    #[test]
    fn test_two_tape_transition() {
        let trans: MultiTapeTransition<char, TropicalWeight, 3> =
            MultiTapeTransition::two_tape(0, 0, 'a', 2, 'c', 1, TropicalWeight::one());

        assert_eq!(trans.tape_label(0), Some(&'a'));
        assert_eq!(trans.tape_label(1), None);
        assert_eq!(trans.tape_label(2), Some(&'c'));
    }

    #[test]
    fn test_source_target() {
        let trans: MultiTapeTransition<char, TropicalWeight, 2> =
            MultiTapeTransition::epsilon(5, 10, TropicalWeight::one());

        assert_eq!(trans.source(), 5);
        assert_eq!(trans.target(), 10);
    }

    #[test]
    fn test_map_labels() {
        let trans: MultiTapeTransition<i32, TropicalWeight, 2> = MultiTapeTransition::new(
            0,
            MultiTapeLabel::from_values([1, 2]),
            1,
            TropicalWeight::one(),
        );

        let mapped = trans.map_labels(|&x| x * 10);
        assert_eq!(mapped.tape_label(0), Some(&10));
        assert_eq!(mapped.tape_label(1), Some(&20));
    }

    #[test]
    fn test_debug_format() {
        let trans: MultiTapeTransition<char, TropicalWeight, 2> = MultiTapeTransition::new(
            0,
            MultiTapeLabel::new([Some('a'), None]),
            1,
            TropicalWeight::new(1.0),
        );

        let s = format!("{:?}", trans);
        assert!(s.contains("Transition"));
        assert!(s.contains("'a'"));
    }
}
