//! Laziness and lifecycle metrics for `lling_wfst_compose` over foreign
//! `vt.scalar-wfst.1` providers.
//!
//! Uses the in-repo [`TestWfst`] provider (metrics-instrumented adjacency
//! lists) to PROVE, by callback counts rather than by inspection:
//! - composition construction is $`\mathcal{O}(1)`$ in provider work:
//!   exactly one
//!   `snapshot` per input, zero `state_info` / `state_arcs`;
//! - expansion is demand-driven: reading only the composed start state
//!   touches exactly the two component start states, even in 6-state inputs;
//! - the captured snapshot caches dedup provider work: a full traversal
//!   expands every component state exactly once;
//! - the composition outlives freed input handles (it holds retained
//!   snapshots), and the retain/release ledger balances to zero once every
//!   owner is gone;
//! - providers WITHOUT `PARALLEL_REENTRANT` are still consumed correctly
//!   through the per-provider serial call gate, while the composed resource
//!   itself keeps advertising the parallel/reentrant contract.
//!
//! Formal-model correspondence (invariant registry owned by the coordinator):
//! - `// INVARIANT-HOOK: LLING-LAZY-1` — laziness: provider expansion
//!   callbacks happen only for product states the consumer actually reads,
//!   and snapshot capture happens exactly once per composed input.
//! - `// INVARIANT-HOOK: LLING-GATE-1` / `LLING-GATE-2` — non-reentrant
//!   providers are serialized by the consumer's call gate; the gate is a
//!   per-captured-provider concern invisible in the composed contract.
#![cfg(feature = "ffi")]

mod support;

use lling_llang::ffi::{
    lling_resource_release, lling_wfst_compose, lling_wfst_free, lling_wfst_resource,
    LlingLlangStatus, LlingWfst,
};
use std::ptr;
use support::interop_wfst::{
    chain_states, discover_scalar_wfst, walk_reachable, TestWfst, TestWfstConfig,
};
use vinary_tree_interop::{wfst_flags, VtResource, VtStatus, VtWfstArc};

/// Left fixture: 6 states, `abcde` in, all-`m` out, arc weight 1, final 0.5.
const LEFT_PAIRS: [(char, char); 5] = [('a', 'm'), ('b', 'm'), ('c', 'm'), ('d', 'm'), ('e', 'm')];
/// Right fixture: 6 states, all-`m` in, `vwxyz` out, arc weight 2, final 0.25.
const RIGHT_PAIRS: [(char, char); 5] = [('m', 'v'), ('m', 'w'), ('m', 'x'), ('m', 'y'), ('m', 'z')];

fn left_provider(config: TestWfstConfig) -> TestWfst {
    TestWfst::new(chain_states(&LEFT_PAIRS, 1.0, 0.5), 0, config)
}

fn right_provider(config: TestWfstConfig) -> TestWfst {
    TestWfst::new(chain_states(&RIGHT_PAIRS, 2.0, 0.25), 0, config)
}

/// Compose two providers through the C surface and hand back the handle plus
/// an owned resource retain for traversal.
fn compose(left: &TestWfst, right: &TestWfst) -> (*mut LlingWfst, VtResource) {
    let mut composed: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_compose(left.as_raw(), right.as_raw(), &mut composed),
        LlingLlangStatus::Ok
    );
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(composed, &mut resource) },
        LlingLlangStatus::Ok
    );
    (composed, resource)
}

// INVARIANT-HOOK: LLING-LAZY-1 — construction cost is exactly one snapshot
// per input and ZERO expansion callbacks; the first composed-state read
// expands exactly the two component start states out of six per side; a
// repeated read hits the product cache and adds nothing.
#[test]
fn compose_snapshots_once_and_expands_only_on_demand() {
    let left = left_provider(TestWfstConfig::default());
    let right = right_provider(TestWfstConfig::default());
    let (composed, resource) = compose(&left, &right);

    let left_metrics = left.metrics();
    let right_metrics = right.metrics();
    for (side, metrics) in [("left", &left_metrics), ("right", &right_metrics)] {
        assert_eq!(metrics.snapshots(), 1, "{side}: exactly one snapshot");
        assert_eq!(metrics.state_info_calls(), 0, "{side}: no info before use");
        assert_eq!(metrics.state_arcs_calls(), 0, "{side}: no arcs before use");
    }

    // Reading ONLY the composed start state expands only each side's start.
    unsafe {
        let table = &*discover_scalar_wfst(resource);
        let mut valid = 0;
        let mut is_final = 0;
        let mut final_weight = f64::NAN;
        assert_eq!(
            table.state_info.expect("state_info published")(
                resource.context,
                0,
                &mut valid,
                &mut is_final,
                &mut final_weight,
            ),
            VtStatus::Ok.to_raw()
        );
        assert_eq!((valid, is_final), (1, 0));
    }
    for (side, metrics) in [("left", &left_metrics), ("right", &right_metrics)] {
        assert_eq!(metrics.snapshots(), 1, "{side}: still one snapshot");
        assert_eq!(
            metrics.state_info_calls(),
            1,
            "{side}: only the start state was queried"
        );
        assert_eq!(
            metrics.state_arcs_calls(),
            1,
            "{side}: only the start state was paged"
        );
    }

    // A second read of the same product state is served from the cache.
    unsafe {
        let table = &*discover_scalar_wfst(resource);
        let mut arc = VtWfstArc::default();
        let mut written = 0;
        let mut total = 0;
        assert_eq!(
            table.state_arcs.expect("state_arcs published")(
                resource.context,
                0,
                0,
                &mut arc,
                1,
                &mut written,
                &mut total,
            ),
            VtStatus::Ok.to_raw()
        );
        assert_eq!((written, total), (1, 1));
        assert_eq!(arc.input_label, u64::from('a'));
        assert_eq!(arc.output_label, u64::from('v'));
        assert_eq!(arc.weight, 3.0, "arc weight is left 1.0 (+) right 2.0");
    }
    for (side, metrics) in [("left", &left_metrics), ("right", &right_metrics)] {
        assert_eq!(metrics.state_info_calls(), 1, "{side}: cache hit");
        assert_eq!(metrics.state_arcs_calls(), 1, "{side}: cache hit");
    }

    lling_resource_release(resource);
    unsafe { lling_wfst_free(composed) };
    drop(left);
    drop(right);
    assert_eq!(left_metrics.balance(), 0);
    assert_eq!(right_metrics.balance(), 0);
}

// INVARIANT-HOOK: LLING-LAZY-1 — the captured-snapshot caches dedup provider
// work: a FULL traversal of the composed chain expands every component state
// exactly once (6 = chain length per side), with the snapshot count pinned
// at one throughout.
#[test]
fn full_traversal_expands_each_component_state_exactly_once() {
    // The chains are genuinely acyclic, so advertise the ACYCLIC hint too:
    // consumption must be identical with the extra flag bit set.
    let flags = wfst_flags::PARALLEL_REENTRANT
        | wfst_flags::IMMUTABLE
        | wfst_flags::LAZY
        | wfst_flags::ACYCLIC;
    let left = left_provider(TestWfstConfig::default().with_flags(flags));
    let right = right_provider(TestWfstConfig::default().with_flags(flags));
    let (composed, resource) = compose(&left, &right);

    let (start, states) = unsafe { walk_reachable(resource, 4) };
    assert_eq!(start, 0, "the composed start id is pinned to zero");
    assert_eq!(
        states.len(),
        LEFT_PAIRS.len() + 1,
        "the composed chain has one product state per component state"
    );
    let final_states: Vec<_> = states.values().filter(|state| state.is_final).collect();
    assert_eq!(final_states.len(), 1);
    assert_eq!(
        final_states[0].final_weight, 0.75,
        "composed final weight is left 0.5 (+) right 0.25"
    );

    let left_metrics = left.metrics();
    let right_metrics = right.metrics();
    for (side, metrics) in [("left", &left_metrics), ("right", &right_metrics)] {
        assert_eq!(metrics.snapshots(), 1, "{side}: snapshot-once holds");
        assert_eq!(
            metrics.state_info_calls(),
            LEFT_PAIRS.len() + 1,
            "{side}: each component state queried exactly once"
        );
        assert_eq!(
            metrics.state_arcs_calls(),
            LEFT_PAIRS.len() + 1,
            "{side}: each component state paged exactly once"
        );
    }

    lling_resource_release(resource);
    unsafe { lling_wfst_free(composed) };
    drop(left);
    drop(right);
    assert_eq!(left_metrics.balance(), 0);
    assert_eq!(right_metrics.balance(), 0);
}

// INVARIANT-HOOK: LLING-LAZY-1 — the composition owns retained snapshots, so
// it remains fully traversable after BOTH input handles are freed; the
// ledger balances to zero only once the composition itself is gone.
#[test]
fn composition_outlives_freed_input_handles() {
    let left = left_provider(TestWfstConfig::default());
    let right = right_provider(TestWfstConfig::default());
    let (composed, resource) = compose(&left, &right);

    let left_metrics = left.metrics();
    let right_metrics = right.metrics();

    // Free the inputs FIRST: the retained snapshots must keep both provider
    // contexts alive on behalf of the composition.
    drop(left);
    drop(right);
    assert!(
        left_metrics.balance() > 0 && right_metrics.balance() > 0,
        "the composition must still hold snapshot retains"
    );

    let (_, states) = unsafe { walk_reachable(resource, 3) };
    assert_eq!(
        states.len(),
        LEFT_PAIRS.len() + 1,
        "traversal after input free must see the whole product chain"
    );
    assert_eq!(
        states.values().map(|state| state.arcs.len()).sum::<usize>(),
        LEFT_PAIRS.len(),
        "one matched arc per chain step"
    );

    lling_resource_release(resource);
    unsafe { lling_wfst_free(composed) };
    assert_eq!(left_metrics.balance(), 0, "left ledger settles to zero");
    assert_eq!(right_metrics.balance(), 0, "right ledger settles to zero");
    assert_eq!(left_metrics.snapshots(), 1);
    assert_eq!(right_metrics.snapshots(), 1);
}

// INVARIANT-HOOK: LLING-GATE-1 — providers without PARALLEL_REENTRANT are
// consumed through the per-captured-provider serial gate with identical
// results. INVARIANT-HOOK: LLING-GATE-2 — the gate is invisible in the
// composed contract: the product resource itself still advertises
// PARALLEL_REENTRANT (its own caches provide the safety).
#[test]
fn serial_providers_compose_identically_through_the_call_gate() {
    let left = left_provider(TestWfstConfig::serial());
    let right = right_provider(TestWfstConfig::serial());
    let (composed, resource) = compose(&left, &right);

    unsafe {
        let table = &*discover_scalar_wfst(resource);
        assert_ne!(
            table.flags & wfst_flags::PARALLEL_REENTRANT,
            0,
            "the composed resource advertises parallel/reentrant regardless \
             of its inputs' gates"
        );
    }

    let (_, states) = unsafe { walk_reachable(resource, 256) };
    assert_eq!(states.len(), LEFT_PAIRS.len() + 1);
    let left_metrics = left.metrics();
    let right_metrics = right.metrics();
    assert_eq!(left_metrics.state_arcs_calls(), LEFT_PAIRS.len() + 1);
    assert_eq!(right_metrics.state_arcs_calls(), RIGHT_PAIRS.len() + 1);

    lling_resource_release(resource);
    unsafe { lling_wfst_free(composed) };
    drop(left);
    drop(right);
    assert_eq!(left_metrics.balance(), 0);
    assert_eq!(right_metrics.balance(), 0);
}
