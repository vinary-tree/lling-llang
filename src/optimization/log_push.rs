//! Log-semiring weight pushing for beam search optimization.
//!
//! This module provides specialized weight pushing using the log semiring to
//! improve beam search pruning efficacy. The key insight from Mohri et al. is
//! that log-semiring pushing (NOT tropical) creates a stochastic automaton
//! that "synchronizes" scores for optimal pruning decisions.
//!
//! ## Why Log Semiring?
//!
//! - **Tropical semiring pushing** uses the min-weight potential (best path)
//!   - Can actually *harm* beam search by distorting relative scores
//!   - "May slow down beam-pruned Viterbi decoding many fold"
//!
//! - **Log semiring pushing** uses the sum of all path probabilities
//!   - Creates a stochastic automaton (weights sum to 1 at each state)
//!   - "Has a very large beneficial impact on pruning efficacy"
//!   - Conjecture: "Optimal likelihood ratio test for pruning decisions"
//!
//! ## Algorithm
//!
//! 1. Compute backward potentials V(q) = -log(Σ_{paths from q to final} exp(-weight))
//! 2. Reweight: w'(e) = w(e) + V(target) - V(source)
//! 3. Result: At each state, Σ exp(-outgoing_weights) = 1
//!
//! ## Performance
//!
//! The papers report up to 18× speedup in beam-pruned Viterbi decoding
//! when using log-semiring pushed WFSTs vs unpushed.
//!
//! ## References
//!
//! - Mohri, Pereira, Riley (2002): "WFSTs in Speech Recognition"
//! - Mohri, Pereira, Riley (2008): "Speech Recognition with WFSTs" (Handbook)

use crate::semiring::{DivisibleSemiring, LogWeight, Semiring};
use crate::wfst::{MutableWfst, StateId, WeightedTransition, Wfst, NO_STATE};

/// Configuration for log-semiring weight pushing for beam search.
#[derive(Clone, Debug)]
pub struct LogPushConfig {
    /// Whether to verify stochasticity after pushing.
    pub verify_stochastic: bool,
    /// Tolerance for stochasticity check (weights sum to 1 ± epsilon).
    pub stochastic_epsilon: f64,
    /// Whether to normalize final states (set to LogWeight::one()).
    pub normalize_finals: bool,
}

impl Default for LogPushConfig {
    fn default() -> Self {
        Self {
            verify_stochastic: false,
            stochastic_epsilon: 1e-6,
            normalize_finals: true,
        }
    }
}

impl LogPushConfig {
    /// Create a configuration that verifies stochasticity.
    pub fn verified() -> Self {
        Self {
            verify_stochastic: true,
            ..Default::default()
        }
    }
}

/// Result of preparing a WFST for beam search.
#[derive(Clone, Debug, PartialEq)]
pub struct BeamSearchPrepResult {
    /// Whether the WFST was successfully pushed.
    pub pushed: bool,
    /// The total weight of the original WFST (initial state potential).
    pub total_weight: LogWeight,
    /// Whether the result is stochastic (if verification was enabled).
    pub is_stochastic: Option<bool>,
    /// Number of states in the WFST.
    pub num_states: usize,
    /// Number of transitions in the WFST.
    pub num_transitions: usize,
}

/// Prepare a LogWeight WFST for efficient beam search by applying log-semiring pushing.
///
/// This function applies backward log-semiring weight pushing, which creates a
/// stochastic automaton optimal for beam search pruning. The pushed WFST has
/// the property that at each state, the sum of exp(-weights) equals 1.
///
/// # Algorithm
///
/// 1. Compute backward potentials using log semiring (sum of all path probabilities)
/// 2. Reweight transitions: w' = w + V(target) - V(source)
/// 3. Normalize final weights to LogWeight::one() (0.0 in log space = probability 1)
///
/// # Arguments
///
/// * `fst` - The WFST to prepare (modified in place)
/// * `config` - Configuration options
///
/// # Returns
///
/// * `Ok(BeamSearchPrepResult)` - Push succeeded with statistics
/// * `Err(LogPushError)` - Push failed
///
/// # Example
///
/// ```ignore
/// use lling_llang::optimization::{prepare_for_beam_search, LogPushConfig};
/// use lling_llang::semiring::LogWeight;
/// use lling_llang::wfst::VectorWfst;
///
/// let mut fst: VectorWfst<char, LogWeight> = build_recognition_wfst();
/// let result = prepare_for_beam_search(&mut fst, LogPushConfig::default())?;
/// println!("Total weight: {:?}", result.total_weight);
/// // Now use fst with beam search for improved pruning
/// ```
pub fn prepare_for_beam_search<L, F>(
    fst: &mut F,
    config: LogPushConfig,
) -> Result<BeamSearchPrepResult, LogPushError>
where
    L: Clone,
    F: MutableWfst<L, LogWeight> + Wfst<L, LogWeight>,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(BeamSearchPrepResult {
            pushed: false,
            total_weight: LogWeight::zero(),
            is_stochastic: Some(true),
            num_states: 0,
            num_transitions: 0,
        });
    }

    if fst.start() == NO_STATE {
        return Err(LogPushError::NoStartState);
    }

    // Count transitions
    let num_transitions: usize = (0..n).map(|s| fst.transitions(s as StateId).len()).sum();

    // Compute backward potentials in log semiring
    let potentials = compute_log_potentials(fst)?;

    // Get total weight (potential at start state)
    let start = fst.start() as usize;
    let total_weight = if start < potentials.len() {
        potentials[start].clone()
    } else {
        LogWeight::zero()
    };

    // Apply the push
    apply_log_push(fst, &potentials, config.normalize_finals)?;

    // Verify stochasticity if requested
    let is_stochastic = if config.verify_stochastic {
        Some(verify_stochastic(fst, config.stochastic_epsilon))
    } else {
        None
    };

    Ok(BeamSearchPrepResult {
        pushed: true,
        total_weight,
        is_stochastic,
        num_states: n,
        num_transitions,
    })
}

/// Compute backward log-semiring potentials.
///
/// For each state q, V(q) = -log(Σ_{paths from q to final} exp(-path_weight))
///
/// This is the total probability mass of all paths from state q to any final state.
///
/// # Algorithm
///
/// Uses reverse topological order:
/// 1. Initialize V(final) = final_weight
/// 2. For each state in reverse topological order:
///    V(q) = logadd_{outgoing arcs} (arc_weight + V(target))
///
/// # Complexity
///
/// O(|Q| + |E|) for acyclic WFSTs
pub fn compute_log_potentials<L, F>(fst: &F) -> Result<Vec<LogWeight>, LogPushError>
where
    L: Clone,
    F: Wfst<L, LogWeight>,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(Vec::new());
    }

    if fst.start() == NO_STATE {
        return Err(LogPushError::NoStartState);
    }

    // Initialize potentials to zero (log semiring zero = -∞ = probability 0)
    let mut potentials = vec![LogWeight::zero(); n];

    // Initialize final states with their final weights
    for state in 0..n {
        let state_id = state as StateId;
        if fst.is_final(state_id) {
            potentials[state] = fst.final_weight(state_id);
        }
    }

    // Compute topological order
    let topo_order = compute_topological_order(fst);

    // Process in reverse topological order
    for &state in topo_order.iter().rev() {
        let state_idx = state as usize;

        // Accumulate potential from outgoing transitions
        for trans in fst.transitions(state) {
            let to_idx = trans.to as usize;
            if to_idx >= n {
                continue;
            }

            // V(q) = V(q) ⊕ (arc_weight ⊗ V(target))
            // In log semiring: logadd(V(q), arc_weight + V(target))
            let contribution = trans.weight.times(&potentials[to_idx]);
            potentials[state_idx] = potentials[state_idx].plus(&contribution);
        }
    }

    // Verify start state is reachable to finals
    let start = fst.start() as usize;
    if start < n && potentials[start].is_zero() {
        return Err(LogPushError::NoPathToFinal);
    }

    Ok(potentials)
}

/// Apply log-semiring pushing using precomputed potentials.
///
/// Reweights the WFST so that:
/// - w'(e) = w(e) + V(target) - V(source)
/// - Final weights become LogWeight::one() if normalize_finals is true
///
/// # Arguments
///
/// * `fst` - The WFST to modify
/// * `potentials` - Precomputed backward potentials
/// * `normalize_finals` - Whether to set final weights to one
pub fn apply_log_push<L, F>(
    fst: &mut F,
    potentials: &[LogWeight],
    normalize_finals: bool,
) -> Result<(), LogPushError>
where
    L: Clone,
    F: MutableWfst<L, LogWeight> + Wfst<L, LogWeight>,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(());
    }

    // Collect and reweight all transitions
    let mut new_transitions: Vec<Vec<WeightedTransition<L, LogWeight>>> = vec![Vec::new(); n];

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

            // New weight: w + V(target) - V(source)
            // In log semiring: w ⊗ V(target) ⊗ V(source)^{-1}
            // = w.times(p_to).divide(p_from)
            let w_times_to = trans.weight.times(p_to);
            let new_weight = w_times_to
                .divide(p_from)
                .unwrap_or_else(|| trans.weight.clone());

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

    // Normalize final weights if requested
    if normalize_finals {
        for state in 0..n {
            let state_id = state as StateId;
            if fst.is_final(state_id) {
                fst.set_final(state_id, LogWeight::one());
            }
        }
    }

    Ok(())
}

/// Verify that a WFST is stochastic after pushing.
///
/// A stochastic WFST has the property that at each state, the sum of
/// outgoing transition probabilities (including final probability) equals 1.
///
/// In log space, this means: logadd(all outgoing weights + final weight) ≈ 0
fn verify_stochastic<L, F>(fst: &F, epsilon: f64) -> bool
where
    L: Clone,
    F: Wfst<L, LogWeight>,
{
    for state in 0..fst.num_states() {
        let state_id = state as StateId;

        // Sum all outgoing weights (including final)
        let mut total = fst.final_weight(state_id);

        for trans in fst.transitions(state_id) {
            total = total.plus(&trans.weight);
        }

        // Check if total ≈ one (0.0 in log space)
        // A stochastic state has logadd(weights) = 0, meaning Σ exp(-w) = 1
        if !total.approx_eq(&LogWeight::one(), epsilon) {
            // Allow states with no outgoing transitions and non-final
            if !fst.is_final(state_id) && fst.transitions(state_id).is_empty() {
                continue;
            }
            // Also skip unreachable states (all transitions have infinite weight)
            if total.is_zero() && fst.transitions(state_id).is_empty() && !fst.is_final(state_id) {
                continue;
            }
            return false;
        }
    }
    true
}

/// Compute topological order for the FST.
fn compute_topological_order<L, F>(fst: &F) -> Vec<StateId>
where
    L: Clone,
    F: Wfst<L, LogWeight>,
{
    let n = fst.num_states();
    let mut in_degree = vec![0usize; n];
    let mut order = Vec::with_capacity(n);

    // Count in-degrees
    for s in 0..n {
        let state_id = s as StateId;
        for trans in fst.transitions(state_id) {
            let to = trans.to as usize;
            if to < n {
                in_degree[to] += 1;
            }
        }
    }

    // Start with states having zero in-degree
    let mut queue: Vec<StateId> = (0..n as StateId)
        .filter(|&s| in_degree[s as usize] == 0)
        .collect();

    while let Some(state) = queue.pop() {
        order.push(state);
        for trans in fst.transitions(state) {
            let to = trans.to as usize;
            if to < n {
                in_degree[to] -= 1;
                if in_degree[to] == 0 {
                    queue.push(trans.to);
                }
            }
        }
    }

    // If not all states in order, graph has cycles - use sequential order
    if order.len() < n {
        order = (0..n as StateId).collect();
    }

    order
}

/// Errors that can occur during log-semiring pushing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogPushError {
    /// The WFST has no start state.
    NoStartState,
    /// No path exists from start to any final state.
    NoPathToFinal,
    /// Division by zero during reweighting.
    DivisionByZero,
}

impl std::fmt::Display for LogPushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoStartState => write!(f, "WFST has no start state"),
            Self::NoPathToFinal => write!(f, "No path from start to final states"),
            Self::DivisionByZero => write!(f, "Division by zero during reweighting"),
        }
    }
}

impl std::error::Error for LogPushError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::{MutableWfst as MutableWfstTrait, VectorWfst};

    fn build_simple_chain() -> VectorWfst<char, LogWeight> {
        // 0 --a/1.0--> 1 --b/2.0--> 2 (final, weight 0.0)
        let mut fst = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s2, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s1, Some('b'), Some('b'), s2, LogWeight::new(2.0));
        fst
    }

    fn build_parallel_paths() -> VectorWfst<char, LogWeight> {
        // Two parallel paths from 0 to 1
        // Path 1: weight 1.0
        // Path 2: weight 2.0
        let mut fst = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(2.0));
        fst
    }

    fn build_diamond() -> VectorWfst<char, LogWeight> {
        // Diamond: 0 -> 1 -> 3 (weight 1+1=2)
        //          0 -> 2 -> 3 (weight 2+0.5=2.5)
        let mut fst = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        let s3 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s3, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s0, Some('b'), Some('b'), s2, LogWeight::new(2.0));
        fst.add_arc(s1, Some('c'), Some('c'), s3, LogWeight::new(1.0));
        fst.add_arc(s2, Some('d'), Some('d'), s3, LogWeight::new(0.5));
        fst
    }

    #[test]
    fn test_compute_potentials_chain() {
        let fst = build_simple_chain();
        let potentials = compute_log_potentials(&fst).expect("Should compute potentials");

        assert_eq!(potentials.len(), 3);

        // State 2 (final): potential = final_weight = 0.0
        assert!(
            potentials[2].approx_eq(&LogWeight::one(), 0.001),
            "Final state potential should be one, got {:?}",
            potentials[2]
        );

        // State 1: potential = arc_weight(1->2) + potential[2] = 2.0 + 0.0 = 2.0
        assert!(
            potentials[1].approx_eq(&LogWeight::new(2.0), 0.001),
            "State 1 potential should be 2.0, got {:?}",
            potentials[1]
        );

        // State 0: potential = arc_weight(0->1) + potential[1] = 1.0 + 2.0 = 3.0
        assert!(
            potentials[0].approx_eq(&LogWeight::new(3.0), 0.001),
            "State 0 potential should be 3.0, got {:?}",
            potentials[0]
        );
    }

    #[test]
    fn test_compute_potentials_parallel() {
        let fst = build_parallel_paths();
        let potentials = compute_log_potentials(&fst).expect("Should compute potentials");

        // State 1 (final): potential = 0.0
        assert!(potentials[1].approx_eq(&LogWeight::one(), 0.001));

        // State 0: potential = logadd(1.0, 2.0) = -log(e^-1 + e^-2) ≈ 0.687
        let expected = -((-1.0_f64).exp() + (-2.0_f64).exp()).ln();
        assert!(
            potentials[0].approx_eq(&LogWeight::new(expected), 0.001),
            "State 0 potential should be {:?}, got {:?}",
            expected,
            potentials[0]
        );
    }

    #[test]
    fn test_prepare_for_beam_search_chain() {
        let mut fst = build_simple_chain();
        let result =
            prepare_for_beam_search(&mut fst, LogPushConfig::default()).expect("Should prepare");

        assert!(result.pushed);
        assert_eq!(result.num_states, 3);
        assert_eq!(result.num_transitions, 2);

        // Total weight should be the original path weight
        assert!(
            result.total_weight.approx_eq(&LogWeight::new(3.0), 0.001),
            "Total weight should be 3.0, got {:?}",
            result.total_weight
        );

        // After pushing, final weight should be one
        assert!(fst.final_weight(2).approx_eq(&LogWeight::one(), 0.001));

        // Verify path weight is normalized
        // After backward push, the sum of path weights should be one
        let trans_0 = &fst.transitions(0)[0];
        let trans_1 = &fst.transitions(1)[0];
        let path_weight = trans_0
            .weight
            .times(&trans_1.weight)
            .times(&fst.final_weight(2));

        // The pushed path weight should be one (the total weight is absorbed)
        assert!(
            path_weight.approx_eq(&LogWeight::one(), 0.001),
            "Normalized path weight should be one, got {:?}",
            path_weight
        );
    }

    #[test]
    fn test_prepare_for_beam_search_parallel() {
        let mut fst = build_parallel_paths();
        let result =
            prepare_for_beam_search(&mut fst, LogPushConfig::verified()).expect("Should prepare");

        assert!(result.pushed);
        assert_eq!(result.is_stochastic, Some(true));

        // Check that weights at state 0 sum to 1 (in probability space)
        let mut total = LogWeight::zero();
        for trans in fst.transitions(0) {
            total = total.plus(&trans.weight);
        }

        // Should be approximately one (logadd of pushed weights = 0)
        assert!(
            total.approx_eq(&LogWeight::one(), 0.01),
            "Pushed weights should sum to one, got {:?} (expected ~0.0 in log space)",
            total
        );
    }

    #[test]
    fn test_prepare_for_beam_search_diamond() {
        let mut fst = build_diamond();
        let result =
            prepare_for_beam_search(&mut fst, LogPushConfig::verified()).expect("Should prepare");

        assert!(result.pushed);
        assert_eq!(result.num_states, 4);
        assert_eq!(result.num_transitions, 4);

        // Verify is_stochastic is true (or at least not false)
        // Note: For non-start states with single outgoing edge, the sum may not be exactly one
        // but the stochasticity check should pass for valid states
        assert!(
            result.is_stochastic == Some(true),
            "Should be stochastic after push"
        );
    }

    #[test]
    fn test_prepare_empty_fst() {
        let mut fst: VectorWfst<char, LogWeight> = VectorWfst::new();
        let result = prepare_for_beam_search(&mut fst, LogPushConfig::default())
            .expect("Should handle empty");

        assert!(!result.pushed);
        assert_eq!(result.num_states, 0);
    }

    #[test]
    fn test_prepare_no_start() {
        let mut fst: VectorWfst<char, LogWeight> = VectorWfst::new();
        fst.add_state();

        let result = prepare_for_beam_search(&mut fst, LogPushConfig::default());
        assert_eq!(result, Err(LogPushError::NoStartState));
    }

    #[test]
    fn test_prepare_no_path_to_final() {
        let mut fst: VectorWfst<char, LogWeight> = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        // No arc from s0 to s1

        let result = prepare_for_beam_search(&mut fst, LogPushConfig::default());
        assert_eq!(result, Err(LogPushError::NoPathToFinal));
    }

    #[test]
    fn test_error_display() {
        assert_eq!(
            LogPushError::NoStartState.to_string(),
            "WFST has no start state"
        );
        assert_eq!(
            LogPushError::NoPathToFinal.to_string(),
            "No path from start to final states"
        );
        assert_eq!(
            LogPushError::DivisionByZero.to_string(),
            "Division by zero during reweighting"
        );
    }

    #[test]
    fn test_log_push_preserves_total_weight() {
        let mut fst = build_simple_chain();

        // Original total weight (computed as forward score)
        let original_potentials = compute_log_potentials(&fst).expect("potentials");
        let original_total = original_potentials[0].clone();

        // Push
        let result = prepare_for_beam_search(&mut fst, LogPushConfig::default()).expect("push");

        // Total weight from result should match original
        assert!(
            result.total_weight.approx_eq(&original_total, 0.001),
            "Total weight should be preserved: expected {:?}, got {:?}",
            original_total,
            result.total_weight
        );
    }
}
