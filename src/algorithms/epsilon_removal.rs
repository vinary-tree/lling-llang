//! Epsilon removal algorithm for WFSTs.
//!
//! Epsilon removal eliminates epsilon (ε) transitions from a WFST while
//! preserving the weighted language. This is essential for:
//!
//! - **Determinization**: Most determinization algorithms require ε-free input
//! - **Composition**: Simplified composition without epsilon filter
//! - **Decoding**: Direct label matching without epsilon handling
//!
//! # Algorithm
//!
//! The algorithm computes the ε-closure for each state (all states reachable
//! via ε-transitions with accumulated weights), then adds direct transitions
//! that bypass ε-transitions.
//!
//! For a transition `p --a:b/w--> q` and ε-closure entry `(q, r, w')`:
//! - Add new transition `p --a:b/(w ⊗ w')--> r`
//!
//! # Complexity
//!
//! - **Acyclic**: O(|Q|² + |Q||E|(T⊕ + T⊗))
//! - **General (complete semiring)**: O(|Q|³(T⊕ + T⊗ + T*) + |Q||E|(T⊕ + T⊗))
//!
//! # References
//!
//! - Mohri, M. (2009). "Weighted Automata Algorithms"

use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::semiring::{Semiring, StarSemiring};
use crate::wfst::{MutableWfst, StateId, WeightedTransition, Wfst, NO_STATE};

use super::connect::{connect, ConnectConfig};
use super::shortest_distance::ShortestDistanceConfig;

/// Configuration for epsilon removal.
#[derive(Clone, Debug)]
pub struct EpsilonRemovalConfig {
    /// Whether to remove unreachable states after ε-removal.
    pub connect: bool,
    /// Shortest-distance configuration for ε-closure computation.
    pub distance_config: ShortestDistanceConfig,
}

impl Default for EpsilonRemovalConfig {
    fn default() -> Self {
        Self {
            connect: true,
            distance_config: ShortestDistanceConfig::default(),
        }
    }
}

impl EpsilonRemovalConfig {
    /// Create a configuration for acyclic graphs.
    pub fn acyclic() -> Self {
        Self {
            connect: true,
            distance_config: ShortestDistanceConfig::acyclic(),
        }
    }
}

/// Remove epsilon transitions from a WFST.
///
/// This operation modifies the WFST in place, removing all epsilon transitions
/// while preserving the weighted language (set of accepted strings with weights).
///
/// # Type Parameters
///
/// - `L`: Label type
/// - `W`: Weight type (must implement [`Semiring`])
/// - `F`: WFST type
///
/// # Returns
///
/// - `Ok(())` if epsilon removal succeeds
/// - `Err(EpsilonRemovalError)` if removal fails
///
/// # Example
///
/// ```ignore
/// use lling_llang::algorithms::{remove_epsilon, EpsilonRemovalConfig};
///
/// let mut fst = build_some_wfst();
/// remove_epsilon(&mut fst, EpsilonRemovalConfig::default())?;
/// ```
pub fn remove_epsilon<L, W, F>(
    fst: &mut F,
    config: EpsilonRemovalConfig,
) -> Result<(), EpsilonRemovalError>
where
    L: Clone + PartialEq,
    W: Semiring,
    F: MutableWfst<L, W> + Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(());
    }

    if fst.start() == NO_STATE {
        return Err(EpsilonRemovalError::NoStartState);
    }

    let closures = compute_epsilon_closures(fst)?;
    apply_epsilon_removal(fst, config, closures)
}

fn apply_epsilon_removal<L, W, F>(
    fst: &mut F,
    config: EpsilonRemovalConfig,
    closures: Vec<HashMap<StateId, W>>,
) -> Result<(), EpsilonRemovalError>
where
    L: Clone + PartialEq,
    W: Semiring,
    F: MutableWfst<L, W> + Wfst<L, W>,
{
    let n = fst.num_states();
    // Collect new transitions. Pre-size each state's bucket to the exact
    // pre-deduplication expansion count so dense closures do not repeatedly grow.
    let mut new_transitions: Vec<Vec<WeightedTransition<L, W>>> = (0..n)
        .map(|state| {
            let state_id = state as StateId;
            Vec::with_capacity(expanded_transition_capacity(fst, &closures, state_id))
        })
        .collect();

    for state in 0..n {
        let state_id = state as StateId;
        let Some(source_closure) = closures.get(state) else {
            continue;
        };

        for (closure_state, closure_weight) in source_closure {
            if *closure_state as usize >= n {
                continue;
            }

            for trans in fst.transitions(*closure_state) {
                if trans.input.is_none() && trans.output.is_none() {
                    continue;
                }

                let Some(to_closure) = closures.get(trans.to as usize) else {
                    continue;
                };

                for (dest_state, dest_weight) in to_closure {
                    if *dest_state as usize >= n {
                        continue;
                    }

                    let new_weight = closure_weight.times(&trans.weight).times(dest_weight);
                    new_transitions[state].push(WeightedTransition {
                        from: state_id,
                        to: *dest_state,
                        input: trans.input.clone(),
                        output: trans.output.clone(),
                        weight: new_weight,
                    });
                }
            }
        }
    }

    // Deduplicate transitions (combine weights for identical transitions)
    for state_trans in &mut new_transitions {
        deduplicate_transitions(state_trans);
    }

    // Apply new transitions
    for state in 0..n {
        let state_id = state as StateId;
        fst.clear_transitions(state_id);
        for trans in new_transitions[state].drain(..) {
            fst.add_transition(trans);
        }
    }

    let original_final_weights: Vec<W> = (0..n)
        .map(|state| fst.final_weight(state as StateId))
        .collect();

    // Update final weights for all states based on their ε-closures
    for state in 0..n {
        let state_id = state as StateId;
        let closure = &closures[state];
        let mut new_final = original_final_weights[state];

        for (closure_state, closure_weight) in closure {
            let closure_index = *closure_state as usize;
            if closure_index >= n {
                continue;
            }

            let closure_final_weight = original_final_weights[closure_index];
            if *closure_state != state_id && !closure_final_weight.is_zero() {
                let contribution = closure_weight.times(&closure_final_weight);
                new_final = new_final.plus(&contribution);
            }
        }

        if !new_final.is_zero() {
            fst.set_final(state_id, new_final);
        }
    }

    // Connect if requested - remove unreachable and non-coaccessible states
    if config.connect {
        connect(fst, ConnectConfig::trim());
    }

    Ok(())
}

fn expanded_transition_capacity<L, W, F>(
    fst: &F,
    closures: &[HashMap<StateId, W>],
    state_id: StateId,
) -> usize
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let mut capacity = 0usize;

    let Some(source_closure) = closures.get(state_id as usize) else {
        return capacity;
    };

    for &closure_state in source_closure.keys() {
        if closure_state as usize >= closures.len() {
            continue;
        }

        for trans in fst.transitions(closure_state) {
            if trans.input.is_none() && trans.output.is_none() {
                continue;
            }

            capacity =
                capacity.saturating_add(closures.get(trans.to as usize).map_or(0, HashMap::len));
        }
    }

    capacity
}

/// Compute epsilon closures for all states.
///
/// Returns a vector where `closures[s]` contains all (state, weight) pairs
/// reachable from state `s` via epsilon transitions only.
fn compute_epsilon_closures<L, W, F>(
    fst: &F,
) -> Result<Vec<HashMap<StateId, W>>, EpsilonRemovalError>
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    let mut epsilon_adjacency: Vec<Vec<(StateId, W)>> = vec![Vec::new(); n];
    let mut in_degree = vec![0usize; n];

    for state in 0..n {
        for trans in fst.transitions(state as StateId) {
            if trans.input.is_none() && trans.output.is_none() && !trans.weight.is_zero() {
                let to = trans.to as usize;
                if to >= n {
                    continue;
                }

                epsilon_adjacency[state].push((trans.to, trans.weight));
                in_degree[to] += 1;
            }
        }
    }

    let mut ready = VecDeque::with_capacity(n);
    for (state, &degree) in in_degree.iter().enumerate() {
        if degree == 0 {
            ready.push_back(state);
        }
    }

    let mut order = Vec::with_capacity(n);
    while let Some(state) = ready.pop_front() {
        order.push(state);
        for &(next, _) in &epsilon_adjacency[state] {
            let next = next as usize;
            in_degree[next] -= 1;
            if in_degree[next] == 0 {
                ready.push_back(next);
            }
        }
    }

    if order.len() != n {
        return Err(EpsilonRemovalError::NonConvergentCycle);
    }

    let mut closures: Vec<HashMap<StateId, W>> = (0..n)
        .map(|state| HashMap::with_capacity(epsilon_adjacency[state].len().saturating_add(1)))
        .collect();

    for &state in order.iter().rev() {
        let capacity = epsilon_adjacency[state]
            .iter()
            .map(|&(next, _)| closures[next as usize].len())
            .fold(1usize, usize::saturating_add);
        let mut closure = HashMap::with_capacity(capacity);
        closure.insert(state as StateId, W::one());

        for &(next, edge_weight) in &epsilon_adjacency[state] {
            for (&reachable, suffix_weight) in &closures[next as usize] {
                let candidate = edge_weight.times(suffix_weight);
                let entry = closure.entry(reachable).or_insert_with(W::zero);
                *entry = entry.plus(&candidate);
            }
        }

        closures[state] = closure;
    }

    Ok(closures)
}

fn compute_epsilon_closures_star<L, W, F>(
    fst: &F,
) -> Result<Vec<HashMap<StateId, W>>, EpsilonRemovalError>
where
    L: Clone,
    W: StarSemiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    let mut closure = vec![vec![W::zero(); n]; n];

    for state in 0..n {
        for trans in fst.transitions(state as StateId) {
            if trans.input.is_none() && trans.output.is_none() {
                let from = state;
                let to = trans.to as usize;
                if to >= n {
                    continue;
                }

                closure[from][to] = closure[from][to].plus(&trans.weight);
            }
        }
    }

    for pivot in 0..n {
        let pivot_star = closure[pivot][pivot]
            .star()
            .ok_or(EpsilonRemovalError::NonConvergentCycle)?;

        let pivot_row = closure[pivot].clone();

        for source in 0..n {
            let source_to_pivot = closure[source][pivot];
            if source_to_pivot.is_zero() {
                continue;
            }

            let source_prefix = source_to_pivot.times(&pivot_star);
            for (target, pivot_to_target) in pivot_row.iter().copied().enumerate() {
                if pivot_to_target.is_zero() {
                    continue;
                }

                let candidate = source_prefix.times(&pivot_to_target);
                closure[source][target] = closure[source][target].plus(&candidate);
            }
        }
    }

    let mut maps = Vec::with_capacity(n);
    for source in 0..n {
        let capacity = closure[source]
            .iter()
            .filter(|weight| !weight.is_zero())
            .count()
            .saturating_add(1);
        let mut map = HashMap::with_capacity(capacity);
        map.insert(source as StateId, W::one());

        for target in 0..n {
            let weight = closure[source][target];
            if !weight.is_zero() {
                let entry = map.entry(target as StateId).or_insert_with(W::zero);
                *entry = entry.plus(&weight);
            }
        }

        maps.push(map);
    }

    Ok(maps)
}

/// Deduplicate transitions by combining weights for identical (from, to, input, output) tuples.
fn deduplicate_transitions<L, W>(transitions: &mut Vec<WeightedTransition<L, W>>)
where
    L: Clone + PartialEq,
    W: Semiring,
{
    if transitions.len() <= 1 {
        return;
    }

    // Bucket by target first, then compare labels within the target bucket.
    // This avoids requiring Hash/Eq on labels while preserving distinct labels
    // that share a destination.
    let mut groups: BTreeMap<StateId, Vec<WeightedTransition<L, W>>> = BTreeMap::new();

    for trans in transitions.drain(..) {
        let bucket = groups.entry(trans.to).or_default();

        if let Some(existing) = bucket.iter_mut().find(|existing| {
            existing.from == trans.from
                && existing.to == trans.to
                && existing.input.as_ref() == trans.input.as_ref()
                && existing.output.as_ref() == trans.output.as_ref()
        }) {
            existing.weight = existing.weight.plus(&trans.weight);
        } else {
            bucket.push(trans);
        }
    }

    // Rebuild transitions with combined weights
    for (_, mut bucket) in groups {
        transitions.append(&mut bucket);
    }
}

/// Remove epsilon transitions using the star semiring for cyclic graphs.
///
/// This variant handles cycles in the epsilon graph using the star operation.
/// Required for complete semirings where epsilon cycles may have non-trivial
/// closure values.
pub fn remove_epsilon_star<L, W, F>(
    fst: &mut F,
    config: EpsilonRemovalConfig,
) -> Result<(), EpsilonRemovalError>
where
    L: Clone + PartialEq,
    W: StarSemiring,
    F: MutableWfst<L, W> + Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(());
    }

    if fst.start() == NO_STATE {
        return Err(EpsilonRemovalError::NoStartState);
    }

    let closures = compute_epsilon_closures_star(fst)?;
    apply_epsilon_removal(fst, config, closures)
}

/// Errors that can occur during epsilon removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EpsilonRemovalError {
    /// The WFST has no start state.
    NoStartState,
    /// Epsilon cycle with non-converging weight.
    NonConvergentCycle,
}

impl std::fmt::Display for EpsilonRemovalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoStartState => write!(f, "WFST has no start state"),
            Self::NonConvergentCycle => write!(f, "Epsilon cycle with non-converging weight"),
        }
    }
}

impl std::error::Error for EpsilonRemovalError {}

/// Check if a WFST has any epsilon transitions.
pub fn has_epsilon_transitions<L, W, F>(fst: &F) -> bool
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    for state in 0..fst.num_states() {
        for trans in fst.transitions(state as StateId) {
            if trans.input.is_none() && trans.output.is_none() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::{ProbabilityWeight, TropicalWeight};
    use crate::wfst::{MutableWfst, VectorWfst, VectorWfstBuilder};

    // Property-based tests
    mod property_tests {
        use super::*;
        use crate::test_utils::arb_tropical_wfst;
        use proptest::prelude::*;

        proptest! {
            /// Epsilon removal should produce a WFST with no epsilon transitions.
            #[test]
            fn epsilon_removal_complete(
                mut fst in arb_tropical_wfst(8, 3)
            ) {
                if fst.num_states() == 0 || fst.start() == NO_STATE {
                    return Ok(());
                }

                let result = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());
                if result.is_ok() {
                    prop_assert!(
                        !has_epsilon_transitions(&fst),
                        "FST still has epsilon transitions after removal"
                    );
                }
            }

            /// Epsilon removal should preserve state count or reduce it.
            #[test]
            fn epsilon_removal_state_bound(
                mut fst in arb_tropical_wfst(8, 3)
            ) {
                if fst.num_states() == 0 {
                    return Ok(());
                }

                let original_states = fst.num_states();
                let _ = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());

                // State count should not increase
                prop_assert!(
                    fst.num_states() <= original_states,
                    "Epsilon removal increased states from {} to {}",
                    original_states,
                    fst.num_states()
                );
            }

            /// has_epsilon_transitions should return false for epsilon-free WFSTs.
            #[test]
            fn has_epsilon_after_removal(
                mut fst in arb_tropical_wfst(6, 2)
            ) {
                if fst.num_states() == 0 || fst.start() == NO_STATE {
                    return Ok(());
                }

                let _ = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());

                // Verify the predicate matches reality
                let predicate_result = has_epsilon_transitions(&fst);

                // Actually check for epsilon transitions
                let mut found_epsilon = false;
                for state in 0..fst.num_states() {
                    for trans in fst.transitions(state as StateId) {
                        if trans.input.is_none() && trans.output.is_none() {
                            found_epsilon = true;
                            break;
                        }
                    }
                    if found_epsilon {
                        break;
                    }
                }

                prop_assert_eq!(
                    predicate_result,
                    found_epsilon,
                    "has_epsilon_transitions() returned {}, but manual check found {}",
                    predicate_result,
                    found_epsilon
                );
            }

            /// Epsilon removal on already epsilon-free FST should be identity.
            #[test]
            fn epsilon_removal_identity_when_no_epsilon(
                fst in arb_tropical_wfst(6, 2)
            ) {
                if fst.num_states() == 0 || fst.start() == NO_STATE {
                    return Ok(());
                }

                // Remove any epsilons first
                let mut clean_fst = fst.clone();
                let _ = remove_epsilon(&mut clean_fst, EpsilonRemovalConfig::default());

                if !has_epsilon_transitions(&clean_fst) {
                    // Now remove again - should be essentially identity
                    let original_states = clean_fst.num_states();
                    let mut second_fst = clean_fst.clone();
                    let _ = remove_epsilon(&mut second_fst, EpsilonRemovalConfig::default());

                    prop_assert_eq!(
                        second_fst.num_states(),
                        original_states,
                        "Second epsilon removal changed state count"
                    );
                }
            }
        }
    }

    fn build_simple_epsilon_chain() -> VectorWfst<char, TropicalWeight> {
        // 0 --ε/1.0--> 1 --a/2.0--> 2 (final, weight 0.5)
        let mut fst = VectorWfst::new();
        fst.add_states(3);
        fst.set_start(0);
        fst.add_epsilon(0, 1, TropicalWeight::new(1.0));
        fst.add_arc(1, Some('a'), Some('a'), 2, TropicalWeight::new(2.0));
        fst.set_final(2, TropicalWeight::new(0.5));
        fst
    }

    fn build_epsilon_to_final() -> VectorWfst<char, TropicalWeight> {
        // 0 --a/1.0--> 1 --ε/0.5--> 2 (final, weight 0.0)
        let mut fst = VectorWfst::new();
        fst.add_states(3);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_epsilon(1, 2, TropicalWeight::new(0.5));
        fst.set_final(2, TropicalWeight::one());
        fst
    }

    #[test]
    fn test_remove_epsilon_empty() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        let result = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_epsilon_no_start() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        fst.add_state();
        let result = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());
        assert_eq!(result, Err(EpsilonRemovalError::NoStartState));
    }

    #[test]
    fn test_remove_epsilon_simple_chain() {
        let mut fst = build_simple_epsilon_chain();

        assert!(has_epsilon_transitions(&fst));

        let result = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());
        assert!(result.is_ok());

        // After ε-removal, there should be no epsilon transitions
        assert!(!has_epsilon_transitions(&fst));

        // Check structure
        assert_eq!(fst.num_states(), 3);
        assert_ne!(fst.start(), NO_STATE);

        // State 0 should now have a direct transition to state 2
        // with weight = ε-weight ⊗ a-weight = 1.0 + 2.0 = 3.0
        let trans = fst.transitions(0);
        assert!(!trans.is_empty(), "State 0 should have transitions");
    }

    #[test]
    fn test_remove_epsilon_to_final() {
        let mut fst = build_epsilon_to_final();

        assert!(has_epsilon_transitions(&fst));

        let result = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());
        assert!(result.is_ok());

        // After ε-removal, there should be no epsilon transitions
        assert!(!has_epsilon_transitions(&fst));

        // State 1 should now be final (reachable from final state 2 via ε)
        // with weight = ε-weight ⊗ final-weight = 0.5 + 0.0 = 0.5
        assert!(fst.is_final(1), "State 1 should be final after ε-removal");
    }

    #[test]
    fn test_remove_epsilon_final_weight_not_double_counted() {
        let mut fst: VectorWfst<char, ProbabilityWeight> = VectorWfst::new();
        fst.add_states(2);
        fst.set_start(0);
        fst.add_epsilon(0, 1, ProbabilityWeight::new(0.5));
        fst.set_final(1, ProbabilityWeight::one());

        let mut config = EpsilonRemovalConfig::default();
        config.connect = false;
        remove_epsilon(&mut fst, config).expect("acyclic epsilon removal should succeed");

        assert!(fst.is_final(0));
        assert!(fst
            .final_weight(0)
            .approx_eq(&ProbabilityWeight::new(0.5), 1e-12));
    }

    #[test]
    fn test_remove_epsilon_final_weight_uses_original_final_snapshot() {
        let mut fst: VectorWfst<char, ProbabilityWeight> = VectorWfst::new();
        fst.add_states(3);
        fst.set_start(2);
        fst.add_epsilon(2, 1, ProbabilityWeight::new(0.5));
        fst.add_epsilon(1, 0, ProbabilityWeight::new(0.5));
        fst.set_final(0, ProbabilityWeight::one());

        let mut config = EpsilonRemovalConfig::default();
        config.connect = false;
        remove_epsilon(&mut fst, config).expect("acyclic epsilon removal should succeed");

        assert!(fst
            .final_weight(1)
            .approx_eq(&ProbabilityWeight::new(0.5), 1e-12));
        assert!(fst
            .final_weight(2)
            .approx_eq(&ProbabilityWeight::new(0.25), 1e-12));
    }

    #[test]
    fn test_remove_epsilon_propagates_revisited_closure_weights() {
        let mut fst: VectorWfst<char, ProbabilityWeight> = VectorWfst::new();
        fst.add_states(4);
        fst.set_start(0);
        fst.add_epsilon(0, 1, ProbabilityWeight::new(0.5));
        fst.add_epsilon(0, 2, ProbabilityWeight::new(0.25));
        fst.add_epsilon(2, 1, ProbabilityWeight::new(0.5));
        fst.add_epsilon(1, 3, ProbabilityWeight::new(0.5));
        fst.set_final(3, ProbabilityWeight::one());

        let mut config = EpsilonRemovalConfig::default();
        config.connect = false;
        remove_epsilon(&mut fst, config).expect("acyclic epsilon DAG should close");

        assert!(fst.is_final(0));
        assert!(fst
            .final_weight(0)
            .approx_eq(&ProbabilityWeight::new(0.3125), 1e-12));
    }

    #[test]
    fn test_remove_epsilon_expands_non_start_residual_state() {
        let mut fst: VectorWfst<char, ProbabilityWeight> = VectorWfst::new();
        fst.add_states(4);
        fst.set_start(0);
        fst.add_arc(0, Some('x'), Some('x'), 1, ProbabilityWeight::one());
        fst.add_epsilon(1, 2, ProbabilityWeight::new(0.5));
        fst.add_arc(2, Some('a'), Some('a'), 3, ProbabilityWeight::new(0.25));
        fst.set_final(3, ProbabilityWeight::one());

        let mut config = EpsilonRemovalConfig::default();
        config.connect = false;
        remove_epsilon(&mut fst, config).expect("acyclic epsilon removal should succeed");

        let transition = fst
            .transitions(1)
            .iter()
            .find(|transition| transition.input == Some('a') && transition.output == Some('a'))
            .expect("non-start state should inherit its epsilon successor's labeled arc");

        assert_eq!(transition.to, 3);
        assert!(transition
            .weight
            .approx_eq(&ProbabilityWeight::new(0.125), 1e-12));
    }

    #[test]
    fn test_remove_epsilon_star_sums_convergent_cycle() {
        let mut fst: VectorWfst<char, ProbabilityWeight> = VectorWfst::new();
        fst.add_states(3);
        fst.set_start(0);
        fst.add_epsilon(0, 1, ProbabilityWeight::new(0.5));
        fst.add_epsilon(1, 1, ProbabilityWeight::new(0.5));
        fst.add_arc(1, Some('a'), Some('a'), 2, ProbabilityWeight::one());
        fst.set_final(2, ProbabilityWeight::one());

        let mut config = EpsilonRemovalConfig::default();
        config.connect = false;
        remove_epsilon_star(&mut fst, config).expect("convergent epsilon cycle should close");

        let transition = fst
            .transitions(0)
            .iter()
            .find(|transition| transition.input == Some('a') && transition.output == Some('a'))
            .expect("start should bypass the closed epsilon cycle");

        assert!(transition
            .weight
            .approx_eq(&ProbabilityWeight::new(1.0), 1e-12));
    }

    #[test]
    fn test_remove_epsilon_star_rejects_nonconvergent_cycle() {
        let mut fst: VectorWfst<char, ProbabilityWeight> = VectorWfst::new();
        fst.add_states(2);
        fst.set_start(0);
        fst.add_epsilon(0, 0, ProbabilityWeight::one());
        fst.add_arc(0, Some('a'), Some('a'), 1, ProbabilityWeight::one());
        fst.set_final(1, ProbabilityWeight::one());

        let result = remove_epsilon_star(&mut fst, EpsilonRemovalConfig::default());
        assert_eq!(result, Err(EpsilonRemovalError::NonConvergentCycle));
    }

    #[test]
    fn test_remove_epsilon_no_epsilons() {
        // FST without any epsilon transitions
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('b'), Some('b'), 2, TropicalWeight::new(2.0))
            .final_state(2, TropicalWeight::one())
            .build();

        assert!(!has_epsilon_transitions(&fst));

        let result = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());
        assert!(result.is_ok());

        // Structure should be unchanged
        assert_eq!(fst.num_states(), 3);
        assert_eq!(fst.transitions(0).len(), 1);
        assert_eq!(fst.transitions(1).len(), 1);
    }

    #[test]
    fn test_remove_epsilon_preserves_distinct_same_target_labels() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
            .arc(0, Some('b'), Some('b'), 1, TropicalWeight::new(2.0))
            .final_state(1, TropicalWeight::one())
            .build();

        let mut config = EpsilonRemovalConfig::default();
        config.connect = false;
        remove_epsilon(&mut fst, config).expect("epsilon-free FST should remain valid");

        let transitions = fst.transitions(0);
        assert_eq!(transitions.len(), 2);
        assert!(transitions
            .iter()
            .any(|transition| transition.input == Some('a') && transition.output == Some('a')));
        assert!(transitions
            .iter()
            .any(|transition| transition.input == Some('b') && transition.output == Some('b')));
    }

    #[test]
    fn test_remove_epsilon_skips_malformed_transition_targets() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        fst.add_states(3);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_arc(0, Some('x'), Some('x'), 99, TropicalWeight::new(1.0));
        fst.add_epsilon(0, 99, TropicalWeight::new(1.0));
        fst.add_epsilon(1, 2, TropicalWeight::new(0.5));
        fst.set_final(2, TropicalWeight::one());

        let mut config = EpsilonRemovalConfig::default();
        config.connect = false;
        remove_epsilon(&mut fst, config).expect("malformed targets should be skipped");

        assert!(!has_epsilon_transitions(&fst));
        assert!(fst
            .transitions(0)
            .iter()
            .all(|transition| (transition.to as usize) < fst.num_states()));
        assert!(fst
            .transitions(0)
            .iter()
            .any(|transition| transition.input == Some('a') && transition.to == 2));
    }

    #[test]
    fn test_remove_epsilon_star_skips_malformed_epsilon_targets() {
        let mut fst: VectorWfst<char, ProbabilityWeight> = VectorWfst::new();
        fst.add_states(3);
        fst.set_start(0);
        fst.add_epsilon(0, 1, ProbabilityWeight::new(0.5));
        fst.add_epsilon(0, 99, ProbabilityWeight::new(0.5));
        fst.add_arc(1, Some('a'), Some('a'), 2, ProbabilityWeight::one());
        fst.set_final(2, ProbabilityWeight::one());

        let mut config = EpsilonRemovalConfig::default();
        config.connect = false;
        remove_epsilon_star(&mut fst, config).expect("malformed targets should be skipped");

        assert!(!has_epsilon_transitions(&fst));
        assert!(fst
            .transitions(0)
            .iter()
            .all(|transition| (transition.to as usize) < fst.num_states()));
        assert!(fst
            .transitions(0)
            .iter()
            .any(|transition| transition.input == Some('a') && transition.to == 2));
    }

    #[test]
    fn test_has_epsilon_transitions() {
        let with_eps = build_simple_epsilon_chain();
        assert!(has_epsilon_transitions(&with_eps));

        let without_eps: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::one())
            .final_state(1, TropicalWeight::one())
            .build();
        assert!(!has_epsilon_transitions(&without_eps));
    }

    #[test]
    fn test_epsilon_removal_error_display() {
        assert_eq!(
            EpsilonRemovalError::NoStartState.to_string(),
            "WFST has no start state"
        );
        assert_eq!(
            EpsilonRemovalError::NonConvergentCycle.to_string(),
            "Epsilon cycle with non-converging weight"
        );
    }
}
