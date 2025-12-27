//! WFST state type with transitions.

use smallvec::SmallVec;

use crate::semiring::Semiring;
use super::{StateId, WeightedTransition};

/// A state in a WFST with its outgoing transitions.
///
/// Uses `SmallVec` for transitions to avoid heap allocation for states
/// with few transitions (common case).
///
/// # Type Parameters
///
/// - `L`: Label type (typically `char`, `u8`, or vocabulary ID)
/// - `W`: Weight type (must implement [`Semiring`])
#[derive(Clone, Debug)]
pub struct WfstState<L, W: Semiring> {
    /// State identifier.
    pub id: StateId,
    /// Whether this is a final (accepting) state.
    pub is_final: bool,
    /// Weight for reaching the final state (used if `is_final` is true).
    pub final_weight: W,
    /// Outgoing transitions from this state.
    /// Uses SmallVec to inline up to 4 transitions without heap allocation.
    pub transitions: SmallVec<[WeightedTransition<L, W>; 4]>,
}

impl<L, W: Semiring> WfstState<L, W> {
    /// Create a new non-final state with no transitions.
    #[inline]
    pub fn new(id: StateId) -> Self {
        Self {
            id,
            is_final: false,
            final_weight: W::zero(),
            transitions: SmallVec::new(),
        }
    }

    /// Create a new final state with the given weight.
    #[inline]
    pub fn final_state(id: StateId, weight: W) -> Self {
        Self {
            id,
            is_final: true,
            final_weight: weight,
            transitions: SmallVec::new(),
        }
    }

    /// Add a transition from this state.
    #[inline]
    pub fn add_transition(&mut self, transition: WeightedTransition<L, W>) {
        debug_assert_eq!(transition.from, self.id, "Transition source must match state ID");
        self.transitions.push(transition);
    }

    /// Add a transition with the given parameters.
    #[inline]
    pub fn add_arc(&mut self, input: Option<L>, output: Option<L>, to: StateId, weight: W) {
        self.transitions.push(WeightedTransition::new(self.id, input, output, to, weight));
    }

    /// Set this state as final with the given weight.
    #[inline]
    pub fn set_final(&mut self, weight: W) {
        self.is_final = true;
        self.final_weight = weight;
    }

    /// Clear the final status of this state.
    #[inline]
    pub fn clear_final(&mut self) {
        self.is_final = false;
        self.final_weight = W::zero();
    }

    /// Number of outgoing transitions.
    #[inline]
    pub fn num_transitions(&self) -> usize {
        self.transitions.len()
    }

    /// Check if this state has any outgoing transitions.
    #[inline]
    pub fn has_transitions(&self) -> bool {
        !self.transitions.is_empty()
    }

    /// Get iterator over transitions.
    #[inline]
    pub fn iter_transitions(&self) -> impl Iterator<Item = &WeightedTransition<L, W>> {
        self.transitions.iter()
    }

    /// Reserve capacity for additional transitions.
    #[inline]
    pub fn reserve_transitions(&mut self, additional: usize) {
        self.transitions.reserve(additional);
    }
}

impl<L: Clone, W: Semiring> WfstState<L, W> {
    /// Get transitions filtered by input label.
    pub fn transitions_by_input<'a>(&'a self, input: &'a Option<L>) -> impl Iterator<Item = &'a WeightedTransition<L, W>>
    where
        L: PartialEq,
    {
        self.transitions.iter().filter(move |t| &t.input == input)
    }

    /// Get epsilon input transitions.
    pub fn epsilon_transitions(&self) -> impl Iterator<Item = &WeightedTransition<L, W>> {
        self.transitions.iter().filter(|t| t.is_epsilon_input())
    }

    /// Get non-epsilon input transitions.
    pub fn labeled_transitions(&self) -> impl Iterator<Item = &WeightedTransition<L, W>> {
        self.transitions.iter().filter(|t| !t.is_epsilon_input())
    }
}

impl<L, W: Semiring> Default for WfstState<L, W> {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_state_creation() {
        let state: WfstState<char, TropicalWeight> = WfstState::new(0);
        assert_eq!(state.id, 0);
        assert!(!state.is_final);
        assert!(state.final_weight.is_zero());
        assert!(state.transitions.is_empty());
    }

    #[test]
    fn test_final_state() {
        let state: WfstState<char, TropicalWeight> = WfstState::final_state(1, TropicalWeight::new(0.5));
        assert_eq!(state.id, 1);
        assert!(state.is_final);
        assert_eq!(state.final_weight.value(), 0.5);
    }

    #[test]
    fn test_add_transitions() {
        let mut state: WfstState<char, TropicalWeight> = WfstState::new(0);

        state.add_arc(Some('a'), Some('b'), 1, TropicalWeight::new(1.0));
        state.add_arc(Some('c'), Some('d'), 2, TropicalWeight::new(2.0));

        assert_eq!(state.num_transitions(), 2);
        assert!(state.has_transitions());
    }

    #[test]
    fn test_transition_filtering() {
        let mut state: WfstState<char, TropicalWeight> = WfstState::new(0);

        state.add_arc(Some('a'), Some('a'), 1, TropicalWeight::one());
        state.add_arc(None, None, 2, TropicalWeight::one()); // epsilon
        state.add_arc(Some('b'), Some('b'), 3, TropicalWeight::one());

        assert_eq!(state.epsilon_transitions().count(), 1);
        assert_eq!(state.labeled_transitions().count(), 2);
    }
}
