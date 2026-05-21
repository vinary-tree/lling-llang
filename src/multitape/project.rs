//! Projection operations for multi-tape WFSTs.
//!
//! Projects a multi-tape WFST to a single-tape WFST or a subset of tapes.

use std::hash::Hash;

use super::{MultiTapeLabel, MultiTapeWfst};
use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, VectorWfst, WeightedTransition};

/// Source for projecting a multi-tape WFST to a single tape.
#[derive(Debug, Clone)]
pub struct ProjectSource<T, const N: usize> {
    /// The source multi-tape WFST.
    source: T,
    /// Which tape to project to.
    tape: usize,
}

impl<T, const N: usize> ProjectSource<T, N> {
    /// Create a new projection source.
    pub fn new(source: T, tape: usize) -> Self {
        assert!(tape < N, "Tape index {} out of range (max {})", tape, N - 1);
        Self { source, tape }
    }

    /// Get the source WFST.
    pub fn source(&self) -> &T {
        &self.source
    }

    /// Get the projected tape index.
    pub fn tape(&self) -> usize {
        self.tape
    }
}

/// A projected multi-tape WFST that behaves like a single-tape WFST.
#[derive(Debug, Clone)]
pub struct ProjectedWfst<L, W: Semiring> {
    /// The projected single-tape WFST.
    wfst: VectorWfst<L, W>,
}

impl<L: Clone + Eq + Hash + Send + Sync, W: Semiring + Clone> ProjectedWfst<L, W> {
    /// Get the underlying single-tape WFST.
    pub fn wfst(&self) -> &VectorWfst<L, W> {
        &self.wfst
    }

    /// Consume and return the underlying WFST.
    pub fn into_wfst(self) -> VectorWfst<L, W> {
        self.wfst
    }
}

/// Project a multi-tape WFST to a single tape.
///
/// This creates a new single-tape WFST where:
/// - States correspond 1-to-1 with the source states
/// - Transitions are labeled with the label on the specified tape
/// - Epsilon transitions on the projected tape become epsilon transitions
pub fn project<L, W, T, const N: usize>(source: &T, tape: usize) -> ProjectedWfst<L, W>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
    T: MultiTapeWfst<L, W, N>,
{
    assert!(tape < N, "Tape index {} out of range (max {})", tape, N - 1);

    let mut wfst = VectorWfst::new();

    // Add states
    for state in source.states() {
        let new_state = wfst.add_state();
        assert_eq!(state, new_state); // Should match

        if source.is_final(state) {
            wfst.set_final(state, source.final_weight(state));
        }
    }

    // Set start state
    wfst.set_start(source.start());

    // Add transitions
    for state in source.states() {
        for trans in source.transitions(state) {
            let label = trans.tape_label(tape).cloned();
            wfst.add_transition(WeightedTransition::new(
                trans.from,
                label.clone(),
                label,
                trans.to,
                trans.weight.clone(),
            ));
        }
    }

    ProjectedWfst { wfst }
}

/// Project a multi-tape WFST to a subset of tapes, creating a new multi-tape WFST.
pub fn project_tapes<L, W, T, const N: usize, const M: usize>(
    source: &T,
    tapes: [usize; M],
) -> crate::multitape::VectorMultiTapeWfst<L, W, M>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
    T: MultiTapeWfst<L, W, N>,
{
    use crate::multitape::{MultiTapeWfstBuilder, VectorMultiTapeWfst};

    // Verify tape indices
    for &tape in &tapes {
        assert!(tape < N, "Tape index {} out of range (max {})", tape, N - 1);
    }

    let mut builder = MultiTapeWfstBuilder::<L, W, M>::new();

    // Add states
    for state in source.states() {
        let new_state = builder.add_state();
        assert_eq!(state, new_state);

        if source.is_final(state) {
            builder.set_final(state, source.final_weight(state));
        }
    }

    builder.set_start(source.start());

    // Add transitions with projected labels
    for state in source.states() {
        for trans in source.transitions(state) {
            let new_labels: [Option<L>; M] =
                std::array::from_fn(|i| trans.tape_label(tapes[i]).cloned());

            builder.add_transition(
                trans.from,
                trans.to,
                MultiTapeLabel::new(new_labels),
                trans.weight.clone(),
            );
        }
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multitape::MultiTapeWfstBuilder;
    use crate::semiring::TropicalWeight;
    use crate::wfst::Wfst;

    fn make_test_mt() -> crate::multitape::VectorMultiTapeWfst<char, TropicalWeight, 3> {
        let mut builder = MultiTapeWfstBuilder::<char, TropicalWeight, 3>::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();
        let s2 = builder.add_final_state(TropicalWeight::one());

        builder.set_start(s0);

        // Transition: (a, x, 1)
        builder.add_transition(
            s0,
            s1,
            MultiTapeLabel::from_values(['a', 'x', '1']),
            TropicalWeight::one(),
        );

        // Transition: (b, y, 2)
        builder.add_transition(
            s1,
            s2,
            MultiTapeLabel::from_values(['b', 'y', '2']),
            TropicalWeight::one(),
        );

        builder.build()
    }

    #[test]
    fn test_project_to_tape_0() {
        let mt = make_test_mt();
        let projected = project(&mt, 0);
        let wfst = projected.wfst();

        assert_eq!(wfst.num_states(), 3);

        // Check that labels are from tape 0
        let transitions = wfst.transitions(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].input, Some('a'));
    }

    #[test]
    fn test_project_to_tape_1() {
        let mt = make_test_mt();
        let projected = project(&mt, 1);
        let wfst = projected.wfst();

        let transitions = wfst.transitions(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].input, Some('x'));
    }

    #[test]
    fn test_project_to_tape_2() {
        let mt = make_test_mt();
        let projected = project(&mt, 2);
        let wfst = projected.wfst();

        let transitions = wfst.transitions(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].input, Some('1'));
    }

    #[test]
    fn test_project_preserves_finals() {
        let mt = make_test_mt();
        let projected = project(&mt, 0);
        let wfst = projected.wfst();

        assert!(!wfst.is_final(0));
        assert!(!wfst.is_final(1));
        assert!(wfst.is_final(2));
    }

    #[test]
    fn test_project_preserves_start() {
        let mt = make_test_mt();
        let projected = project(&mt, 0);

        assert_eq!(projected.wfst().start(), 0);
    }

    #[test]
    fn test_project_epsilon_tape() {
        let mut builder = MultiTapeWfstBuilder::<char, TropicalWeight, 2>::new();

        let s0 = builder.add_state();
        let s1 = builder.add_final_state(TropicalWeight::one());

        builder.set_start(s0);

        // Transition with epsilon on tape 1
        builder.add_transition(
            s0,
            s1,
            MultiTapeLabel::single(0, 'a'),
            TropicalWeight::one(),
        );

        let mt = builder.build();

        // Project to tape 1 (which is epsilon)
        let projected = project(&mt, 1);
        let wfst = projected.wfst();

        let transitions = wfst.transitions(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].input, None); // Epsilon
    }

    #[test]
    fn test_project_tapes() {
        let mt = make_test_mt();

        // Project to tapes 0 and 2
        let projected: crate::multitape::VectorMultiTapeWfst<char, TropicalWeight, 2> =
            project_tapes(&mt, [0, 2]);

        assert_eq!(projected.num_states(), 3);
        assert_eq!(projected.num_tapes(), 2);

        use crate::multitape::MultiTapeWfst;
        let trans = &projected.transitions(0)[0];
        assert_eq!(trans.tape_label(0), Some(&'a'));
        assert_eq!(trans.tape_label(1), Some(&'1'));
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn test_project_invalid_tape() {
        let mt = make_test_mt();
        let _ = project(&mt, 5); // Only 3 tapes (0, 1, 2)
    }
}
