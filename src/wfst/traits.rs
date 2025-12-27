//! WFST traits for immutable access, construction, and lazy evaluation.

use std::fmt::Debug;

use crate::semiring::Semiring;
use super::{StateId, WeightedTransition, WfstState};

/// Core trait for immutable WFST access.
///
/// Provides read-only access to transducer structure: states, transitions,
/// and final weights. Implementations should be efficient for repeated access.
///
/// # Type Parameters
///
/// - `L`: Label type (typically `char`, `u8`, or vocabulary ID)
/// - `W`: Weight type (must implement [`Semiring`])
pub trait Wfst<L, W: Semiring>: Clone + Send + Sync {
    /// Get the start state ID.
    fn start(&self) -> StateId;

    /// Check if a state is final (accepting).
    fn is_final(&self, state: StateId) -> bool;

    /// Get the final weight for a state.
    ///
    /// Returns the semiring zero for non-final states.
    fn final_weight(&self, state: StateId) -> W;

    /// Get the outgoing transitions from a state.
    ///
    /// Returns an empty slice for invalid state IDs.
    fn transitions(&self, state: StateId) -> &[WeightedTransition<L, W>];

    /// Get the number of states in the transducer.
    fn num_states(&self) -> usize;

    /// Check if a state ID is valid.
    #[inline]
    fn is_valid_state(&self, state: StateId) -> bool {
        (state as usize) < self.num_states()
    }

    /// Get the number of transitions from a state.
    #[inline]
    fn num_transitions(&self, state: StateId) -> usize {
        self.transitions(state).len()
    }

    /// Get total number of transitions in the transducer.
    fn total_transitions(&self) -> usize {
        (0..self.num_states() as StateId)
            .map(|s| self.num_transitions(s))
            .sum()
    }

    /// Check if the transducer is empty (no states).
    #[inline]
    fn is_empty(&self) -> bool {
        self.num_states() == 0
    }

    /// Get state info including transitions.
    fn state(&self, state: StateId) -> Option<WfstState<L, W>>
    where
        L: Clone,
    {
        if !self.is_valid_state(state) {
            return None;
        }

        let mut s = if self.is_final(state) {
            WfstState::final_state(state, self.final_weight(state))
        } else {
            WfstState::new(state)
        };

        s.transitions = self.transitions(state).iter().cloned().collect();
        Some(s)
    }
}

/// Trait for constructing and modifying WFSTs.
///
/// Extends [`Wfst`] with mutation operations for building transducers.
pub trait MutableWfst<L, W: Semiring>: Wfst<L, W> {
    /// Add a new state and return its ID.
    fn add_state(&mut self) -> StateId;

    /// Add multiple states and return the first state's ID.
    fn add_states(&mut self, count: usize) -> StateId {
        let first = self.add_state();
        for _ in 1..count {
            self.add_state();
        }
        first
    }

    /// Set the start state.
    fn set_start(&mut self, state: StateId);

    /// Set a state as final with the given weight.
    fn set_final(&mut self, state: StateId, weight: W);

    /// Clear final status from a state.
    fn clear_final(&mut self, state: StateId) {
        self.set_final(state, W::zero());
    }

    /// Add a transition to the transducer.
    fn add_transition(&mut self, transition: WeightedTransition<L, W>);

    /// Add a transition with explicit parameters.
    #[inline]
    fn add_arc(
        &mut self,
        from: StateId,
        input: Option<L>,
        output: Option<L>,
        to: StateId,
        weight: W,
    ) {
        self.add_transition(WeightedTransition::new(from, input, output, to, weight));
    }

    /// Add an epsilon transition.
    #[inline]
    fn add_epsilon(&mut self, from: StateId, to: StateId, weight: W) {
        self.add_transition(WeightedTransition::epsilon(from, to, weight));
    }

    /// Reserve capacity for states.
    fn reserve_states(&mut self, additional: usize);

    /// Reserve capacity for transitions from a specific state.
    fn reserve_transitions(&mut self, state: StateId, additional: usize);

    /// Clear all transitions from a state.
    ///
    /// This removes all outgoing transitions from the specified state
    /// but keeps the state itself.
    fn clear_transitions(&mut self, state: StateId);

    /// Replace all transitions from a state with new ones.
    ///
    /// This is more efficient than clearing and adding individually.
    fn set_transitions(&mut self, state: StateId, transitions: Vec<WeightedTransition<L, W>>)
    where
        L: Clone,
    {
        self.clear_transitions(state);
        for trans in transitions {
            self.add_transition(trans);
        }
    }
}

/// Caching policy for lazy state expansion.
///
/// Controls how computed states are cached in lazy WFSTs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CachePolicy {
    /// Cache all visited states (default).
    ///
    /// Best for traversals that may revisit states.
    #[default]
    CacheAll,

    /// LRU cache with maximum size.
    ///
    /// Bounds memory usage while keeping recently accessed states.
    Lru {
        /// Maximum number of states to cache.
        max_states: usize,
    },

    /// No caching (recompute each time).
    ///
    /// Lowest memory usage but may recompute states.
    NoCache,
}

/// Trait for WFSTs that support lazy (on-demand) state expansion.
///
/// Lazy WFSTs compute states only when they are accessed, which is critical
/// for composition operations where the product state space can be exponential.
/// Instead of computing all states upfront, lazy WFSTs expand states during
/// traversal, caching results according to the configured [`CachePolicy`].
///
/// # Example Usage
///
/// ```ignore
/// let mut lazy_fst = LazyComposition::new(fst1, fst2);
///
/// // States are computed on-demand
/// for path in lazy_fst.accepting_paths() {
///     // Only states reachable along accepting paths are computed
/// }
///
/// // Check how many states were actually computed
/// println!("Computed {} states", lazy_fst.computed_states());
/// ```
pub trait LazyWfst<L, W: Semiring>: Wfst<L, W> {
    /// Check if a state has been expanded (transitions computed).
    fn is_expanded(&self, state: StateId) -> bool;

    /// Force expansion of a state.
    ///
    /// Useful for prefetching or ensuring a state is computed.
    fn expand(&mut self, state: StateId);

    /// Get transitions, computing them lazily if needed.
    ///
    /// This is the primary method for lazy access - it returns transitions
    /// for a state, computing them on first access.
    fn transitions_lazy(&mut self, state: StateId) -> &[WeightedTransition<L, W>];

    /// Get the current cache policy.
    fn cache_policy(&self) -> CachePolicy;

    /// Set the cache policy.
    fn set_cache_policy(&mut self, policy: CachePolicy);

    /// Get the number of states that have been computed so far.
    fn computed_states(&self) -> usize;

    /// Clear the state cache.
    ///
    /// Useful for freeing memory after traversal.
    fn clear_cache(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_policy_default() {
        let policy = CachePolicy::default();
        assert_eq!(policy, CachePolicy::CacheAll);
    }

    #[test]
    fn test_cache_policy_lru() {
        let policy = CachePolicy::Lru { max_states: 1000 };
        if let CachePolicy::Lru { max_states } = policy {
            assert_eq!(max_states, 1000);
        } else {
            panic!("Expected Lru policy");
        }
    }
}
