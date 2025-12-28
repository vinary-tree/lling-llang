//! Integration tests for composition operations.
//!
//! Tests WFST composition.

use lling_llang::semiring::{TropicalWeight, Semiring};
use lling_llang::wfst::{VectorWfst, MutableWfst, Wfst};

/// Test basic WFST construction.
#[test]
fn test_wfst_construction() {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    fst.set_start(s0);
    fst.set_final(s1, TropicalWeight::one());
    fst.add_arc(s0, Some('a'), Some('b'), s1, TropicalWeight::one());

    assert_eq!(fst.num_states(), 2);
    assert_eq!(fst.start(), s0);
    assert!(fst.is_final(s1));
}

/// Test WFST with multiple paths.
#[test]
fn test_wfst_multiple_paths() {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    let s3 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s3, TropicalWeight::one());

    // Two paths to final state
    fst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));
    fst.add_arc(s1, Some('b'), Some('b'), s3, TropicalWeight::new(1.0));

    fst.add_arc(s0, Some('c'), Some('c'), s2, TropicalWeight::new(2.0));
    fst.add_arc(s2, Some('d'), Some('d'), s3, TropicalWeight::new(2.0));

    assert_eq!(fst.num_states(), 4);
}
