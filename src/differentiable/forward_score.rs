//! Forward score computation for differentiable WFSTs.
//!
//! This module computes the forward score (total path weight) of a WFST
//! using the log semiring, enabling gradient computation for training.

use crate::semiring::{LogWeight, Semiring};
use crate::wfst::StateId;
use super::gradient::GradientWfst;

/// Compute the forward score of a WFST in the log semiring.
///
/// The forward score is the log-sum-exp over all path weights, which corresponds
/// to the total probability mass of all paths when weights are log-probabilities.
///
/// # Algorithm
///
/// 1. Initialize α[start] = 1̄ (log semiring one = 0.0)
/// 2. Process states in topological order
/// 3. For each arc (s, t, w): α[t] = α[t] ⊕ (α[s] ⊗ w)
/// 4. Total score = ⊕_{f ∈ F} (α[f] ⊗ final_weight[f])
///
/// # Complexity
///
/// O(|Q| + |E|) for acyclic WFSTs.
///
/// # Example
///
/// ```rust
/// use lling_llang::differentiable::{forward_score, GradientWfst};
/// use lling_llang::wfst::{VectorWfst, MutableWfst};
/// use lling_llang::semiring::{LogWeight, Semiring};
///
/// let mut fst = VectorWfst::<char, LogWeight>::new();
/// let s0 = fst.add_state();
/// let s1 = fst.add_state();
/// fst.set_start(s0);
/// fst.set_final(s1, LogWeight::one());
/// fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
///
/// let grad_fst = GradientWfst::from_wfst(&fst);
/// let score = forward_score(&grad_fst);
/// assert!((score.value() - 1.0).abs() < 1e-6);
/// ```
pub fn forward_score<L: Clone + Send + Sync>(grad_fst: &GradientWfst<L>) -> LogWeight {
    let num_states = grad_fst.num_states();

    if num_states == 0 {
        return LogWeight::zero();
    }

    let start = grad_fst.start();

    // Initialize forward scores
    // All states start at zero (log semiring zero = -∞)
    // Start state gets one (log semiring one = 0.0)
    for s in 0..num_states as StateId {
        grad_fst.set_forward_score(s, LogWeight::zero());
    }
    grad_fst.set_forward_score(start, LogWeight::one());

    // Compute topological order
    let topo_order = compute_topological_order(grad_fst);

    // Forward pass: compute α values
    for &state in &topo_order {
        let alpha_state = grad_fst.forward_score(state);

        // Skip if this state is unreachable
        if alpha_state.is_zero() {
            continue;
        }

        // Propagate to successors
        for trans in grad_fst.transitions(state) {
            let to_state = trans.to;
            let arc_weight = trans.weight;

            // α[to] = α[to] ⊕ (α[from] ⊗ arc_weight)
            let contribution = alpha_state.times(&arc_weight);
            let current = grad_fst.forward_score(to_state);
            grad_fst.set_forward_score(to_state, current.plus(&contribution));
        }
    }

    // Compute total score: sum over final states
    let mut total = LogWeight::zero();
    for s in 0..num_states as StateId {
        if grad_fst.is_final(s) {
            let alpha_s = grad_fst.forward_score(s);
            let final_weight = grad_fst.final_weight(s);
            let contribution = alpha_s.times(&final_weight);
            total = total.plus(&contribution);
        }
    }

    // Mark forward pass as complete and cache total score
    grad_fst.set_forward_computed(true);
    grad_fst.set_total_score(total);

    total
}

/// Compute log-sum-exp over all paths (alias for forward_score).
///
/// This function is an alias for `forward_score` that emphasizes
/// the mathematical operation being performed.
pub fn log_sum_exp_paths<L: Clone + Send + Sync>(grad_fst: &GradientWfst<L>) -> LogWeight {
    forward_score(grad_fst)
}

/// Compute topological order for forward pass.
fn compute_topological_order<L: Clone + Send + Sync>(grad_fst: &GradientWfst<L>) -> Vec<StateId> {
    let num_states = grad_fst.num_states();
    let mut in_degree = vec![0usize; num_states];
    let mut order = Vec::with_capacity(num_states);

    // Count in-degrees
    for s in 0..num_states as StateId {
        for trans in grad_fst.transitions(s) {
            in_degree[trans.to as usize] += 1;
        }
    }

    // Start with states having zero in-degree
    let mut queue: Vec<StateId> = (0..num_states as StateId)
        .filter(|&s| in_degree[s as usize] == 0)
        .collect();

    while let Some(state) = queue.pop() {
        order.push(state);
        for trans in grad_fst.transitions(state) {
            let to = trans.to as usize;
            in_degree[to] -= 1;
            if in_degree[to] == 0 {
                queue.push(trans.to);
            }
        }
    }

    // If not all states are in order, graph has cycles - use BFS order as fallback
    if order.len() < num_states {
        order = (0..num_states as StateId).collect();
    }

    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::{VectorWfst, MutableWfst};

    #[test]
    fn test_forward_score_empty() {
        let fst = VectorWfst::<char, LogWeight>::new();
        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = forward_score(&grad_fst);
        assert!(score.is_zero());
    }

    #[test]
    fn test_forward_score_no_path() {
        // Start state with no transitions to final
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        // No arc from s0 to s1

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = forward_score(&grad_fst);
        assert!(score.is_zero());
    }

    #[test]
    fn test_forward_score_single_arc() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::new(-0.5)); // Final weight = -0.5
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = forward_score(&grad_fst);

        // Path weight = 0.0 (start) + (-1.0) (arc) + (-0.5) (final) = -1.5
        assert!((score.value() - (-1.5)).abs() < 1e-6);
    }

    #[test]
    fn test_forward_score_chain() {
        // 0 --(-1.0)--> 1 --(-2.0)--> 2 (final, weight -0.5)
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s2, LogWeight::new(-0.5));
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));
        fst.add_arc(s1, Some('b'), Some('b'), s2, LogWeight::new(-2.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = forward_score(&grad_fst);

        // Path weight = 0 + (-1) + (-2) + (-0.5) = -3.5
        assert!((score.value() - (-3.5)).abs() < 1e-6);
    }

    #[test]
    fn test_forward_score_parallel_paths() {
        // Two parallel paths: 0 -> 1 with weights 1.0 and 2.0
        // (representing -log(prob), so prob = e^-1 and e^-2)
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(2.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = forward_score(&grad_fst);

        // Sum of probabilities: e^-1 + e^-2 ≈ 0.503
        // Negative log: -log(0.503) ≈ 0.687
        let prob_sum = (-1.0_f64).exp() + (-2.0_f64).exp();
        let expected = -prob_sum.ln();
        assert!((score.value() - expected).abs() < 1e-6);
    }

    #[test]
    fn test_forward_score_diamond() {
        // Diamond: 0 -> 1 -> 2 and 0 -> 2
        // LogWeight stores negative log probabilities (positive values = valid probs < 1)
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s2, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0)); // prob e^-1
        fst.add_arc(s1, Some('b'), Some('b'), s2, LogWeight::new(1.0)); // prob e^-1
        fst.add_arc(s0, Some('c'), Some('c'), s2, LogWeight::new(1.5)); // prob e^-1.5

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = forward_score(&grad_fst);

        // Path 1: 1.0 + 1.0 = 2.0 (prob e^-2)
        // Path 2: 1.5 (prob e^-1.5)
        // Total weight = -log(e^-2 + e^-1.5) ≈ 1.03
        let expected = -((-2.0_f64).exp() + (-1.5_f64).exp()).ln();
        assert!((score.value() - expected).abs() < 1e-6);
    }

    #[test]
    fn test_forward_score_multiple_finals() {
        // Two final states
        // LogWeight stores negative log probabilities (positive values = valid probs < 1)
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.set_final(s2, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0)); // prob e^-1
        fst.add_arc(s0, Some('b'), Some('b'), s2, LogWeight::new(2.0)); // prob e^-2

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = forward_score(&grad_fst);

        // Two paths: weight 1.0 (prob e^-1) and weight 2.0 (prob e^-2)
        // Total weight = -log(e^-1 + e^-2) ≈ 0.687
        let expected = -((-1.0_f64).exp() + (-2.0_f64).exp()).ln();
        assert!((score.value() - expected).abs() < 1e-6);
    }

    #[test]
    fn test_forward_computed_flag() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s0, LogWeight::one());

        let grad_fst = GradientWfst::from_wfst(&fst);
        assert!(!grad_fst.is_forward_computed());

        let _ = forward_score(&grad_fst);
        assert!(grad_fst.is_forward_computed());
    }
}
