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

use std::collections::BinaryHeap;
use std::cmp::Ordering;
use std::hash::Hash;
use std::marker::PhantomData;

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::semiring::Semiring;
use crate::wfst::{StateId, Wfst, CachePolicy};
use super::{EpsilonFilter, EpsilonFilterType, FilterState};

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
            filter: EpsilonFilter::new(filter_type),
            policy: CachePolicy::CacheAll,
            start,
            _marker: PhantomData,
        }
    }

    /// Set cache policy.
    pub fn with_cache_policy(mut self, policy: CachePolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Get the start product state.
    pub fn start(&self) -> ProductStateId {
        self.start
    }

    /// Get the number of states computed so far.
    pub fn computed_states(&self) -> usize {
        self.state_cache.len()
    }

    /// Check if a product state is final.
    pub fn is_final(&mut self, state: ProductStateId) -> bool {
        self.ensure_computed(state);
        self.state_cache.get(&state).map(|s| s.is_final).unwrap_or(false)
    }

    /// Get the final weight of a product state.
    pub fn final_weight(&mut self, state: ProductStateId) -> W {
        self.ensure_computed(state);
        self.state_cache
            .get(&state)
            .map(|s| s.final_weight)
            .unwrap_or_else(W::zero)
    }

    /// Get transitions from a product state.
    pub fn transitions(&mut self, state: ProductStateId) -> SmallVec<[ComposedTransition<L, W>; 4]> {
        self.ensure_computed(state);
        self.state_cache
            .get(&state)
            .map(|s| s.transitions.clone())
            .unwrap_or_default()
    }

    /// Ensure a product state is computed and cached.
    fn ensure_computed(&mut self, state: ProductStateId) {
        if self.state_cache.contains_key(&state) {
            return;
        }

        let cached = self.compute_state(state);

        match self.policy {
            CachePolicy::CacheAll => {
                self.state_cache.insert(state, cached);
            }
            CachePolicy::Lru { max_states } => {
                if self.state_cache.len() >= max_states {
                    // Simple eviction: remove oldest entry
                    // For a proper LRU, we'd need access timestamps
                    if let Some(key) = self.state_cache.keys().next().cloned() {
                        self.state_cache.remove(&key);
                    }
                }
                self.state_cache.insert(state, cached);
            }
            CachePolicy::NoCache => {
                // Don't cache - state will be recomputed each time
                self.state_cache.insert(state, cached);
            }
        }
    }

    /// Compute a product state's transitions.
    fn compute_state(&self, state: ProductStateId) -> CachedState<L, W> {
        let ProductStateId { s1, s2, filter } = state;

        // Check if final
        let is_final = self.fst1.is_final(s1) && self.fst2.is_final(s2);
        let final_weight = if is_final {
            self.fst1.final_weight(s1).times(&self.fst2.final_weight(s2))
        } else {
            W::zero()
        };

        // Get transitions from both FSTs
        let trans1 = self.fst1.transitions(s1);
        let trans2 = self.fst2.transitions(s2);

        let (can_eps1, can_eps2, can_match) = self.filter.allowed_moves(filter);

        let mut transitions = SmallVec::new();

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
            for t1 in trans1 {
                if let Some(ref out1) = t1.output {
                    for t2 in trans2 {
                        if let Some(ref in2) = t2.input {
                            if out1 == in2 {
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
        }

        CachedState {
            is_final,
            final_weight,
            transitions,
        }
    }

    /// Iterate over accepting paths lazily.
    pub fn accepting_paths(&mut self) -> AcceptingPathIterator<'_, F1, F2, L, W> {
        AcceptingPathIterator::new(self)
    }

    /// Clear the state cache.
    pub fn clear_cache(&mut self) {
        self.state_cache.clear();
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
            Some(false) => {
                match other.0.path.weight.natural_less(&self.0.path.weight) {
                    Some(true) => Ordering::Less,
                    _ => Ordering::Equal,
                }
            }
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
                    let extended = partial.extend(
                        trans.target,
                        trans.input,
                        trans.output,
                        trans.weight,
                    );
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
        let fst1 = build_simple_fst();      // a:b
        let fst2 = build_identity_fst();    // b:b

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
        let fst1 = build_simple_fst();      // a:b
        let fst2 = build_identity_fst();    // b:b

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
        paths.sort_by(|a, b| {
            a.weight.value().partial_cmp(&b.weight.value()).unwrap()
        });

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

        let composed = compose(fst1, fst2)
            .with_cache_policy(CachePolicy::Lru { max_states: 10 });

        assert!(matches!(composed.policy, CachePolicy::Lru { max_states: 10 }));
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

        let composed = LazyComposition::with_filter(
            fst1, fst2, EpsilonFilterType::Matching
        );

        assert_eq!(composed.filter.filter_type(), EpsilonFilterType::Matching);
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

        let extended = path.extend(
            Some('a'),
            Some('b'),
            TropicalWeight::new(1.0),
        );

        assert_eq!(extended.inputs, vec!['a']);
        assert_eq!(extended.outputs, vec!['b']);
        assert_eq!(extended.weight.value(), 1.0);
    }
}
