//! Materialization of lazy compositions into eager WFSTs.
//!
//! This module provides the [`materialize`] function which converts a lazy
//! composition into an eager [`VectorWfst`] by BFS traversal from the start
//! state, adding all reachable states and transitions.

use std::collections::VecDeque;
use std::hash::Hash;

use rustc_hash::FxHashMap;

use super::fst_fst::{LazyComposition, ProductStateId};
use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst};

/// Materialize a lazy composition into an eager VectorWfst.
///
/// This function performs a BFS traversal from the start state of the lazy
/// composition, converting all reachable product states into concrete states
/// in the resulting VectorWfst.
///
/// # Type Parameters
///
/// * `F1`, `F2` - The component FST types
/// * `L` - Label type (must be Clone + Eq + Hash + Send + Sync)
/// * `W` - Weight type (must implement Semiring)
///
/// # Example
///
/// ```rust,ignore
/// use lling_llang::composition::{compose, materialize};
/// use lling_llang::wfst::VectorWfst;
/// use lling_llang::semiring::TropicalWeight;
///
/// let fst1: VectorWfst<char, TropicalWeight> = /* ... */;
/// let fst2: VectorWfst<char, TropicalWeight> = /* ... */;
///
/// let lazy = compose(fst1, fst2);
/// let eager = materialize(lazy);
///
/// // Now eager is a VectorWfst with all reachable states
/// println!("Materialized {} states", eager.num_states());
/// ```
///
/// # Complexity
///
/// - Time: O(|V| + |E|) where V and E are the reachable states and transitions
/// - Space: O(|V| + |E|) for the resulting VectorWfst plus O(|V|) for the mapping
///
/// # Notes
///
/// This operation converts a potentially unbounded lazy structure into a finite
/// eager structure. For compositions that may produce infinite or very large
/// state spaces, consider using the lazy composition directly with iterators.
pub fn materialize<F1, F2, L, W>(mut lazy: LazyComposition<F1, F2, L, W>) -> VectorWfst<L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring,
{
    let mut result: VectorWfst<L, W> = VectorWfst::new();

    // Map from ProductStateId to StateId in the result
    let mut state_map: FxHashMap<ProductStateId, StateId> = FxHashMap::default();

    // BFS queue
    let mut queue: VecDeque<ProductStateId> = VecDeque::new();

    // Add start state
    let start_product = lazy.start();
    let start_id = result.add_state();
    result.set_start(start_id);
    state_map.insert(start_product, start_id);
    queue.push_back(start_product);

    // BFS traversal
    while let Some(product_state) = queue.pop_front() {
        let Some(&current_id) = state_map.get(&product_state) else {
            continue;
        };

        // Check if this is a final state
        if lazy.is_final(product_state) {
            let final_weight = lazy.final_weight(product_state);
            result.set_final(current_id, final_weight);
        }

        // Get transitions and add them
        let transitions = lazy.transitions(product_state);
        result.reserve_transitions(current_id, transitions.len());
        for trans in transitions {
            // Get or create target state
            let target_id = if let Some(&id) = state_map.get(&trans.target) {
                id
            } else {
                let new_id = result.add_state();
                state_map.insert(trans.target, new_id);
                queue.push_back(trans.target);
                new_id
            };

            // Add the transition. `transitions` is an owned SmallVec (it is
            // cloned out of the composition cache), so `trans` is owned and
            // dropped at the end of this iteration — move its labels in rather
            // than cloning them a second time.
            result.add_arc(
                current_id,
                trans.input,
                trans.output,
                target_id,
                trans.weight,
            );
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::super::fst_fst::compose;
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::VectorWfstBuilder;

    #[test]
    fn test_materialize_simple() {
        // FST1: 0 -a:x/1.0-> 1 (final)
        let fst1 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .build();

        // FST2: 0 -x:b/0.5-> 1 (final)
        let fst2 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('x'), Some('b'), 1, TropicalWeight::new(0.5))
            .build();

        let lazy = compose(fst1, fst2);
        let result = materialize(lazy);

        // Result should have at least start state
        assert!(result.num_states() > 0);
        assert_eq!(result.start(), 0);

        // Should have transitions
        assert!(!result.transitions(0).is_empty());
    }

    #[test]
    fn test_materialize_chain() {
        // FST1: 0 -a:x/1.0-> 1 -b:y/1.0-> 2 (final)
        let fst1 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(3)
            .start(0)
            .final_state(2, TropicalWeight::one())
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('b'), Some('y'), 2, TropicalWeight::new(1.0))
            .build();

        // FST2: 0 -x:p/0.5-> 1 -y:q/0.5-> 2 (final)
        let fst2 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(3)
            .start(0)
            .final_state(2, TropicalWeight::one())
            .arc(0, Some('x'), Some('p'), 1, TropicalWeight::new(0.5))
            .arc(1, Some('y'), Some('q'), 2, TropicalWeight::new(0.5))
            .build();

        let lazy = compose(fst1, fst2);
        let result = materialize(lazy);

        // Should create a chain of states
        assert!(result.num_states() >= 2);

        // Check for final states
        let final_count = (0..result.num_states() as StateId)
            .filter(|&s| result.is_final(s))
            .count();
        assert!(final_count >= 1);
    }

    #[test]
    fn test_materialize_no_match() {
        // FST1: 0 -a:x/1.0-> 1 (final)
        let fst1 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .build();

        // FST2: 0 -z:b/0.5-> 1 (final) - no matching label
        let fst2 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('z'), Some('b'), 1, TropicalWeight::new(0.5))
            .build();

        let lazy = compose(fst1, fst2);
        let result = materialize(lazy);

        // Result should have start state but no final states reachable
        assert!(result.num_states() >= 1);

        // Check no final states (since labels don't match)
        let has_final = (0..result.num_states() as StateId).any(|s| result.is_final(s));
        assert!(!has_final);
    }

    #[test]
    fn test_materialize_multiple_paths() {
        // FST1: two paths from 0 to 1
        let fst1 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .arc(0, Some('b'), Some('x'), 1, TropicalWeight::new(2.0))
            .build();

        // FST2: x -> y
        let fst2 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::one())
            .arc(0, Some('x'), Some('y'), 1, TropicalWeight::new(0.5))
            .build();

        let lazy = compose(fst1, fst2);
        let result = materialize(lazy);

        // Should have multiple transitions from start
        assert!(result.num_states() >= 2);

        // Start state should have 2 outgoing transitions (a:y and b:y)
        assert_eq!(result.transitions(0).len(), 2);
    }

    #[test]
    fn test_materialize_preserves_weights() {
        let fst1 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::new(0.1))
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
            .build();

        let fst2 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(2)
            .start(0)
            .final_state(1, TropicalWeight::new(0.2))
            .arc(0, Some('x'), Some('b'), 1, TropicalWeight::new(0.5))
            .build();

        let lazy = compose(fst1, fst2);
        let result = materialize(lazy);

        // Check arc weight (should be 1.0 + 0.5 = 1.5 in tropical)
        let trans = result.transitions(0);
        assert_eq!(trans.len(), 1);
        assert_eq!(trans[0].weight.value(), 1.5);

        // Check final weight (should be 0.1 + 0.2 = 0.3 in tropical)
        let final_state = trans[0].to;
        assert!(result.is_final(final_state));
        assert!((result.final_weight(final_state).value() - 0.3).abs() < 1e-9);
    }

    #[test]
    fn test_materialize_empty_composition() {
        // FST1 with only start state (final)
        let fst1 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(1)
            .start(0)
            .final_state(0, TropicalWeight::one())
            .build();

        // FST2 with only start state (final)
        let fst2 = VectorWfstBuilder::<char, TropicalWeight>::new()
            .add_states(1)
            .start(0)
            .final_state(0, TropicalWeight::one())
            .build();

        let lazy = compose(fst1, fst2);
        let result = materialize(lazy);

        // Should have exactly one state (the product of two final states)
        assert_eq!(result.num_states(), 1);
        assert!(result.is_final(0));
    }
}
