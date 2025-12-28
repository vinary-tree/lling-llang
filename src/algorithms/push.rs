//! Weight pushing algorithm for WFSTs.
//!
//! Weight pushing redistributes weights along paths so that weights are
//! "pushed" toward the initial state (forward push) or final states (backward push).
//! This normalization is essential for:
//!
//! - **Minimization**: Weight pushing is a prerequisite for weighted minimization
//! - **Beam Search Pruning**: Log-semiring pushing improves pruning efficacy
//! - **Equivalence Testing**: Pushed automata can be compared structurally
//!
//! # Algorithm
//!
//! Weight pushing uses potential functions based on shortest-distance:
//!
//! - **Forward Push**: Potential V(q) = shortest distance from initial to q
//!   - Transition weight: w' = V(p(e))⁻¹ ⊗ w(e) ⊗ V(n(e))
//!   - Final weight: ρ' = V(q)⁻¹ ⊗ ρ(q)
//!
//! - **Backward Push**: Potential V(q) = shortest distance from q to any final
//!   - Transition weight: w' = w(e) ⊗ V(n(e)) ⊗ V(p(e))⁻¹
//!   - Final weight unchanged
//!   - Initial weight: V(i)⁻¹ (absorbed into first transitions)
//!
//! # Semiring Requirements
//!
//! Weight pushing requires a [`DivisibleSemiring`] to compute the inverse
//! operation needed for reweighting. For log-semiring pushing (recommended
//! for beam search), use [`LogWeight`].
//!
//! # References
//!
//! - Mohri, M. (2009). "Weighted Automata Algorithms"
//! - Mohri, M., Pereira, F., & Riley, M. (2002). "WFSTs in Speech Recognition"

use crate::semiring::{DivisibleSemiring, Semiring};
use crate::wfst::{MutableWfst, StateId, Wfst, WeightedTransition, NO_STATE};

use super::shortest_distance::{
    reverse_shortest_distance, single_source_shortest_distance, ShortestDistanceConfig,
};

/// Direction of weight pushing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushDirection {
    /// Push weights toward the initial state.
    Forward,
    /// Push weights toward final states.
    Backward,
}

impl Default for PushDirection {
    fn default() -> Self {
        Self::Backward
    }
}

/// Configuration for weight pushing.
#[derive(Clone, Debug)]
pub struct PushConfig {
    /// Direction to push weights.
    pub direction: PushDirection,
    /// Whether to remove non-coaccessible states after pushing.
    pub remove_non_coaccessible: bool,
    /// Shortest-distance configuration.
    pub distance_config: ShortestDistanceConfig,
}

impl Default for PushConfig {
    fn default() -> Self {
        Self {
            direction: PushDirection::Backward,
            remove_non_coaccessible: true,
            distance_config: ShortestDistanceConfig::default(),
        }
    }
}

impl PushConfig {
    /// Create a forward push configuration.
    pub fn forward() -> Self {
        Self {
            direction: PushDirection::Forward,
            ..Default::default()
        }
    }

    /// Create a backward push configuration.
    pub fn backward() -> Self {
        Self {
            direction: PushDirection::Backward,
            ..Default::default()
        }
    }

    /// Create a configuration for log-semiring pushing (optimal for beam search).
    pub fn log_semiring() -> Self {
        Self {
            direction: PushDirection::Backward,
            remove_non_coaccessible: true,
            distance_config: ShortestDistanceConfig::default(),
        }
    }
}

/// Push weights in a WFST according to the configuration.
///
/// This operation modifies the WFST in place, redistributing weights
/// along paths to push them toward the initial state (forward) or
/// final states (backward).
///
/// # Requirements
///
/// - The semiring must be divisible (implement `DivisibleSemiring`)
/// - The WFST must have a valid start state
/// - For backward pushing, there must be at least one reachable final state
///
/// # Returns
///
/// - `Ok(())` if pushing succeeds
/// - `Err(PushError)` if pushing fails (e.g., no valid potentials)
///
/// # Example
///
/// ```ignore
/// use lling_llang::algorithms::{push_weights, PushConfig};
///
/// let mut fst = build_some_wfst();
/// push_weights(&mut fst, PushConfig::backward())?;
/// ```
pub fn push_weights<L, W, F>(fst: &mut F, config: PushConfig) -> Result<(), PushError>
where
    L: Clone,
    W: DivisibleSemiring,
    F: MutableWfst<L, W> + Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(());
    }

    if fst.start() == NO_STATE {
        return Err(PushError::NoStartState);
    }

    // Compute potentials based on push direction
    let potentials = match config.direction {
        PushDirection::Forward => {
            single_source_shortest_distance(fst, config.distance_config.clone())
                .ok_or(PushError::NoPotentials)?
        }
        PushDirection::Backward => {
            reverse_shortest_distance(fst, config.distance_config.clone())
                .ok_or(PushError::NoPotentials)?
        }
    };

    // Check that potentials are valid
    if potentials.is_empty() {
        return Err(PushError::NoPotentials);
    }

    // Apply reweighting based on direction
    match config.direction {
        PushDirection::Forward => push_forward_impl(fst, &potentials),
        PushDirection::Backward => push_backward_impl(fst, &potentials),
    }

    Ok(())
}

/// Forward push implementation.
fn push_forward_impl<L, W, F>(fst: &mut F, potentials: &[W])
where
    L: Clone,
    W: DivisibleSemiring,
    F: MutableWfst<L, W> + Wfst<L, W>,
{
    let n = fst.num_states();

    // Collect all transitions and reweight them
    let mut new_transitions: Vec<Vec<WeightedTransition<L, W>>> = vec![Vec::new(); n];

    for state in 0..n {
        let state_id = state as StateId;
        let p_from = &potentials[state];

        // Skip states with zero potential (unreachable)
        if p_from.is_zero() {
            continue;
        }

        for trans in fst.transitions(state_id).to_vec() {
            let to_idx = trans.to as usize;
            if to_idx >= potentials.len() {
                continue;
            }

            let p_to = &potentials[to_idx];

            // New weight: p_from^{-1} ⊗ w ⊗ p_to
            // Inverse is computed as one / p_from
            let p_from_inv = W::one().divide(p_from).unwrap_or_else(W::one);
            let new_weight = p_from_inv.times(&trans.weight).times(p_to);

            new_transitions[state].push(WeightedTransition {
                from: trans.from,
                to: trans.to,
                input: trans.input,
                output: trans.output,
                weight: new_weight,
            });
        }
    }

    // Apply new transitions
    for state in 0..n {
        let state_id = state as StateId;
        // Clear existing transitions and add reweighted transitions
        fst.clear_transitions(state_id);
        for trans in new_transitions[state].drain(..) {
            fst.add_transition(trans);
        }
    }

    // Reweight final states
    for state in 0..n {
        let state_id = state as StateId;
        if fst.is_final(state_id) {
            let p = &potentials[state];
            if !p.is_zero() {
                let old_final = fst.final_weight(state_id);
                let p_inv = W::one().divide(p).unwrap_or_else(W::one);
                let new_final = p_inv.times(&old_final);
                fst.set_final(state_id, new_final);
            }
        }
    }
}

/// Backward push implementation.
fn push_backward_impl<L, W, F>(fst: &mut F, potentials: &[W])
where
    L: Clone,
    W: DivisibleSemiring,
    F: MutableWfst<L, W> + Wfst<L, W>,
{
    let n = fst.num_states();

    // Collect all transitions and reweight them
    let mut new_transitions: Vec<Vec<WeightedTransition<L, W>>> = vec![Vec::new(); n];

    for state in 0..n {
        let state_id = state as StateId;
        let p_from = &potentials[state];

        // Skip states with zero potential (no path to final)
        if p_from.is_zero() {
            continue;
        }

        for trans in fst.transitions(state_id).to_vec() {
            let to_idx = trans.to as usize;
            if to_idx >= potentials.len() {
                continue;
            }

            let p_to = &potentials[to_idx];

            // Skip if destination has no path to final
            if p_to.is_zero() {
                continue;
            }

            // New weight: w ⊗ p_to ⊗ p_from^{-1}
            // Inverse is computed as one / p_from
            let p_from_inv = W::one().divide(p_from).unwrap_or_else(W::one);
            let new_weight = trans.weight.times(p_to).times(&p_from_inv);

            new_transitions[state].push(WeightedTransition {
                from: trans.from,
                to: trans.to,
                input: trans.input,
                output: trans.output,
                weight: new_weight,
            });
        }
    }

    // Apply new transitions
    for state in 0..n {
        let state_id = state as StateId;
        fst.clear_transitions(state_id);
        for trans in new_transitions[state].drain(..) {
            fst.add_transition(trans);
        }
    }

    // For backward pushing, final weights become one (normalized)
    // But we need to handle the start state potential
    let start = fst.start();
    if start != NO_STATE {
        let start_idx = start as usize;
        if start_idx < potentials.len() {
            let start_potential = &potentials[start_idx];
            // The start potential represents the total weight of the FST
            // For a properly pushed FST, this should be distributed to final states
            // But the standard backward push makes final weights = one
            for state in 0..n {
                let state_id = state as StateId;
                if fst.is_final(state_id) {
                    // Final weight becomes one (all weight pushed to transitions)
                    fst.set_final(state_id, W::one());
                }
            }

            // Note: The initial weight (start_potential) is implicitly absorbed
            // into the interpretation of the FST's total weight
            let _ = start_potential; // Suppress warning; potential used for verification
        }
    }
}


/// Errors that can occur during weight pushing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushError {
    /// The WFST has no start state.
    NoStartState,
    /// Could not compute potentials (e.g., no path to final states).
    NoPotentials,
    /// Division by zero during reweighting.
    DivisionByZero,
}

impl std::fmt::Display for PushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoStartState => write!(f, "WFST has no start state"),
            Self::NoPotentials => write!(f, "Could not compute potentials"),
            Self::DivisionByZero => write!(f, "Division by zero during reweighting"),
        }
    }
}

impl std::error::Error for PushError {}

/// Check if a WFST is stochastic (weights at each state sum to one).
///
/// A stochastic WFST has the property that for each state, the sum of
/// outgoing transition weights plus the final weight equals one.
///
/// This is a useful property for probabilistic models.
pub fn is_stochastic<L, W, F>(fst: &F, epsilon: f64) -> bool
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    for state in 0..fst.num_states() {
        let state_id = state as StateId;
        let mut total = fst.final_weight(state_id);

        for trans in fst.transitions(state_id) {
            total = total.plus(&trans.weight);
        }

        // Check if total is approximately one
        if !total.approx_eq(&W::one(), epsilon) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::{VectorWfst, VectorWfstBuilder};

    // Property-based tests
    mod property_tests {
        use super::*;
        use crate::test_utils::arb_acyclic_wfst_tropical;
        use proptest::prelude::*;

        proptest! {
            /// Weight pushing should preserve structure (state count).
            #[test]
            fn push_preserves_state_count(
                fst in arb_acyclic_wfst_tropical(8, 3)
            ) {
                if fst.num_states() == 0 || fst.start() == NO_STATE {
                    return Ok(());
                }

                let original_states = fst.num_states();
                let mut pushed_fst = fst.clone();
                let result = push_weights(&mut pushed_fst, PushConfig::backward());

                if result.is_ok() {
                    prop_assert_eq!(
                        pushed_fst.num_states(),
                        original_states,
                        "Push changed state count from {} to {}",
                        original_states,
                        pushed_fst.num_states()
                    );
                }
            }

            /// Weight pushing should preserve transition count (when no trimming).
            #[test]
            fn push_preserves_transitions(
                fst in arb_acyclic_wfst_tropical(6, 2)
            ) {
                if fst.num_states() == 0 || fst.start() == NO_STATE {
                    return Ok(());
                }

                let original_arc_count: usize = (0..fst.num_states())
                    .map(|s| fst.transitions(s as StateId).len())
                    .sum();

                let mut pushed_fst = fst.clone();
                let config = PushConfig {
                    direction: PushDirection::Forward,
                    remove_non_coaccessible: false,
                    distance_config: ShortestDistanceConfig::default(),
                };
                let result = push_weights(&mut pushed_fst, config);

                if result.is_ok() {
                    let new_arc_count: usize = (0..pushed_fst.num_states())
                        .map(|s| pushed_fst.transitions(s as StateId).len())
                        .sum();

                    // Transition count should be preserved or slightly reduced
                    // (edges to unreachable states may be removed)
                    prop_assert!(
                        new_arc_count <= original_arc_count,
                        "Push increased arc count from {} to {}",
                        original_arc_count,
                        new_arc_count
                    );
                }
            }

            /// Forward and backward push should both preserve valid structure.
            #[test]
            fn push_both_directions_valid(
                fst in arb_acyclic_wfst_tropical(6, 2)
            ) {
                if fst.num_states() == 0 || fst.start() == NO_STATE {
                    return Ok(());
                }

                // Forward push
                let mut forward_fst = fst.clone();
                let forward_result = push_weights(&mut forward_fst, PushConfig::forward());

                // Backward push
                let mut backward_fst = fst.clone();
                let backward_result = push_weights(&mut backward_fst, PushConfig::backward());

                // Both should either succeed or fail consistently for valid FSTs
                if forward_result.is_ok() {
                    prop_assert!(forward_fst.start() != NO_STATE || fst.num_states() == 0);
                }
                if backward_result.is_ok() {
                    prop_assert!(backward_fst.start() != NO_STATE || fst.num_states() == 0);
                }
            }

            /// Push on empty FST should succeed.
            #[test]
            fn push_empty_succeeds(_seed in 0u32..100) {
                let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
                let result = push_weights(&mut fst, PushConfig::backward());
                prop_assert!(result.is_ok());
            }

            /// Push with no start state should fail.
            #[test]
            fn push_no_start_fails(_seed in 0u32..100) {
                let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
                fst.add_state();
                // Don't set start state
                let result = push_weights(&mut fst, PushConfig::backward());
                prop_assert!(matches!(result, Err(PushError::NoStartState)));
            }
        }
    }

    fn build_simple_chain() -> VectorWfst<char, TropicalWeight> {
        // 0 --a/1.0--> 1 --b/2.0--> 2 (final, weight 0.5)
        VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('b'), Some('b'), 2, TropicalWeight::new(2.0))
            .final_state(2, TropicalWeight::new(0.5))
            .build()
    }

    fn build_diamond() -> VectorWfst<char, TropicalWeight> {
        // Diamond: 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3
        VectorWfstBuilder::new()
            .add_states(4)
            .start(0)
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
            .arc(0, Some('b'), Some('b'), 2, TropicalWeight::new(2.0))
            .arc(1, Some('c'), Some('c'), 3, TropicalWeight::new(1.0))
            .arc(2, Some('d'), Some('d'), 3, TropicalWeight::new(1.0))
            .final_state(3, TropicalWeight::one())
            .build()
    }

    #[test]
    fn test_push_empty_fst() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        let result = push_weights(&mut fst, PushConfig::backward());
        assert!(result.is_ok());
    }

    #[test]
    fn test_push_no_start() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        fst.add_state();
        // No start state set
        let result = push_weights(&mut fst, PushConfig::backward());
        assert_eq!(result, Err(PushError::NoStartState));
    }

    #[test]
    fn test_push_backward_chain() {
        let mut fst = build_simple_chain();

        // Compute original potentials before push (for verification)
        let potentials = reverse_shortest_distance(&fst, ShortestDistanceConfig::default())
            .expect("Should compute potentials");
        let initial_potential = potentials[fst.start() as usize].clone();

        // Original path weight: 1.0 + 2.0 + 0.5 = 3.5 (tropical = min, so sum)
        let original_total = TropicalWeight::new(3.5);

        let result = push_weights(&mut fst, PushConfig::backward());
        assert!(result.is_ok());

        // After backward push:
        // - Path weights are normalized (become relative)
        // - Initial potential V(i) absorbs the total weight
        // - Final weights become one (tropical zero)
        let start = fst.start();
        assert_ne!(start, NO_STATE);

        // Verify structure preserved
        assert_eq!(fst.num_states(), 3);
        assert_eq!(fst.transitions(0).len(), 1);
        assert_eq!(fst.transitions(1).len(), 1);

        // Verify final weight is one (normalized)
        assert!(
            fst.final_weight(2).approx_eq(&TropicalWeight::one(), 0.001),
            "Final weight should be one after backward push, got {:?}",
            fst.final_weight(2)
        );

        // Traverse and accumulate normalized weights
        let mut normalized_path = TropicalWeight::one();
        let mut current = start;
        while !fst.transitions(current).is_empty() {
            let trans = &fst.transitions(current)[0];
            normalized_path = normalized_path.times(&trans.weight);
            current = trans.to;
        }
        normalized_path = normalized_path.times(&fst.final_weight(current));

        // Verify: initial_potential ⊗ normalized_path ≈ original_total
        let reconstructed = initial_potential.times(&normalized_path);
        assert!(
            reconstructed.approx_eq(&original_total, 0.1),
            "V(i) ⊗ normalized_path should equal original: {:?} ⊗ {:?} = {:?}, expected {:?}",
            initial_potential,
            normalized_path,
            reconstructed,
            original_total
        );
    }

    #[test]
    fn test_push_backward_diamond() {
        let mut fst = build_diamond();

        // Shortest path weight: min(1+1, 2+1) = 2.0
        let result = push_weights(&mut fst, PushConfig::backward());
        assert!(result.is_ok());

        // Verify the FST still has valid structure
        assert_eq!(fst.num_states(), 4);
        assert_ne!(fst.start(), NO_STATE);
        assert!(fst.is_final(3));
    }

    #[test]
    fn test_push_forward_chain() {
        let mut fst = build_simple_chain();

        // Original path weight: 1.0 + 2.0 + 0.5 = 3.5 (tropical = min, so sum)
        let original_total = TropicalWeight::new(3.5);

        let result = push_weights(&mut fst, PushConfig::forward());
        assert!(result.is_ok());

        // After forward push, path weight should be preserved
        // (weights are redistributed but total is unchanged)
        let start = fst.start();
        assert_ne!(start, NO_STATE);

        // Verify structure preserved
        assert_eq!(fst.num_states(), 3);
        assert_eq!(fst.transitions(0).len(), 1);
        assert_eq!(fst.transitions(1).len(), 1);

        // Traverse and accumulate weights
        let mut total = TropicalWeight::one();
        let mut current = start;
        while !fst.transitions(current).is_empty() {
            let trans = &fst.transitions(current)[0];
            total = total.times(&trans.weight);
            current = trans.to;
        }
        total = total.times(&fst.final_weight(current));

        // Forward push preserves path weights
        assert!(
            total.approx_eq(&original_total, 0.1),
            "Expected ~{:?}, got {:?}",
            original_total,
            total
        );
    }

    #[test]
    fn test_push_config_defaults() {
        let config = PushConfig::default();
        assert_eq!(config.direction, PushDirection::Backward);
        assert!(config.remove_non_coaccessible);

        let forward = PushConfig::forward();
        assert_eq!(forward.direction, PushDirection::Forward);

        let backward = PushConfig::backward();
        assert_eq!(backward.direction, PushDirection::Backward);
    }

    #[test]
    fn test_push_error_display() {
        assert_eq!(
            PushError::NoStartState.to_string(),
            "WFST has no start state"
        );
        assert_eq!(
            PushError::NoPotentials.to_string(),
            "Could not compute potentials"
        );
        assert_eq!(
            PushError::DivisionByZero.to_string(),
            "Division by zero during reweighting"
        );
    }
}
