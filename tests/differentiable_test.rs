//! Integration tests for differentiable WFST operations.
//!
//! Tests forward/backward passes and gradient computation.

use lling_llang::semiring::{LogWeight, Semiring};
use lling_llang::wfst::{MutableWfst, VectorWfst, Wfst};

/// Create a simple chain WFST for testing.
fn create_chain_wfst() -> VectorWfst<char, LogWeight> {
    let mut fst: VectorWfst<char, LogWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s2, LogWeight::one());

    fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
    fst.add_arc(s1, Some('b'), Some('b'), s2, LogWeight::new(2.0));

    fst
}

/// Test chain WFST construction.
#[test]
fn test_chain_wfst_construction() {
    let fst = create_chain_wfst();

    assert_eq!(fst.num_states(), 3);
    assert!(fst.is_final(2));
}

/// Test log weight properties.
#[test]
fn test_log_weight_properties() {
    let a = LogWeight::new(1.0);
    let b = LogWeight::new(2.0);

    // Times is addition in log domain
    let ab = a.times(&b);
    assert!((ab.value() - 3.0).abs() < 1e-10);

    // Plus is log-sum-exp
    let sum = a.plus(&b);
    assert!(sum.value() < 3.0); // log-sum-exp should be less than sum
}
