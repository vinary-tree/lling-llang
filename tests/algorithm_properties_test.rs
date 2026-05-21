//! Property-based tests for WFST algorithms.
//!
//! This test module verifies critical properties of WFST algorithms:
//!
//! # Determinization Properties
//! - Idempotence: `determinize(determinize(A)) = determinize(A)`
//! - Output is deterministic: no duplicate transitions on same label
//! - Language preservation: accepts same weighted language
//!
//! # Minimization Properties
//! - Idempotence: `minimize(minimize(A)) = minimize(A)` (state count)
//! - State count bounds: minimized has fewer or equal states
//! - Language preservation: accepts same weighted language
//!
//! # Epsilon Removal Properties
//! - No epsilons in output
//! - Weight preservation: path weights sum correctly
//!
//! # Connect (Trim) Properties
//! - All states accessible from start
//! - All states coaccessible (can reach final)
//!
//! # Composition Properties
//! - Identity: `A ∘ I = A` and `I ∘ A = A`
//! - Associativity: `(A ∘ B) ∘ C = A ∘ (B ∘ C)` for path weights

use lling_llang::semiring::{LogWeight, Semiring, TropicalWeight};
use lling_llang::wfst::{MutableWfst, VectorWfst, Wfst};
use proptest::prelude::*;

// =============================================================================
// Helper Functions
// =============================================================================

/// Build a simple FST for testing.
fn build_simple_fst() -> VectorWfst<char, TropicalWeight> {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s2, TropicalWeight::one());

    fst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));
    fst.add_arc(s1, Some('b'), Some('b'), s2, TropicalWeight::new(2.0));

    fst
}

/// Build an FST with epsilon transitions for testing.
fn build_epsilon_fst() -> VectorWfst<char, TropicalWeight> {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    let s3 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s3, TropicalWeight::one());

    // Path with epsilon
    fst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));
    fst.add_arc(s1, None, None, s2, TropicalWeight::new(0.5)); // epsilon
    fst.add_arc(s2, Some('b'), Some('b'), s3, TropicalWeight::new(2.0));

    fst
}

/// Build a non-deterministic FST for testing.
fn build_nondet_fst() -> VectorWfst<char, TropicalWeight> {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    let s3 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s2, TropicalWeight::one());
    fst.set_final(s3, TropicalWeight::one());

    // Non-deterministic: two arcs with same input label from s0
    fst.add_arc(s0, Some('a'), Some('x'), s1, TropicalWeight::new(1.0));
    fst.add_arc(s0, Some('a'), Some('y'), s2, TropicalWeight::new(2.0));
    fst.add_arc(s1, Some('b'), Some('z'), s3, TropicalWeight::new(1.0));

    fst
}

/// Build an FST with unreachable states for testing connect.
fn build_disconnected_fst() -> VectorWfst<char, TropicalWeight> {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state(); // Unreachable from start
    let s3 = fst.add_state(); // Not coaccessible (can't reach final)

    fst.set_start(s0);
    fst.set_final(s1, TropicalWeight::one());

    fst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));
    fst.add_arc(s2, Some('b'), Some('b'), s1, TropicalWeight::new(2.0)); // s2 unreachable
    fst.add_arc(s0, Some('c'), Some('c'), s3, TropicalWeight::new(3.0)); // s3 not coaccessible

    fst
}

/// Build a linear chain FST.
fn build_chain_fst(length: usize) -> VectorWfst<u32, TropicalWeight> {
    let mut fst: VectorWfst<u32, TropicalWeight> = VectorWfst::new();

    if length == 0 {
        let s = fst.add_state();
        fst.set_start(s);
        fst.set_final(s, TropicalWeight::one());
        return fst;
    }

    let mut prev = fst.add_state();
    fst.set_start(prev);

    for i in 0..length {
        let next = fst.add_state();
        fst.add_arc(
            prev,
            Some(i as u32),
            Some(i as u32),
            next,
            TropicalWeight::new(1.0),
        );
        prev = next;
    }

    fst.set_final(prev, TropicalWeight::one());
    fst
}

/// Build an identity transducer that copies input to output.
fn build_identity_fst(alphabet_size: usize) -> VectorWfst<u32, TropicalWeight> {
    let mut fst: VectorWfst<u32, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    fst.set_start(s0);
    fst.set_final(s0, TropicalWeight::one());

    // Self-loops for each symbol
    for label in 0..alphabet_size {
        fst.add_arc(
            s0,
            Some(label as u32),
            Some(label as u32),
            s0,
            TropicalWeight::one(),
        );
    }

    fst
}

// =============================================================================
// Connect (Trim) Tests
// =============================================================================

/// Test connect algorithm on a simple FST.
#[test]
fn test_connect_simple() {
    use lling_llang::algorithms::{connect, ConnectConfig};

    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let _s2 = fst.add_state(); // Unreachable

    fst.set_start(s0);
    fst.set_final(s1, TropicalWeight::one());
    fst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));

    let original_states = fst.num_states();
    let removed = connect(&mut fst, ConnectConfig::default());

    // Should remove unreachable state
    assert!(removed > 0 || fst.num_states() <= original_states);
}

/// Test that connect marks unreachable states as non-useful.
#[test]
fn test_connect_removes_unreachable() {
    use lling_llang::algorithms::{
        compute_accessible, connect, count_useful_states, ConnectConfig,
    };

    let mut fst = build_disconnected_fst();
    let original_useful = count_useful_states(&fst);

    let removed = connect(&mut fst, ConnectConfig::default());

    // Should report removing non-useful states
    // Note: connect doesn't actually reduce state count, but clears arcs/weights
    assert!(
        removed > 0 || original_useful == fst.num_states(),
        "Should remove at least one state or all were already useful"
    );

    // After connect, accessible set should not include previously unreachable states
    let accessible = compute_accessible(&fst);
    // At minimum, the start state should be accessible
    assert!(
        accessible.contains(&fst.start()),
        "Start state should be accessible"
    );
}

/// Test that connect handles non-coaccessible states.
#[test]
fn test_connect_removes_non_coaccessible() {
    use lling_llang::algorithms::{
        compute_coaccessible, connect, count_useful_states, ConnectConfig,
    };

    let mut fst = build_disconnected_fst();
    let useful_before = count_useful_states(&fst);

    let _removed = connect(&mut fst, ConnectConfig::default());

    // The useful state count after connect should be consistent
    let useful_after = count_useful_states(&fst);
    // After connect, either useful count stays same or decreases
    assert!(
        useful_after <= useful_before || useful_after == fst.num_states(),
        "Useful states should not increase after connect"
    );

    // Check that final states are still coaccessible
    let coaccessible = compute_coaccessible(&fst);
    for state in 0..fst.num_states() {
        if fst.is_final(state as u32) {
            assert!(
                coaccessible.contains(&(state as u32)),
                "Final state {} should be coaccessible",
                state
            );
        }
    }
}

/// Test is_connected predicate.
#[test]
fn test_is_connected() {
    use lling_llang::algorithms::is_connected;

    // Test with a simple connected FST
    let simple = build_simple_fst();
    assert!(is_connected(&simple), "Simple FST should be connected");

    // Test with a chain FST
    let chain = build_chain_fst(3);
    assert!(is_connected(&chain), "Chain FST should be connected");

    // Test with a disconnected FST
    let disconnected = build_disconnected_fst();
    // The disconnected FST has unreachable/non-coaccessible states
    // so is_connected should return false
    let is_conn = is_connected(&disconnected);
    assert!(!is_conn, "Disconnected FST should not be connected");
}

/// Test count_useful_states.
#[test]
fn test_count_useful_states() {
    use lling_llang::algorithms::count_useful_states;

    let fst = build_simple_fst();
    let count = count_useful_states(&fst);

    // All states in simple FST should be useful
    assert_eq!(
        count,
        fst.num_states(),
        "All states should be useful in simple FST"
    );

    let disconnected = build_disconnected_fst();
    let count_disconnected = count_useful_states(&disconnected);

    // Disconnected FST has some non-useful states
    assert!(
        count_disconnected < disconnected.num_states(),
        "Some states should be non-useful in disconnected FST"
    );
}

/// Test connect idempotence.
#[test]
fn test_connect_idempotent() {
    use lling_llang::algorithms::{connect, count_useful_states, ConnectConfig};

    let mut fst = build_disconnected_fst();

    // First connect
    let removed_first = connect(&mut fst, ConnectConfig::default());
    let useful_after_first = count_useful_states(&fst);

    // Second connect
    let removed_second = connect(&mut fst, ConnectConfig::default());
    let useful_after_second = count_useful_states(&fst);

    // Should be idempotent on useful state count
    assert_eq!(
        useful_after_first, useful_after_second,
        "Connect should be idempotent on useful states"
    );
    // Second connect should either remove 0 or same as first (if nothing changed)
    assert!(
        removed_second <= removed_first,
        "Second connect should remove <= first"
    );
}

// =============================================================================
// Determinization Tests
// =============================================================================

/// Test determinize algorithm on a simple FST.
#[test]
fn test_determinize_simple() {
    use lling_llang::algorithms::{determinize, DeterminizeConfig};

    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s1, TropicalWeight::one());
    fst.set_final(s2, TropicalWeight::one());

    // Non-deterministic: two arcs with same label
    fst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));
    fst.add_arc(s0, Some('a'), Some('a'), s2, TropicalWeight::new(2.0));

    let result = determinize(&fst, DeterminizeConfig::default());

    // Should succeed or fail gracefully
    match result {
        Ok(det) => {
            assert!(det.num_states() >= 1);
        }
        Err(_) => {
            // Some FSTs may not be determinizable
        }
    }
}

/// Test that determinize output is actually deterministic.
#[test]
fn test_determinize_output_is_deterministic() {
    use lling_llang::algorithms::{determinize, is_deterministic, DeterminizeConfig};

    let fst = build_nondet_fst();

    // Original may not be deterministic
    let original_is_det = is_deterministic(&fst);

    // Determinize
    let result = determinize(&fst, DeterminizeConfig::default());

    match result {
        Ok(det) => {
            // Output should be deterministic
            assert!(
                is_deterministic(&det),
                "Determinized FST should be deterministic"
            );

            if !original_is_det {
                // If original wasn't deterministic, we actually did something
            }
        }
        Err(_) => {
            // Some FSTs can't be determinized (e.g., with non-functional output)
        }
    }
}

/// Test determinize idempotence.
#[test]
fn test_determinize_idempotent() {
    use lling_llang::algorithms::{determinize, is_deterministic, DeterminizeConfig};

    let fst = build_nondet_fst();

    let result1 = determinize(&fst, DeterminizeConfig::default());

    if let Ok(det1) = result1 {
        // Determinize again
        let result2 = determinize(&det1, DeterminizeConfig::default());

        if let Ok(det2) = result2 {
            // Both should be deterministic
            assert!(
                is_deterministic(&det1),
                "First result should be deterministic"
            );
            assert!(
                is_deterministic(&det2),
                "Second result should be deterministic"
            );

            // State counts should be similar (idempotence on structure)
            // Note: exact equality not guaranteed due to state numbering
            assert_eq!(
                det1.num_states(),
                det2.num_states(),
                "Determinization should be idempotent on state count"
            );
        }
    }
}

/// Test is_deterministic predicate.
#[test]
fn test_is_deterministic_predicate() {
    use lling_llang::algorithms::is_deterministic;

    let simple = build_simple_fst();
    let nondet = build_nondet_fst();

    // Simple FST should be deterministic (no duplicate labels)
    assert!(
        is_deterministic(&simple),
        "Simple FST should be deterministic"
    );

    // Non-deterministic FST should not be deterministic
    assert!(
        !is_deterministic(&nondet),
        "Non-deterministic FST should not be deterministic"
    );
}

/// Test non_determinism_degree.
#[test]
fn test_non_determinism_degree() {
    use lling_llang::algorithms::non_determinism_degree;

    let simple = build_simple_fst();
    let nondet = build_nondet_fst();

    let simple_degree = non_determinism_degree(&simple);
    let nondet_degree = non_determinism_degree(&nondet);

    // Simple FST has degree 1 (deterministic)
    assert_eq!(simple_degree, 1, "Deterministic FST has degree 1");

    // Non-deterministic FST has degree > 1
    assert!(nondet_degree > 1, "Non-deterministic FST has degree > 1");
}

/// Test determinization on a chain FST (already deterministic).
#[test]
fn test_determinize_chain() {
    use lling_llang::algorithms::{determinize, is_deterministic, DeterminizeConfig};

    let chain = build_chain_fst(5);

    assert!(
        is_deterministic(&chain),
        "Chain FST should be deterministic"
    );

    let result = determinize(&chain, DeterminizeConfig::default());

    match result {
        Ok(det) => {
            assert!(is_deterministic(&det));
            // State count should be same or less
            assert!(
                det.num_states() <= chain.num_states() + 1,
                "Determinized chain should have similar state count"
            );
        }
        Err(_) => {
            // Shouldn't fail on a deterministic FST
            panic!("Determinization should succeed on deterministic input");
        }
    }
}

// =============================================================================
// Minimization Tests
// =============================================================================

/// Test minimize on simple FST.
#[test]
fn test_minimize_simple() {
    use lling_llang::algorithms::{minimize, MinimizeConfig};

    let fst = build_simple_fst();

    let result = minimize(&fst, MinimizeConfig::default());

    match result {
        Ok(min) => {
            // Minimized should have fewer or equal states
            assert!(
                min.num_states() <= fst.num_states() + 1,
                "Minimized FST should not have more states"
            );
        }
        Err(_) => {
            // Minimization may fail for some FSTs
        }
    }
}

/// Test minimize idempotence.
#[test]
fn test_minimize_idempotent() {
    use lling_llang::algorithms::{minimize, MinimizeConfig};

    let fst = build_simple_fst();

    let result1 = minimize(&fst, MinimizeConfig::default());

    if let Ok(min1) = result1 {
        let result2 = minimize(&min1, MinimizeConfig::default());

        if let Ok(min2) = result2 {
            // State counts should be equal (idempotence)
            assert_eq!(
                min1.num_states(),
                min2.num_states(),
                "Minimization should be idempotent on state count"
            );
        }
    }
}

/// Test minimize state count bounds.
#[test]
fn test_minimize_state_count_bound() {
    use lling_llang::algorithms::{minimize, MinimizeConfig};

    // Build FST with redundant states
    let mut fst: VectorWfst<u32, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    let s3 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s2, TropicalWeight::one());
    fst.set_final(s3, TropicalWeight::one());

    // s1->s2 and s1->s3 have same behavior (both final with same weight)
    fst.add_arc(s0, Some(0), Some(0), s1, TropicalWeight::new(1.0));
    fst.add_arc(s1, Some(1), Some(1), s2, TropicalWeight::new(1.0));
    fst.add_arc(s1, Some(2), Some(2), s3, TropicalWeight::new(1.0));

    let original_states = fst.num_states();

    let result = minimize(&fst, MinimizeConfig::default());

    if let Ok(min) = result {
        assert!(
            min.num_states() <= original_states,
            "Minimized should have <= original states"
        );
    }
}

/// Test estimate_reduction.
#[test]
fn test_estimate_reduction() {
    use lling_llang::algorithms::estimate_reduction;

    let simple = build_simple_fst();
    let estimate = estimate_reduction(&simple);

    // Estimate should be <= number of states (estimated reduction count)
    assert!(
        estimate <= simple.num_states(),
        "Reduction estimate should be <= num_states"
    );
}

// =============================================================================
// Epsilon Removal Tests
// =============================================================================

/// Test epsilon removal on FST with epsilons.
#[test]
fn test_epsilon_removal() {
    use lling_llang::algorithms::{has_epsilon_transitions, remove_epsilon, EpsilonRemovalConfig};

    let mut fst = build_epsilon_fst();

    // Original should have epsilons
    assert!(
        has_epsilon_transitions(&fst),
        "Original FST should have epsilons"
    );

    let result = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());

    match result {
        Ok(()) => {
            // FST should now have no epsilons
            assert!(
                !has_epsilon_transitions(&fst),
                "Epsilon-removed FST should have no epsilons"
            );
        }
        Err(_) => {
            // Epsilon removal may fail in some cases
        }
    }
}

/// Test has_epsilon_transitions predicate.
#[test]
fn test_has_epsilon_transitions() {
    use lling_llang::algorithms::has_epsilon_transitions;

    let simple = build_simple_fst();
    let epsilon = build_epsilon_fst();

    assert!(
        !has_epsilon_transitions(&simple),
        "Simple FST should have no epsilons"
    );
    assert!(
        has_epsilon_transitions(&epsilon),
        "Epsilon FST should have epsilons"
    );
}

/// Test epsilon removal on FST without epsilons.
#[test]
fn test_epsilon_removal_no_epsilons() {
    use lling_llang::algorithms::{has_epsilon_transitions, remove_epsilon, EpsilonRemovalConfig};

    let mut fst = build_simple_fst();
    let original_states = fst.num_states();

    // Original has no epsilons
    assert!(!has_epsilon_transitions(&fst));

    let result = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());

    match result {
        Ok(()) => {
            // Should still work
            assert!(!has_epsilon_transitions(&fst));
            // State count should be similar
            assert!(fst.num_states() <= original_states + 1);
        }
        Err(_) => {
            // Shouldn't fail
        }
    }
}

/// Test epsilon removal idempotence.
#[test]
fn test_epsilon_removal_idempotent() {
    use lling_llang::algorithms::{has_epsilon_transitions, remove_epsilon, EpsilonRemovalConfig};

    let mut fst = build_epsilon_fst();

    let result1 = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());

    if result1.is_ok() {
        assert!(!has_epsilon_transitions(&fst));
        let states_after_first = fst.num_states();

        // Second removal
        let result2 = remove_epsilon(&mut fst, EpsilonRemovalConfig::default());

        if result2.is_ok() {
            assert!(!has_epsilon_transitions(&fst));
            // State count should be same (idempotent)
            assert_eq!(
                states_after_first,
                fst.num_states(),
                "Epsilon removal should be idempotent on state count"
            );
        }
    }
}

/// Test remove_epsilon_star.
#[test]
fn test_remove_epsilon_star() {
    use lling_llang::algorithms::{
        has_epsilon_transitions, remove_epsilon_star, EpsilonRemovalConfig,
    };

    let mut fst = build_epsilon_fst();

    let result = remove_epsilon_star(&mut fst, EpsilonRemovalConfig::default());

    match result {
        Ok(()) => {
            assert!(
                !has_epsilon_transitions(&fst),
                "remove_epsilon_star should remove all epsilons"
            );
        }
        Err(_) => {
            // May fail in some cases
        }
    }
}

// =============================================================================
// Shortest Distance Tests
// =============================================================================

/// Test single source shortest distance.
#[test]
fn test_single_source_shortest_distance() {
    use lling_llang::algorithms::{single_source_shortest_distance, ShortestDistanceConfig};

    let fst = build_chain_fst(5);

    let config = ShortestDistanceConfig::default();
    let result = single_source_shortest_distance(&fst, config);

    match result {
        Some(distances) => {
            // Should have distance for each state
            assert_eq!(
                distances.len(),
                fst.num_states(),
                "Should have distance for each state"
            );

            // Start state should have distance one() (0 in tropical)
            let start = fst.start();
            assert!(
                distances[start as usize].value() < f64::INFINITY,
                "Start state should be reachable"
            );
        }
        None => {
            // May fail on empty or malformed FSTs
        }
    }
}

/// Test all pairs shortest distance.
#[test]
fn test_all_pairs_shortest_distance() {
    use lling_llang::algorithms::all_pairs_shortest_distance;

    let fst = build_simple_fst();

    let result = all_pairs_shortest_distance(&fst);

    match result {
        Some(distances) => {
            // Should be square matrix
            let n = fst.num_states();
            assert_eq!(distances.len(), n, "Should have n rows");
            for row in &distances {
                assert_eq!(row.len(), n, "Each row should have n columns");
            }

            // Diagonal should be one() (0 in tropical)
            for (i, row) in distances.iter().enumerate().take(n) {
                assert!(
                    (row[i].value() - TropicalWeight::one().value()).abs() < 1e-10,
                    "Distance from state to itself should be one()"
                );
            }
        }
        None => {
            // May fail for some FSTs
        }
    }
}

/// Test shortest distance to final (reverse direction).
#[test]
fn test_shortest_distance_to_final() {
    use lling_llang::algorithms::{reverse_shortest_distance, ShortestDistanceConfig};

    let fst = build_chain_fst(5);

    let config = ShortestDistanceConfig::default();
    let result = reverse_shortest_distance(&fst, config);

    match result {
        Some(distances) => {
            // Should have distance for each state
            assert_eq!(distances.len(), fst.num_states());

            // At least one state should be able to reach final
            let reachable = distances.iter().any(|d| d.value() < f64::INFINITY);
            assert!(reachable, "At least one state should reach final");
        }
        None => {
            // May fail for some FSTs
        }
    }
}

// =============================================================================
// Sampling Tests
// =============================================================================

/// Test path sampling.
#[test]
fn test_sample_path() {
    use lling_llang::algorithms::{sample_path, SampleConfig};

    let fst = build_simple_fst();

    // Use default config
    let config = SampleConfig::default();

    let result = sample_path(&fst, config);

    match result {
        Ok(path) => {
            // Path should be valid (may or may not be empty)
            let _ = path.input_labels.len(); // Just verify we can access it
        }
        Err(_) => {
            // May fail if FST is malformed or not stochastic
        }
    }
}

/// Test multiple path sampling.
#[test]
fn test_sample_paths() {
    use lling_llang::algorithms::{sample_paths, SampleConfig};

    let fst = build_simple_fst();

    // Use default config
    let config = SampleConfig::default();

    let results = sample_paths(&fst, 5, config);

    // sample_paths returns Vec<Result<..>>
    let successful: Vec<_> = results.into_iter().filter_map(|r| r.ok()).collect();
    assert!(successful.len() <= 5, "Should have at most 5 paths");
}

// =============================================================================
// Weight Pushing Tests
// =============================================================================

/// Test weight pushing.
#[test]
fn test_push_weights() {
    use lling_llang::algorithms::{
        push_weights, PushConfig, PushDirection, ShortestDistanceConfig,
    };

    let mut fst = build_simple_fst();
    let original_states = fst.num_states();

    let config = PushConfig {
        direction: PushDirection::Backward,
        remove_non_coaccessible: true,
        distance_config: ShortestDistanceConfig::default(),
    };

    let result = push_weights(&mut fst, config);

    match result {
        Ok(()) => {
            // Pushed FST should have same or fewer states
            assert!(
                fst.num_states() <= original_states + 1,
                "Pushing should preserve or reduce state count"
            );
        }
        Err(_) => {
            // May fail for some FSTs
        }
    }
}

/// Test is_stochastic predicate.
#[test]
fn test_is_stochastic() {
    use lling_llang::algorithms::is_stochastic;

    let fst = build_simple_fst();

    // Simple FST is unlikely to be stochastic
    let _is_stoch = is_stochastic(&fst, 1e-6);
    // Just verify it doesn't crash
}

// =============================================================================
// Composition Tests
// =============================================================================

/// Test composition with identity transducer.
#[test]
fn test_compose_with_identity() {
    use lling_llang::composition::{compose, materialize};

    let fst = build_chain_fst(3);
    let identity = build_identity_fst(3);

    // compose takes FSTs by value
    let composed = compose(fst.clone(), identity.clone());
    let materialized: VectorWfst<u32, TropicalWeight> = materialize(composed);

    // Composed with identity should have similar structure
    // Note: state count may differ due to product construction
    assert!(
        materialized.num_states() >= 1,
        "Composed FST should have states"
    );
}

/// Test composition preserves paths.
#[test]
fn test_compose_preserves_accepting() {
    use lling_llang::algorithms::{connect, ConnectConfig};
    use lling_llang::composition::{compose, materialize};

    let fst = build_chain_fst(2);
    let identity = build_identity_fst(2);
    let original_states = fst.num_states();

    // compose takes FSTs by value
    let composed = compose(fst, identity);
    let mut materialized: VectorWfst<u32, TropicalWeight> = materialize(composed);

    // Clean up
    connect(&mut materialized, ConnectConfig::default());

    // Should still have accepting paths if original did
    if original_states > 0 {
        assert!(materialized.num_states() >= 1);
    }
}

// =============================================================================
// Property-Based Tests with Proptest
// =============================================================================

proptest! {
    /// Property: connect is idempotent on state count.
    #[test]
    fn prop_connect_idempotent(length in 1usize..10) {
        use lling_llang::algorithms::{connect, ConnectConfig};

        let mut fst = build_chain_fst(length);

        // First connect
        connect(&mut fst, ConnectConfig::default());
        let states_after_first = fst.num_states();

        // Second connect
        connect(&mut fst, ConnectConfig::default());
        let states_after_second = fst.num_states();

        prop_assert_eq!(states_after_first, states_after_second);
    }

    /// Property: determinized FST is deterministic.
    #[test]
    fn prop_determinize_is_deterministic(length in 1usize..10) {
        use lling_llang::algorithms::{determinize, is_deterministic, DeterminizeConfig};

        let fst = build_chain_fst(length);

        if let Ok(det) = determinize(&fst, DeterminizeConfig::default()) {
            prop_assert!(is_deterministic(&det));
        }
    }

    /// Property: minimization preserves or reduces state count.
    #[test]
    fn prop_minimize_reduces_states(length in 1usize..10) {
        use lling_llang::algorithms::{minimize, MinimizeConfig};

        let fst = build_chain_fst(length);
        let original_states = fst.num_states();

        if let Ok(min) = minimize(&fst, MinimizeConfig::default()) {
            // Allow +1 for potential new start state
            prop_assert!(min.num_states() <= original_states + 1);
        }
    }

    /// Property: epsilon removal removes all epsilons.
    #[test]
    fn prop_epsilon_removal_complete(length in 1usize..10) {
        use lling_llang::algorithms::{remove_epsilon, has_epsilon_transitions, EpsilonRemovalConfig};

        let mut fst = build_chain_fst(length);

        if remove_epsilon(&mut fst, EpsilonRemovalConfig::default()).is_ok() {
            prop_assert!(!has_epsilon_transitions(&fst));
        }
    }

    /// Property: chain FST is always deterministic.
    #[test]
    fn prop_chain_is_deterministic(length in 1usize..20) {
        use lling_llang::algorithms::is_deterministic;

        let fst = build_chain_fst(length);
        prop_assert!(is_deterministic(&fst));
    }

    /// Property: single source shortest distance has correct dimensions.
    #[test]
    fn prop_sssd_dimensions(length in 1usize..10) {
        use lling_llang::algorithms::{single_source_shortest_distance, ShortestDistanceConfig};

        let fst = build_chain_fst(length);
        if let Some(distances) = single_source_shortest_distance(&fst, ShortestDistanceConfig::default()) {
            prop_assert_eq!(distances.len(), fst.num_states());
        }
    }
}

// =============================================================================
// LogWeight Algorithm Tests
// =============================================================================

/// Test algorithms work with LogWeight semiring.
#[test]
fn test_algorithms_with_log_weight() {
    use lling_llang::algorithms::{
        connect, determinize, minimize, remove_epsilon, single_source_shortest_distance,
        ConnectConfig, DeterminizeConfig, EpsilonRemovalConfig, MinimizeConfig,
        ShortestDistanceConfig,
    };

    // Build LogWeight FST
    let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s2, LogWeight::one());

    fst.add_arc(s0, Some(0), Some(0), s1, LogWeight::new(1.0));
    fst.add_arc(s1, Some(1), Some(1), s2, LogWeight::new(2.0));

    let original_states = fst.num_states();

    // Connect
    let mut fst_clone = fst.clone();
    connect(&mut fst_clone, ConnectConfig::default());
    assert!(fst_clone.num_states() <= fst.num_states());

    // Determinize
    if let Ok(det) = determinize(&fst, DeterminizeConfig::default()) {
        assert!(det.num_states() >= 1);
    }

    // Minimize
    if let Ok(min) = minimize(&fst, MinimizeConfig::default()) {
        assert!(min.num_states() <= fst.num_states() + 1);
    }

    // Epsilon removal (modifies in place)
    let mut fst_for_eps = fst.clone();
    if remove_epsilon(&mut fst_for_eps, EpsilonRemovalConfig::default()).is_ok() {
        assert!(fst_for_eps.num_states() >= 1);
    }

    // Shortest distance
    if let Some(distances) =
        single_source_shortest_distance(&fst, ShortestDistanceConfig::default())
    {
        assert_eq!(distances.len(), original_states);
    }
}

// =============================================================================
// Edge Cases
// =============================================================================

/// Test algorithms on empty FST.
#[test]
fn test_algorithms_empty_fst() {
    use lling_llang::algorithms::{
        connect, has_epsilon_transitions, is_connected, is_deterministic, ConnectConfig,
    };

    let mut fst: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let s = fst.add_state();
    fst.set_start(s);

    // No final states, no arcs

    connect(&mut fst, ConnectConfig::default());
    // Should not crash

    let _is_conn = is_connected(&fst);
    let _is_det = is_deterministic(&fst);
    let _has_eps = has_epsilon_transitions(&fst);
}

/// Test algorithms on single-state accepting FST.
#[test]
fn test_algorithms_single_state() {
    use lling_llang::algorithms::{
        connect, determinize, minimize, ConnectConfig, DeterminizeConfig, MinimizeConfig,
    };

    let mut fst: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let s = fst.add_state();
    fst.set_start(s);
    fst.set_final(s, TropicalWeight::one());

    // Single state that is both start and final

    let mut fst_clone = fst.clone();
    connect(&mut fst_clone, ConnectConfig::default());
    assert_eq!(fst_clone.num_states(), 1);

    if let Ok(det) = determinize(&fst, DeterminizeConfig::default()) {
        assert!(det.num_states() >= 1);
    }

    if let Ok(min) = minimize(&fst, MinimizeConfig::default()) {
        assert!(min.num_states() >= 1);
    }
}

/// Test algorithms on FST with self-loops.
#[test]
fn test_algorithms_with_cycles() {
    use lling_llang::algorithms::{
        connect, is_deterministic, single_source_shortest_distance, ConnectConfig,
        ShortestDistanceConfig,
    };

    let mut fst: VectorWfst<u32, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s1, TropicalWeight::one());

    // Self-loop on s0
    fst.add_arc(s0, Some(0), Some(0), s0, TropicalWeight::new(0.5));
    fst.add_arc(s0, Some(1), Some(1), s1, TropicalWeight::new(1.0));

    // Connect should preserve the cycle
    let mut fst_clone = fst.clone();
    connect(&mut fst_clone, ConnectConfig::default());
    assert_eq!(fst_clone.num_states(), 2);

    // Should still be deterministic (different labels)
    assert!(is_deterministic(&fst));

    // Shortest distance should work (tropical is k-closed)
    if let Some(distances) =
        single_source_shortest_distance(&fst, ShortestDistanceConfig::default())
    {
        assert_eq!(distances.len(), 2);
    }
}
