//! Lazy WFST types for on-demand state expansion.
//!
//! This module provides infrastructure for lazy WFSTs that compute states
//! only when accessed. This is critical for composition operations where
//! the product state space can be exponentially large.
//!
//! # Architecture
//!
//! - [`LazyState`]: Represents a state that may or may not be computed
//! - [`StateSource`]: Trait for types that can produce states on demand
//! - [`LazyWfstWrapper`]: Generic lazy WFST wrapper around a StateSource

use std::collections::VecDeque;

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use super::traits::{CachePolicy, LazyWfst, Wfst};
use super::transition::WeightedTransition;
use super::types::StateId;
use crate::semiring::Semiring;

/// A state that may or may not have been computed yet.
///
/// Used in lazy WFSTs to track which states have been expanded.
#[derive(Clone, Debug, Default)]
pub enum LazyState<L, W: Semiring> {
    /// State exists but transitions not yet computed.
    #[default]
    Pending,

    /// State fully computed with all information.
    Computed {
        /// Whether this is a final state.
        is_final: bool,
        /// Final weight (semiring zero if not final).
        final_weight: W,
        /// Outgoing transitions.
        transitions: SmallVec<[WeightedTransition<L, W>; 4]>,
    },
}

impl<L, W: Semiring> LazyState<L, W> {
    /// Create a computed non-final state.
    pub fn non_final(transitions: SmallVec<[WeightedTransition<L, W>; 4]>) -> Self {
        LazyState::Computed {
            is_final: false,
            final_weight: W::zero(),
            transitions,
        }
    }

    /// Create a computed final state.
    pub fn final_state(weight: W, transitions: SmallVec<[WeightedTransition<L, W>; 4]>) -> Self {
        LazyState::Computed {
            is_final: true,
            final_weight: weight,
            transitions,
        }
    }

    /// Check if this state has been computed.
    #[inline]
    pub fn is_computed(&self) -> bool {
        matches!(self, LazyState::Computed { .. })
    }

    /// Get transitions if computed.
    #[inline]
    pub fn transitions(&self) -> Option<&[WeightedTransition<L, W>]> {
        match self {
            LazyState::Computed { transitions, .. } => Some(transitions.as_slice()),
            LazyState::Pending => None,
        }
    }
}

/// Trait for types that can produce WFST states on demand.
///
/// Implement this trait to create custom lazy WFSTs (e.g., for composition).
pub trait StateSource<L, W: Semiring>: Clone + Send + Sync {
    /// Compute the state information for a given state ID.
    ///
    /// This method should compute and return a fully populated [`LazyState`].
    fn compute_state(&self, state: StateId) -> LazyState<L, W>;

    /// Get the start state ID.
    fn start(&self) -> StateId;

    /// Get an upper bound on the number of states.
    ///
    /// Returns `None` if the number of states is unbounded or unknown.
    fn num_states_hint(&self) -> Option<usize> {
        None
    }
}

/// Generic lazy WFST wrapper that computes states on demand.
///
/// Wraps a [`StateSource`] and caches computed states according to
/// the configured [`CachePolicy`].
///
/// # Type Parameters
///
/// - `S`: The state source type
/// - `L`: Label type
/// - `W`: Weight type
#[derive(Debug)]
pub struct LazyWfstWrapper<S, L, W>
where
    S: StateSource<L, W>,
    W: Semiring,
{
    /// The underlying state source.
    source: S,

    /// Cache of computed states.
    cache: FxHashMap<StateId, LazyState<L, W>>,

    /// Most recently computed state when caching is disabled.
    transient_state: Option<(StateId, LazyState<L, W>)>,

    /// Access order for LRU eviction.
    access_order: VecDeque<StateId>,

    /// Caching policy.
    policy: CachePolicy,

    /// Counter for computed states.
    computed_count: usize,

    /// Start state ID.
    start: StateId,
}

impl<S, L, W> Clone for LazyWfstWrapper<S, L, W>
where
    S: StateSource<L, W> + Clone,
    L: Clone,
    W: Semiring,
{
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            cache: self.cache.clone(),
            transient_state: self.transient_state.clone(),
            access_order: self.access_order.clone(),
            policy: self.policy,
            computed_count: self.computed_count,
            start: self.start,
        }
    }
}

impl<S, L, W> LazyWfstWrapper<S, L, W>
where
    S: StateSource<L, W>,
    L: Clone + Send + Sync,
    W: Semiring,
{
    /// Create a new lazy WFST with default caching.
    pub fn new(source: S) -> Self {
        let start = source.start();
        let initial_capacity = source.num_states_hint().unwrap_or(16);

        Self {
            source,
            cache: FxHashMap::with_capacity_and_hasher(initial_capacity, Default::default()),
            transient_state: None,
            access_order: VecDeque::with_capacity(initial_capacity),
            policy: CachePolicy::default(),
            computed_count: 0,
            start,
        }
    }

    /// Create with a specific cache policy.
    pub fn with_cache_policy(source: S, policy: CachePolicy) -> Self {
        let mut wrapper = Self::new(source);
        wrapper.policy = policy;
        wrapper
    }

    /// Ensure a state is computed and cached, returning a reference.
    fn ensure_computed(&mut self, state: StateId) -> &LazyState<L, W> {
        if self.uses_transient_storage() {
            if self.state_entry(state).is_none() {
                let computed = self.source.compute_state(state);
                self.insert_cached(state, computed);
            }
            return self.state_entry(state).unwrap_or(&LazyState::Pending);
        }

        if !self.cache.contains_key(&state) {
            let computed = self.source.compute_state(state);
            self.insert_cached(state, computed);
        } else if matches!(self.policy, CachePolicy::Lru { .. }) {
            // Update access order for LRU
            self.touch_lru(state);
        }

        self.state_entry(state).unwrap_or(&LazyState::Pending)
    }

    /// Return a computed state from the persistent cache or transient no-cache slot.
    fn state_entry(&self, state: StateId) -> Option<&LazyState<L, W>> {
        self.cache.get(&state).or_else(|| {
            self.transient_state
                .as_ref()
                .filter(|(transient_state, _)| *transient_state == state)
                .map(|(_, entry)| entry)
        })
    }

    /// Insert a computed state into the cache.
    fn insert_cached(&mut self, state: StateId, computed: LazyState<L, W>) {
        match self.policy {
            CachePolicy::NoCache => {
                self.transient_state = Some((state, computed));
                self.record_computation();
            }
            CachePolicy::CacheAll => {
                self.transient_state = None;
                self.cache.insert(state, computed);
                self.record_computation();
            }
            CachePolicy::Lru { max_states } => {
                self.transient_state = None;
                if max_states == 0 {
                    self.transient_state = Some((state, computed));
                    self.record_computation();
                    return;
                }

                let replacing_cached_state = self.cache.contains_key(&state);
                while !replacing_cached_state && self.cache.len() >= max_states {
                    self.evict_lru();
                }

                self.cache.insert(state, computed);
                if let Some(pos) = self.access_order.iter().position(|&s| s == state) {
                    self.access_order.remove(pos);
                }
                self.access_order.push_back(state);
                self.record_computation();
            }
        }
    }

    /// Record a state-source computation without wrapping on extremely long traversals.
    fn record_computation(&mut self) {
        self.computed_count = self.computed_count.saturating_add(1);
    }

    /// Whether the active cache policy keeps only the current computed state.
    fn uses_transient_storage(&self) -> bool {
        matches!(
            self.policy,
            CachePolicy::NoCache | CachePolicy::Lru { max_states: 0 }
        )
    }

    /// Update LRU access order.
    fn touch_lru(&mut self, state: StateId) {
        // Remove from current position and add to back
        if let Some(pos) = self.access_order.iter().position(|&s| s == state) {
            self.access_order.remove(pos);
            self.access_order.push_back(state);
        } else if self.cache.contains_key(&state) {
            self.access_order.push_back(state);
        }
    }

    /// Keep LRU bookkeeping aligned with the current persistent cache contents.
    fn reconcile_lru_order(&mut self) {
        let cache = &self.cache;
        let mut seen = FxHashSet::with_capacity_and_hasher(cache.len(), Default::default());
        self.access_order
            .retain(|state| cache.contains_key(state) && seen.insert(*state));

        let mut cached_states: Vec<_> = self.cache.keys().copied().collect();
        cached_states.sort_unstable();
        for state in cached_states {
            if seen.insert(state) {
                self.access_order.push_back(state);
            }
        }
    }

    /// Evict the least-recently used persistent cached state.
    fn evict_lru(&mut self) {
        while let Some(evict) = self.access_order.pop_front() {
            if self.cache.remove(&evict).is_some() {
                return;
            }
        }

        if let Some(evict) = self.cache.keys().copied().min() {
            self.cache.remove(&evict);
        }
    }

    /// Get the underlying source.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// Get mutable access to the underlying source.
    pub fn source_mut(&mut self) -> &mut S {
        &mut self.source
    }

    /// Take ownership of the source, discarding the cache.
    pub fn into_source(self) -> S {
        self.source
    }
}

impl<S, L, W> Wfst<L, W> for LazyWfstWrapper<S, L, W>
where
    S: StateSource<L, W>,
    L: Clone + Send + Sync,
    W: Semiring,
{
    fn start(&self) -> StateId {
        self.start
    }

    fn is_final(&self, state: StateId) -> bool {
        // Note: This requires mutable access in practice
        // For immutable access, we check the cache
        self.state_entry(state)
            .map(|s| matches!(s, LazyState::Computed { is_final: true, .. }))
            .unwrap_or(false)
    }

    fn final_weight(&self, state: StateId) -> W {
        self.state_entry(state)
            .map(|s| match s {
                LazyState::Computed { final_weight, .. } => *final_weight,
                LazyState::Pending => W::zero(),
            })
            .unwrap_or_else(W::zero)
    }

    fn transitions(&self, state: StateId) -> &[WeightedTransition<L, W>] {
        // For immutable access, return empty if not computed
        self.state_entry(state)
            .and_then(|s| s.transitions())
            .unwrap_or(&[])
    }

    fn num_states(&self) -> usize {
        self.source.num_states_hint().unwrap_or(0)
    }
}

impl<S, L, W> LazyWfst<L, W> for LazyWfstWrapper<S, L, W>
where
    S: StateSource<L, W>,
    L: Clone + Send + Sync,
    W: Semiring,
{
    fn is_expanded(&self, state: StateId) -> bool {
        self.state_entry(state)
            .map(|s| s.is_computed())
            .unwrap_or(false)
    }

    fn expand(&mut self, state: StateId) {
        if !self.is_expanded(state) {
            let computed = self.source.compute_state(state);
            self.insert_cached(state, computed);
        }
    }

    fn transitions_lazy(&mut self, state: StateId) -> &[WeightedTransition<L, W>] {
        self.ensure_computed(state);
        self.transitions(state)
    }

    fn cache_policy(&self) -> CachePolicy {
        self.policy
    }

    fn set_cache_policy(&mut self, policy: CachePolicy) {
        self.policy = policy;
        match policy {
            CachePolicy::NoCache | CachePolicy::Lru { max_states: 0 } => {
                self.cache.clear();
                self.access_order.clear();
                self.transient_state = None;
            }
            CachePolicy::CacheAll => {
                self.transient_state = None;
                self.access_order.clear();
            }
            CachePolicy::Lru { max_states } => {
                self.transient_state = None;
                self.reconcile_lru_order();
                while self.cache.len() > max_states {
                    self.evict_lru();
                }
            }
        }
    }

    fn computed_states(&self) -> usize {
        self.computed_count
    }

    fn clear_cache(&mut self) {
        self.cache.clear();
        self.transient_state = None;
        self.access_order.clear();
        // Don't reset computed_count - it tracks total ever computed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    /// Simple test source that generates a linear chain.
    #[derive(Clone)]
    struct LinearChainSource {
        num_states: usize,
    }

    impl StateSource<char, TropicalWeight> for LinearChainSource {
        fn compute_state(&self, state: StateId) -> LazyState<char, TropicalWeight> {
            let state_idx = state as usize;

            if state_idx >= self.num_states {
                return LazyState::Pending;
            }

            let is_final = state_idx == self.num_states - 1;
            let mut transitions = SmallVec::new();

            if state_idx < self.num_states - 1 {
                transitions.push(WeightedTransition::new(
                    state,
                    Some('a'),
                    Some('a'),
                    state + 1,
                    TropicalWeight::new(1.0),
                ));
            }

            if is_final {
                LazyState::final_state(TropicalWeight::one(), transitions)
            } else {
                LazyState::non_final(transitions)
            }
        }

        fn start(&self) -> StateId {
            0
        }

        fn num_states_hint(&self) -> Option<usize> {
            Some(self.num_states)
        }
    }

    #[test]
    fn test_lazy_wrapper_basic() {
        let source = LinearChainSource { num_states: 5 };
        let mut lazy = LazyWfstWrapper::new(source);

        assert_eq!(lazy.start(), 0);
        assert_eq!(lazy.computed_states(), 0);

        // Access a state lazily
        let transitions = lazy.transitions_lazy(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(lazy.computed_states(), 1);

        // Access another state
        let transitions = lazy.transitions_lazy(1);
        assert_eq!(transitions.len(), 1);
        assert_eq!(lazy.computed_states(), 2);

        // Final state
        lazy.expand(4);
        assert!(lazy.is_expanded(4));
        assert_eq!(lazy.computed_states(), 3);
    }

    #[test]
    fn test_lru_eviction() {
        let source = LinearChainSource { num_states: 10 };
        let mut lazy =
            LazyWfstWrapper::with_cache_policy(source, CachePolicy::Lru { max_states: 3 });

        // Expand 5 states, should evict older ones
        for i in 0..5 {
            lazy.expand(i);
        }

        // Only 3 should be cached
        assert_eq!(lazy.cache.len(), 3);

        // Most recent should still be cached
        assert!(lazy.is_expanded(4));
        assert!(lazy.is_expanded(3));
        assert!(lazy.is_expanded(2));

        // Oldest should be evicted
        assert!(!lazy.is_expanded(0));
        assert!(!lazy.is_expanded(1));
    }

    #[test]
    fn test_lru_policy_reconciles_existing_cache_on_policy_change() {
        let source = LinearChainSource { num_states: 10 };
        let mut lazy = LazyWfstWrapper::new(source);

        lazy.expand(0);
        lazy.expand(1);
        lazy.expand(2);
        assert_eq!(lazy.cache.len(), 3);
        assert!(lazy.access_order.is_empty());

        lazy.set_cache_policy(CachePolicy::Lru { max_states: 2 });

        assert_eq!(lazy.cache.len(), 2);
        assert_eq!(lazy.access_order.len(), 2);

        lazy.expand(3);

        assert_eq!(lazy.cache.len(), 2);
        assert_eq!(lazy.access_order.len(), 2);
        assert!(lazy.is_expanded(3));
    }

    #[test]
    fn test_lru_hit_restores_missing_access_order_entry() {
        let source = LinearChainSource { num_states: 10 };
        let mut lazy =
            LazyWfstWrapper::with_cache_policy(source, CachePolicy::Lru { max_states: 2 });

        lazy.expand(0);
        lazy.expand(1);
        lazy.access_order.clear();

        let transitions = lazy.transitions_lazy(0);

        assert_eq!(transitions.len(), 1);
        assert_eq!(lazy.cache.len(), 2);
        assert_eq!(lazy.access_order.back(), Some(&0));
    }

    #[test]
    fn test_lru_eviction_preserves_capacity_with_stale_order() {
        let source = LinearChainSource { num_states: 10 };
        let mut lazy =
            LazyWfstWrapper::with_cache_policy(source, CachePolicy::Lru { max_states: 2 });

        lazy.expand(0);
        lazy.expand(1);
        lazy.access_order.clear();
        lazy.access_order.push_back(99);

        lazy.expand(2);

        assert_eq!(lazy.cache.len(), 2);
        assert!(lazy.is_expanded(2));
    }

    #[test]
    fn test_no_cache_policy_returns_transitions_without_retaining_state() {
        let source = LinearChainSource { num_states: 5 };
        let mut lazy = LazyWfstWrapper::with_cache_policy(source, CachePolicy::NoCache);

        let transitions = lazy.transitions_lazy(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 1);
        assert_eq!(lazy.computed_states(), 1);
        assert_eq!(lazy.cache.len(), 0);
        assert!(lazy.is_expanded(0));

        let transitions = lazy.transitions_lazy(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 1);
        assert_eq!(lazy.computed_states(), 1);
        assert_eq!(lazy.cache.len(), 0);
        assert!(lazy.is_expanded(0));

        let transitions = lazy.transitions_lazy(1);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 2);
        assert_eq!(lazy.computed_states(), 2);
        assert_eq!(lazy.cache.len(), 0);
        assert!(!lazy.is_expanded(0));
        assert!(lazy.is_expanded(1));
    }

    #[test]
    fn test_zero_capacity_lru_uses_transient_state() {
        let source = LinearChainSource { num_states: 5 };
        let mut lazy =
            LazyWfstWrapper::with_cache_policy(source, CachePolicy::Lru { max_states: 0 });

        let transitions = lazy.transitions_lazy(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 1);
        assert_eq!(lazy.computed_states(), 1);
        assert_eq!(lazy.cache.len(), 0);
        assert!(lazy.is_expanded(0));

        let transitions = lazy.transitions_lazy(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 1);
        assert_eq!(lazy.computed_states(), 1);
        assert_eq!(lazy.cache.len(), 0);
        assert!(lazy.is_expanded(0));
    }

    #[test]
    fn test_switching_to_no_cache_discards_persistent_cache() {
        let source = LinearChainSource { num_states: 5 };
        let mut lazy = LazyWfstWrapper::new(source);

        assert_eq!(lazy.transitions_lazy(0).len(), 1);
        assert_eq!(lazy.computed_states(), 1);
        assert_eq!(lazy.cache.len(), 1);

        lazy.set_cache_policy(CachePolicy::NoCache);
        assert_eq!(lazy.cache.len(), 0);

        assert_eq!(lazy.transitions_lazy(0).len(), 1);
        assert_eq!(lazy.computed_states(), 2);
        assert_eq!(lazy.cache.len(), 0);
        assert!(lazy.is_expanded(0));
    }

    #[test]
    fn test_clear_cache() {
        let source = LinearChainSource { num_states: 5 };
        let mut lazy = LazyWfstWrapper::new(source);

        lazy.expand(0);
        lazy.expand(1);
        lazy.expand(2);

        assert_eq!(lazy.cache.len(), 3);
        assert_eq!(lazy.computed_states(), 3);

        lazy.clear_cache();

        assert_eq!(lazy.cache.len(), 0);
        // computed_states tracks total ever computed
        assert_eq!(lazy.computed_states(), 3);
    }
}
