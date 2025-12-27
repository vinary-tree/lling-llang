//! VectorWfst - Eager WFST implementation using vector storage.

use crate::semiring::Semiring;
use super::{StateId, WeightedTransition, WfstState, NO_STATE};
use super::traits::{Wfst, MutableWfst};

/// Eager WFST implementation storing all states in memory.
///
/// Uses a vector of states for O(1) state access. Suitable for:
/// - Small to medium WFSTs that fit in memory
/// - WFSTs that are frequently traversed
/// - Building WFSTs programmatically
///
/// For composition of large WFSTs where the product space is huge,
/// prefer [`LazyComposition`](super::lazy::LazyWfstWrapper) instead.
///
/// # Type Parameters
///
/// - `L`: Label type (typically `char`, `u8`, or vocabulary ID)
/// - `W`: Weight type (must implement [`Semiring`])
///
/// # Example
///
/// ```
/// use lling_llang::wfst::{VectorWfst, MutableWfst, Wfst};
/// use lling_llang::semiring::{Semiring, TropicalWeight};
///
/// let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
///
/// // Add states
/// let s0 = fst.add_state();
/// let s1 = fst.add_state();
///
/// // Set start state
/// fst.set_start(s0);
///
/// // Add transition
/// fst.add_arc(s0, Some('a'), Some('b'), s1, TropicalWeight::new(1.0));
///
/// // Set final state
/// fst.set_final(s1, TropicalWeight::one());
///
/// assert_eq!(fst.num_states(), 2);
/// assert!(fst.is_final(s1));
/// ```
#[derive(Clone, Debug)]
pub struct VectorWfst<L, W: Semiring> {
    /// States stored as a vector.
    states: Vec<WfstState<L, W>>,
    /// Start state ID (NO_STATE if not set).
    start: StateId,
}

impl<L, W: Semiring> VectorWfst<L, W> {
    /// Create a new empty WFST.
    #[inline]
    pub fn new() -> Self {
        Self {
            states: Vec::new(),
            start: NO_STATE,
        }
    }

    /// Create a WFST with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(num_states: usize) -> Self {
        Self {
            states: Vec::with_capacity(num_states),
            start: NO_STATE,
        }
    }

    /// Get mutable access to a state.
    #[inline]
    pub fn state_mut(&mut self, state: StateId) -> Option<&mut WfstState<L, W>> {
        self.states.get_mut(state as usize)
    }

    /// Sort transitions of all states by input label.
    ///
    /// Useful for binary search on input labels.
    pub fn sort_transitions<F>(&mut self, compare: F)
    where
        F: Fn(&WeightedTransition<L, W>, &WeightedTransition<L, W>) -> std::cmp::Ordering + Copy,
    {
        for state in &mut self.states {
            state.transitions.sort_by(compare);
        }
    }

    /// Get all final states.
    pub fn final_states(&self) -> impl Iterator<Item = StateId> + '_ {
        self.states
            .iter()
            .filter(|s| s.is_final)
            .map(|s| s.id)
    }

    /// Shrink internal storage to fit current size.
    pub fn shrink_to_fit(&mut self) {
        self.states.shrink_to_fit();
        for state in &mut self.states {
            state.transitions.shrink_to_fit();
        }
    }
}

impl<L, W: Semiring> Default for VectorWfst<L, W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L: Clone + Send + Sync, W: Semiring> Wfst<L, W> for VectorWfst<L, W> {
    #[inline]
    fn start(&self) -> StateId {
        self.start
    }

    #[inline]
    fn is_final(&self, state: StateId) -> bool {
        self.states
            .get(state as usize)
            .map(|s| s.is_final)
            .unwrap_or(false)
    }

    #[inline]
    fn final_weight(&self, state: StateId) -> W {
        self.states
            .get(state as usize)
            .map(|s| s.final_weight)
            .unwrap_or_else(W::zero)
    }

    #[inline]
    fn transitions(&self, state: StateId) -> &[WeightedTransition<L, W>] {
        self.states
            .get(state as usize)
            .map(|s| s.transitions.as_slice())
            .unwrap_or(&[])
    }

    #[inline]
    fn num_states(&self) -> usize {
        self.states.len()
    }
}

impl<L: Clone + Send + Sync, W: Semiring> MutableWfst<L, W> for VectorWfst<L, W> {
    fn add_state(&mut self) -> StateId {
        let id = self.states.len() as StateId;
        self.states.push(WfstState::new(id));
        id
    }

    #[inline]
    fn set_start(&mut self, state: StateId) {
        debug_assert!(
            (state as usize) < self.states.len(),
            "Invalid start state: {}",
            state
        );
        self.start = state;
    }

    fn set_final(&mut self, state: StateId, weight: W) {
        if let Some(s) = self.states.get_mut(state as usize) {
            if weight.is_zero() {
                s.is_final = false;
                s.final_weight = W::zero();
            } else {
                s.is_final = true;
                s.final_weight = weight;
            }
        }
    }

    fn add_transition(&mut self, transition: WeightedTransition<L, W>) {
        if let Some(s) = self.states.get_mut(transition.from as usize) {
            s.transitions.push(transition);
        }
    }

    fn reserve_states(&mut self, additional: usize) {
        self.states.reserve(additional);
    }

    fn reserve_transitions(&mut self, state: StateId, additional: usize) {
        if let Some(s) = self.states.get_mut(state as usize) {
            s.transitions.reserve(additional);
        }
    }

    fn clear_transitions(&mut self, state: StateId) {
        if let Some(s) = self.states.get_mut(state as usize) {
            s.transitions.clear();
        }
    }
}

/// Builder for constructing VectorWfst with a fluent API.
#[derive(Clone, Debug)]
pub struct VectorWfstBuilder<L, W: Semiring> {
    fst: VectorWfst<L, W>,
}

impl<L: Clone + Send + Sync, W: Semiring> VectorWfstBuilder<L, W> {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            fst: VectorWfst::new(),
        }
    }

    /// Create a builder with pre-allocated capacity.
    pub fn with_capacity(num_states: usize) -> Self {
        Self {
            fst: VectorWfst::with_capacity(num_states),
        }
    }

    /// Add states to the WFST.
    pub fn add_states(mut self, count: usize) -> Self {
        self.fst.add_states(count);
        self
    }

    /// Set the start state.
    pub fn start(mut self, state: StateId) -> Self {
        self.fst.set_start(state);
        self
    }

    /// Set a final state.
    pub fn final_state(mut self, state: StateId, weight: W) -> Self {
        self.fst.set_final(state, weight);
        self
    }

    /// Add a transition.
    pub fn arc(
        mut self,
        from: StateId,
        input: Option<L>,
        output: Option<L>,
        to: StateId,
        weight: W,
    ) -> Self {
        self.fst.add_arc(from, input, output, to, weight);
        self
    }

    /// Add an epsilon transition.
    pub fn epsilon(mut self, from: StateId, to: StateId, weight: W) -> Self {
        self.fst.add_epsilon(from, to, weight);
        self
    }

    /// Build the WFST.
    pub fn build(self) -> VectorWfst<L, W> {
        self.fst
    }
}

impl<L: Clone + Send + Sync, W: Semiring> Default for VectorWfstBuilder<L, W> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_empty_wfst() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        assert!(fst.is_empty());
        assert_eq!(fst.num_states(), 0);
        assert_eq!(fst.start(), NO_STATE);
    }

    #[test]
    fn test_add_states() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

        let s0 = fst.add_state();
        let s1 = fst.add_state();

        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(fst.num_states(), 2);
    }

    #[test]
    fn test_start_and_final() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

        let s0 = fst.add_state();
        let s1 = fst.add_state();

        fst.set_start(s0);
        fst.set_final(s1, TropicalWeight::new(0.5));

        assert_eq!(fst.start(), s0);
        assert!(!fst.is_final(s0));
        assert!(fst.is_final(s1));
        assert_eq!(fst.final_weight(s1).value(), 0.5);
    }

    #[test]
    fn test_transitions() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();

        fst.add_arc(s0, Some('a'), Some('x'), s1, TropicalWeight::new(1.0));
        fst.add_arc(s0, Some('b'), Some('y'), s2, TropicalWeight::new(2.0));
        fst.add_epsilon(s1, s2, TropicalWeight::new(0.5));

        assert_eq!(fst.transitions(s0).len(), 2);
        assert_eq!(fst.transitions(s1).len(), 1);
        assert_eq!(fst.transitions(s2).len(), 0);
    }

    #[test]
    fn test_builder() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), Some('b'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('c'), Some('d'), 2, TropicalWeight::new(2.0))
            .final_state(2, TropicalWeight::one())
            .build();

        assert_eq!(fst.num_states(), 3);
        assert_eq!(fst.start(), 0);
        assert!(fst.is_final(2));
        assert_eq!(fst.transitions(0).len(), 1);
        assert_eq!(fst.transitions(1).len(), 1);
    }

    #[test]
    fn test_final_states_iterator() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(4)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .final_state(3, TropicalWeight::one())
            .build();

        let finals: Vec<_> = fst.final_states().collect();
        assert_eq!(finals, vec![1, 3]);
    }
}
