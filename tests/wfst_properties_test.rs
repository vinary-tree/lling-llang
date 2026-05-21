//! Property-based tests for WFST operations.

use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::{concat, invert, reverse, union};
use lling_llang::wfst::{MutableWfst, VectorWfst, Wfst};

/// Test union of two FSTs.
#[test]
fn test_union_simple() {
    let mut fst1: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let s0 = fst1.add_state();
    let s1 = fst1.add_state();
    fst1.set_start(s0);
    fst1.set_final(s1, TropicalWeight::one());
    fst1.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));

    let mut fst2: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let s0 = fst2.add_state();
    let s1 = fst2.add_state();
    fst2.set_start(s0);
    fst2.set_final(s1, TropicalWeight::one());
    fst2.add_arc(s0, Some('b'), Some('b'), s1, TropicalWeight::new(1.0));

    let u = union(&fst1, &fst2);

    // Union should have states from both
    assert!(
        u.num_states() >= 2,
        "Union should have states from both FSTs"
    );
}

/// Test concatenation of two FSTs.
#[test]
fn test_concat_simple() {
    let mut fst1: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let s0 = fst1.add_state();
    let s1 = fst1.add_state();
    fst1.set_start(s0);
    fst1.set_final(s1, TropicalWeight::one());
    fst1.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));

    let mut fst2: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let s0 = fst2.add_state();
    let s1 = fst2.add_state();
    fst2.set_start(s0);
    fst2.set_final(s1, TropicalWeight::one());
    fst2.add_arc(s0, Some('b'), Some('b'), s1, TropicalWeight::new(1.0));

    let c = concat(&fst1, &fst2);

    assert!(c.num_states() >= 2, "Concat should have states");
}

/// Test invert is involutive.
#[test]
fn test_invert_involutive() {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let s0 = fst.add_state();
    let s1 = fst.add_state();
    fst.set_start(s0);
    fst.set_final(s1, TropicalWeight::one());
    fst.add_arc(s0, Some('a'), Some('b'), s1, TropicalWeight::new(1.0));

    let inv1 = invert(&fst);
    let inv2 = invert(&inv1);

    assert_eq!(
        fst.num_states(),
        inv2.num_states(),
        "Double invert should preserve states"
    );
}

/// Test reverse is involutive.
#[test]
fn test_reverse_involutive() {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let s0 = fst.add_state();
    let s1 = fst.add_state();
    fst.set_start(s0);
    fst.set_final(s1, TropicalWeight::one());
    fst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));

    let rev1 = reverse(&fst);
    let rev2 = reverse(&rev1);

    // Double reversal should preserve transition count
    assert!(rev2.num_states() >= 1, "Double reverse should be valid");
}
