//! Rational operations on WFSTs: Union, Concatenation, and Kleene Closure.
//!
//! These operations form the "rational" part of rational transducers and are
//! fundamental building blocks for constructing complex WFSTs.
//!
//! All operations are lazy (on-demand) - states are computed only when accessed.
//!
//! # Operations
//!
//! - **Union (T₁ ⊕ T₂)**: Accepts strings from either T₁ or T₂
//! - **Concatenation (T₁ ⊗ T₂)**: Accepts strings from T₁ followed by T₂
//! - **Kleene Closure (T*)**: Zero or more repetitions of T
//!
//! # Example
//!
//! ```
//! use lling_llang::wfst::{VectorWfst, VectorWfstBuilder, MutableWfst, Wfst};
//! use lling_llang::wfst::rational::{union, concat, closure};
//! use lling_llang::semiring::{Semiring, TropicalWeight};
//!
//! // Create two simple WFSTs
//! let fst1: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
//!     .add_states(2)
//!     .start(0)
//!     .arc(0, Some('a'), Some('a'), 1, TropicalWeight::one())
//!     .final_state(1, TropicalWeight::one())
//!     .build();
//!
//! let fst2: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
//!     .add_states(2)
//!     .start(0)
//!     .arc(0, Some('b'), Some('b'), 1, TropicalWeight::one())
//!     .final_state(1, TropicalWeight::one())
//!     .build();
//!
//! // Union: accepts "a" or "b"
//! let u = union(&fst1, &fst2);
//!
//! // Concatenation: accepts "ab"
//! let c = concat(&fst1, &fst2);
//!
//! // Closure: accepts "", "a", "aa", "aaa", ...
//! let k = closure(&fst1);
//! ```

use smallvec::SmallVec;

use super::lazy::{LazyState, LazyWfstWrapper, StateSource};
use super::{StateId, WeightedTransition, Wfst};
use crate::semiring::Semiring;

// =============================================================================
// State ID Encoding
// =============================================================================

/// Encodes which FST a state belongs to in union/concat operations.
///
/// State ID layout for Union:
/// - State 0: Super-start state
/// - States 1..=n1: States from T₁ (offset by 1)
/// - States n1+1..=n1+n2: States from T₂ (offset by n1+1)
///
/// State ID layout for Concat:
/// - States 0..n1: States from T₁
/// - States n1..n1+n2: States from T₂ (offset by n1)
///
/// State ID layout for Closure:
/// - State 0: Super-start state (also final)
/// - States 1..=n: States from T (offset by 1)

// =============================================================================
// Union (Sum): T₁ ⊕ T₂
// =============================================================================

/// Lazy union of two WFSTs.
///
/// Creates a new start state with ε-transitions to both input FSTs.
/// Accepts strings from either T₁ or T₂.
///
/// Complexity: O(|T₁| + |T₂|) - states computed on demand.
#[derive(Clone)]
pub struct UnionSource<L, W, T1, T2>
where
    W: Semiring,
    T1: Wfst<L, W>,
    T2: Wfst<L, W>,
{
    fst1: T1,
    fst2: T2,
    /// Number of states in fst1 (for offset calculation)
    n1: usize,
    _phantom: std::marker::PhantomData<(L, W)>,
}

impl<L, W, T1, T2> UnionSource<L, W, T1, T2>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T1: Wfst<L, W>,
    T2: Wfst<L, W>,
{
    /// Create a new union source.
    pub fn new(fst1: T1, fst2: T2) -> Self {
        let n1 = fst1.num_states();
        Self {
            fst1,
            fst2,
            n1,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Decode a union state ID to (fst_index, original_state).
    /// Returns (0, state) for super-start, (1, state) for fst1, (2, state) for fst2.
    #[inline]
    fn decode_state(&self, state: StateId) -> (u8, StateId) {
        if state == 0 {
            (0, 0) // Super-start
        } else if (state as usize) <= self.n1 {
            (1, state - 1) // fst1
        } else {
            (2, state - 1 - self.n1 as StateId) // fst2
        }
    }

    /// Encode a state from fst1 to union state ID.
    #[inline]
    fn encode_fst1(&self, state: StateId) -> StateId {
        state + 1
    }

    /// Encode a state from fst2 to union state ID.
    #[inline]
    fn encode_fst2(&self, state: StateId) -> StateId {
        state + 1 + self.n1 as StateId
    }
}

impl<L, W, T1, T2> StateSource<L, W> for UnionSource<L, W, T1, T2>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T1: Wfst<L, W>,
    T2: Wfst<L, W>,
{
    fn compute_state(&self, state: StateId) -> LazyState<L, W> {
        let (fst_idx, original) = self.decode_state(state);

        match fst_idx {
            0 => {
                // Super-start state: ε-transitions to both FST starts
                let mut transitions = SmallVec::new();

                let start1 = self.fst1.start();
                let start2 = self.fst2.start();

                // ε-transition to fst1 start
                if start1 != super::NO_STATE {
                    transitions.push(WeightedTransition::epsilon(
                        state,
                        self.encode_fst1(start1),
                        W::one(),
                    ));
                }

                // ε-transition to fst2 start
                if start2 != super::NO_STATE {
                    transitions.push(WeightedTransition::epsilon(
                        state,
                        self.encode_fst2(start2),
                        W::one(),
                    ));
                }

                LazyState::non_final(transitions)
            }
            1 => {
                // State from fst1
                let is_final = self.fst1.is_final(original);
                let final_weight = self.fst1.final_weight(original);

                let transitions: SmallVec<[WeightedTransition<L, W>; 4]> = self
                    .fst1
                    .transitions(original)
                    .iter()
                    .map(|t| WeightedTransition {
                        from: state,
                        input: t.input.clone(),
                        output: t.output.clone(),
                        to: self.encode_fst1(t.to),
                        weight: t.weight,
                    })
                    .collect();

                if is_final {
                    LazyState::final_state(final_weight, transitions)
                } else {
                    LazyState::non_final(transitions)
                }
            }
            2 => {
                // State from fst2
                let is_final = self.fst2.is_final(original);
                let final_weight = self.fst2.final_weight(original);

                let transitions: SmallVec<[WeightedTransition<L, W>; 4]> = self
                    .fst2
                    .transitions(original)
                    .iter()
                    .map(|t| WeightedTransition {
                        from: state,
                        input: t.input.clone(),
                        output: t.output.clone(),
                        to: self.encode_fst2(t.to),
                        weight: t.weight,
                    })
                    .collect();

                if is_final {
                    LazyState::final_state(final_weight, transitions)
                } else {
                    LazyState::non_final(transitions)
                }
            }
            _ => unreachable!(),
        }
    }

    fn start(&self) -> StateId {
        0 // Super-start state
    }

    fn num_states_hint(&self) -> Option<usize> {
        Some(1 + self.n1 + self.fst2.num_states())
    }
}

/// Type alias for a lazy union WFST.
pub type UnionWfst<L, W, T1, T2> = LazyWfstWrapper<UnionSource<L, W, T1, T2>, L, W>;

/// Create a lazy union of two WFSTs.
///
/// The resulting WFST accepts strings from either input FST.
///
/// # Arguments
///
/// * `fst1` - First input WFST
/// * `fst2` - Second input WFST
///
/// # Returns
///
/// A lazy WFST representing T₁ ⊕ T₂
pub fn union<L, W, T1, T2>(fst1: &T1, fst2: &T2) -> UnionWfst<L, W, T1, T2>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T1: Wfst<L, W>,
    T2: Wfst<L, W>,
{
    let source = UnionSource::new(fst1.clone(), fst2.clone());
    LazyWfstWrapper::new(source)
}

// =============================================================================
// Concatenation (Product): T₁ ⊗ T₂
// =============================================================================

/// Lazy concatenation of two WFSTs.
///
/// Creates ε-transitions from T₁ final states to T₂ start state.
/// Accepts strings from T₁ followed by strings from T₂.
///
/// Complexity: O(|T₁| + |T₂| + |F₁||I₂|) - states computed on demand.
#[derive(Clone)]
pub struct ConcatSource<L, W, T1, T2>
where
    W: Semiring,
    T1: Wfst<L, W>,
    T2: Wfst<L, W>,
{
    fst1: T1,
    fst2: T2,
    /// Number of states in fst1 (for offset calculation)
    n1: usize,
    _phantom: std::marker::PhantomData<(L, W)>,
}

impl<L, W, T1, T2> ConcatSource<L, W, T1, T2>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T1: Wfst<L, W>,
    T2: Wfst<L, W>,
{
    /// Create a new concatenation source.
    pub fn new(fst1: T1, fst2: T2) -> Self {
        let n1 = fst1.num_states();
        Self {
            fst1,
            fst2,
            n1,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Check if state is from fst1.
    #[inline]
    fn is_fst1_state(&self, state: StateId) -> bool {
        (state as usize) < self.n1
    }

    /// Decode a state to (is_fst1, original_state).
    #[inline]
    fn decode_state(&self, state: StateId) -> (bool, StateId) {
        if self.is_fst1_state(state) {
            (true, state)
        } else {
            (false, state - self.n1 as StateId)
        }
    }

    /// Encode a state from fst2 to concat state ID.
    #[inline]
    fn encode_fst2(&self, state: StateId) -> StateId {
        state + self.n1 as StateId
    }
}

impl<L, W, T1, T2> StateSource<L, W> for ConcatSource<L, W, T1, T2>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T1: Wfst<L, W>,
    T2: Wfst<L, W>,
{
    fn compute_state(&self, state: StateId) -> LazyState<L, W> {
        let (is_fst1, original) = self.decode_state(state);

        if is_fst1 {
            // State from fst1
            let is_final_in_fst1 = self.fst1.is_final(original);
            let final_weight_fst1 = self.fst1.final_weight(original);

            let mut transitions: SmallVec<[WeightedTransition<L, W>; 4]> = self
                .fst1
                .transitions(original)
                .iter()
                .map(|t| WeightedTransition {
                    from: state,
                    input: t.input.clone(),
                    output: t.output.clone(),
                    to: t.to, // fst1 states keep their IDs
                    weight: t.weight,
                })
                .collect();

            // If this is a final state in fst1, add ε-transition to fst2 start
            if is_final_in_fst1 {
                let start2 = self.fst2.start();
                if start2 != super::NO_STATE {
                    transitions.push(WeightedTransition::epsilon(
                        state,
                        self.encode_fst2(start2),
                        final_weight_fst1,
                    ));
                }
            }

            // fst1 states are NOT final in the concatenation
            // (unless fst2 is empty with an accepting start state)
            LazyState::non_final(transitions)
        } else {
            // State from fst2
            let is_final = self.fst2.is_final(original);
            let final_weight = self.fst2.final_weight(original);

            let transitions: SmallVec<[WeightedTransition<L, W>; 4]> = self
                .fst2
                .transitions(original)
                .iter()
                .map(|t| WeightedTransition {
                    from: state,
                    input: t.input.clone(),
                    output: t.output.clone(),
                    to: self.encode_fst2(t.to),
                    weight: t.weight,
                })
                .collect();

            if is_final {
                LazyState::final_state(final_weight, transitions)
            } else {
                LazyState::non_final(transitions)
            }
        }
    }

    fn start(&self) -> StateId {
        self.fst1.start()
    }

    fn num_states_hint(&self) -> Option<usize> {
        Some(self.n1 + self.fst2.num_states())
    }
}

/// Type alias for a lazy concatenation WFST.
pub type ConcatWfst<L, W, T1, T2> = LazyWfstWrapper<ConcatSource<L, W, T1, T2>, L, W>;

/// Create a lazy concatenation of two WFSTs.
///
/// The resulting WFST accepts strings from the first FST followed by
/// strings from the second FST.
///
/// # Arguments
///
/// * `fst1` - First input WFST (prefix)
/// * `fst2` - Second input WFST (suffix)
///
/// # Returns
///
/// A lazy WFST representing T₁ ⊗ T₂
pub fn concat<L, W, T1, T2>(fst1: &T1, fst2: &T2) -> ConcatWfst<L, W, T1, T2>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T1: Wfst<L, W>,
    T2: Wfst<L, W>,
{
    let source = ConcatSource::new(fst1.clone(), fst2.clone());
    LazyWfstWrapper::new(source)
}

// =============================================================================
// Kleene Closure: T*
// =============================================================================

/// Lazy Kleene closure of a WFST.
///
/// Creates a new start state (also final) with ε-transition to T start,
/// and ε-transitions from T final states back to T start.
/// Accepts zero or more repetitions of strings from T.
///
/// Complexity: O(|T|) - states computed on demand.
#[derive(Clone)]
pub struct ClosureSource<L, W, T>
where
    W: Semiring,
    T: Wfst<L, W>,
{
    fst: T,
    /// Number of states in the original FST
    n: usize,
    _phantom: std::marker::PhantomData<(L, W)>,
}

impl<L, W, T> ClosureSource<L, W, T>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    /// Create a new closure source.
    pub fn new(fst: T) -> Self {
        let n = fst.num_states();
        Self {
            fst,
            n,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Check if state is the super-start.
    #[inline]
    fn is_super_start(&self, state: StateId) -> bool {
        state == 0
    }

    /// Decode a closure state ID to original state ID.
    /// Super-start is state 0, original states are offset by 1.
    #[inline]
    fn decode_state(&self, state: StateId) -> StateId {
        state - 1
    }

    /// Encode an original state ID to closure state ID.
    #[inline]
    fn encode_state(&self, state: StateId) -> StateId {
        state + 1
    }
}

impl<L, W, T> StateSource<L, W> for ClosureSource<L, W, T>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    fn compute_state(&self, state: StateId) -> LazyState<L, W> {
        if self.is_super_start(state) {
            // Super-start state: final with weight 1̄, ε-transition to FST start
            let mut transitions = SmallVec::new();

            let fst_start = self.fst.start();
            if fst_start != super::NO_STATE {
                transitions.push(WeightedTransition::epsilon(
                    state,
                    self.encode_state(fst_start),
                    W::one(),
                ));
            }

            // Super-start is final with weight one (accepts empty string)
            LazyState::final_state(W::one(), transitions)
        } else {
            // State from original FST
            let original = self.decode_state(state);
            let is_final = self.fst.is_final(original);
            let final_weight = self.fst.final_weight(original);

            let mut transitions: SmallVec<[WeightedTransition<L, W>; 4]> = self
                .fst
                .transitions(original)
                .iter()
                .map(|t| WeightedTransition {
                    from: state,
                    input: t.input.clone(),
                    output: t.output.clone(),
                    to: self.encode_state(t.to),
                    weight: t.weight,
                })
                .collect();

            // If this is a final state, add ε-transition back to FST start
            // (for the closure/repetition)
            if is_final {
                let fst_start = self.fst.start();
                if fst_start != super::NO_STATE {
                    transitions.push(WeightedTransition::epsilon(
                        state,
                        self.encode_state(fst_start),
                        final_weight,
                    ));
                }
            }

            // Final states in closure are still final (can exit after any repetition)
            if is_final {
                LazyState::final_state(final_weight, transitions)
            } else {
                LazyState::non_final(transitions)
            }
        }
    }

    fn start(&self) -> StateId {
        0 // Super-start state
    }

    fn num_states_hint(&self) -> Option<usize> {
        Some(1 + self.n)
    }
}

/// Type alias for a lazy closure WFST.
pub type ClosureWfst<L, W, T> = LazyWfstWrapper<ClosureSource<L, W, T>, L, W>;

/// Create a lazy Kleene closure of a WFST.
///
/// The resulting WFST accepts zero or more repetitions of strings
/// from the input FST.
///
/// # Arguments
///
/// * `fst` - Input WFST
///
/// # Returns
///
/// A lazy WFST representing T*
pub fn closure<L, W, T>(fst: &T) -> ClosureWfst<L, W, T>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    let source = ClosureSource::new(fst.clone());
    LazyWfstWrapper::new(source)
}

/// Create a lazy Kleene plus (T⁺) of a WFST.
///
/// Equivalent to T ⊗ T* - accepts one or more repetitions.
///
/// This is implemented as concatenation of T with closure of T.
pub fn closure_plus<L, W, T>(fst: &T) -> ConcatWfst<L, W, T, ClosureWfst<L, W, T>>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    concat(fst, &closure(fst))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::{LazyWfst, VectorWfst, VectorWfstBuilder};

    fn make_single_arc_fst(label: char) -> VectorWfst<char, TropicalWeight> {
        VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some(label), Some(label), 1, TropicalWeight::one())
            .final_state(1, TropicalWeight::one())
            .build()
    }

    #[test]
    fn test_union_basic() {
        let fst_a = make_single_arc_fst('a');
        let fst_b = make_single_arc_fst('b');

        let mut u = union(&fst_a, &fst_b);

        // Super-start state
        assert_eq!(u.start(), 0);

        // Expand start state
        let start_trans = u.transitions_lazy(0);
        assert_eq!(start_trans.len(), 2); // ε to fst1 start, ε to fst2 start

        // Both transitions should be epsilon
        assert!(start_trans[0].is_epsilon());
        assert!(start_trans[1].is_epsilon());

        // Check that we have the right number of states
        assert_eq!(u.num_states(), 5); // 1 super-start + 2 from fst_a + 2 from fst_b
    }

    #[test]
    fn test_union_final_states() {
        let fst_a = make_single_arc_fst('a');
        let fst_b = make_single_arc_fst('b');

        let mut u = union(&fst_a, &fst_b);

        // Expand all states
        for i in 0..5 {
            u.expand(i);
        }

        // State 2 (fst1's final state, offset by 1) should be final
        assert!(u.is_final(2));
        // State 4 (fst2's final state, offset by 3) should be final
        assert!(u.is_final(4));
        // Super-start should not be final
        assert!(!u.is_final(0));
    }

    #[test]
    fn test_concat_basic() {
        let fst_a = make_single_arc_fst('a');
        let fst_b = make_single_arc_fst('b');

        let mut c = concat(&fst_a, &fst_b);

        // Start is fst1's start
        assert_eq!(c.start(), 0);

        // State 0 should have 'a' transition
        let s0_trans = c.transitions_lazy(0);
        assert_eq!(s0_trans.len(), 1);
        assert_eq!(s0_trans[0].input, Some('a'));

        // State 1 (fst1's final) should have ε-transition to fst2's start
        let s1_trans = c.transitions_lazy(1);
        assert!(s1_trans.iter().any(|t| t.is_epsilon()));
    }

    #[test]
    fn test_concat_final_states() {
        let fst_a = make_single_arc_fst('a');
        let fst_b = make_single_arc_fst('b');

        let mut c = concat(&fst_a, &fst_b);

        // Expand all states
        for i in 0..4 {
            c.expand(i);
        }

        // Only fst2's final state should be final in the concatenation
        assert!(!c.is_final(0)); // fst1 start
        assert!(!c.is_final(1)); // fst1 final (NOT final in concat)
        assert!(!c.is_final(2)); // fst2 start
        assert!(c.is_final(3)); // fst2 final (IS final in concat)
    }

    #[test]
    fn test_closure_basic() {
        let fst_a = make_single_arc_fst('a');

        let mut k = closure(&fst_a);

        // Start is super-start (state 0)
        assert_eq!(k.start(), 0);

        // Super-start should be final (accepts empty string)
        k.expand(0);
        assert!(k.is_final(0));

        // Super-start should have ε-transition to FST start
        let s0_trans = k.transitions_lazy(0);
        assert_eq!(s0_trans.len(), 1);
        assert!(s0_trans[0].is_epsilon());
    }

    #[test]
    fn test_closure_loop_back() {
        let fst_a = make_single_arc_fst('a');

        let mut k = closure(&fst_a);

        // Expand the final state (state 2 = original state 1 + 1)
        let s2_trans = k.transitions_lazy(2);

        // Should have ε-transition back to FST start (for repetition)
        assert!(s2_trans.iter().any(|t| t.is_epsilon() && t.to == 1));
    }

    #[test]
    fn test_closure_plus() {
        let fst_a = make_single_arc_fst('a');

        let mut kp = closure_plus(&fst_a);

        // Start should be fst_a's start (not a super-start that accepts empty)
        assert_eq!(kp.start(), 0);

        // Expand start
        kp.expand(0);

        // Start should NOT be final (closure_plus doesn't accept empty)
        assert!(!kp.is_final(0));
    }

    #[test]
    fn test_empty_fst_union() {
        let empty: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        let fst_a = make_single_arc_fst('a');

        let mut u = union(&empty, &fst_a);

        // Should still work - super-start has ε-transition to fst_a
        let start_trans = u.transitions_lazy(0);
        // Only one ε-transition (to fst_a, not to empty fst which has NO_STATE start)
        assert_eq!(start_trans.len(), 1);
    }

    // =========================================================================
    // Algebraic Property Tests
    // =========================================================================

    #[test]
    fn test_union_commutativity_structure() {
        // T₁ ⊕ T₂ and T₂ ⊕ T₁ should have the same structure
        // (though state ordering may differ)
        let fst_a = make_single_arc_fst('a');
        let fst_b = make_single_arc_fst('b');

        let mut u1 = union(&fst_a, &fst_b);
        let mut u2 = union(&fst_b, &fst_a);

        // Both should have same number of states
        assert_eq!(u1.num_states(), u2.num_states());

        // Both super-starts should have 2 epsilon transitions
        let u1_trans = u1.transitions_lazy(0);
        let u2_trans = u2.transitions_lazy(0);
        assert_eq!(u1_trans.len(), u2_trans.len());

        // Both should have same number of final states
        let u1_finals: Vec<_> = (0..u1.num_states() as StateId)
            .filter(|&s| {
                u1.expand(s);
                u1.is_final(s)
            })
            .collect();
        let u2_finals: Vec<_> = (0..u2.num_states() as StateId)
            .filter(|&s| {
                u2.expand(s);
                u2.is_final(s)
            })
            .collect();
        assert_eq!(u1_finals.len(), u2_finals.len());
    }

    #[test]
    fn test_union_associativity_states() {
        // (T₁ ⊕ T₂) ⊕ T₃ and T₁ ⊕ (T₂ ⊕ T₃) should have equivalent paths
        let fst_a = make_single_arc_fst('a');
        let fst_b = make_single_arc_fst('b');
        let fst_c = make_single_arc_fst('c');

        // Helper to count final states in any lazy FST
        fn count_finals<S>(fst: &mut LazyWfstWrapper<S, char, TropicalWeight>) -> usize
        where
            S: StateSource<char, TropicalWeight> + Send + Sync,
        {
            let n = fst.num_states();
            (0..n as StateId)
                .filter(|&s| {
                    fst.expand(s);
                    fst.is_final(s)
                })
                .count()
        }

        // (T₁ ⊕ T₂) ⊕ T₃
        let mut u12 = union(&fst_a, &fst_b);
        // Expand u12 so we can use it in another union
        for s in 0..u12.num_states() as StateId {
            u12.expand(s);
        }
        let mut u12_3 = union(&u12, &fst_c);

        // T₁ ⊕ (T₂ ⊕ T₃)
        let mut u23 = union(&fst_b, &fst_c);
        for s in 0..u23.num_states() as StateId {
            u23.expand(s);
        }
        let mut u1_23 = union(&fst_a, &u23);

        // Each original FST contributes 1 final state
        assert_eq!(count_finals(&mut u12_3), 3);
        assert_eq!(count_finals(&mut u1_23), 3);
    }

    #[test]
    fn test_concat_associativity_path_length() {
        // (T₁ ⊗ T₂) ⊗ T₃ and T₁ ⊗ (T₂ ⊗ T₃) should have same path structure
        let fst_a = make_single_arc_fst('a');
        let fst_b = make_single_arc_fst('b');
        let fst_c = make_single_arc_fst('c');

        // Helper to count non-epsilon arcs in any lazy FST
        fn count_arcs<S>(fst: &mut LazyWfstWrapper<S, char, TropicalWeight>) -> usize
        where
            S: StateSource<char, TropicalWeight> + Send + Sync,
        {
            let n = fst.num_states();
            (0..n as StateId)
                .flat_map(|s| fst.transitions_lazy(s).to_vec())
                .filter(|t| !t.is_epsilon())
                .count()
        }

        // (T₁ ⊗ T₂) ⊗ T₃
        let mut c12 = concat(&fst_a, &fst_b);
        // Expand c12 so it can be read by the outer concat
        for s in 0..c12.num_states() as StateId {
            c12.expand(s);
        }
        let mut c12_3 = concat(&c12, &fst_c);

        // T₁ ⊗ (T₂ ⊗ T₃)
        let mut c23 = concat(&fst_b, &fst_c);
        // Expand c23 so it can be read by the outer concat
        for s in 0..c23.num_states() as StateId {
            c23.expand(s);
        }
        let mut c1_23 = concat(&fst_a, &c23);

        // Each FST has 1 non-epsilon arc, so total = 3
        assert_eq!(count_arcs(&mut c12_3), 3);
        assert_eq!(count_arcs(&mut c1_23), 3);
    }

    #[test]
    fn test_closure_idempotence() {
        // (T*)* should behave like T* (both accept any repetition including empty)
        let fst_a = make_single_arc_fst('a');

        let mut k = closure(&fst_a);
        // Expand all states of k so we can use it in another closure
        for s in 0..k.num_states() as StateId {
            k.expand(s);
        }
        let mut kk = closure(&k);

        // Both should accept empty string (final at start)
        k.expand(0);
        kk.expand(0);
        assert!(k.is_final(0));
        assert!(kk.is_final(0));
    }

    #[test]
    fn test_union_identity() {
        // T ⊕ ∅ ≡ T (union with empty FST)
        let fst_a = make_single_arc_fst('a');
        let empty: VectorWfst<char, TropicalWeight> = VectorWfst::new();

        let mut u = union(&fst_a, &empty);

        // Should have the paths of fst_a
        // Super-start has 1 valid epsilon transition (empty FST contributes nothing)
        let start_trans = u.transitions_lazy(0);
        assert_eq!(start_trans.len(), 1); // Only to fst_a

        // Should have same final state behavior
        u.expand(2); // State 2 = fst_a's final state (offset by 1)
        assert!(u.is_final(2));
    }

    #[test]
    fn test_concat_with_closure_distributivity() {
        // T ⊗ T* includes T⁺ (one or more repetitions)
        let fst_a = make_single_arc_fst('a');

        let mut k = closure(&fst_a);
        // Expand closure so concat can read from it
        for s in 0..k.num_states() as StateId {
            k.expand(s);
        }
        let mut c = concat(&fst_a, &k);

        // Start should NOT be final (first 'a' is required)
        c.expand(0);
        assert!(!c.is_final(0));

        // But there should be a path to a final state
        // The structure: fst_a (non-final) -> ε -> closure start (final)
        // After going through 'a', we reach fst_a's final state
        // which connects via epsilon to closure start (also final)
        let n = c.num_states();
        let has_final = (0..n as StateId).any(|s| {
            c.expand(s);
            c.is_final(s)
        });
        assert!(has_final);
    }

    // =========================================================================
    // Property-Based Tests (proptest)
    // =========================================================================
    mod property_tests {
        use super::*;
        use crate::test_utils::arb_tropical_wfst;
        use crate::wfst::NO_STATE;
        use proptest::prelude::*;

        proptest! {
            /// Union should have correct state count: 1 + |T₁| + |T₂|.
            #[test]
            fn union_state_count(
                fst1 in arb_tropical_wfst(5, 2),
                fst2 in arb_tropical_wfst(5, 2)
            ) {
                let u = union(&fst1, &fst2);
                let expected = 1 + fst1.num_states() + fst2.num_states();
                prop_assert_eq!(u.num_states(), expected);
            }

            /// Union with empty FST should preserve the other FST's structure.
            #[test]
            fn union_identity_with_empty(
                fst in arb_tropical_wfst(5, 2)
            ) {
                let empty: VectorWfst<char, TropicalWeight> = VectorWfst::new();
                let mut u = union(&fst, &empty);

                // Super-start should have only 1 epsilon transition (to fst, not empty)
                if fst.start() != NO_STATE {
                    let trans = u.transitions_lazy(0);
                    prop_assert_eq!(trans.len(), 1, "Union with empty should have 1 epsilon");
                }
            }

            /// Concatenation should have state count: |T₁| + |T₂|.
            #[test]
            fn concat_state_count(
                fst1 in arb_tropical_wfst(5, 2),
                fst2 in arb_tropical_wfst(5, 2)
            ) {
                let c = concat(&fst1, &fst2);
                let expected = fst1.num_states() + fst2.num_states();
                prop_assert_eq!(c.num_states(), expected);
            }

            /// Closure should have state count: 1 + |T|.
            #[test]
            fn closure_state_count(
                fst in arb_tropical_wfst(5, 2)
            ) {
                let k = closure(&fst);
                let expected = 1 + fst.num_states();
                prop_assert_eq!(k.num_states(), expected);
            }

            /// Closure always accepts empty string (super-start is final).
            #[test]
            fn closure_accepts_empty(
                fst in arb_tropical_wfst(5, 2)
            ) {
                let mut k = closure(&fst);
                k.expand(0);
                prop_assert!(k.is_final(0), "Closure super-start should be final");
            }

            /// Closure plus start is NEVER final because it's concat(fst, closure(fst))
            /// and concat makes fst1 states non-final.
            #[test]
            fn closure_plus_start_not_final(
                fst in arb_tropical_wfst(5, 2)
            ) {
                if fst.num_states() == 0 || fst.start() == NO_STATE {
                    return Ok(());
                }

                let mut kp = closure_plus(&fst);
                kp.expand(0);

                // closure_plus = concat(fst, closure(fst))
                // In concat, fst1 states are NEVER final (they have epsilon to fst2)
                prop_assert!(
                    !kp.is_final(0),
                    "Closure+ start should never be final (concat makes fst1 non-final)"
                );
            }

            /// Union preserves final state count (sum of both FSTs' finals).
            #[test]
            fn union_preserves_finals(
                fst1 in arb_tropical_wfst(5, 2),
                fst2 in arb_tropical_wfst(5, 2)
            ) {
                let mut u = union(&fst1, &fst2);

                // Count finals in original FSTs
                let finals1: usize = (0..fst1.num_states() as StateId)
                    .filter(|&s| fst1.is_final(s))
                    .count();
                let finals2: usize = (0..fst2.num_states() as StateId)
                    .filter(|&s| fst2.is_final(s))
                    .count();

                // Count finals in union (excluding super-start which is not final)
                let union_finals: usize = (0..u.num_states() as StateId)
                    .filter(|&s| {
                        u.expand(s);
                        u.is_final(s)
                    })
                    .count();

                prop_assert_eq!(union_finals, finals1 + finals2);
            }
        }
    }
}
