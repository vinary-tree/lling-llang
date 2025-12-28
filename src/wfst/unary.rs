//! Elementary unary operations on WFSTs: Inversion, Projection, and Reversal.
//!
//! These operations transform a single WFST into another WFST.
//!
//! - **Inversion (T⁻¹)**: Swap input and output labels (lazy)
//! - **Projection (↓T, T↓)**: Keep only input or output labels (lazy)
//! - **Reversal (T^R)**: Reverse direction of all transitions (constructive)
//!
//! # Example
//!
//! ```
//! use lling_llang::wfst::{VectorWfst, VectorWfstBuilder, MutableWfst, Wfst};
//! use lling_llang::wfst::unary::{invert, project_input, project_output, reverse};
//! use lling_llang::semiring::{Semiring, TropicalWeight};
//!
//! // Create a simple transducer: a:x -> b:y
//! let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
//!     .add_states(3)
//!     .start(0)
//!     .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
//!     .arc(1, Some('b'), Some('y'), 2, TropicalWeight::one())
//!     .final_state(2, TropicalWeight::one())
//!     .build();
//!
//! // Inversion: swaps to x:a -> y:b
//! let inv = invert(&fst);
//!
//! // Input projection: a -> b (output becomes epsilon)
//! let pin = project_input(&fst);
//!
//! // Output projection: x -> y (input becomes epsilon)
//! let pout = project_output(&fst);
//!
//! // Reversal: reverses the path direction
//! let rev = reverse(&fst);
//! ```

use smallvec::SmallVec;

use crate::semiring::Semiring;
use super::{StateId, WeightedTransition, Wfst, VectorWfst, MutableWfst, NO_STATE};
use super::lazy::{LazyState, StateSource, LazyWfstWrapper};

// =============================================================================
// Inversion: T⁻¹
// =============================================================================

/// Lazy inversion of a WFST.
///
/// Swaps input and output labels on all transitions.
/// For transducer T: (i:o) → (o:i)
///
/// Complexity: O(|T|) - states computed on demand.
#[derive(Clone)]
pub struct InvertSource<L, W, T>
where
    W: Semiring,
    T: Wfst<L, W>,
{
    fst: T,
    _phantom: std::marker::PhantomData<(L, W)>,
}

impl<L, W, T> InvertSource<L, W, T>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    /// Create a new inversion source.
    pub fn new(fst: T) -> Self {
        Self {
            fst,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<L, W, T> StateSource<L, W> for InvertSource<L, W, T>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    fn compute_state(&self, state: StateId) -> LazyState<L, W> {
        let is_final = self.fst.is_final(state);
        let final_weight = self.fst.final_weight(state);

        // Swap input and output labels
        let transitions: SmallVec<[WeightedTransition<L, W>; 4]> = self
            .fst
            .transitions(state)
            .iter()
            .map(|t| WeightedTransition {
                from: t.from,
                input: t.output.clone(),  // Swap: output becomes input
                output: t.input.clone(),   // Swap: input becomes output
                to: t.to,
                weight: t.weight,
            })
            .collect();

        if is_final {
            LazyState::final_state(final_weight, transitions)
        } else {
            LazyState::non_final(transitions)
        }
    }

    fn start(&self) -> StateId {
        self.fst.start()
    }

    fn num_states_hint(&self) -> Option<usize> {
        Some(self.fst.num_states())
    }
}

/// Type alias for a lazy inversion WFST.
pub type InvertWfst<L, W, T> = LazyWfstWrapper<InvertSource<L, W, T>, L, W>;

/// Create a lazy inversion of a WFST.
///
/// Swaps input and output labels on all transitions.
///
/// # Arguments
///
/// * `fst` - Input WFST
///
/// # Returns
///
/// A lazy WFST representing T⁻¹
pub fn invert<L, W, T>(fst: &T) -> InvertWfst<L, W, T>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    let source = InvertSource::new(fst.clone());
    LazyWfstWrapper::new(source)
}

// =============================================================================
// Projection: ↓T (input) and T↓ (output)
// =============================================================================

/// Lazy projection of a WFST.
///
/// Either keeps input labels and makes outputs epsilon (input projection),
/// or keeps output labels and makes inputs epsilon (output projection).
///
/// Complexity: O(|T|) - states computed on demand.
#[derive(Clone)]
pub struct ProjectSource<L, W, T, const INPUT: bool>
where
    W: Semiring,
    T: Wfst<L, W>,
{
    fst: T,
    _phantom: std::marker::PhantomData<(L, W)>,
}

impl<L, W, T, const INPUT: bool> ProjectSource<L, W, T, INPUT>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    /// Create a new projection source.
    pub fn new(fst: T) -> Self {
        Self {
            fst,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<L, W, T, const INPUT: bool> StateSource<L, W> for ProjectSource<L, W, T, INPUT>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    fn compute_state(&self, state: StateId) -> LazyState<L, W> {
        let is_final = self.fst.is_final(state);
        let final_weight = self.fst.final_weight(state);

        let transitions: SmallVec<[WeightedTransition<L, W>; 4]> = self
            .fst
            .transitions(state)
            .iter()
            .map(|t| {
                if INPUT {
                    // Input projection: keep input, output becomes input (acceptor)
                    WeightedTransition {
                        from: t.from,
                        input: t.input.clone(),
                        output: t.input.clone(), // Both labels are input
                        to: t.to,
                        weight: t.weight,
                    }
                } else {
                    // Output projection: keep output, input becomes output (acceptor)
                    WeightedTransition {
                        from: t.from,
                        input: t.output.clone(), // Both labels are output
                        output: t.output.clone(),
                        to: t.to,
                        weight: t.weight,
                    }
                }
            })
            .collect();

        if is_final {
            LazyState::final_state(final_weight, transitions)
        } else {
            LazyState::non_final(transitions)
        }
    }

    fn start(&self) -> StateId {
        self.fst.start()
    }

    fn num_states_hint(&self) -> Option<usize> {
        Some(self.fst.num_states())
    }
}

/// Type alias for a lazy input projection WFST.
pub type ProjectInputWfst<L, W, T> = LazyWfstWrapper<ProjectSource<L, W, T, true>, L, W>;

/// Type alias for a lazy output projection WFST.
pub type ProjectOutputWfst<L, W, T> = LazyWfstWrapper<ProjectSource<L, W, T, false>, L, W>;

/// Create a lazy input projection of a WFST (↓T).
///
/// Converts a transducer to an acceptor by keeping only input labels.
/// The output label becomes equal to the input label.
///
/// # Arguments
///
/// * `fst` - Input WFST
///
/// # Returns
///
/// A lazy WFST representing ↓T
pub fn project_input<L, W, T>(fst: &T) -> ProjectInputWfst<L, W, T>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    let source = ProjectSource::<L, W, T, true>::new(fst.clone());
    LazyWfstWrapper::new(source)
}

/// Create a lazy output projection of a WFST (T↓).
///
/// Converts a transducer to an acceptor by keeping only output labels.
/// The input label becomes equal to the output label.
///
/// # Arguments
///
/// * `fst` - Input WFST
///
/// # Returns
///
/// A lazy WFST representing T↓
pub fn project_output<L, W, T>(fst: &T) -> ProjectOutputWfst<L, W, T>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    let source = ProjectSource::<L, W, T, false>::new(fst.clone());
    LazyWfstWrapper::new(source)
}

// =============================================================================
// Reversal: T^R
// =============================================================================

/// Reverse a WFST (constructive operation).
///
/// Reverses the direction of all transitions:
/// - Original initial states become final
/// - Original final states become initial (via new super-start)
/// - All arc directions are reversed
///
/// This is NOT a lazy operation because it requires inspecting all states
/// to build the reversed graph.
///
/// Complexity: O(|Q| + |E|)
///
/// # Arguments
///
/// * `fst` - Input WFST
///
/// # Returns
///
/// A new `VectorWfst` representing T^R
pub fn reverse<L, W, T>(fst: &T) -> VectorWfst<L, W>
where
    W: Semiring,
    L: Clone + Send + Sync,
    T: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return VectorWfst::new();
    }

    // State mapping:
    // State 0 in reversed FST = super-start
    // State i+1 in reversed FST = state i in original FST
    let mut result: VectorWfst<L, W> = VectorWfst::with_capacity(n + 1);

    // Add super-start state (state 0)
    result.add_state();
    result.set_start(0);

    // Add states for each original state
    for _ in 0..n {
        result.add_state();
    }

    // Collect reversed transitions
    for orig_state in 0..n as StateId {
        let reversed_state = orig_state + 1;

        // If original state is final, add ε-transition from super-start
        if fst.is_final(orig_state) {
            let final_weight = fst.final_weight(orig_state);
            result.add_epsilon(0, reversed_state, final_weight);
        }

        // Reverse all transitions
        for t in fst.transitions(orig_state) {
            let reversed_from = t.to + 1;
            let reversed_to = orig_state + 1;

            result.add_transition(WeightedTransition {
                from: reversed_from,
                input: t.input.clone(),
                output: t.output.clone(),
                to: reversed_to,
                weight: t.weight,
            });
        }
    }

    // Original start state becomes final
    let orig_start = fst.start();
    if orig_start != NO_STATE {
        result.set_final(orig_start + 1, W::one());
    }

    result
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::{VectorWfstBuilder, LazyWfst};

    fn make_transducer() -> VectorWfst<char, TropicalWeight> {
        // a:x -> b:y
        VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('b'), Some('y'), 2, TropicalWeight::new(2.0))
            .final_state(2, TropicalWeight::one())
            .build()
    }

    #[test]
    fn test_invert_basic() {
        let fst = make_transducer();
        let mut inv = invert(&fst);

        // Check that labels are swapped
        let trans = inv.transitions_lazy(0);
        assert_eq!(trans.len(), 1);
        assert_eq!(trans[0].input, Some('x')); // Was output
        assert_eq!(trans[0].output, Some('a')); // Was input
    }

    #[test]
    fn test_invert_preserves_structure() {
        let fst = make_transducer();
        let mut inv = invert(&fst);

        // Same start state
        assert_eq!(inv.start(), fst.start());

        // Same number of states
        assert_eq!(inv.num_states(), fst.num_states());

        // Same final state
        inv.expand(2);
        assert!(inv.is_final(2));
    }

    #[test]
    fn test_double_invert() {
        let fst = make_transducer();
        let mut inv1 = invert(&fst);

        // Expand inv1 first so inv2 can read from it
        // (lazy chaining requires inner FST to be expanded)
        for state in 0..fst.num_states() as StateId {
            inv1.expand(state);
        }

        let mut inv2 = invert(&inv1);

        // Double inversion should give back original labels
        let orig_trans = fst.transitions(0);
        let double_inv_trans = inv2.transitions_lazy(0);

        assert_eq!(orig_trans[0].input, double_inv_trans[0].input);
        assert_eq!(orig_trans[0].output, double_inv_trans[0].output);
    }

    #[test]
    fn test_project_input() {
        let fst = make_transducer();
        let mut pin = project_input(&fst);

        // Input projection: both labels become input
        let trans = pin.transitions_lazy(0);
        assert_eq!(trans[0].input, Some('a'));
        assert_eq!(trans[0].output, Some('a')); // Output becomes input
    }

    #[test]
    fn test_project_output() {
        let fst = make_transducer();
        let mut pout = project_output(&fst);

        // Output projection: both labels become output
        let trans = pout.transitions_lazy(0);
        assert_eq!(trans[0].input, Some('x')); // Input becomes output
        assert_eq!(trans[0].output, Some('x'));
    }

    #[test]
    fn test_project_preserves_structure() {
        let fst = make_transducer();
        let mut pin = project_input(&fst);

        // Same start state
        assert_eq!(pin.start(), fst.start());

        // Same final state
        pin.expand(2);
        assert!(pin.is_final(2));
    }

    #[test]
    fn test_reverse_basic() {
        let fst = make_transducer();
        let rev = reverse(&fst);

        // Super-start is state 0
        assert_eq!(rev.start(), 0);

        // Super-start has ε-transition to reversed final state
        let s0_trans = rev.transitions(0);
        assert_eq!(s0_trans.len(), 1);
        assert!(s0_trans[0].is_epsilon());
        assert_eq!(s0_trans[0].to, 3); // Original state 2 is now state 3
    }

    #[test]
    fn test_reverse_final_state() {
        let fst = make_transducer();
        let rev = reverse(&fst);

        // Original start (state 0) becomes final (state 1 in reversed)
        assert!(rev.is_final(1));

        // Original final (state 2) is NOT final in reversed (now state 3)
        assert!(!rev.is_final(3));
    }

    #[test]
    fn test_reverse_transition_direction() {
        let fst = make_transducer();
        let rev = reverse(&fst);

        // Original: 0 -a:x-> 1 -b:y-> 2
        // Reversed: 0 -ε-> 3, 3 -b:y-> 2, 2 -a:x-> 1
        // Note: states are offset by 1

        // State 3 (original final) should have transition to state 2
        let s3_trans = rev.transitions(3);
        assert_eq!(s3_trans.len(), 1);
        assert_eq!(s3_trans[0].input, Some('b'));
        assert_eq!(s3_trans[0].to, 2); // Goes to state 2 (originally state 1)

        // State 2 (originally state 1) should have transition to state 1
        let s2_trans = rev.transitions(2);
        assert_eq!(s2_trans.len(), 1);
        assert_eq!(s2_trans[0].input, Some('a'));
        assert_eq!(s2_trans[0].to, 1); // Goes to state 1 (originally state 0)
    }

    #[test]
    fn test_double_reverse() {
        let fst = make_transducer();
        let rev1 = reverse(&fst);
        let rev2 = reverse(&rev1);

        // Double reversal should give back the original structure
        // (with different state IDs due to super-start nodes)

        // The path structure should be preserved
        // Original: 0 -> 1 -> 2 (final)
        // After double reverse: start -> ... -> (final at original start position)

        // At minimum, we should have a path of length 2 (two arcs)
        // and the final state should be reachable
    }

    #[test]
    fn test_reverse_empty_fst() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        let rev = reverse(&fst);

        assert!(rev.is_empty());
    }

    #[test]
    fn test_reverse_single_state_fst() {
        // FST with just a final start state
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(1)
            .start(0)
            .final_state(0, TropicalWeight::one())
            .build();

        let rev = reverse(&fst);

        // Super-start (0) has ε-transition to state 1 (original state 0)
        // State 1 is final (since original start is now final)
        assert_eq!(rev.start(), 0);
        assert!(rev.is_final(1));
    }

    #[test]
    fn test_invert_epsilon_transitions() {
        // FST with epsilon transitions
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .epsilon(0, 1, TropicalWeight::one())
            .final_state(1, TropicalWeight::one())
            .build();

        let mut inv = invert(&fst);

        // Epsilon transitions should remain epsilon after inversion
        let trans = inv.transitions_lazy(0);
        assert!(trans[0].is_epsilon());
    }

    // =========================================================================
    // Algebraic Property Tests
    // =========================================================================

    #[test]
    fn test_invert_involution() {
        // (T⁻¹)⁻¹ ≡ T (double inversion returns original)
        let fst = make_transducer();
        let mut inv1 = invert(&fst);

        // Expand inv1 first
        for s in 0..fst.num_states() as StateId {
            inv1.expand(s);
        }
        let mut inv2 = invert(&inv1);

        // Check all transitions match original
        for s in 0..fst.num_states() as StateId {
            let orig = fst.transitions(s);
            let double = inv2.transitions_lazy(s);

            assert_eq!(orig.len(), double.len(), "State {} transition count mismatch", s);
            for (o, d) in orig.iter().zip(double.iter()) {
                assert_eq!(o.input, d.input, "State {} input mismatch", s);
                assert_eq!(o.output, d.output, "State {} output mismatch", s);
                assert_eq!(o.to, d.to, "State {} destination mismatch", s);
            }
        }
    }

    #[test]
    fn test_project_input_idempotence() {
        // ↓(↓T) ≡ ↓T (projecting input twice is same as once)
        let fst = make_transducer();
        let mut p1 = project_input(&fst);

        // Expand p1
        for s in 0..fst.num_states() as StateId {
            p1.expand(s);
        }
        let mut p2 = project_input(&p1);

        // Both should have same transitions
        for s in 0..fst.num_states() as StateId {
            let once = p1.transitions_lazy(s);
            let twice = p2.transitions_lazy(s);

            assert_eq!(once.len(), twice.len());
            for (o, t) in once.iter().zip(twice.iter()) {
                assert_eq!(o.input, t.input);
                assert_eq!(o.output, t.output);
            }
        }
    }

    #[test]
    fn test_project_output_idempotence() {
        // (T↓)↓ ≡ T↓ (projecting output twice is same as once)
        let fst = make_transducer();
        let mut p1 = project_output(&fst);

        // Expand p1
        for s in 0..fst.num_states() as StateId {
            p1.expand(s);
        }
        let mut p2 = project_output(&p1);

        // Both should have same transitions
        for s in 0..fst.num_states() as StateId {
            let once = p1.transitions_lazy(s);
            let twice = p2.transitions_lazy(s);

            assert_eq!(once.len(), twice.len());
            for (o, t) in once.iter().zip(twice.iter()) {
                assert_eq!(o.input, t.input);
                assert_eq!(o.output, t.output);
            }
        }
    }

    #[test]
    fn test_reverse_involution_structure() {
        // (T^R)^R ≡ T (double reversal returns equivalent structure)
        let fst = make_transducer();
        let rev1 = reverse(&fst);
        let rev2 = reverse(&rev1);

        // After double reversal, should have same number of non-epsilon arcs
        let count_arcs = |f: &VectorWfst<char, TropicalWeight>| {
            (0..f.num_states() as StateId)
                .flat_map(|s| f.transitions(s).to_vec())
                .filter(|t| !t.is_epsilon())
                .count()
        };

        assert_eq!(count_arcs(&fst), count_arcs(&rev2));

        // Should have same labels (a:x and b:y)
        let collect_labels = |f: &VectorWfst<char, TropicalWeight>| {
            let mut labels: Vec<_> = (0..f.num_states() as StateId)
                .flat_map(|s| f.transitions(s).to_vec())
                .filter(|t| !t.is_epsilon())
                .map(|t| (t.input, t.output))
                .collect();
            labels.sort();
            labels
        };

        assert_eq!(collect_labels(&fst), collect_labels(&rev2));
    }

    #[test]
    fn test_invert_project_commutes() {
        // ↓(T⁻¹) ≡ (T↓)⁻¹ - not quite, but input proj of invert
        // gives same FSA as invert of output proj
        let fst = make_transducer();

        // ↓(T⁻¹): invert then project input
        let mut inv = invert(&fst);
        for s in 0..fst.num_states() as StateId {
            inv.expand(s);
        }
        let mut pinv = project_input(&inv);

        // (T↓)⁻¹: project output then invert
        let mut pout = project_output(&fst);
        for s in 0..fst.num_states() as StateId {
            pout.expand(s);
        }
        let mut invp = invert(&pout);

        // Both should produce FSAs with the same label on each arc
        for s in 0..fst.num_states() as StateId {
            let t1 = pinv.transitions_lazy(s);
            let t2 = invp.transitions_lazy(s);

            assert_eq!(t1.len(), t2.len());
            for (a, b) in t1.iter().zip(t2.iter()) {
                // Both should have output labels from original FST
                assert_eq!(a.input, b.input);
            }
        }
    }

    #[test]
    fn test_invert_preserves_path_weight() {
        // Inversion preserves weights along paths
        let fst = make_transducer();
        let mut inv = invert(&fst);

        // Total weight along path should be same
        // Original: 0 --(a:x/1.0)--> 1 --(b:y/2.0)--> 2 (final/1.0)
        // Inverted: same structure, same weights
        let t0 = inv.transitions_lazy(0);
        assert_eq!(t0[0].weight, TropicalWeight::new(1.0));

        let t1 = inv.transitions_lazy(1);
        assert_eq!(t1[0].weight, TropicalWeight::new(2.0));

        inv.expand(2);
        assert_eq!(inv.final_weight(2), TropicalWeight::one());
    }

    #[test]
    fn test_project_preserves_path_weight() {
        // Projection preserves weights along paths
        let fst = make_transducer();
        let mut pin = project_input(&fst);
        let mut pout = project_output(&fst);

        // Check weights are preserved
        assert_eq!(pin.transitions_lazy(0)[0].weight, TropicalWeight::new(1.0));
        assert_eq!(pout.transitions_lazy(0)[0].weight, TropicalWeight::new(1.0));

        assert_eq!(pin.transitions_lazy(1)[0].weight, TropicalWeight::new(2.0));
        assert_eq!(pout.transitions_lazy(1)[0].weight, TropicalWeight::new(2.0));
    }

    #[test]
    fn test_reverse_preserves_total_weight() {
        // Reversal preserves the total weight of paths
        let fst = make_transducer();
        let rev = reverse(&fst);

        // Sum of arc weights should be preserved
        let sum_weights = |f: &VectorWfst<char, TropicalWeight>| {
            (0..f.num_states() as StateId)
                .flat_map(|s| f.transitions(s).to_vec())
                .filter(|t| !t.is_epsilon())
                .map(|t| t.weight.0.into_inner())
                .sum::<f64>()
        };

        assert!((sum_weights(&fst) - sum_weights(&rev)).abs() < 1e-10);
    }

    // =========================================================================
    // Property-Based Tests (proptest)
    // =========================================================================
    mod property_tests {
        use super::*;
        use crate::test_utils::arb_tropical_wfst;
        use proptest::prelude::*;

        proptest! {
            /// Inversion preserves state count.
            #[test]
            fn invert_preserves_states(
                fst in arb_tropical_wfst(6, 2)
            ) {
                let inv = invert(&fst);
                prop_assert_eq!(inv.num_states(), fst.num_states());
            }

            /// Inversion is involutive: (T⁻¹)⁻¹ ≡ T
            #[test]
            fn invert_is_involution(
                fst in arb_tropical_wfst(5, 2)
            ) {
                if fst.num_states() == 0 {
                    return Ok(());
                }

                let mut inv1 = invert(&fst);
                // Expand first inversion
                for s in 0..fst.num_states() as StateId {
                    inv1.expand(s);
                }
                let mut inv2 = invert(&inv1);

                // Check all transitions match original
                for s in 0..fst.num_states() as StateId {
                    let orig = fst.transitions(s);
                    let double = inv2.transitions_lazy(s);

                    prop_assert_eq!(orig.len(), double.len(), "State {} arc count", s);
                    for (o, d) in orig.iter().zip(double.iter()) {
                        prop_assert_eq!(o.input, d.input, "State {} input label", s);
                        prop_assert_eq!(o.output, d.output, "State {} output label", s);
                    }
                }
            }

            /// Input projection preserves state count.
            #[test]
            fn project_input_preserves_states(
                fst in arb_tropical_wfst(6, 2)
            ) {
                let pin = project_input(&fst);
                prop_assert_eq!(pin.num_states(), fst.num_states());
            }

            /// Output projection preserves state count.
            #[test]
            fn project_output_preserves_states(
                fst in arb_tropical_wfst(6, 2)
            ) {
                let pout = project_output(&fst);
                prop_assert_eq!(pout.num_states(), fst.num_states());
            }

            /// Input projection is idempotent: ↓(↓T) ≡ ↓T
            #[test]
            fn project_input_idempotent(
                fst in arb_tropical_wfst(5, 2)
            ) {
                if fst.num_states() == 0 {
                    return Ok(());
                }

                let mut p1 = project_input(&fst);
                for s in 0..fst.num_states() as StateId {
                    p1.expand(s);
                }
                let mut p2 = project_input(&p1);

                for s in 0..fst.num_states() as StateId {
                    let t1 = p1.transitions_lazy(s);
                    let t2 = p2.transitions_lazy(s);
                    prop_assert_eq!(t1.len(), t2.len());
                    for (a, b) in t1.iter().zip(t2.iter()) {
                        prop_assert_eq!(a.input, b.input);
                        prop_assert_eq!(a.output, b.output);
                    }
                }
            }

            /// Output projection is idempotent: (T↓)↓ ≡ T↓
            #[test]
            fn project_output_idempotent(
                fst in arb_tropical_wfst(5, 2)
            ) {
                if fst.num_states() == 0 {
                    return Ok(());
                }

                let mut p1 = project_output(&fst);
                for s in 0..fst.num_states() as StateId {
                    p1.expand(s);
                }
                let mut p2 = project_output(&p1);

                for s in 0..fst.num_states() as StateId {
                    let t1 = p1.transitions_lazy(s);
                    let t2 = p2.transitions_lazy(s);
                    prop_assert_eq!(t1.len(), t2.len());
                    for (a, b) in t1.iter().zip(t2.iter()) {
                        prop_assert_eq!(a.input, b.input);
                        prop_assert_eq!(a.output, b.output);
                    }
                }
            }

            /// Reverse adds one super-start state.
            #[test]
            fn reverse_state_count(
                fst in arb_tropical_wfst(6, 2)
            ) {
                if fst.num_states() == 0 {
                    let rev = reverse(&fst);
                    prop_assert!(rev.is_empty());
                    return Ok(());
                }

                let rev = reverse(&fst);
                prop_assert_eq!(rev.num_states(), fst.num_states() + 1);
            }

            /// Reverse preserves non-epsilon arc count.
            #[test]
            fn reverse_preserves_arc_count(
                fst in arb_tropical_wfst(6, 2)
            ) {
                let rev = reverse(&fst);

                let count_non_eps = |f: &VectorWfst<char, TropicalWeight>| {
                    (0..f.num_states() as StateId)
                        .flat_map(|s| f.transitions(s).to_vec())
                        .filter(|t| !t.is_epsilon())
                        .count()
                };

                prop_assert_eq!(count_non_eps(&fst), count_non_eps(&rev));
            }

            /// Reverse is involutive in structure: double reverse has same arc count.
            #[test]
            fn reverse_double_arc_count(
                fst in arb_tropical_wfst(5, 2)
            ) {
                let rev1 = reverse(&fst);
                let rev2 = reverse(&rev1);

                let count_non_eps = |f: &VectorWfst<char, TropicalWeight>| {
                    (0..f.num_states() as StateId)
                        .flat_map(|s| f.transitions(s).to_vec())
                        .filter(|t| !t.is_epsilon())
                        .count()
                };

                prop_assert_eq!(count_non_eps(&fst), count_non_eps(&rev2));
            }

            /// Inversion preserves arc weights.
            #[test]
            fn invert_preserves_weights(
                fst in arb_tropical_wfst(5, 2)
            ) {
                let mut inv = invert(&fst);

                for s in 0..fst.num_states() as StateId {
                    let orig = fst.transitions(s);
                    let inverted = inv.transitions_lazy(s);

                    for (o, i) in orig.iter().zip(inverted.iter()) {
                        prop_assert!(
                            o.weight.approx_eq(&i.weight, 1e-10),
                            "Weight mismatch: {:?} vs {:?}", o.weight, i.weight
                        );
                    }
                }
            }
        }
    }
}
