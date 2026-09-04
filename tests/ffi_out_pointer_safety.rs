//! Out-pointer validate-before-materialize discipline for the WFST C ABI.
//!
//! Regression coverage for a leak/orphan class of bug where a NULL out-pointer
//! was validated only AFTER a resource had already been materialized. Rust
//! assignment evaluates its right operand first, so `*out = Box::into_raw(..)`
//! with a null `out` leaked the fully-built handle — and, for `compose`, both
//! captured snapshot retains — while still returning `NullPointer`.
//! `add_state` had the sibling shape: it mutated the builder graph before
//! validating its out-pointer, leaving an orphan state behind on the failure.
//!
//! The fixed contract these pin: on a null out-pointer the ABI returns
//! `NullPointer` and performs NO provider work, takes NO retain, and mutates NO
//! caller state. The retain balance is proven via the metrics-instrumented
//! in-repo provider; the residual owned-graph leak on the old `import` path is
//! additionally a Miri/valgrind target in the dynamic-analysis wave.
#![cfg(feature = "ffi")]

mod support;

use lling_llang::ffi::{
    lling_wfst_builder_add_state, lling_wfst_builder_free, lling_wfst_builder_new,
    lling_wfst_compose, lling_wfst_import, LlingLlangStatus,
};
use std::ptr;
use support::interop_wfst::{chain_states, TestWfst};

const PAIRS: [(char, char); 3] = [('a', 'x'), ('b', 'y'), ('c', 'z')];

fn provider() -> TestWfst {
    TestWfst::tropical(chain_states(&PAIRS, 1.0, 0.5), 0)
}

/// `lling_wfst_compose` validates its out-pointer before composing: a null out
/// takes no snapshot retain and leaks nothing. Before the fix it captured both
/// inputs (one snapshot each) and then leaked the composition together with
/// those two retains, so each provider ledger stayed at +1 forever.
#[test]
fn compose_with_null_out_pointer_captures_nothing() {
    let left = provider();
    let right = provider();
    let left_metrics = left.metrics();
    let right_metrics = right.metrics();

    let status = lling_wfst_compose(left.as_raw(), right.as_raw(), ptr::null_mut());
    assert_eq!(status, LlingLlangStatus::NullPointer);
    assert_eq!(
        left_metrics.snapshots(),
        0,
        "no snapshot may be taken on the null-out path"
    );
    assert_eq!(right_metrics.snapshots(), 0);

    drop(left);
    drop(right);
    assert_eq!(
        left_metrics.balance(),
        0,
        "no leaked retain (the ledger was +1 before the fix)"
    );
    assert_eq!(right_metrics.balance(), 0);
}

/// `lling_wfst_import` validates its out-pointer before doing any provider
/// work: a null out reads nothing from the provider, so it cannot have
/// materialized (and leaked) the imported graph.
#[test]
fn import_with_null_out_pointer_does_no_provider_work() {
    let source = provider();
    let source_metrics = source.metrics();

    let status = lling_wfst_import(source.as_raw(), ptr::null_mut());
    assert_eq!(status, LlingLlangStatus::NullPointer);
    assert_eq!(
        source_metrics.snapshots(),
        0,
        "the import must not touch the provider before validating out_wfst"
    );

    drop(source);
    assert_eq!(source_metrics.balance(), 0);
}

/// `lling_wfst_builder_add_state` validates its out-pointer before mutating the
/// graph: a null out adds no state, so the next successful add returns the very
/// next id — no orphan consumed one.
#[test]
fn add_state_with_null_out_pointer_adds_no_orphan() {
    let mut builder = ptr::null_mut();
    assert_eq!(lling_wfst_builder_new(&mut builder), LlingLlangStatus::Ok);

    let mut s0 = u32::MAX;
    assert_eq!(
        lling_wfst_builder_add_state(builder, &mut s0),
        LlingLlangStatus::Ok
    );

    assert_eq!(
        lling_wfst_builder_add_state(builder, ptr::null_mut()),
        LlingLlangStatus::NullPointer
    );

    let mut s1 = u32::MAX;
    assert_eq!(
        lling_wfst_builder_add_state(builder, &mut s1),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        s1,
        s0 + 1,
        "the failed add_state must not have consumed a state id"
    );

    unsafe { lling_wfst_builder_free(builder) };
}
