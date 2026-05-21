//! Tests for known bugs that have been fixed and edge cases.

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
