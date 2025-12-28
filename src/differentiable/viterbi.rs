//! Viterbi score computation for differentiable WFSTs.
//!
//! This module computes the Viterbi (best path) score of a WFST
//! using the tropical semiring interpretation, with gradient support.

use crate::semiring::{LogWeight, Semiring};
use crate::wfst::StateId;
use super::gradient::{GradientWfst, ArcIndex, GradientAccumulator};

/// Result of Viterbi path computation with gradients.
#[derive(Clone, Debug)]
pub struct ViterbiGradResult {
    /// The Viterbi (best path) score.
    pub score: LogWeight,
    /// The best path as a sequence of arc indices.
    pub path: Vec<ArcIndex>,
    /// Gradients for arcs (1.0 for arcs on best path, 0.0 otherwise).
    pub gradients: GradientAccumulator,
}

/// Compute the Viterbi (best path) score of a WFST.
///
/// This computes the minimum weight path (in the tropical semiring sense)
/// from start to any final state. For log-probability weights, this
/// corresponds to the maximum probability path.
///
/// # Algorithm
///
/// 1. Initialize δ[start] = 0 (tropical one)
/// 2. Process states in topological order
/// 3. For each arc (s, t, w): δ[t] = min(δ[t], δ[s] + w)
/// 4. Best score = min_{f ∈ F}(δ[f] + final_weight[f])
///
/// # Complexity
///
/// O(|Q| + |E|) for acyclic WFSTs.
///
/// # Example
///
/// ```rust
/// use lling_llang::differentiable::{viterbi_score, GradientWfst};
/// use lling_llang::wfst::{VectorWfst, MutableWfst};
/// use lling_llang::semiring::{LogWeight, Semiring};
///
/// let mut fst = VectorWfst::<char, LogWeight>::new();
/// let s0 = fst.add_state();
/// let s1 = fst.add_state();
/// fst.set_start(s0);
/// fst.set_final(s1, LogWeight::one());
/// fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
/// fst.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(2.0));
///
/// let grad_fst = GradientWfst::from_wfst(&fst);
/// let score = viterbi_score(&grad_fst);
/// // Best path has weight 1.0 (min of 1.0 and 2.0 = highest probability)
/// assert!((score.value() - 1.0).abs() < 1e-6);
/// ```
pub fn viterbi_score<L: Clone + Send + Sync>(grad_fst: &GradientWfst<L>) -> LogWeight {
    let num_states = grad_fst.num_states();

    if num_states == 0 {
        return LogWeight::zero();
    }

    let start = grad_fst.start();

    // Use tropical semiring for Viterbi (min, +)
    // We use f64::INFINITY as the tropical zero (unreachable)
    let mut delta = vec![f64::INFINITY; num_states];
    delta[start as usize] = 0.0;

    // Compute topological order
    let topo_order = compute_topological_order(grad_fst);

    // Forward pass: compute δ values (best path to each state)
    for &state in &topo_order {
        let delta_state = delta[state as usize];

        // Skip if unreachable
        if delta_state.is_infinite() {
            continue;
        }

        for trans in grad_fst.transitions(state) {
            let to_state = trans.to;
            // In log semiring, arc weights are already negative log-probs
            // Tropical ⊗ = +, so new_delta = delta_state + arc_weight
            let arc_weight = trans.weight.value();
            let new_delta = delta_state + arc_weight;

            // Tropical ⊕ = min
            if new_delta < delta[to_state as usize] {
                delta[to_state as usize] = new_delta;
            }
        }
    }

    // Find best final state score
    let mut best_score = f64::INFINITY;
    for s in 0..num_states as StateId {
        if grad_fst.is_final(s) {
            let final_weight = grad_fst.final_weight(s).value();
            let total = delta[s as usize] + final_weight;
            if total < best_score {
                best_score = total;
            }
        }
    }

    if best_score.is_infinite() {
        LogWeight::zero()
    } else {
        LogWeight::new(best_score)
    }
}

/// Compute Viterbi path with gradients.
///
/// This returns both the best path and gradients that are 1.0 for arcs
/// on the best path and 0.0 for other arcs. This is useful for
/// sequence-level training where gradients only flow through the best path.
///
/// # Returns
///
/// A `ViterbiGradResult` containing the score, path, and gradients.
pub fn viterbi_path_with_grad<L: Clone + Send + Sync>(
    grad_fst: &GradientWfst<L>,
) -> ViterbiGradResult {
    let num_states = grad_fst.num_states();

    if num_states == 0 {
        return ViterbiGradResult {
            score: LogWeight::zero(),
            path: Vec::new(),
            gradients: GradientAccumulator::new(),
        };
    }

    let start = grad_fst.start();

    // Forward pass with backpointers
    let mut delta = vec![f64::INFINITY; num_states];
    let mut backpointers: Vec<Option<(StateId, usize)>> = vec![None; num_states];
    delta[start as usize] = 0.0;

    let topo_order = compute_topological_order(grad_fst);

    for &state in &topo_order {
        let delta_state = delta[state as usize];

        if delta_state.is_infinite() {
            continue;
        }

        for (arc_idx, trans) in grad_fst.transitions(state).iter().enumerate() {
            let to_state = trans.to;
            let arc_weight = trans.weight.value();
            let new_delta = delta_state + arc_weight;

            if new_delta < delta[to_state as usize] {
                delta[to_state as usize] = new_delta;
                backpointers[to_state as usize] = Some((state, arc_idx));
            }
        }
    }

    // Find best final state
    let mut best_final: Option<StateId> = None;
    let mut best_score = f64::INFINITY;

    for s in 0..num_states as StateId {
        if grad_fst.is_final(s) {
            let final_weight = grad_fst.final_weight(s).value();
            let total = delta[s as usize] + final_weight;
            if total < best_score {
                best_score = total;
                best_final = Some(s);
            }
        }
    }

    // Traceback to get path
    let mut path = Vec::new();
    if let Some(final_state) = best_final {
        let mut current = final_state;
        while let Some((prev_state, arc_idx)) = backpointers[current as usize] {
            path.push(ArcIndex::new(prev_state, arc_idx));
            current = prev_state;
        }
        path.reverse();
    }

    // Build gradients (1.0 for arcs on path, 0.0 otherwise)
    let mut gradients = GradientAccumulator::new();
    for arc in &path {
        gradients.add_gradient(*arc, 1.0);
    }

    ViterbiGradResult {
        score: if best_score.is_infinite() {
            LogWeight::zero()
        } else {
            LogWeight::new(best_score)
        },
        path,
        gradients,
    }
}

/// Compute topological order for Viterbi.
fn compute_topological_order<L: Clone + Send + Sync>(grad_fst: &GradientWfst<L>) -> Vec<StateId> {
    let num_states = grad_fst.num_states();
    let mut in_degree = vec![0usize; num_states];
    let mut order = Vec::with_capacity(num_states);

    for s in 0..num_states as StateId {
        for trans in grad_fst.transitions(s) {
            in_degree[trans.to as usize] += 1;
        }
    }

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
    use crate::wfst::{VectorWfst, MutableWfst, Wfst};
    use proptest::prelude::*;

    /// Strategy for generating parallel path WFSTs.
    fn arb_parallel_wfst(max_paths: usize) -> impl Strategy<Value = VectorWfst<char, LogWeight>> {
        proptest::collection::vec(-5.0f64..5.0, 1..=max_paths)
            .prop_map(|weights| {
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

    /// Strategy for generating chain WFSTs.
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
                    fst.add_arc(i as u32, Some(label), Some(label), (i + 1) as u32, LogWeight::new(*w));
                }
                fst
            })
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Viterbi finds the minimum weight path for parallel paths.
        #[test]
        fn viterbi_finds_min_weight(fst in arb_parallel_wfst(5)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let score = viterbi_score(&grad_fst);

            // Find minimum weight manually
            let min_weight = fst.transitions(0).iter()
                .map(|arc| arc.weight.value())
                .fold(f64::INFINITY, f64::min);

            prop_assert!((score.value() - min_weight).abs() < 1e-6,
                "Viterbi score {} != min weight {}", score.value(), min_weight);
        }

        /// Viterbi score equals chain weight sum.
        #[test]
        fn viterbi_chain_equals_sum(fst in arb_chain_wfst(5)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let score = viterbi_score(&grad_fst);

            // For a chain, Viterbi score = sum of arc weights
            let expected: f64 = fst.transitions(0).iter()
                .chain((1..fst.num_states() as u32).flat_map(|s| fst.transitions(s).iter()))
                .map(|arc| arc.weight.value())
                .sum();

            prop_assert!((score.value() - expected).abs() < 1e-6,
                "Viterbi chain score {} != expected {}", score.value(), expected);
        }

        /// Viterbi score is deterministic.
        #[test]
        fn viterbi_deterministic(fst in arb_parallel_wfst(4)) {
            let grad_fst1 = GradientWfst::from_wfst(&fst);
            let grad_fst2 = GradientWfst::from_wfst(&fst);

            let score1 = viterbi_score(&grad_fst1);
            let score2 = viterbi_score(&grad_fst2);

            prop_assert!((score1.value() - score2.value()).abs() < 1e-9,
                "Viterbi scores differ: {} vs {}", score1.value(), score2.value());
        }

        /// Viterbi score <= forward score (best path <= total).
        #[test]
        fn viterbi_leq_forward(fst in arb_parallel_wfst(5)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let viterbi = viterbi_score(&grad_fst);

            // Reset and compute forward score
            grad_fst.reset();
            let forward = super::super::forward_score::forward_score(&grad_fst);

            // Viterbi (min) <= Forward (log-sum-exp)
            prop_assert!(viterbi.value() >= forward.value() - 1e-6,
                "Viterbi {} < forward {} (should be >=)", viterbi.value(), forward.value());
        }

        /// Viterbi path has correct length.
        #[test]
        fn viterbi_path_correct_length(fst in arb_chain_wfst(4)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let result = viterbi_path_with_grad(&grad_fst);

            // Chain should have path length = number of arcs
            let expected_len = fst.num_states() - 1;
            prop_assert_eq!(result.path.len(), expected_len,
                "Path length {} != expected {}", result.path.len(), expected_len);
        }

        /// Viterbi path gradients sum to path length.
        #[test]
        fn viterbi_path_grad_sum(fst in arb_chain_wfst(4)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let result = viterbi_path_with_grad(&grad_fst);

            // Each arc on path has gradient 1.0
            let grad_sum: f64 = result.gradients.arc_gradients.iter()
                .map(|g| g.gradient)
                .sum();

            prop_assert!((grad_sum - result.path.len() as f64).abs() < 1e-6,
                "Gradient sum {} != path length {}", grad_sum, result.path.len());
        }

        /// Viterbi path only contains best arc when parallel.
        #[test]
        fn viterbi_path_selects_best(fst in arb_parallel_wfst(5)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let result = viterbi_path_with_grad(&grad_fst);

            // Should have exactly one arc in path
            prop_assert_eq!(result.path.len(), 1);

            // Find the minimum weight arc index
            let min_idx = fst.transitions(0).iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.weight.value().partial_cmp(&b.weight.value()).unwrap()
                })
                .map(|(i, _)| i)
                .unwrap();

            prop_assert_eq!(result.path[0].arc_idx, min_idx,
                "Path arc {} != min arc {}", result.path[0].arc_idx, min_idx);
        }

        /// Viterbi path score matches score function.
        #[test]
        fn viterbi_path_score_matches(fst in arb_parallel_wfst(4)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let score = viterbi_score(&grad_fst);

            let grad_fst2 = GradientWfst::from_wfst(&fst);
            let result = viterbi_path_with_grad(&grad_fst2);

            prop_assert!((score.value() - result.score.value()).abs() < 1e-9,
                "viterbi_score {} != viterbi_path_with_grad score {}",
                score.value(), result.score.value());
        }

        /// Viterbi path arcs form valid sequence.
        #[test]
        fn viterbi_path_valid_sequence(fst in arb_chain_wfst(4)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let result = viterbi_path_with_grad(&grad_fst);

            // Verify path starts at state 0
            if !result.path.is_empty() {
                prop_assert_eq!(result.path[0].from, 0,
                    "Path should start at state 0, got {}", result.path[0].from);
            }

            // Verify consecutive arcs connect
            for i in 1..result.path.len() {
                let prev_arc = &result.path[i - 1];
                let curr_arc = &result.path[i];
                // Each arc should start where the previous one ends
                // In a chain: arc[i].from = i
                prop_assert_eq!(curr_arc.from as usize, i,
                    "Arc {} should start at state {}", i, i);
            }
        }

        /// Viterbi gradient non-zero only on path arcs.
        #[test]
        fn viterbi_gradient_sparse(fst in arb_parallel_wfst(5)) {
            let grad_fst = GradientWfst::from_wfst(&fst);
            let result = viterbi_path_with_grad(&grad_fst);

            // Only arcs on path should have non-zero gradient
            let num_arcs = fst.transitions(0).len();
            for arc_idx in 0..num_arcs {
                let arc = ArcIndex::new(0, arc_idx);
                let grad = result.gradients.get_gradient(arc);
                let on_path = result.path.iter().any(|p| *p == arc);

                if on_path {
                    prop_assert!((grad - 1.0).abs() < 1e-6,
                        "Path arc gradient {} should be 1.0", grad);
                } else {
                    prop_assert!((grad - 0.0).abs() < 1e-6,
                        "Non-path arc gradient {} should be 0.0", grad);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::{VectorWfst, MutableWfst};

    #[test]
    fn test_viterbi_empty() {
        let fst = VectorWfst::<char, LogWeight>::new();
        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = viterbi_score(&grad_fst);
        assert!(score.is_zero());
    }

    #[test]
    fn test_viterbi_no_path() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = viterbi_score(&grad_fst);
        assert!(score.is_zero());
    }

    #[test]
    fn test_viterbi_single_path() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = viterbi_score(&grad_fst);
        assert!((score.value() - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_viterbi_two_paths() {
        // Two paths: -1.0 and -2.0, best is -2.0 (most negative = lowest cost)
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));
        fst.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(-2.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = viterbi_score(&grad_fst);
        assert!((score.value() - (-2.0)).abs() < 1e-6);
    }

    #[test]
    fn test_viterbi_chain() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s2, LogWeight::new(-0.5));
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));
        fst.add_arc(s1, Some('b'), Some('b'), s2, LogWeight::new(-2.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = viterbi_score(&grad_fst);
        // Path: -1.0 + -2.0 + -0.5 = -3.5
        assert!((score.value() - (-3.5)).abs() < 1e-6);
    }

    #[test]
    fn test_viterbi_path_with_grad() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));
        fst.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(-2.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let result = viterbi_path_with_grad(&grad_fst);

        assert!((result.score.value() - (-2.0)).abs() < 1e-6);
        assert_eq!(result.path.len(), 1);
        assert_eq!(result.path[0].from, 0);
        assert_eq!(result.path[0].arc_idx, 1); // Second arc (index 1) is best

        // Gradient should be 1.0 for best arc
        assert!((result.gradients.get_gradient(result.path[0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_viterbi_path_chain() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s2, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));
        fst.add_arc(s1, Some('b'), Some('b'), s2, LogWeight::new(-2.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let result = viterbi_path_with_grad(&grad_fst);

        assert_eq!(result.path.len(), 2);
        assert_eq!(result.path[0].from, 0);
        assert_eq!(result.path[1].from, 1);

        // Both arcs should have gradient 1.0
        for arc in &result.path {
            assert!((result.gradients.get_gradient(*arc) - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_viterbi_diamond() {
        // Diamond: 0 -> 1 -> 2 (cost -2) and 0 -> 2 (cost -1.5)
        // Best path is 0 -> 1 -> 2 with cost -2.0
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s2, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));
        fst.add_arc(s1, Some('b'), Some('b'), s2, LogWeight::new(-1.0));
        fst.add_arc(s0, Some('c'), Some('c'), s2, LogWeight::new(-1.5));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = viterbi_score(&grad_fst);

        // Best path: 0 -> 1 -> 2 with cost -2.0
        assert!((score.value() - (-2.0)).abs() < 1e-6);
    }
}
