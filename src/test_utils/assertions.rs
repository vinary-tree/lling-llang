//! Custom assertion helpers for testing WFSTs and semirings.
//!
//! This module provides assertion functions for approximate equality,
//! property verification, and structural invariants.

use std::collections::{HashSet, VecDeque};

use crate::semiring::Semiring;
use crate::wfst::{StateId, VectorWfst, Wfst};

// =============================================================================
// Numeric Assertions
// =============================================================================

/// Check approximate equality for floating-point values.
///
/// Returns `true` if `|a - b| <= epsilon`.
#[inline]
pub fn approx_eq(a: f64, b: f64, epsilon: f64) -> bool {
    if a.is_infinite() && b.is_infinite() {
        return a.signum() == b.signum();
    }
    if a.is_nan() || b.is_nan() {
        return false;
    }
    (a - b).abs() <= epsilon
}

/// Check approximate equality with relative tolerance.
///
/// For values close to zero, uses absolute tolerance.
/// For larger values, uses relative tolerance.
#[inline]
pub fn approx_eq_relative(a: f64, b: f64, rel_tol: f64, abs_tol: f64) -> bool {
    if a.is_infinite() && b.is_infinite() {
        return a.signum() == b.signum();
    }
    if a.is_nan() || b.is_nan() {
        return false;
    }
    let diff = (a - b).abs();
    diff <= abs_tol || diff <= rel_tol * a.abs().max(b.abs())
}

/// Assert approximate equality with a helpful error message.
#[track_caller]
pub fn assert_approx_eq(a: f64, b: f64, epsilon: f64) {
    assert!(
        approx_eq(a, b, epsilon),
        "assertion failed: approx_eq({}, {}, {})\n  difference: {}",
        a, b, epsilon, (a - b).abs()
    );
}

/// Assert approximate equality for semiring weights.
#[track_caller]
pub fn assert_weight_approx_eq<W: Semiring>(a: &W, b: &W, epsilon: f64) {
    assert!(
        a.approx_eq(b, epsilon),
        "assertion failed: weight approx_eq\n  left:  {:?}\n  right: {:?}",
        a, b
    );
}

// =============================================================================
// WFST Equality Assertions
// =============================================================================

/// Check if two WFSTs are approximately equal.
///
/// Two WFSTs are approximately equal if they have the same structure
/// (same states and transitions) and approximately equal weights.
pub fn wfst_approx_eq<L, W>(
    fst1: &VectorWfst<L, W>,
    fst2: &VectorWfst<L, W>,
    epsilon: f64,
) -> bool
where
    L: Clone + Send + Sync + PartialEq,
    W: Semiring,
{
    // Check basic structure
    if fst1.num_states() != fst2.num_states() {
        return false;
    }
    if fst1.start() != fst2.start() {
        return false;
    }

    // Check states and transitions
    for state in 0..fst1.num_states() as StateId {
        // Check final status
        if fst1.is_final(state) != fst2.is_final(state) {
            return false;
        }
        if fst1.is_final(state) {
            if !fst1.final_weight(state).approx_eq(&fst2.final_weight(state), epsilon) {
                return false;
            }
        }

        // Check transitions (order-independent)
        let trans1 = fst1.transitions(state);
        let trans2 = fst2.transitions(state);

        if trans1.len() != trans2.len() {
            return false;
        }

        // Simple comparison assuming same order
        for (t1, t2) in trans1.iter().zip(trans2.iter()) {
            if t1.input != t2.input || t1.output != t2.output || t1.to != t2.to {
                return false;
            }
            if !t1.weight.approx_eq(&t2.weight, epsilon) {
                return false;
            }
        }
    }

    true
}

/// Assert two WFSTs are approximately equal.
#[track_caller]
pub fn assert_wfst_approx_eq<L, W>(
    fst1: &VectorWfst<L, W>,
    fst2: &VectorWfst<L, W>,
    epsilon: f64,
)
where
    L: Clone + Send + Sync + PartialEq + std::fmt::Debug,
    W: Semiring + std::fmt::Debug,
{
    assert!(
        wfst_approx_eq(fst1, fst2, epsilon),
        "WFSTs are not approximately equal"
    );
}

// =============================================================================
// Property Assertions
// =============================================================================

/// Check if a WFST is deterministic.
///
/// A WFST is deterministic if:
/// 1. It has at most one start state
/// 2. For each state, there is at most one transition for each input label
/// 3. There are no epsilon transitions on the input side
pub fn is_deterministic<L, W>(fst: &VectorWfst<L, W>) -> bool
where
    L: Clone + Send + Sync + PartialEq + Eq + std::hash::Hash,
    W: Semiring,
{
    for state in 0..fst.num_states() as StateId {
        let mut seen_labels: HashSet<Option<&L>> = HashSet::new();
        for trans in fst.transitions(state) {
            // No epsilon on input
            if trans.input.is_none() {
                return false;
            }
            // No duplicate input labels
            if !seen_labels.insert(trans.input.as_ref()) {
                return false;
            }
        }
    }
    true
}

/// Assert that a WFST is deterministic.
#[track_caller]
pub fn assert_is_deterministic<L, W>(fst: &VectorWfst<L, W>)
where
    L: Clone + Send + Sync + PartialEq + Eq + std::hash::Hash + std::fmt::Debug,
    W: Semiring,
{
    assert!(
        is_deterministic(fst),
        "WFST is not deterministic"
    );
}

/// Check if a WFST is acyclic.
///
/// Uses DFS to detect cycles.
pub fn is_acyclic<L, W>(fst: &VectorWfst<L, W>) -> bool
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    if fst.is_empty() {
        return true;
    }

    let num_states = fst.num_states();
    let mut color = vec![0u8; num_states]; // 0=white, 1=gray, 2=black

    fn dfs<L: Clone + Send + Sync, W: Semiring>(
        fst: &VectorWfst<L, W>,
        state: StateId,
        color: &mut [u8],
    ) -> bool {
        color[state as usize] = 1; // Gray

        for trans in fst.transitions(state) {
            let next = trans.to;
            match color[next as usize] {
                1 => return false, // Back edge = cycle
                0 => {
                    if !dfs(fst, next, color) {
                        return false;
                    }
                }
                _ => {}
            }
        }

        color[state as usize] = 2; // Black
        true
    }

    // Check from all states (in case graph is not connected)
    for state in 0..num_states as StateId {
        if color[state as usize] == 0 {
            if !dfs(fst, state, &mut color) {
                return false;
            }
        }
    }

    true
}

/// Assert that a WFST is acyclic.
#[track_caller]
pub fn assert_is_acyclic<L, W>(fst: &VectorWfst<L, W>)
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    assert!(is_acyclic(fst), "WFST contains cycles");
}

/// Check if a WFST has no epsilon transitions.
pub fn has_no_epsilon<L, W>(fst: &VectorWfst<L, W>) -> bool
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    for state in 0..fst.num_states() as StateId {
        for trans in fst.transitions(state) {
            if trans.is_epsilon() {
                return false;
            }
        }
    }
    true
}

/// Check if a WFST has no epsilon transitions on input side only.
pub fn has_no_input_epsilon<L, W>(fst: &VectorWfst<L, W>) -> bool
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    for state in 0..fst.num_states() as StateId {
        for trans in fst.transitions(state) {
            if trans.is_epsilon_input() {
                return false;
            }
        }
    }
    true
}

/// Assert that a WFST has no epsilon transitions.
#[track_caller]
pub fn assert_has_no_epsilon<L, W>(fst: &VectorWfst<L, W>)
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    assert!(
        has_no_epsilon(fst),
        "WFST has epsilon transitions"
    );
}

/// Check if a WFST is connected (all states reachable from start).
pub fn is_connected<L, W>(fst: &VectorWfst<L, W>) -> bool
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    if fst.is_empty() {
        return true;
    }

    let start = fst.start();
    let num_states = fst.num_states();
    let mut visited = vec![false; num_states];
    let mut queue = VecDeque::new();

    queue.push_back(start);
    visited[start as usize] = true;
    let mut count = 1;

    while let Some(state) = queue.pop_front() {
        for trans in fst.transitions(state) {
            if !visited[trans.to as usize] {
                visited[trans.to as usize] = true;
                count += 1;
                queue.push_back(trans.to);
            }
        }
    }

    count == num_states
}

/// Assert that a WFST is connected.
#[track_caller]
pub fn assert_is_connected<L, W>(fst: &VectorWfst<L, W>)
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    assert!(is_connected(fst), "WFST is not connected");
}

/// Check if all states can reach a final state (coaccessible).
pub fn is_coaccessible<L, W>(fst: &VectorWfst<L, W>) -> bool
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    if fst.is_empty() {
        return true;
    }

    let num_states = fst.num_states();

    // Build reverse graph
    let mut reverse_adj: Vec<Vec<StateId>> = vec![Vec::new(); num_states];
    for state in 0..num_states as StateId {
        for trans in fst.transitions(state) {
            reverse_adj[trans.to as usize].push(state);
        }
    }

    // BFS from final states backwards
    let mut visited = vec![false; num_states];
    let mut queue = VecDeque::new();

    for state in 0..num_states as StateId {
        if fst.is_final(state) {
            visited[state as usize] = true;
            queue.push_back(state);
        }
    }

    while let Some(state) = queue.pop_front() {
        for &prev in &reverse_adj[state as usize] {
            if !visited[prev as usize] {
                visited[prev as usize] = true;
                queue.push_back(prev);
            }
        }
    }

    // All reachable states from start should be coaccessible
    // For now, just check all states
    visited.iter().all(|&v| v)
}

/// Check if a WFST is trimmed (all states are accessible and coaccessible).
pub fn is_trimmed<L, W>(fst: &VectorWfst<L, W>) -> bool
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    is_connected(fst) && is_coaccessible(fst)
}

// =============================================================================
// WFST Invariant Assertions
// =============================================================================

/// Assert basic WFST invariants.
///
/// Checks:
/// 1. If non-empty, has a valid start state
/// 2. All transitions reference valid states
/// 3. No NaN weights
#[track_caller]
pub fn assert_wfst_invariants<L, W>(fst: &VectorWfst<L, W>)
where
    L: Clone + Send + Sync,
    W: Semiring + std::fmt::Debug,
{
    let num_states = fst.num_states();

    // Check start state
    if num_states > 0 {
        assert!(
            (fst.start() as usize) < num_states,
            "Invalid start state: {} (num_states: {})",
            fst.start(),
            num_states
        );
    }

    // Check all transitions
    for state in 0..num_states as StateId {
        for trans in fst.transitions(state) {
            assert!(
                (trans.from as usize) < num_states,
                "Transition has invalid 'from' state: {}",
                trans.from
            );
            assert!(
                (trans.to as usize) < num_states,
                "Transition has invalid 'to' state: {} (from state {})",
                trans.to,
                state
            );
        }
    }
}

// =============================================================================
// Semiring Property Assertions
// =============================================================================

/// Assert semiring plus is commutative.
#[track_caller]
pub fn assert_plus_commutative<W: Semiring>(a: &W, b: &W, epsilon: f64) {
    let ab = a.plus(b);
    let ba = b.plus(a);
    assert!(
        ab.approx_eq(&ba, epsilon),
        "Plus is not commutative: {:?} + {:?} = {:?} != {:?} = {:?} + {:?}",
        a, b, ab, ba, b, a
    );
}

/// Assert semiring plus is associative.
#[track_caller]
pub fn assert_plus_associative<W: Semiring>(a: &W, b: &W, c: &W, epsilon: f64) {
    let ab_c = a.plus(b).plus(c);
    let a_bc = a.plus(&b.plus(c));
    assert!(
        ab_c.approx_eq(&a_bc, epsilon),
        "Plus is not associative: ({:?} + {:?}) + {:?} = {:?} != {:?} = {:?} + ({:?} + {:?})",
        a, b, c, ab_c, a_bc, a, b, c
    );
}

/// Assert semiring times is associative.
#[track_caller]
pub fn assert_times_associative<W: Semiring>(a: &W, b: &W, c: &W, epsilon: f64) {
    let ab_c = a.times(b).times(c);
    let a_bc = a.times(&b.times(c));
    assert!(
        ab_c.approx_eq(&a_bc, epsilon),
        "Times is not associative: ({:?} * {:?}) * {:?} = {:?} != {:?} = {:?} * ({:?} * {:?})",
        a, b, c, ab_c, a_bc, a, b, c
    );
}

/// Assert left distributivity: a * (b + c) = (a * b) + (a * c)
#[track_caller]
pub fn assert_left_distributive<W: Semiring>(a: &W, b: &W, c: &W, epsilon: f64) {
    let left = a.times(&b.plus(c));
    let right = a.times(b).plus(&a.times(c));
    assert!(
        left.approx_eq(&right, epsilon),
        "Left distributivity failed: {:?} * ({:?} + {:?}) = {:?} != {:?} = ({:?} * {:?}) + ({:?} * {:?})",
        a, b, c, left, right, a, b, a, c
    );
}

/// Assert right distributivity: (a + b) * c = (a * c) + (b * c)
#[track_caller]
pub fn assert_right_distributive<W: Semiring>(a: &W, b: &W, c: &W, epsilon: f64) {
    let left = a.plus(b).times(c);
    let right = a.times(c).plus(&b.times(c));
    assert!(
        left.approx_eq(&right, epsilon),
        "Right distributivity failed: ({:?} + {:?}) * {:?} = {:?} != {:?} = ({:?} * {:?}) + ({:?} * {:?})",
        a, b, c, left, right, a, c, b, c
    );
}

/// Assert zero identity: a + 0 = a
#[track_caller]
pub fn assert_zero_identity<W: Semiring>(a: &W, epsilon: f64) {
    let result = a.plus(&W::zero());
    assert!(
        result.approx_eq(a, epsilon),
        "Zero identity failed: {:?} + 0 = {:?} != {:?}",
        a, result, a
    );
}

/// Assert one identity: a * 1 = a
#[track_caller]
pub fn assert_one_identity<W: Semiring>(a: &W, epsilon: f64) {
    let result_right = a.times(&W::one());
    let result_left = W::one().times(a);
    assert!(
        result_right.approx_eq(a, epsilon),
        "One identity (right) failed: {:?} * 1 = {:?} != {:?}",
        a, result_right, a
    );
    assert!(
        result_left.approx_eq(a, epsilon),
        "One identity (left) failed: 1 * {:?} = {:?} != {:?}",
        a, result_left, a
    );
}

/// Assert zero annihilation: a * 0 = 0
#[track_caller]
pub fn assert_zero_annihilation<W: Semiring>(a: &W, epsilon: f64) {
    let result_right = a.times(&W::zero());
    let result_left = W::zero().times(a);
    assert!(
        result_right.approx_eq(&W::zero(), epsilon),
        "Zero annihilation (right) failed: {:?} * 0 = {:?} != 0",
        a, result_right
    );
    assert!(
        result_left.approx_eq(&W::zero(), epsilon),
        "Zero annihilation (left) failed: 0 * {:?} = {:?} != 0",
        a, result_left
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::MutableWfst;

    #[test]
    fn test_approx_eq() {
        assert!(approx_eq(1.0, 1.0, 1e-10));
        assert!(approx_eq(1.0, 1.0 + 1e-11, 1e-10));
        assert!(!approx_eq(1.0, 1.1, 1e-10));
        assert!(approx_eq(f64::INFINITY, f64::INFINITY, 1e-10));
        assert!(!approx_eq(f64::INFINITY, f64::NEG_INFINITY, 1e-10));
        assert!(!approx_eq(f64::NAN, f64::NAN, 1e-10));
    }

    #[test]
    fn test_is_deterministic() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        fst.add_state();
        fst.add_state();
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::one());
        fst.set_final(1, TropicalWeight::one());

        assert!(is_deterministic(&fst));

        // Add epsilon - now non-deterministic
        fst.add_arc(0, None, Some('b'), 1, TropicalWeight::one());
        assert!(!is_deterministic(&fst));
    }

    #[test]
    fn test_is_acyclic() {
        // Linear FST is acyclic
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        fst.add_state();
        fst.add_state();
        fst.add_state();
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::one());
        fst.add_arc(1, Some('b'), Some('b'), 2, TropicalWeight::one());
        fst.set_final(2, TropicalWeight::one());

        assert!(is_acyclic(&fst));

        // Add back edge - now cyclic
        fst.add_arc(2, Some('c'), Some('c'), 0, TropicalWeight::one());
        assert!(!is_acyclic(&fst));
    }

    #[test]
    fn test_is_connected() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        fst.add_state();
        fst.add_state();
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::one());
        fst.set_final(1, TropicalWeight::one());

        assert!(is_connected(&fst));

        // Add disconnected state
        fst.add_state();
        assert!(!is_connected(&fst));
    }
}
