//! Tests for known bugs that have been fixed and edge cases.

use lling_llang::algorithms::{
    all_pairs_shortest_distance, compute_accessible, compute_coaccessible, connect,
    single_source_shortest_distance, ConnectConfig, ShortestDistanceConfig,
};
use lling_llang::semiring::{LogWeight, Semiring, TropicalWeight};
use lling_llang::wfst::{MutableWfst, VectorWfst, Wfst, NO_STATE};

/// Empty WFST edge cases.
#[test]
fn empty_wfst_operations() {
    let empty: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    assert_eq!(empty.num_states(), 0);
    assert_eq!(empty.start(), NO_STATE);
}

/// Single state FST that is both start and final.
#[test]
fn single_state_start_final() {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let s = fst.add_state();
    fst.set_start(s);
    fst.set_final(s, TropicalWeight::one());

    assert!(fst.final_weight(s) != TropicalWeight::zero());
    assert_eq!(fst.start(), s);
}

/// Zero weight handling.
#[test]
fn zero_weight_handling() {
    let zero = TropicalWeight::zero();
    let one = TropicalWeight::one();

    // Zero annihilates
    assert_eq!(one.times(&zero).value(), zero.value());
    assert_eq!(zero.times(&one).value(), zero.value());
}

/// Infinity weight handling (tropical zero is infinity).
#[test]
fn infinity_weight_handling() {
    let zero = TropicalWeight::zero();

    // Tropical zero is infinity
    assert!(zero.value().is_infinite());

    // Plus with infinity gives the other value
    let finite = TropicalWeight::new(5.0);
    let result = finite.plus(&zero);
    assert_eq!(result.value(), 5.0);
}

/// Log weight edge cases.
#[test]
fn log_weight_edge_cases() {
    let zero = LogWeight::zero();
    let one = LogWeight::one();

    // One is identity for times
    let a = LogWeight::new(5.0);
    let result = a.times(&one);
    assert!((result.value() - 5.0).abs() < 1e-10);

    // Times with zero gives zero
    let z_result = a.times(&zero);
    assert!(z_result.value().is_infinite() && z_result.value() > 0.0);
}

/// Public graph algorithms should tolerate malformed target IDs.
#[test]
fn malformed_transition_targets_are_ignored() {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    fst.add_states(2);
    fst.set_start(0);
    fst.set_final(1, TropicalWeight::one());
    fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
    fst.add_arc(0, Some('x'), Some('x'), 99, TropicalWeight::new(1.0));

    let accessible = compute_accessible(&fst);
    assert!(accessible.contains(&0));
    assert!(accessible.contains(&1));
    assert!(!accessible.contains(&99));

    let coaccessible = compute_coaccessible(&fst);
    assert!(coaccessible.contains(&0));
    assert!(coaccessible.contains(&1));
    assert!(!coaccessible.contains(&99));

    let distances = single_source_shortest_distance(&fst, ShortestDistanceConfig::default())
        .expect("malformed targets should be skipped");
    assert_eq!(distances.len(), 2);
    assert_eq!(distances[1].value(), 1.0);

    let all_pairs = all_pairs_shortest_distance(&fst).expect("malformed targets should be skipped");
    assert_eq!(all_pairs.len(), 2);
    assert_eq!(all_pairs[0][1].value(), 1.0);

    let removed = connect(&mut fst, ConnectConfig::trim());
    assert_eq!(removed, 0);
    assert!(fst
        .transitions(0)
        .iter()
        .all(|transition| transition.to < 2));
}
