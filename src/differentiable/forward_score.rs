//! Forward score computation for differentiable WFSTs.
//!
//! This module computes the forward score (total path weight) of a WFST
//! using the log semiring, enabling gradient computation for training.

use super::gradient::GradientWfst;
use crate::semiring::{LogWeight, Semiring};
use crate::wfst::StateId;

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

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::wfst::{MutableWfst, VectorWfst, Wfst};
    use proptest::prelude::*;

    /// Strategy for generating simple chain WFSTs.
    fn arb_chain_wfst(max_length: usize) -> impl Strategy<Value = VectorWfst<char, LogWeight>> {
        (1..=max_length).prop_flat_map(|len| {
            proptest::collection::vec(-5.0f64..5.0, len).prop_map(move |weights| {
                let mut fst = VectorWfst::new();
                for _ in 0..=len {
                    fst.add_state();
                }
                fst.set_start(0);
                fst.set_final(len as u32, LogWeight::one());
                for (i, w) in weights.iter().enumerate() {
                    let label = (b'a' + (i % 26) as u8) as char;
                    fst.add_arc(
                        i as u32,
                        Some(label),
                        Some(label),
                        (i + 1) as u32,
                        LogWeight::new(*w),
                    );
                }
                fst
            })
        })
    }

    /// Strategy for generating parallel path WFSTs.
    fn arb_parallel_wfst(max_paths: usize) -> impl Strategy<Value = VectorWfst<char, LogWeight>> {
        proptest::collection::vec(-5.0f64..5.0, 1..=max_paths).prop_map(|weights| {
            let mut fst = VectorWfst::new();
            let s0 = fst.add_state();
            let s1 = fst.add_state();
            fst.set_start(s0);
            fst.set_final(s1, LogWeight::one());
            for (i, w) in weights.iter().enumerate() {
                let label = (b'a' + (i % 26) as u8) as char;
                fst.add_arc(s0, Some(label), Some(label), s1, LogWeight::new(*w));
            }
            fst
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Forward score of chain equals sum of arc weights.
        #[test]
        fn forward_chain_equals_weight_sum(fst in arb_chain_wfst(5)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let score = forward_score(&grad_fst);

            // For a chain, forward score = sum of arc weights
            let mut expected = 0.0;
            for s in 0..fst.num_states() as u32 {
                for arc in fst.transitions(s) {
                    expected += arc.weight.value();
                }
            }

            prop_assert!((score.value() - expected).abs() < 1e-6,
                "Chain score {} != expected {}", score.value(), expected);
        }

        /// Forward score of parallel paths is log-sum-exp.
        #[test]
        fn forward_parallel_is_logsumexp(fst in arb_parallel_wfst(5)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let score = forward_score(&grad_fst);

            // For parallel paths, total = -log(sum_i exp(-w_i))
            let weights: Vec<f64> = fst.transitions(0).iter()
                .map(|arc| arc.weight.value())
                .collect();

            if !weights.is_empty() {
                let probs: f64 = weights.iter().map(|w| (-w).exp()).sum();
                let expected = -probs.ln();
                prop_assert!((score.value() - expected).abs() < 1e-6,
                    "Parallel score {} != expected {}", score.value(), expected);
            }
        }

        /// Forward pass sets computed flag.
        #[test]
        fn forward_sets_computed_flag(fst in arb_chain_wfst(3)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            prop_assert!(!grad_fst.is_forward_computed());
            let _ = forward_score(&grad_fst);
            prop_assert!(grad_fst.is_forward_computed());
        }

        /// Forward score is deterministic.
        #[test]
        fn forward_is_deterministic(fst in arb_parallel_wfst(4)) {
            let grad_fst1 = GradientWfst::from_wfst(&fst);
            let grad_fst2 = GradientWfst::from_wfst(&fst);

            let score1 = forward_score(&grad_fst1);
            let score2 = forward_score(&grad_fst2);

            prop_assert!((score1.value() - score2.value()).abs() < 1e-9,
                "Scores differ: {} vs {}", score1.value(), score2.value());
        }

        /// log_sum_exp_paths is alias for forward_score.
        #[test]
        fn logsumexp_alias(fst in arb_chain_wfst(3)) {
            let grad_fst1 = GradientWfst::from_wfst(&fst);
            let grad_fst2 = GradientWfst::from_wfst(&fst);

            let score1 = forward_score(&grad_fst1);
            let score2 = log_sum_exp_paths(&grad_fst2);

            prop_assert!((score1.value() - score2.value()).abs() < 1e-9,
                "forward_score {} != log_sum_exp_paths {}", score1.value(), score2.value());
        }

        /// Forward score caches total in GradientWfst.
        #[test]
        fn forward_caches_total(fst in arb_chain_wfst(3)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            prop_assert!(grad_fst.total_score().is_none());

            let score = forward_score(&grad_fst);
            let cached = grad_fst.total_score();

            prop_assert!(cached.is_some());
            prop_assert!((cached.expect("differentiable/forward_score.rs: required value was None/Err").value() - score.value()).abs() < 1e-9);
        }

        /// Forward scores at final states match total for chain.
        #[test]
        fn forward_final_matches_total(fst in arb_chain_wfst(4)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let score = forward_score(&grad_fst);

            // For a chain, forward score at final state equals total
            let final_state = (fst.num_states() - 1) as u32;
            let final_score = grad_fst.forward_score(final_state);

            // Total includes final weight (which is 1 = log 0)
            prop_assert!((final_score.value() - score.value()).abs() < 1e-6,
                "Final state score {} != total {}", final_score.value(), score.value());
        }

        /// Forward score is non-zero for connected WFST.
        #[test]
        fn forward_connected_non_zero(fst in arb_chain_wfst(3)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let score = forward_score(&grad_fst);
            prop_assert!(!score.is_zero(), "Forward score should not be zero for connected WFST");
        }

        /// Forward score increases with added high-probability path.
        #[test]
        fn forward_increases_with_better_path(
            base_weight in -5.0f64..5.0,
            better_weight in -10.0f64..-5.0
        ) {
            // Create base FST with one path
            let mut fst1 = VectorWfst::<char, LogWeight>::new();
            let s0 = fst1.add_state();
            let s1 = fst1.add_state();
            fst1.set_start(s0);
            fst1.set_final(s1, LogWeight::one());
            fst1.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(base_weight));

            // Create FST with additional better path (lower weight = higher probability)
            let mut fst2 = VectorWfst::<char, LogWeight>::new();
            let s0 = fst2.add_state();
            let s1 = fst2.add_state();
            fst2.set_start(s0);
            fst2.set_final(s1, LogWeight::one());
            fst2.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(base_weight));
            fst2.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(better_weight));

            let grad1 = GradientWfst::from_wfst(&fst1);
            let grad2 = GradientWfst::from_wfst(&fst2);

            let score1 = forward_score(&grad1);
            let score2 = forward_score(&grad2);

            // More paths = lower total weight (higher total probability)
            prop_assert!(score2.value() <= score1.value() + 1e-9,
                "Adding path should decrease weight: {} should be <= {}", score2.value(), score1.value());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::{MutableWfst, VectorWfst};

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
