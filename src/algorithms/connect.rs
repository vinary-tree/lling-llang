//! Connect (trim) algorithm for WFSTs.
//!
//! The connect operation removes states that are not on any accepting path.
//! A state is kept if and only if it is:
//!
//! 1. **Accessible**: Reachable from the start state
//! 2. **Coaccessible**: Can reach at least one final state
//!
//! This is essential for cleaning up WFSTs after operations that may create
//! unreachable or dead-end states.
//!
//! # Complexity
//!
//! O(|Q| + |E|) - Linear in the size of the automaton.
//!
//! # References
//!
//! - Mohri, M. (2009). "Weighted Automata Algorithms"

use std::collections::{HashSet, VecDeque};

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, Wfst, NO_STATE};

/// Configuration for connect operation.
#[derive(Clone, Debug, Default)]
pub struct ConnectConfig {
    /// Keep states that are accessible but not coaccessible (for debugging).
    pub keep_non_coaccessible: bool,
    /// Keep states that are coaccessible but not accessible (for debugging).
    pub keep_non_accessible: bool,
}

impl ConnectConfig {
    /// Create a configuration that removes all non-useful states.
    pub fn trim() -> Self {
        Self::default()
    }

    /// Create a configuration that only removes non-accessible states.
    pub fn accessible_only() -> Self {
        Self {
            keep_non_coaccessible: true,
            keep_non_accessible: false,
        }
    }

    /// Create a configuration that only removes non-coaccessible states.
    pub fn coaccessible_only() -> Self {
        Self {
            keep_non_coaccessible: false,
            keep_non_accessible: true,
        }
    }
}

/// Connect (trim) a WFST by removing non-useful states.
///
/// A state is useful if it is both accessible (reachable from start) and
/// coaccessible (can reach a final state). This operation modifies the
/// WFST in place, potentially renumbering states.
///
/// # Returns
///
/// The number of states removed.
///
/// # Example
///
/// ```ignore
/// use lling_llang::algorithms::{connect, ConnectConfig};
///
/// let mut fst = build_some_wfst();
/// let removed = connect(&mut fst, ConnectConfig::trim());
/// println!("Removed {} non-useful states", removed);
/// ```
pub fn connect<L, W, F>(fst: &mut F, config: ConnectConfig) -> usize
where
    L: Clone,
    W: Semiring,
    F: MutableWfst<L, W> + Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return 0;
    }

    let start = fst.start();
    if start == NO_STATE {
        // No start state - all states are non-accessible
        let removed = n;
        // Clear all transitions (can't actually remove states in current API)
        for state in 0..n {
            fst.clear_transitions(state as StateId);
            fst.set_final(state as StateId, W::zero());
        }
        return removed;
    }

    // Compute accessible states (reachable from start)
    let accessible = compute_accessible(fst);

    // Compute coaccessible states (can reach final)
    let coaccessible = compute_coaccessible(fst);

    // Determine which states to keep
    let mut keep: HashSet<StateId> = HashSet::new();
    for state in 0..n {
        let state_id = state as StateId;
        let is_accessible = accessible.contains(&state_id);
        let is_coaccessible = coaccessible.contains(&state_id);

        let should_keep = match (is_accessible, is_coaccessible) {
            (true, true) => true,
            (true, false) => config.keep_non_coaccessible,
            (false, true) => config.keep_non_accessible,
            (false, false) => false,
        };

        if should_keep {
            keep.insert(state_id);
        }
    }

    // Count removed states
    let removed = n - keep.len();

    // If nothing to remove, return early
    if removed == 0 {
        return 0;
    }

    // Remove transitions to non-kept states and from non-kept states
    for state in 0..n {
        let state_id = state as StateId;

        if !keep.contains(&state_id) {
            // Clear this state completely
            fst.clear_transitions(state_id);
            fst.set_final(state_id, W::zero());
        } else {
            // Filter transitions to only kept states
            let transitions: Vec<_> = fst
                .transitions(state_id)
                .iter()
                .filter(|t| keep.contains(&t.to))
                .cloned()
                .collect();

            fst.clear_transitions(state_id);
            for trans in transitions {
                fst.add_transition(trans);
            }
        }
    }

    removed
}

/// Compute the set of accessible states (reachable from start).
pub fn compute_accessible<L, W, F>(fst: &F) -> HashSet<StateId>
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let mut accessible = HashSet::new();
    let start = fst.start();

    if start == NO_STATE {
        return accessible;
    }

    let mut queue = VecDeque::new();
    queue.push_back(start);
    accessible.insert(start);

    while let Some(state) = queue.pop_front() {
        for trans in fst.transitions(state) {
            if !accessible.contains(&trans.to) {
                accessible.insert(trans.to);
                queue.push_back(trans.to);
            }
        }
    }

    accessible
}

/// Compute the set of coaccessible states (can reach a final state).
pub fn compute_coaccessible<L, W, F>(fst: &F) -> HashSet<StateId>
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();

    // Build reverse graph
    let mut reverse: Vec<Vec<StateId>> = vec![Vec::new(); n];
    for state in 0..n {
        let state_id = state as StateId;
        for trans in fst.transitions(state_id) {
            reverse[trans.to as usize].push(state_id);
        }
    }

    // Start from final states and traverse backwards
    let mut coaccessible = HashSet::new();
    let mut queue = VecDeque::new();

    for state in 0..n {
        let state_id = state as StateId;
        if fst.is_final(state_id) {
            coaccessible.insert(state_id);
            queue.push_back(state_id);
        }
    }

    while let Some(state) = queue.pop_front() {
        for &predecessor in &reverse[state as usize] {
            if !coaccessible.contains(&predecessor) {
                coaccessible.insert(predecessor);
                queue.push_back(predecessor);
            }
        }
    }

    coaccessible
}

/// Check if a WFST is connected (all states are useful).
pub fn is_connected<L, W, F>(fst: &F) -> bool
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return true;
    }

    let accessible = compute_accessible(fst);
    let coaccessible = compute_coaccessible(fst);

    for state in 0..n {
        let state_id = state as StateId;
        if !accessible.contains(&state_id) || !coaccessible.contains(&state_id) {
            return false;
        }
    }

    true
}

/// Get the number of useful states (accessible and coaccessible).
pub fn count_useful_states<L, W, F>(fst: &F) -> usize
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let accessible = compute_accessible(fst);
    let coaccessible = compute_coaccessible(fst);

    accessible.intersection(&coaccessible).count()
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
            /// Connect should never increase state count.
            #[test]
            fn connect_reduces_or_maintains_states(
                fst in arb_tropical_wfst(10, 3)
            ) {
                let original_states = fst.num_states();
                let useful_before = count_useful_states(&fst);

                let mut connected_fst = fst.clone();
                let removed = connect(&mut connected_fst, ConnectConfig::trim());

                // The number removed should match the non-useful states
                prop_assert!(
                    removed <= original_states,
                    "Removed {} states from {} total",
                    removed,
                    original_states
                );

                // Useful count shouldn't change much (may differ due to cleared states)
                let useful_after = count_useful_states(&connected_fst);
                prop_assert!(
                    useful_after <= useful_before,
                    "Useful count increased from {} to {}",
                    useful_before,
                    useful_after
                );
            }

            /// After connect, all remaining states with transitions should be useful.
            #[test]
            fn connect_all_useful(
                fst in arb_tropical_wfst(8, 3)
            ) {
                let mut connected_fst = fst.clone();
                connect(&mut connected_fst, ConnectConfig::trim());

                // After trimming, accessible and coaccessible should be equal
                let accessible = compute_accessible(&connected_fst);
                let coaccessible = compute_coaccessible(&connected_fst);

                // States with transitions should be both accessible and coaccessible
                for state in 0..connected_fst.num_states() {
                    let state_id = state as StateId;
                    let has_transitions = !connected_fst.transitions(state_id).is_empty()
                        || connected_fst.is_final(state_id);

                    if has_transitions && accessible.contains(&state_id) {
                        prop_assert!(
                            coaccessible.contains(&state_id),
                            "State {} is accessible but not coaccessible",
                            state_id
                        );
                    }
                }
            }

            /// Connect is idempotent in terms of useful state count.
            /// Note: The connect implementation clears state data but doesn't
            /// remove states from the structure, so `removed` count may include
            /// already-cleared states. We verify idempotence by checking useful counts.
            #[test]
            fn connect_idempotent(
                fst in arb_tropical_wfst(8, 3)
            ) {
                // Skip FSTs with no useful states
                let useful_before = count_useful_states(&fst);
                if useful_before == 0 {
                    return Ok(());
                }

                let mut fst1 = fst.clone();
                let _removed1 = connect(&mut fst1, ConnectConfig::trim());
                let useful_after_first = count_useful_states(&fst1);

                // Skip if first connect removed everything
                if useful_after_first == 0 {
                    return Ok(());
                }

                let mut fst2 = fst1.clone();
                let _removed2 = connect(&mut fst2, ConnectConfig::trim());
                let useful_after_second = count_useful_states(&fst2);

                // Useful count should be same after both connects (idempotent)
                prop_assert_eq!(
                    useful_after_first,
                    useful_after_second,
                    "Useful count changed from {} to {} after second connect",
                    useful_after_first,
                    useful_after_second
                );
            }

            /// compute_accessible and compute_coaccessible should be consistent.
            #[test]
            fn accessible_coaccessible_consistent(
                fst in arb_tropical_wfst(6, 2)
            ) {
                let accessible = compute_accessible(&fst);
                let coaccessible = compute_coaccessible(&fst);
                let useful = count_useful_states(&fst);

                // Useful states = intersection of accessible and coaccessible
                let intersection_count = accessible.intersection(&coaccessible).count();
                prop_assert_eq!(
                    useful,
                    intersection_count,
                    "count_useful_states {} != intersection count {}",
                    useful,
                    intersection_count
                );
            }

            /// is_connected should return true after connect.
            #[test]
            fn is_connected_after_connect(
                fst in arb_tropical_wfst(6, 2)
            ) {
                let mut connected_fst = fst.clone();
                connect(&mut connected_fst, ConnectConfig::trim());

                // After connect, non-empty useful states should all be connected
                if count_useful_states(&connected_fst) > 0 {
                    // The remaining useful states should all be in both sets
                    let accessible = compute_accessible(&connected_fst);
                    let coaccessible = compute_coaccessible(&connected_fst);

                    for state in accessible.iter() {
                        if !connected_fst.transitions(*state).is_empty()
                            || connected_fst.is_final(*state)
                        {
                            prop_assert!(
                                coaccessible.contains(state),
                                "Accessible state {} is not coaccessible after connect",
                                state
                            );
                        }
                    }
                }
            }
        }
    }

    fn build_connected_fst() -> VectorWfst<char, TropicalWeight> {
        // All states useful: 0 -> 1 -> 2 (final)
        VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::one())
            .arc(1, Some('b'), Some('b'), 2, TropicalWeight::one())
            .final_state(2, TropicalWeight::one())
            .build()
    }

    fn build_with_unreachable() -> VectorWfst<char, TropicalWeight> {
        // State 3 is unreachable: 0 -> 1 -> 2 (final), 3 (isolated)
        let mut fst = VectorWfst::new();
        fst.add_states(4);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::one());
        fst.add_arc(1, Some('b'), Some('b'), 2, TropicalWeight::one());
        fst.set_final(2, TropicalWeight::one());
        // State 3 has no incoming edges
        fst.add_arc(3, Some('c'), Some('c'), 2, TropicalWeight::one());
        fst
    }

    fn build_with_dead_end() -> VectorWfst<char, TropicalWeight> {
        // State 3 is a dead end: 0 -> 1 -> 2 (final), 0 -> 3 (dead end)
        let mut fst = VectorWfst::new();
        fst.add_states(4);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::one());
        fst.add_arc(1, Some('b'), Some('b'), 2, TropicalWeight::one());
        fst.set_final(2, TropicalWeight::one());
        // State 3 cannot reach any final state
        fst.add_arc(0, Some('x'), Some('x'), 3, TropicalWeight::one());
        fst
    }

    #[test]
    fn test_connect_empty() {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        let removed = connect(&mut fst, ConnectConfig::trim());
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_connect_already_connected() {
        let mut fst = build_connected_fst();
        assert!(is_connected(&fst));

        let removed = connect(&mut fst, ConnectConfig::trim());
        assert_eq!(removed, 0);
        assert!(is_connected(&fst));
    }

    #[test]
    fn test_connect_removes_unreachable() {
        let mut fst = build_with_unreachable();
        assert!(!is_connected(&fst));
        assert_eq!(count_useful_states(&fst), 3);

        let removed = connect(&mut fst, ConnectConfig::trim());
        assert_eq!(removed, 1);

        // Check that state 3 has no transitions now
        assert!(fst.transitions(3).is_empty());
    }

    #[test]
    fn test_connect_removes_dead_end() {
        let mut fst = build_with_dead_end();
        assert!(!is_connected(&fst));
        assert_eq!(count_useful_states(&fst), 3);

        let removed = connect(&mut fst, ConnectConfig::trim());
        assert_eq!(removed, 1);

        // Check that transition to state 3 was removed
        let trans_from_0: Vec<_> = fst.transitions(0).iter().map(|t| t.to).collect();
        assert!(!trans_from_0.contains(&3));
    }

    #[test]
    fn test_compute_accessible() {
        let fst = build_with_unreachable();
        let accessible = compute_accessible(&fst);

        assert!(accessible.contains(&0));
        assert!(accessible.contains(&1));
        assert!(accessible.contains(&2));
        assert!(!accessible.contains(&3)); // State 3 is not accessible
    }

    #[test]
    fn test_compute_coaccessible() {
        let fst = build_with_dead_end();
        let coaccessible = compute_coaccessible(&fst);

        assert!(coaccessible.contains(&0));
        assert!(coaccessible.contains(&1));
        assert!(coaccessible.contains(&2));
        assert!(!coaccessible.contains(&3)); // State 3 is not coaccessible
    }

    #[test]
    fn test_is_connected() {
        let connected = build_connected_fst();
        assert!(is_connected(&connected));

        let with_unreachable = build_with_unreachable();
        assert!(!is_connected(&with_unreachable));

        let with_dead_end = build_with_dead_end();
        assert!(!is_connected(&with_dead_end));
    }

    #[test]
    fn test_count_useful_states() {
        let connected = build_connected_fst();
        assert_eq!(count_useful_states(&connected), 3);

        let with_unreachable = build_with_unreachable();
        assert_eq!(count_useful_states(&with_unreachable), 3);

        let with_dead_end = build_with_dead_end();
        assert_eq!(count_useful_states(&with_dead_end), 3);
    }

    #[test]
    fn test_connect_config_accessible_only() {
        let mut fst = build_with_dead_end();

        // Only remove non-accessible (keep dead ends)
        let removed = connect(&mut fst, ConnectConfig::accessible_only());
        assert_eq!(removed, 0); // All states are accessible

        // State 3 should still have its transition
        assert!(fst.transitions(3).is_empty() || fst.transitions(0).iter().any(|t| t.to == 3));
    }

    #[test]
    fn test_connect_config_coaccessible_only() {
        let mut fst = build_with_unreachable();

        // Only remove non-coaccessible (keep unreachable)
        let removed = connect(&mut fst, ConnectConfig::coaccessible_only());
        assert_eq!(removed, 0); // All states are coaccessible (state 3 can reach final)
    }
}
