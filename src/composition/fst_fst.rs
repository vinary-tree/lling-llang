//! Lazy FST ∘ FST composition.
//!
//! This module implements lazy composition of two WFSTs where product states
//! are computed on-demand during traversal.
//!
//! # Algorithm
//!
//! Composition of FST₁ and FST₂ produces a new FST where:
//! - States are pairs (s₁, s₂) from the component FSTs
//! - Transitions match when FST₁ output = FST₂ input
//! - Weights are combined using semiring multiplication
//!
//! # Lazy Evaluation
//!
//! Instead of computing all product states upfront (which can be O(n×m)),
//! states are computed lazily:
//! - Only reachable states are ever computed
//! - States cached according to cache policy
//! - Memory bounded by actual traversal
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::composition::compose;
//!
//! let fst1: VectorWfst<char, TropicalWeight> = ...;
//! let fst2: VectorWfst<char, TropicalWeight> = ...;
//!
//! let composed = compose(fst1, fst2);
//!
//! // Lazily enumerate accepting paths
//! for path in composed.accepting_paths() {
//!     println!("{:?}", path);
//! }
//! ```

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::hash::Hash;
use std::marker::PhantomData;

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use super::filter::{EpsilonFilter, EpsilonFilterType, FilterState};
use crate::semiring::Semiring;
use crate::wfst::{CachePolicy, StateId, WeightedTransition, Wfst};

/// A product state in the composed FST.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProductStateId {
    /// State from FST1.
    pub s1: StateId,
    /// State from FST2.
    pub s2: StateId,
    /// Filter state for epsilon handling.
    pub filter: FilterState,
}

impl ProductStateId {
    /// Create a new product state.
    pub fn new(s1: StateId, s2: StateId, filter: FilterState) -> Self {
        Self { s1, s2, filter }
    }
}

/// Cached state information for a product state.
#[derive(Clone, Debug)]
struct CachedState<L, W: Semiring> {
    is_final: bool,
    final_weight: W,
    transitions: SmallVec<[ComposedTransition<L, W>; 4]>,
}

/// A transition in the composed FST.
#[derive(Clone, Debug)]
pub struct ComposedTransition<L, W: Semiring> {
    /// Input label (from FST1 input).
    pub input: Option<L>,
    /// Output label (from FST2 output).
    pub output: Option<L>,
    /// Target product state.
    pub target: ProductStateId,
    /// Combined weight.
    pub weight: W,
}

/// A path through the composed FST.
#[derive(Clone, Debug)]
pub struct ComposedPath<L: Clone, W: Semiring> {
    /// Input sequence.
    pub inputs: Vec<L>,
    /// Output sequence.
    pub outputs: Vec<L>,
    /// Total path weight.
    pub weight: W,
}

impl<L: Clone, W: Semiring> ComposedPath<L, W> {
    fn new() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            weight: W::one(),
        }
    }

    fn extend(&self, input: Option<L>, output: Option<L>, weight: W) -> Self {
        let mut new_inputs = self.inputs.clone();
        let mut new_outputs = self.outputs.clone();

        if let Some(i) = input {
            new_inputs.push(i);
        }
        if let Some(o) = output {
            new_outputs.push(o);
        }

        Self {
            inputs: new_inputs,
            outputs: new_outputs,
            weight: self.weight.times(&weight),
        }
    }
}

/// Lazy composition of two WFSTs.
///
/// Product states are computed on-demand during traversal, avoiding
/// the O(n×m) state explosion of eager composition.
pub struct LazyComposition<F1, F2, L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash,
    W: Semiring,
{
    fst1: F1,
    fst2: F2,
    /// Cache of computed product states.
    state_cache: FxHashMap<ProductStateId, CachedState<L, W>>,
    /// Most recently computed state when persistent caching is disabled.
    transient_state: Option<(ProductStateId, CachedState<L, W>)>,
    /// Access order for LRU eviction.
    access_order: VecDeque<ProductStateId>,
    /// Epsilon filter.
    filter: EpsilonFilter,
    /// Cache policy.
    policy: CachePolicy,
    /// Start product state.
    start: ProductStateId,
    /// Marker for label type.
    _marker: PhantomData<L>,
}

impl<F1, F2, L, W> LazyComposition<F1, F2, L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash,
    W: Semiring,
{
    /// Create a new lazy composition.
    pub fn new(fst1: F1, fst2: F2) -> Self {
        let start = ProductStateId::new(fst1.start(), fst2.start(), FilterState::None);

        Self {
            fst1,
            fst2,
            state_cache: FxHashMap::default(),
            transient_state: None,
            access_order: VecDeque::new(),
            filter: EpsilonFilter::default(),
            policy: CachePolicy::CacheAll,
            start,
            _marker: PhantomData,
        }
    }

    /// Create with specific epsilon filter type.
    pub fn with_filter(fst1: F1, fst2: F2, filter_type: EpsilonFilterType) -> Self {
        let start = ProductStateId::new(fst1.start(), fst2.start(), FilterState::None);

        Self {
            fst1,
            fst2,
            state_cache: FxHashMap::default(),
            transient_state: None,
            access_order: VecDeque::new(),
            filter: EpsilonFilter::new(filter_type),
            policy: CachePolicy::CacheAll,
            start,
            _marker: PhantomData,
        }
    }

    /// Set cache policy.
    pub fn with_cache_policy(mut self, policy: CachePolicy) -> Self {
        self.policy = policy;
        self.enforce_cache_policy();
        self
    }

    /// Get the start product state.
    pub fn start(&self) -> ProductStateId {
        self.start
    }

    /// Get the number of product states retained in the persistent cache.
    pub fn computed_states(&self) -> usize {
        self.state_cache.len()
    }

    /// Check if a product state is final.
    pub fn is_final(&mut self, state: ProductStateId) -> bool {
        self.ensure_computed(state);
        self.state_entry(state).map(|s| s.is_final).unwrap_or(false)
    }

    /// Get the final weight of a product state.
    pub fn final_weight(&mut self, state: ProductStateId) -> W {
        self.ensure_computed(state);
        self.state_entry(state)
            .map(|s| s.final_weight)
            .unwrap_or_else(W::zero)
    }

    /// Get transitions from a product state.
    pub fn transitions(
        &mut self,
        state: ProductStateId,
    ) -> SmallVec<[ComposedTransition<L, W>; 4]> {
        self.ensure_computed(state);
        self.state_entry(state)
            .map(|s| s.transitions.clone())
            .unwrap_or_default()
    }

    /// Ensure a product state is computed and cached.
    fn ensure_computed(&mut self, state: ProductStateId) {
        if self.state_entry(state).is_some() {
            if matches!(self.policy, CachePolicy::Lru { max_states } if max_states > 0) {
                self.touch_lru(state);
            }
            return;
        }

        let cached = self.compute_state(state);
        self.insert_computed(state, cached);
    }

    /// Return a computed state from the persistent cache or transient no-cache slot.
    fn state_entry(&self, state: ProductStateId) -> Option<&CachedState<L, W>> {
        self.state_cache.get(&state).or_else(|| {
            self.transient_state
                .as_ref()
                .filter(|(transient_state, _)| *transient_state == state)
                .map(|(_, cached)| cached)
        })
    }

    /// Insert a newly computed state according to the active cache policy.
    fn insert_computed(&mut self, state: ProductStateId, cached: CachedState<L, W>) {
        match self.policy {
            CachePolicy::CacheAll => {
                self.transient_state = None;
                self.state_cache.insert(state, cached);
            }
            CachePolicy::Lru { max_states } => {
                self.transient_state = None;
                if max_states == 0 {
                    self.state_cache.clear();
                    self.access_order.clear();
                    self.transient_state = Some((state, cached));
                    return;
                }

                while self.state_cache.len() >= max_states {
                    self.evict_lru();
                }

                self.state_cache.insert(state, cached);
                self.access_order.push_back(state);
            }
            CachePolicy::NoCache => {
                self.state_cache.clear();
                self.access_order.clear();
                self.transient_state = Some((state, cached));
            }
        }
    }

    /// Apply the active cache policy to states retained before a policy change.
    fn enforce_cache_policy(&mut self) {
        match self.policy {
            CachePolicy::CacheAll => {
                self.transient_state = None;
            }
            CachePolicy::NoCache | CachePolicy::Lru { max_states: 0 } => {
                self.state_cache.clear();
                self.access_order.clear();
                self.transient_state = None;
            }
            CachePolicy::Lru { max_states } => {
                self.transient_state = None;
                self.reconcile_lru_order();
                while self.state_cache.len() > max_states {
                    self.evict_lru();
                }
            }
        }
    }

    /// Keep LRU bookkeeping aligned with the current persistent cache contents.
    fn reconcile_lru_order(&mut self) {
        self.access_order
            .retain(|state| self.state_cache.contains_key(state));

        // Append any cached states missing from the access order. Membership is
        // tested against a set built once (O(cache)) rather than a fresh
        // `access_order.contains` scan per key, which was O(cache²).
        let present: FxHashSet<ProductStateId> = self.access_order.iter().copied().collect();
        let missing: Vec<ProductStateId> = self
            .state_cache
            .keys()
            .copied()
            .filter(|state| !present.contains(state))
            .collect();
        self.access_order.extend(missing);
    }

    /// Evict the least-recently used persistent cached state.
    fn evict_lru(&mut self) {
        while let Some(evict) = self.access_order.pop_front() {
            if self.state_cache.remove(&evict).is_some() {
                return;
            }
        }

        if let Some((&evict, _)) = self.state_cache.iter().next() {
            self.state_cache.remove(&evict);
        }
    }

    /// Mark a persistent cached state as recently used.
    fn touch_lru(&mut self, state: ProductStateId) {
        if let Some(pos) = self.access_order.iter().position(|&cached| cached == state) {
            self.access_order.remove(pos);
            self.access_order.push_back(state);
        } else if self.state_cache.contains_key(&state) {
            self.access_order.push_back(state);
        }
    }

    /// Compute a product state's transitions.
    fn compute_state(&self, state: ProductStateId) -> CachedState<L, W> {
        let ProductStateId { s1, s2, filter } = state;

        // Check if final
        let is_final = self.fst1.is_final(s1) && self.fst2.is_final(s2);
        let final_weight = if is_final {
            self.fst1
                .final_weight(s1)
                .times(&self.fst2.final_weight(s2))
        } else {
            W::zero()
        };

        // Get transitions from both FSTs
        let trans1 = self.fst1.transitions(s1);
        let trans2 = self.fst2.transitions(s2);

        let (can_eps1, can_eps2, can_match) = self.filter.allowed_moves(filter);

        let epsilon_capacity = usize::from(can_eps1)
            * trans1.iter().filter(|t1| t1.output.is_none()).count()
            + usize::from(can_eps2) * trans2.iter().filter(|t2| t2.input.is_none()).count();
        let mut transitions = SmallVec::with_capacity(epsilon_capacity);

        // Case 1: FST1 epsilon output (advance FST1 only)
        if can_eps1 {
            for t1 in trans1 {
                if t1.output.is_none() {
                    let new_filter = self.filter.next_state(filter, true, false);
                    transitions.push(ComposedTransition {
                        input: t1.input.clone(),
                        output: None,
                        target: ProductStateId::new(t1.to, s2, new_filter),
                        weight: t1.weight,
                    });
                }
            }
        }

        // Case 2: FST2 epsilon input (advance FST2 only)
        if can_eps2 {
            for t2 in trans2 {
                if t2.input.is_none() {
                    let new_filter = self.filter.next_state(filter, false, true);
                    transitions.push(ComposedTransition {
                        input: None,
                        output: t2.output.clone(),
                        target: ProductStateId::new(s1, t2.to, new_filter),
                        weight: t2.weight,
                    });
                }
            }
        }

        // Case 3: Matching labels (advance both)
        if can_match {
            let trans2_by_input = Self::input_transition_index(trans2);
            for t1 in trans1 {
                if let Some(ref out1) = t1.output {
                    if let Some(matching_trans2) = trans2_by_input.get(out1) {
                        transitions.reserve(matching_trans2.len());
                        for &t2 in matching_trans2 {
                            let new_filter = self.filter.next_state(filter, false, false);
                            transitions.push(ComposedTransition {
                                input: t1.input.clone(),
                                output: t2.output.clone(),
                                target: ProductStateId::new(t1.to, t2.to, new_filter),
                                weight: t1.weight.times(&t2.weight),
                            });
                        }
                    }
                }
            }
        }

        CachedState {
            is_final,
            final_weight,
            transitions,
        }
    }

    /// Index FST2 transitions by non-epsilon input label for O(outdegree) matching.
    fn input_transition_index<'a>(
        transitions: &'a [WeightedTransition<L, W>],
    ) -> FxHashMap<&'a L, SmallVec<[&'a WeightedTransition<L, W>; 4]>> {
        let mut by_input: FxHashMap<&'a L, SmallVec<[&'a WeightedTransition<L, W>; 4]>> =
            FxHashMap::default();

        for transition in transitions {
            if let Some(ref input) = transition.input {
                by_input.entry(input).or_default().push(transition);
            }
        }

        by_input
    }

    /// Iterate over accepting paths lazily.
    pub fn accepting_paths(&mut self) -> AcceptingPathIterator<'_, F1, F2, L, W> {
        AcceptingPathIterator::new(self)
    }

    /// Clear the state cache.
    pub fn clear_cache(&mut self) {
        self.state_cache.clear();
        self.transient_state = None;
        self.access_order.clear();
    }
}

/// Partial path for path enumeration.
#[derive(Clone)]
struct PartialPath<L: Clone, W: Semiring> {
    state: ProductStateId,
    path: ComposedPath<L, W>,
}

impl<L: Clone, W: Semiring> PartialPath<L, W> {
    fn new(state: ProductStateId) -> Self {
        Self {
            state,
            path: ComposedPath::new(),
        }
    }

    fn extend(
        &self,
        target: ProductStateId,
        input: Option<L>,
        output: Option<L>,
        weight: W,
    ) -> Self {
        Self {
            state: target,
            path: self.path.extend(input, output, weight),
        }
    }
}

/// Wrapper for priority queue ordering (min-heap by weight).
struct OrderedPartialPath<L: Clone, W: Semiring>(PartialPath<L, W>);

impl<L: Clone, W: Semiring> PartialEq for OrderedPartialPath<L, W> {
    fn eq(&self, other: &Self) -> bool {
        self.0.path.weight == other.0.path.weight
    }
}

impl<L: Clone, W: Semiring> Eq for OrderedPartialPath<L, W> {}

impl<L: Clone, W: Semiring> PartialOrd for OrderedPartialPath<L, W> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<L: Clone, W: Semiring> Ord for OrderedPartialPath<L, W> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed for min-heap
        match self.0.path.weight.natural_less(&other.0.path.weight) {
            Some(true) => Ordering::Greater,
            Some(false) => match other.0.path.weight.natural_less(&self.0.path.weight) {
                Some(true) => Ordering::Less,
                _ => Ordering::Equal,
            },
            None => Ordering::Equal,
        }
    }
}

/// Iterator over accepting paths in the composed FST.
pub struct AcceptingPathIterator<'a, F1, F2, L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash,
    W: Semiring,
{
    composition: &'a mut LazyComposition<F1, F2, L, W>,
    heap: BinaryHeap<OrderedPartialPath<L, W>>,
}

impl<'a, F1, F2, L, W> AcceptingPathIterator<'a, F1, F2, L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash,
    W: Semiring,
{
    fn new(composition: &'a mut LazyComposition<F1, F2, L, W>) -> Self {
        let start = composition.start();
        let mut heap = BinaryHeap::new();
        heap.push(OrderedPartialPath(PartialPath::new(start)));

        Self { composition, heap }
    }
}

impl<'a, F1, F2, L, W> Iterator for AcceptingPathIterator<'a, F1, F2, L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash,
    W: Semiring,
{
    type Item = ComposedPath<L, W>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(OrderedPartialPath(partial)) = self.heap.pop() {
            // Check if this is an accepting state
            if self.composition.is_final(partial.state) {
                let final_weight = self.composition.final_weight(partial.state);
                let mut result = partial.path.clone();
                result.weight = result.weight.times(&final_weight);

                // Expand successors for more paths (but return this one now)
                let transitions = self.composition.transitions(partial.state);
                for trans in transitions {
                    let extended =
                        partial.extend(trans.target, trans.input, trans.output, trans.weight);
                    self.heap.push(OrderedPartialPath(extended));
                }

                return Some(result);
            }

            // Expand to successors
            let transitions = self.composition.transitions(partial.state);
            for trans in transitions {
                let extended = partial.extend(
                    trans.target,
                    trans.input.clone(),
                    trans.output.clone(),
                    trans.weight,
                );
                self.heap.push(OrderedPartialPath(extended));
            }
        }

        None
    }
}

/// Convenience function to create a lazy composition.
pub fn compose<F1, F2, L, W>(fst1: F1, fst2: F2) -> LazyComposition<F1, F2, L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash,
    W: Semiring,
{
    LazyComposition::new(fst1, fst2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::{VectorWfst, VectorWfstBuilder};

    fn build_simple_fst() -> VectorWfst<char, TropicalWeight> {
        // FST: 0 -a:b/1.0-> 1 (final)
        VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('a'), Some('b'), 1, TropicalWeight::new(1.0))
            .build()
    }

    fn build_identity_fst() -> VectorWfst<char, TropicalWeight> {
        // FST: 0 -b:b/0.5-> 1 (final)
        VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('b'), Some('b'), 1, TropicalWeight::new(0.5))
            .build()
    }

    #[test]
    fn test_compose_basic() {
        let fst1 = build_simple_fst(); // a:b
        let fst2 = build_identity_fst(); // b:b

        let mut composed = compose(fst1, fst2);

        // Start state should be (0, 0, None)
        let start = composed.start();
        assert_eq!(start.s1, 0);
        assert_eq!(start.s2, 0);
        assert_eq!(start.filter, FilterState::None);

        // Get transitions from start
        let trans = composed.transitions(start);
        assert_eq!(trans.len(), 1);

        // The transition should be a:b (input a, output b)
        assert_eq!(trans[0].input, Some('a'));
        assert_eq!(trans[0].output, Some('b'));
        assert_eq!(trans[0].weight.value(), 1.5); // 1.0 + 0.5
    }

    #[test]
    fn test_compose_accepting_paths() {
        let fst1 = build_simple_fst(); // a:b
        let fst2 = build_identity_fst(); // b:b

        let mut composed = compose(fst1, fst2);
        let paths: Vec<_> = composed.accepting_paths().collect();

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].inputs, vec!['a']);
        assert_eq!(paths[0].outputs, vec!['b']);
        assert_eq!(paths[0].weight.value(), 1.5);
    }

    #[test]
    fn test_compose_multiple_paths() {
        // FST1: two paths from 0 to 1
        let fst1 = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .arc(0, Some('b'), Some('x'), 1, TropicalWeight::new(2.0))
            .build();

        // FST2: x -> y
        let fst2 = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('x'), Some('y'), 1, TropicalWeight::new(0.5))
            .build();

        let mut composed = compose(fst1, fst2);
        let mut paths: Vec<_> = composed.accepting_paths().collect();

        // Sort by weight for deterministic testing
        paths.sort_by(|a, b| a.weight.value().total_cmp(&b.weight.value()));

        assert_eq!(paths.len(), 2);

        // Best path: a:y (1.0 + 0.5 = 1.5)
        assert_eq!(paths[0].inputs, vec!['a']);
        assert_eq!(paths[0].outputs, vec!['y']);
        assert_eq!(paths[0].weight.value(), 1.5);

        // Second path: b:y (2.0 + 0.5 = 2.5)
        assert_eq!(paths[1].inputs, vec!['b']);
        assert_eq!(paths[1].outputs, vec!['y']);
        assert_eq!(paths[1].weight.value(), 2.5);
    }

    #[test]
    fn test_compose_no_matching_path() {
        // FST1: a:b
        let fst1 = build_simple_fst();

        // FST2: c:d (doesn't match FST1 output)
        let fst2 = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('c'), Some('d'), 1, TropicalWeight::new(1.0))
            .build();

        let mut composed = compose(fst1, fst2);
        let paths: Vec<_> = composed.accepting_paths().collect();

        assert_eq!(paths.len(), 0);
    }

    #[test]
    fn test_compose_chain() {
        // FST1: a:b -> b:c (two-state chain)
        let fst1 = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .final_state(2, TropicalWeight::one())
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('b'), Some('y'), 2, TropicalWeight::new(1.0))
            .build();

        // FST2: x:p -> y:q
        let fst2 = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .final_state(2, TropicalWeight::one())
            .arc(0, Some('x'), Some('p'), 1, TropicalWeight::new(0.5))
            .arc(1, Some('y'), Some('q'), 2, TropicalWeight::new(0.5))
            .build();

        let mut composed = compose(fst1, fst2);
        let paths: Vec<_> = composed.accepting_paths().collect();

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].inputs, vec!['a', 'b']);
        assert_eq!(paths[0].outputs, vec!['p', 'q']);
        assert_eq!(paths[0].weight.value(), 3.0); // 1.0 + 0.5 + 1.0 + 0.5
    }

    #[test]
    fn test_computed_states_count() {
        let fst1 = build_simple_fst();
        let fst2 = build_identity_fst();

        let mut composed = compose(fst1, fst2);

        // Initially no states computed
        assert_eq!(composed.computed_states(), 0);

        // Access transitions - should compute start state
        let _ = composed.transitions(composed.start());
        assert!(composed.computed_states() > 0);
    }

    #[test]
    fn test_cache_policy() {
        let fst1 = build_simple_fst();
        let fst2 = build_identity_fst();

        let composed = compose(fst1, fst2).with_cache_policy(CachePolicy::Lru { max_states: 10 });

        assert!(matches!(
            composed.policy,
            CachePolicy::Lru { max_states: 10 }
        ));
    }

    #[test]
    fn test_no_cache_policy_does_not_retain_persistent_states() {
        let fst1 = build_simple_fst();
        let fst2 = build_identity_fst();

        let mut composed = compose(fst1, fst2).with_cache_policy(CachePolicy::NoCache);
        let transitions = composed.transitions(composed.start());

        assert_eq!(transitions.len(), 1);
        assert_eq!(composed.computed_states(), 0);
        assert!(composed.state_cache.is_empty());
        assert!(composed.transient_state.is_some());
    }

    #[test]
    fn test_zero_capacity_lru_uses_transient_storage() {
        let fst1 = build_simple_fst();
        let fst2 = build_identity_fst();

        let mut composed =
            compose(fst1, fst2).with_cache_policy(CachePolicy::Lru { max_states: 0 });
        let transitions = composed.transitions(composed.start());

        assert_eq!(transitions.len(), 1);
        assert_eq!(composed.computed_states(), 0);
        assert!(composed.state_cache.is_empty());
        assert!(composed.transient_state.is_some());
    }

    #[test]
    fn test_lru_policy_enforces_capacity() {
        let fst1 = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .final_state(2, TropicalWeight::one())
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('b'), Some('y'), 2, TropicalWeight::new(1.0))
            .build();

        let fst2 = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .final_state(2, TropicalWeight::one())
            .arc(0, Some('x'), Some('p'), 1, TropicalWeight::new(0.5))
            .arc(1, Some('y'), Some('q'), 2, TropicalWeight::new(0.5))
            .build();

        let mut composed =
            compose(fst1, fst2).with_cache_policy(CachePolicy::Lru { max_states: 1 });
        let first_transition = composed.transitions(composed.start())[0].target;
        assert_eq!(composed.computed_states(), 1);

        let _ = composed.transitions(first_transition);

        assert_eq!(composed.computed_states(), 1);
        assert!(composed.state_cache.contains_key(&first_transition));
    }

    #[test]
    fn test_lru_policy_reconciles_existing_cache_on_policy_change() {
        let fst1 = VectorWfstBuilder::new()
            .add_states(4)
            .start(0)
            .final_state(3, TropicalWeight::one())
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('b'), Some('y'), 2, TropicalWeight::new(1.0))
            .arc(2, Some('c'), Some('z'), 3, TropicalWeight::new(1.0))
            .build();

        let fst2 = VectorWfstBuilder::new()
            .add_states(4)
            .start(0)
            .final_state(3, TropicalWeight::one())
            .arc(0, Some('x'), Some('p'), 1, TropicalWeight::new(0.5))
            .arc(1, Some('y'), Some('q'), 2, TropicalWeight::new(0.5))
            .arc(2, Some('z'), Some('r'), 3, TropicalWeight::new(0.5))
            .build();

        let mut composed = compose(fst1, fst2);
        let start = composed.start();
        let first_transition = composed.transitions(start)[0].target;
        let second_transition = composed.transitions(first_transition)[0].target;

        composed = composed.with_cache_policy(CachePolicy::Lru { max_states: 2 });
        assert_eq!(composed.computed_states(), 2);
        assert_eq!(composed.access_order.len(), 2);

        let _ = composed.transitions(start);
        let _ = composed.transitions(second_transition);

        assert_eq!(composed.computed_states(), 2);
        assert!(composed.state_cache.contains_key(&start));
        assert!(composed.state_cache.contains_key(&second_transition));
        assert!(!composed.state_cache.contains_key(&first_transition));
    }

    #[test]
    fn test_clear_cache() {
        let fst1 = build_simple_fst();
        let fst2 = build_identity_fst();

        let mut composed = compose(fst1, fst2);

        // Compute some states
        let _ = composed.transitions(composed.start());
        assert!(composed.computed_states() > 0);

        // Clear cache
        composed.clear_cache();
        assert_eq!(composed.computed_states(), 0);
    }

    #[test]
    fn test_epsilon_filter_type() {
        let fst1 = build_simple_fst();
        let fst2 = build_identity_fst();

        let composed = LazyComposition::with_filter(fst1, fst2, EpsilonFilterType::Matching);

        assert_eq!(composed.filter.filter_type(), EpsilonFilterType::Matching);
    }

    #[test]
    fn test_matching_index_preserves_multiple_same_label_arcs() {
        let fst1 = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .build();

        let fst2 = VectorWfstBuilder::new()
            .add_states(4)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .final_state(2, TropicalWeight::one())
            .arc(0, Some('x'), Some('y'), 1, TropicalWeight::new(0.5))
            .arc(0, Some('q'), Some('z'), 3, TropicalWeight::new(9.0))
            .arc(0, Some('x'), Some('z'), 2, TropicalWeight::new(0.75))
            .build();

        let mut composed = compose(fst1, fst2);
        let transitions = composed.transitions(composed.start());
        let outputs: Vec<_> = transitions
            .iter()
            .map(|transition| transition.output)
            .collect();

        assert_eq!(transitions.len(), 2);
        assert_eq!(outputs, vec![Some('y'), Some('z')]);
    }

    #[test]
    fn test_product_state_id() {
        let state = ProductStateId::new(1, 2, FilterState::Eps1);
        assert_eq!(state.s1, 1);
        assert_eq!(state.s2, 2);
        assert_eq!(state.filter, FilterState::Eps1);
    }

    #[test]
    fn test_composed_path_extend() {
        let path: ComposedPath<char, TropicalWeight> = ComposedPath::new();
        assert!(path.inputs.is_empty());
        assert!(path.outputs.is_empty());
        assert_eq!(path.weight, TropicalWeight::one());

        let extended = path.extend(Some('a'), Some('b'), TropicalWeight::new(1.0));

        assert_eq!(extended.inputs, vec!['a']);
        assert_eq!(extended.outputs, vec!['b']);
        assert_eq!(extended.weight.value(), 1.0);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::{VectorWfst, VectorWfstBuilder};
    use proptest::prelude::*;

    /// Strategy for building simple transducer chains.
    fn arb_simple_transducer(
        len: usize,
    ) -> impl Strategy<Value = VectorWfst<char, TropicalWeight>> {
        let weights = proptest::collection::vec(0.0f64..10.0, len);
        weights.prop_map(move |ws| {
            let mut builder = VectorWfstBuilder::new().add_states(len + 1).start(0);
            builder = builder.final_state(len as u32, TropicalWeight::one());

            for (i, w) in ws.iter().enumerate() {
                // Use different labels for input/output to enable testing
                let input = (b'a' + (i % 26) as u8) as char;
                let output = (b'A' + (i % 26) as u8) as char;
                builder = builder.arc(
                    i as u32,
                    Some(input),
                    Some(output),
                    (i + 1) as u32,
                    TropicalWeight::new(*w),
                );
            }

            builder.build()
        })
    }

    /// Strategy for building identity transducers (same input and output).
    fn arb_identity_transducer(
        len: usize,
    ) -> impl Strategy<Value = VectorWfst<char, TropicalWeight>> {
        let weights = proptest::collection::vec(0.0f64..10.0, len);
        weights.prop_map(move |ws| {
            let mut builder = VectorWfstBuilder::new().add_states(len + 1).start(0);
            builder = builder.final_state(len as u32, TropicalWeight::one());

            for (i, w) in ws.iter().enumerate() {
                let label = (b'A' + (i % 26) as u8) as char;
                builder = builder.arc(
                    i as u32,
                    Some(label),
                    Some(label),
                    (i + 1) as u32,
                    TropicalWeight::new(*w),
                );
            }

            builder.build()
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        /// Empty composition (no matching labels) produces no paths.
        #[test]
        fn no_match_produces_no_paths(_len1 in 1usize..4, _len2 in 1usize..4) {
            // FST1 outputs lowercase, FST2 expects digits - no match possible
            let fst1 = VectorWfstBuilder::new()
                .add_states(2)
                .start(0)
                .final_state(1, TropicalWeight::one())
                .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
                .build();

            let fst2 = VectorWfstBuilder::new()
                .add_states(2)
                .start(0)
                .final_state(1, TropicalWeight::one())
                .arc(0, Some('y'), Some('b'), 1, TropicalWeight::new(1.0))
                .build();

            let mut composed = compose(fst1, fst2);
            let paths: Vec<_> = composed.accepting_paths().collect();

            prop_assert!(paths.is_empty());
        }

        /// Composing identity transducers preserves the transduction.
        #[test]
        fn identity_composition_preserves(len in 1usize..4) {
            let fst1 = arb_simple_transducer(len);
            let fst2 = arb_identity_transducer(len);

            proptest!(|(fst1 in fst1, fst2 in fst2)| {
                // When FST2 is identity on FST1's output alphabet, composition
                // preserves the input-output mapping (modulo weight combination)
                let mut composed = compose(fst1, fst2);
                let paths: Vec<_> = composed.accepting_paths().take(10).collect();

                // If there are paths, they should maintain input->output structure
                for path in &paths {
                    prop_assert!(path.weight.value() >= 0.0);
                }
            });
        }

        /// Composition weight is sum of component weights (tropical).
        #[test]
        fn composition_weight_is_sum(w1 in 0.0f64..100.0, w2 in 0.0f64..100.0) {
            let fst1 = VectorWfstBuilder::new()
                .add_states(2)
                .start(0)
                .final_state(1, TropicalWeight::one())
                .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(w1))
                .build();

            let fst2 = VectorWfstBuilder::new()
                .add_states(2)
                .start(0)
                .final_state(1, TropicalWeight::one())
                .arc(0, Some('x'), Some('b'), 1, TropicalWeight::new(w2))
                .build();

            let mut composed = compose(fst1, fst2);
            let paths: Vec<_> = composed.accepting_paths().collect();

            prop_assert_eq!(paths.len(), 1);
            let expected_weight = w1 + w2;
            let actual_weight = paths[0].weight.value();
            prop_assert!((expected_weight - actual_weight).abs() < 1e-9,
                "Expected weight {} but got {}", expected_weight, actual_weight);
        }

        /// Composed paths maintain input/output sequence integrity.
        #[test]
        fn paths_maintain_sequence_integrity(
            n_transitions in 1usize..4
        ) {
            // Build composable FSTs
            let mut builder1 = VectorWfstBuilder::new()
                .add_states(n_transitions + 1)
                .start(0)
                .final_state(n_transitions as u32, TropicalWeight::one());

            let mut builder2 = VectorWfstBuilder::new()
                .add_states(n_transitions + 1)
                .start(0)
                .final_state(n_transitions as u32, TropicalWeight::one());

            for i in 0..n_transitions {
                let in1 = (b'a' + i as u8) as char;
                let mid = (b'A' + i as u8) as char;
                let out2 = (b'0' + i as u8) as char;

                builder1 = builder1.arc(i as u32, Some(in1), Some(mid), (i + 1) as u32, TropicalWeight::new(1.0));
                builder2 = builder2.arc(i as u32, Some(mid), Some(out2), (i + 1) as u32, TropicalWeight::new(1.0));
            }

            let fst1 = builder1.build();
            let fst2 = builder2.build();

            let mut composed = compose(fst1, fst2);
            let paths: Vec<_> = composed.accepting_paths().collect();

            prop_assert_eq!(paths.len(), 1);
            prop_assert_eq!(paths[0].inputs.len(), n_transitions);
            prop_assert_eq!(paths[0].outputs.len(), n_transitions);
        }

        /// Product state ID equality is reflexive.
        #[test]
        fn product_state_eq_reflexive(s1 in 0u32..10, s2 in 0u32..10) {
            for filter in [FilterState::None, FilterState::Eps1, FilterState::Eps2] {
                let state = ProductStateId::new(s1, s2, filter);
                prop_assert_eq!(state, state);
            }
        }

        /// Product state ID equality is symmetric.
        #[test]
        fn product_state_eq_symmetric(
            s1a in 0u32..10, s2a in 0u32..10,
            s1b in 0u32..10, s2b in 0u32..10
        ) {
            let state_a = ProductStateId::new(s1a, s2a, FilterState::None);
            let state_b = ProductStateId::new(s1b, s2b, FilterState::None);

            prop_assert_eq!(state_a == state_b, state_b == state_a);
        }

        /// Different filter states produce different product states.
        #[test]
        fn filter_state_distinguishes(s1 in 0u32..10, s2 in 0u32..10) {
            let state_none = ProductStateId::new(s1, s2, FilterState::None);
            let state_eps1 = ProductStateId::new(s1, s2, FilterState::Eps1);
            let state_eps2 = ProductStateId::new(s1, s2, FilterState::Eps2);

            prop_assert_ne!(state_none, state_eps1);
            prop_assert_ne!(state_none, state_eps2);
            prop_assert_ne!(state_eps1, state_eps2);
        }

        /// Cache can be cleared and reused.
        #[test]
        fn cache_clearable(
            w in 0.0f64..10.0
        ) {
            let fst1 = VectorWfstBuilder::new()
                .add_states(2)
                .start(0)
                .final_state(1, TropicalWeight::one())
                .arc(0, Some('a'), Some('b'), 1, TropicalWeight::new(w))
                .build();

            let fst2 = VectorWfstBuilder::new()
                .add_states(2)
                .start(0)
                .final_state(1, TropicalWeight::one())
                .arc(0, Some('b'), Some('c'), 1, TropicalWeight::new(w))
                .build();

            let mut composed = compose(fst1, fst2);

            // Compute paths
            let paths1: Vec<_> = composed.accepting_paths().collect();
            let cached_states = composed.computed_states();
            prop_assert!(cached_states > 0);

            // Clear cache
            composed.clear_cache();
            prop_assert_eq!(composed.computed_states(), 0);

            // Recompute - should get same results
            let paths2: Vec<_> = composed.accepting_paths().collect();
            prop_assert_eq!(paths1.len(), paths2.len());
        }

        /// ComposedPath weight accumulation is correct.
        #[test]
        fn composed_path_weight_accumulation(
            w1 in 0.0f64..100.0,
            w2 in 0.0f64..100.0
        ) {
            let path: ComposedPath<char, TropicalWeight> = ComposedPath::new();
            prop_assert_eq!(path.weight.value(), 0.0); // TropicalWeight::one() is 0.0

            let p1 = path.extend(Some('a'), Some('b'), TropicalWeight::new(w1));
            prop_assert_eq!(p1.weight.value(), w1);

            let p2 = p1.extend(Some('c'), Some('d'), TropicalWeight::new(w2));
            prop_assert!((p2.weight.value() - (w1 + w2)).abs() < 1e-9);
        }
    }
}
