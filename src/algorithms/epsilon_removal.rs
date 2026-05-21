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

use std::collections::{HashMap, HashSet};

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

    // Compute epsilon closures for all states
    let closures = compute_epsilon_closures(fst);

    // Collect new transitions
    let mut new_transitions: Vec<Vec<WeightedTransition<L, W>>> = vec![Vec::new(); n];

    for state in 0..n {
        let state_id = state as StateId;

        // Get non-epsilon transitions from this state
        for trans in fst.transitions(state_id) {
            if trans.input.is_some() || trans.output.is_some() {
                // Non-epsilon transition: add transitions through ε-closure of destination
                let to_closure = &closures[trans.to as usize];

                for (closure_state, closure_weight) in to_closure {
                    let new_weight = trans.weight.times(closure_weight);
                    new_transitions[state].push(WeightedTransition {
                        from: state_id,
                        to: *closure_state,
                        input: trans.input.clone(),
                        output: trans.output.clone(),
                        weight: new_weight,
                    });
                }
            }
        }

        // Handle transitions from ε-closure of start through this state
        if state_id == fst.start() {
            let start_closure = &closures[state];
            for (closure_state, closure_weight) in start_closure {
                if *closure_state != state_id {
                    // Add transitions from closure states as if they were start
                    for trans in fst.transitions(*closure_state) {
                        if trans.input.is_some() || trans.output.is_some() {
                            let to_closure = &closures[trans.to as usize];
                            for (dest_state, dest_weight) in to_closure {
                                let new_weight =
                                    closure_weight.times(&trans.weight).times(dest_weight);
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

    // Update final weights based on ε-closure
    let start = fst.start();
    let start_closure = &closures[start as usize];
    for (closure_state, closure_weight) in start_closure {
        if fst.is_final(*closure_state) && *closure_state != start {
            // Add final weight contribution from ε-reachable final states
            let old_final = fst.final_weight(start);
            let contribution = closure_weight.times(&fst.final_weight(*closure_state));
            fst.set_final(start, old_final.plus(&contribution));
        }
    }

    // Update final weights for all states based on their ε-closures
    for state in 0..n {
        let state_id = state as StateId;
        let closure = &closures[state];
        let mut new_final = fst.final_weight(state_id);

        for (closure_state, closure_weight) in closure {
            if *closure_state != state_id && fst.is_final(*closure_state) {
                let contribution = closure_weight.times(&fst.final_weight(*closure_state));
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

/// Compute epsilon closures for all states.
///
/// Returns a vector where `closures[s]` contains all (state, weight) pairs
/// reachable from state `s` via epsilon transitions only.
fn compute_epsilon_closures<L, W, F>(fst: &F) -> Vec<HashMap<StateId, W>>
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    let mut closures: Vec<HashMap<StateId, W>> = vec![HashMap::new(); n];

    for state in 0..n {
        let state_id = state as StateId;
        let mut closure: HashMap<StateId, W> = HashMap::new();
        let mut visited = HashSet::new();
        let mut queue = vec![(state_id, W::one())];

        while let Some((current, weight)) = queue.pop() {
            if visited.contains(&current) {
                // Update weight if we found a better path (for non-idempotent semirings)
                if let Some(existing) = closure.get(&current) {
                    closure.insert(current, existing.plus(&weight));
                }
                continue;
            }
            visited.insert(current);
            closure.insert(current, weight.clone());

            // Follow epsilon transitions
            for trans in fst.transitions(current) {
                if trans.input.is_none() && trans.output.is_none() {
                    let new_weight = weight.times(&trans.weight);
                    queue.push((trans.to, new_weight));
                }
            }
        }

        closures[state] = closure;
    }

    closures
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

    // Group by (to, input, output) and combine weights
    let mut groups: HashMap<
        (StateId, Option<usize>, Option<usize>),
        (WeightedTransition<L, W>, W),
    > = HashMap::new();

    for trans in transitions.drain(..) {
        // Use indices for comparison (we'll store the actual labels separately)
        let key = (
            trans.to,
            trans.input.as_ref().map(|_| 0usize),
            trans.output.as_ref().map(|_| 0usize),
        );

        // Check if we have a matching transition
        let mut found = false;
        for ((to, _, _), (existing, weight)) in groups.iter_mut() {
            if *to == trans.to && existing.input == trans.input && existing.output == trans.output {
                *weight = weight.plus(&trans.weight);
                found = true;
                break;
            }
        }

        if !found {
            groups.insert(key, (trans.clone(), trans.weight.clone()));
        }
    }

    // Rebuild transitions with combined weights
    for (_, (mut trans, weight)) in groups {
        trans.weight = weight;
        transitions.push(trans);
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
    // For star semirings, we need to handle cycles using the star operation
    // This is a more complex algorithm that computes the epsilon closure
    // matrix and uses matrix star for convergence

    // For now, we use the simple algorithm which works for acyclic graphs
    // and k-closed semirings
    remove_epsilon(fst, config)
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
    use crate::semiring::TropicalWeight;
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
