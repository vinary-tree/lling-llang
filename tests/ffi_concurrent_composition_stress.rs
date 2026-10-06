//! Concurrent traversal stress over one shared composed resource.
//!
//! `CompositionResource` deliberately has NO resource-wide evaluation gate:
//! its product registry and state cache are `RwLock`-protected and expansion
//! is performed by whichever caller misses the cache first (a racing second
//! expansion of the same product state is benign duplicated work whose
//! result is discarded by the first-writer-wins cache insert). These tests
//! drive N threads over ONE composed resource — each thread walking in its
//! own randomized order with its own page capacity — and require:
//!
//! - every thread observes IDENTICAL per-state information and arc pages
//!   (same product-state ids, same targets, same weights);
//! - the concurrent view canonicalizes to exactly the single-threaded walk
//!   of a fresh composition of the same inputs (numbering-invariant);
//! - provider callbacks stay balanced: each component state is expanded at
//!   least once and at most once per racing thread, snapshot capture stays
//!   at exactly one per input, and the retain/release ledger settles to
//!   zero — under BOTH provider gate regimes.
//!
//! Hosted CI also runs this boundary suite under AddressSanitizer and
//! ThreadSanitizer. The assertions themselves check behavior directly;
//! callback delays only make illegal overlap observable, not acceptable.
//!
//! Formal-model correspondence (invariant registry owned by the coordinator):
//! - `// INVARIANT-HOOK: LLING-GATE-1` — serial (non-PARALLEL_REENTRANT)
//!   providers are safely serialized by the context-shared call gate
//!   even under concurrent product traversal (no deadlock, identical views).
//! - `// INVARIANT-HOOK: LLING-GATE-3..6` — registration/wakeup, recursive
//!   rejection, unlocked customer callbacks, and waiter accounting are
//!   checked by the turnstile model and the generated gate-state tests.
//! - `// INVARIANT-HOOK: LLING-GATE-7..8` — nested callback cycles reject
//!   without parking while independent captured providers may overlap.
//! - `// INVARIANT-HOOK: LLING-GATE-9..11` — generated schedules check the
//!   admission state domain, active ownership, and parking ownership.
//! - `// INVARIANT-HOOK: LLING-GATE-2` — PARALLEL_REENTRANT providers run
//!   gate-free with genuinely concurrent callbacks and identical results.
//! - `// INVARIANT-HOOK: LLING-COMP-1` — the concurrently traversed product
//!   is the same machine the single-threaded composition denotes.
#![cfg(feature = "ffi")]

mod support;

use lling_llang::ffi::{
    lling_resource_release, lling_wfst_compose, lling_wfst_free, lling_wfst_resource,
    LlingLlangStatus, LlingWfst,
};
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::ptr;
use std::sync::{Arc, Barrier};
use std::time::Duration;
use support::interop_wfst::{
    alias_query_calls, canonical_of_walk, discover_scalar_wfst, walk_reachable, TestArc, TestState,
    TestWfst, TestWfstConfig, WalkedState,
};
use vinary_tree_interop::{wfst_flags, VtResource, VtStatus, VtWfstArc};

const THREADS: usize = 8;
const LAYERS: usize = 6;
const WIDTH: usize = 4;

/// `VtResource` is two raw words; the composed producer advertises
/// PARALLEL_REENTRANT, so sharing the words across walker threads is sound.
///
/// The accessor exists so closures capture the WHOLE Send wrapper: RFC 2229
/// precise capture would otherwise narrow a field access (even through a
/// destructuring `let`) down to the non-Send `VtResource` field itself.
#[derive(Clone, Copy)]
struct SharedResource(VtResource);
unsafe impl Send for SharedResource {}
unsafe impl Sync for SharedResource {}
impl SharedResource {
    fn get(self) -> VtResource {
        self.0
    }
}

/// Which side of the composition a layered fixture feeds.
#[derive(Clone, Copy)]
enum Side {
    /// Outputs the match alphabet {x, y}.
    Left,
    /// Consumes the match alphabet {x, y}.
    Right,
}

/// Deterministic layered DAG: one start state fanning into `LAYERS` layers
/// of `WIDTH` states; every state carries two arcs into the next layer
/// (slots `j` and `j+1 mod WIDTH`). Left-side arcs OUTPUT the match alphabet
/// {x, y} (input `a`..`c` by layer); right-side arcs CONSUME it (output
/// `p`..`r` by layer), so the product branches at every matched slot.
fn layered_states(side: Side) -> Vec<TestState> {
    let match_alphabet = ['x', 'y'];
    let state_id = |layer: usize, slot: usize| -> u64 {
        u64::try_from(1 + (layer - 1) * WIDTH + slot).expect("layered fixture fits u64")
    };
    let arc = |layer: usize, from_slot: usize, to_slot: usize| -> TestArc {
        let matched = match_alphabet[(from_slot + to_slot) % 2];
        let weight = 1.0 + (from_slot as f64) * 0.25;
        match side {
            Side::Left => {
                let input = char::from(b'a' + (layer % 3) as u8);
                TestArc::pair(input, matched, state_id(layer + 1, to_slot), weight)
            }
            Side::Right => {
                let output = char::from(b'p' + (layer % 3) as u8);
                TestArc::pair(matched, output, state_id(layer + 1, to_slot), weight)
            }
        }
    };

    let mut states = Vec::with_capacity(1 + LAYERS * WIDTH);
    // Start state fans into layer 1 slots 0 and 1 (arc(0, s, s) targets
    // state_id(1, s) by construction).
    states.push(TestState::interior(vec![arc(0, 0, 0), arc(0, 1, 1)]));
    for layer in 1..=LAYERS {
        for slot in 0..WIDTH {
            if layer == LAYERS {
                states.push(TestState::accepting((slot as f64) * 0.5, Vec::new()));
            } else {
                states.push(TestState::interior(vec![
                    arc(layer, slot, slot),
                    arc(layer, slot, (slot + 1) % WIDTH),
                ]));
            }
        }
    }
    states
}

/// Splitmix-style deterministic generator for shuffled walk orders.
struct XorShift(u64);
impl XorShift {
    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }
}

/// Walk the reachable graph in a RANDOMIZED order (per `seed`), paging arcs
/// `page_capacity` at a time, asserting `out_total` stability per state.
///
/// # Safety
/// `resource` must be a live `vt.scalar-wfst.1` resource safe for concurrent
/// calls.
unsafe fn walk_shuffled(
    resource: VtResource,
    page_capacity: usize,
    seed: u64,
) -> BTreeMap<u64, WalkedState> {
    let table = &*discover_scalar_wfst(resource);
    let state_info = table.state_info.expect("state_info published");
    let state_arcs = table.state_arcs.expect("state_arcs published");
    let mut start = 0;
    assert_eq!(
        table.start.expect("start published")(resource.context, &mut start),
        VtStatus::Ok.to_raw()
    );

    let mut rng = XorShift(seed | 1);
    let mut pending = vec![start];
    let mut states: BTreeMap<u64, WalkedState> = BTreeMap::new();
    let mut page = vec![VtWfstArc::default(); page_capacity];
    while !pending.is_empty() {
        let pick = usize::try_from(rng.next()).unwrap_or(usize::MAX) % pending.len();
        let state = pending.swap_remove(pick);
        if states.contains_key(&state) {
            continue;
        }

        let mut valid = 0;
        let mut is_final = 0;
        let mut final_weight = f64::NAN;
        assert_eq!(
            state_info(
                resource.context,
                state,
                &mut valid,
                &mut is_final,
                &mut final_weight,
            ),
            VtStatus::Ok.to_raw()
        );
        assert_eq!(valid, 1, "reachable product state {state} must be valid");

        let mut arcs = Vec::new();
        let mut offset = 0usize;
        let mut expected_total = None;
        loop {
            let mut written = usize::MAX;
            let mut total = usize::MAX;
            assert_eq!(
                state_arcs(
                    resource.context,
                    state,
                    offset,
                    page.as_mut_ptr(),
                    page.len(),
                    &mut written,
                    &mut total,
                ),
                VtStatus::Ok.to_raw()
            );
            assert!(written <= page.len());
            match expected_total {
                None => expected_total = Some(total),
                Some(expected) => assert_eq!(total, expected, "out_total must stay stable"),
            }
            arcs.extend_from_slice(&page[..written]);
            offset += written;
            if offset >= total {
                assert_eq!(offset, total);
                break;
            }
            assert!(written > 0, "provider must make progress");
        }
        for arc in &arcs {
            if !states.contains_key(&arc.target_state) {
                pending.push(arc.target_state);
            }
        }
        states.insert(
            state,
            WalkedState {
                is_final: is_final == 1,
                final_weight,
                arcs,
            },
        );
    }
    states
}

/// Compose two layered providers under `config` and race `THREADS` walkers
/// over the ONE composed resource; verify identical views, canonical
/// equality with a fresh single-threaded composition, provider-callback
/// bounds, and a zero ledger balance at the end.
fn run_concurrent_stress(config: TestWfstConfig) {
    let left = TestWfst::new(layered_states(Side::Left), 0, config);
    let right = TestWfst::new(layered_states(Side::Right), 0, config);
    let left_metrics = left.metrics();
    let right_metrics = right.metrics();

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
    let shared = SharedResource(resource);

    // Race THREADS shuffled walkers over the SAME lazily expanding product.
    let views: Vec<BTreeMap<u64, WalkedState>> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(THREADS);
        for thread_index in 0..THREADS {
            handles.push(scope.spawn(move || {
                // The method call captures the WHOLE Send wrapper (see
                // SharedResource::get), not its raw-pointer field.
                let resource = shared.get();
                let capacity = 1 + thread_index % 5;
                let seed = 0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(thread_index as u64 + 1);
                unsafe { walk_shuffled(resource, capacity, seed) }
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("walker thread must not panic"))
            .collect()
    });

    // Every thread saw the same machine, id-for-id and arc-for-arc.
    let reference_view = &views[0];
    assert!(
        !reference_view.is_empty(),
        "the layered product must be non-trivial"
    );
    for (thread_index, view) in views.iter().enumerate().skip(1) {
        assert_eq!(
            view, reference_view,
            "thread {thread_index} observed a different product"
        );
    }

    // The concurrent view is the SAME machine a fresh, single-threaded
    // composition of the same inputs denotes (canonical, id-invariant).
    let fresh_left = TestWfst::new(layered_states(Side::Left), 0, config);
    let fresh_right = TestWfst::new(layered_states(Side::Right), 0, config);
    let mut fresh_composed: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_compose(
            fresh_left.as_raw(),
            fresh_right.as_raw(),
            &mut fresh_composed
        ),
        LlingLlangStatus::Ok
    );
    let mut fresh_resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(fresh_composed, &mut fresh_resource) },
        LlingLlangStatus::Ok
    );
    let (fresh_start, fresh_states) = unsafe { walk_reachable(fresh_resource, 256) };
    assert_eq!(
        canonical_of_walk(0, reference_view),
        canonical_of_walk(fresh_start, &fresh_states),
        "concurrent and single-threaded compositions must denote one machine"
    );

    // Provider-callback discipline under racing: snapshot-once still holds,
    // and duplicated work from racing cache misses is bounded — a thread's
    // own first expansion of a component state is visible to its later
    // calls, so each thread misses each component state at most once
    // (first-writer-wins cache). Only REACHABLE component states are ever
    // expanded, so the lower bound is the two start states' expansions.
    let component_states = 1 + LAYERS * WIDTH;
    for (side, metrics) in [("left", &left_metrics), ("right", &right_metrics)] {
        assert_eq!(metrics.snapshots(), 1, "{side}: snapshot-once under racing");
        let arcs_calls = metrics.state_arcs_calls();
        assert!(
            (1..=component_states * THREADS).contains(&arcs_calls),
            "{side}: expansion calls {arcs_calls} must lie in \
             [1, {}]",
            component_states * THREADS
        );
    }

    // Teardown in adversarial order: inputs first, compositions after.
    drop(left);
    drop(right);
    drop(fresh_left);
    drop(fresh_right);
    lling_resource_release(resource);
    lling_resource_release(fresh_resource);
    unsafe {
        lling_wfst_free(composed);
        lling_wfst_free(fresh_composed);
    }
    assert_eq!(left_metrics.balance(), 0, "left ledger settles to zero");
    assert_eq!(right_metrics.balance(), 0, "right ledger settles to zero");
}

// INVARIANT-HOOK: LLING-GATE-2 — PARALLEL_REENTRANT inputs: no gate at any
// layer; concurrent expansion races resolve to one consistent machine.
// INVARIANT-HOOK: LLING-COMP-1 — the machine equals the single-threaded one.
#[test]
fn concurrent_walkers_agree_over_parallel_reentrant_inputs() {
    run_concurrent_stress(TestWfstConfig::default());
}

// INVARIANT-HOOK: LLING-GATE-1 — serial inputs: every provider callback is
// serialized by the context-shared gate while the product layer stays
// gate-free; no deadlock, identical views, balanced ledger.
#[test]
fn concurrent_walkers_agree_over_serial_inputs() {
    run_concurrent_stress(TestWfstConfig::serial());
}

fn assert_capture_overlap(
    config: TestWfstConfig,
    expected_parallel: bool,
    callers: usize,
    distinct_snapshot: bool,
) {
    // Independent compositions capture the immutable source twice. Each
    // snapshot may retain that context or use its own allocation. The delayed
    // callback ledger observes real overlap, not just equal gate pointers.
    let source = TestWfst::new(
        vec![
            TestState::interior(vec![TestArc::pair('a', 'a', 1, 0.0)]),
            TestState::accepting(0.0, Vec::new()),
        ],
        0,
        config.with_callback_delay(Duration::from_millis(5)),
    );
    let metrics = source.metrics();
    let live = source.as_raw();
    let table = unsafe { &*discover_scalar_wfst(live) };
    let mut snapshot = VtResource::NULL;
    assert_eq!(
        unsafe { table.snapshot.unwrap()(live.context, &mut snapshot) },
        VtStatus::Ok.to_raw()
    );
    assert_eq!(snapshot.context == live.context, !distinct_snapshot);
    unsafe { ((*snapshot.vtable).release.unwrap())(snapshot.context) };
    let mut owned = Vec::new();
    for _ in 0..callers {
        let mut composed: *mut LlingWfst = ptr::null_mut();
        assert_eq!(
            lling_wfst_compose(source.as_raw(), source.as_raw(), &mut composed),
            LlingLlangStatus::Ok
        );
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(composed, &mut resource) },
            LlingLlangStatus::Ok
        );
        owned.push((composed, SharedResource(resource)));
    }
    assert_eq!(metrics.snapshots(), 1 + 2 * callers);
    // From here only captured snapshot retains may keep provider contexts
    // alive; every callback below must execute before those owners release.
    drop(source);
    assert!(metrics.balance() > 0);
    let ready = Arc::new(Barrier::new(callers));
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for (_, resource) in &owned {
            let resource = *resource;
            let ready = Arc::clone(&ready);
            workers.push(scope.spawn(move || {
                ready.wait();
                let (_, states) = unsafe { walk_reachable(resource.get(), 2) };
                assert!(!states.is_empty());
            }));
        }
        for worker in workers {
            worker.join().expect("same-context walk must complete");
        }
    });
    assert_eq!(metrics.callbacks_in_flight(), 0);
    if expected_parallel || distinct_snapshot {
        assert!(
            metrics.peak_callbacks_in_flight() >= 2,
            "independent or parallel snapshot contexts must exercise callback overlap"
        );
    } else {
        assert_eq!(
            metrics.peak_callbacks_in_flight(),
            1,
            "all captures of one serial context must share admission"
        );
    }
    for (composed, resource) in owned {
        lling_resource_release(resource.get());
        unsafe { lling_wfst_free(composed) };
    }
    assert_eq!(metrics.balance(), 0);
}

// INVARIANT-HOOK: LLING-ALIAS-1..13 — two snapshots of one context share
// admission, while a genuinely parallel context still overlaps callbacks.
#[test]
fn independent_captures_of_one_serial_context_never_overlap_callbacks() {
    assert_capture_overlap(
        TestWfstConfig::serial().with_snapshot_base_vtable_alias(),
        false,
        4,
        false,
    );
}

#[test]
fn same_context_snapshot_may_use_a_distinct_compatible_base_vtable() {
    let source = TestWfst::new(
        vec![TestState::accepting(0.0, Vec::new())],
        0,
        TestWfstConfig::serial().with_snapshot_base_vtable_alias(),
    );
    let metrics = source.metrics();
    let live = source.as_raw();
    let alias_queries_before = alias_query_calls();
    let mut snapshot = VtResource::NULL;
    let table = unsafe { &*discover_scalar_wfst(live) };
    assert_eq!(
        unsafe { table.snapshot.unwrap()(live.context, &mut snapshot) },
        VtStatus::Ok.to_raw()
    );
    assert_eq!(live.context, snapshot.context);
    assert_ne!(live.vtable, snapshot.vtable);
    assert!(!unsafe { discover_scalar_wfst(snapshot) }.is_null());
    assert!(alias_query_calls() > alias_queries_before);
    unsafe { ((*snapshot.vtable).release.unwrap())(snapshot.context) };
    drop(source);
    assert_eq!(metrics.balance(), 0);
}

#[test]
fn concurrent_captures_serialize_pre_flag_interface_discovery() {
    let source = TestWfst::new(
        vec![TestState::accepting(0.0, Vec::new())],
        0,
        TestWfstConfig::serial()
            .with_snapshot_base_vtable_alias()
            .with_callback_delay(Duration::from_millis(5)),
    );
    let metrics = source.metrics();
    let borrowed = SharedResource(source.as_raw());
    let ready = Arc::new(Barrier::new(4));
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..4 {
            let ready = Arc::clone(&ready);
            workers.push(scope.spawn(move || {
                ready.wait();
                let mut composed: *mut LlingWfst = ptr::null_mut();
                assert_eq!(
                    lling_wfst_compose(borrowed.get(), borrowed.get(), &mut composed),
                    LlingLlangStatus::Ok
                );
                unsafe { lling_wfst_free(composed) };
            }));
        }
        for worker in workers {
            worker.join().expect("concurrent capture must complete");
        }
    });
    assert_eq!(metrics.snapshots(), 8);
    assert_eq!(metrics.callbacks_in_flight(), 0);
    assert_eq!(
        metrics.peak_callbacks_in_flight(),
        1,
        "base discovery and WFST callbacks must share admission"
    );
    drop(source);
    assert_eq!(metrics.balance(), 0);
}

#[test]
fn independent_captures_of_one_parallel_context_do_overlap_callbacks() {
    assert_capture_overlap(TestWfstConfig::default(), true, 4, false);
}

#[test]
#[should_panic(expected = "snapshot alias split provider identity")]
fn host_provider_snapshot_vtable_identity_mutant_is_detected() {
    let source = TestWfst::new(
        vec![TestState::accepting(0.0, Vec::new())],
        0,
        TestWfstConfig::serial().with_snapshot_base_vtable_alias(),
    );
    let metrics = source.metrics();
    let live = source.as_raw();
    let table = unsafe { &*discover_scalar_wfst(live) };
    let mut snapshot = VtResource::NULL;
    assert_eq!(
        unsafe { table.snapshot.unwrap()(live.context, &mut snapshot) },
        VtStatus::Ok.to_raw()
    );
    assert_eq!(snapshot.context, live.context);
    assert_ne!(snapshot.vtable, live.vtable);
    // Mutant: key the admission registry by vtable pointer rather than the
    // retained context identity, splitting one provider into two gates.
    let mutant_same_identity = snapshot.vtable == live.vtable;
    unsafe { ((*snapshot.vtable).release.unwrap())(snapshot.context) };
    drop(source);
    assert_eq!(metrics.balance(), 0);
    assert!(
        mutant_same_identity,
        "snapshot alias split provider identity"
    );
}

#[test]
#[should_panic(expected = "serial callback bypassed admission")]
fn host_provider_serial_gate_bypass_mutant_is_detected() {
    let source = TestWfst::new(
        vec![TestState::accepting(0.0, Vec::new())],
        0,
        TestWfstConfig::serial(),
    );
    let metrics = source.metrics();
    let resource = SharedResource(source.as_raw());
    let table_address = unsafe { discover_scalar_wfst(resource.get()) } as usize;
    metrics.set_callback_barrier(Some(Arc::new(Barrier::new(2))));
    std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(move || {
                    let table = table_address as *const vinary_tree_interop::VtWfstVTable;
                    let mut valid = 0;
                    let mut finality = 0;
                    let mut weight = 0.0;
                    let raw = unsafe {
                        ((*table).state_info.unwrap())(
                            resource.get().context,
                            0,
                            &mut valid,
                            &mut finality,
                            &mut weight,
                        )
                    };
                    assert_eq!(raw, VtStatus::Ok.to_raw());
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
    });
    metrics.set_callback_barrier(None);
    assert_eq!(metrics.callbacks_in_flight(), 0);
    let peak = metrics.peak_callbacks_in_flight();
    drop(source);
    assert_eq!(metrics.balance(), 0);
    assert_eq!(peak, 1, "serial callback bypassed admission");
}

// INVARIANT-HOOK: LLING-HOST-7..8 — actual FFI captures share one context
// gate, pin snapshot ownership, and admit overlap only for a parallel claim.
// INVARIANT-HOOK: LLING-HOST-12 — release the base owner before traversal;
// captured snapshot retains must keep every provider callback live.
// INVARIANT-HOOK: LLING-HOST-16 — the generated context-gate schedule in
// src/bindings.rs tracks each callback's owner through admission and release.
// INVARIANT-HOOK: LLING-HOST-17 — the parallel fixture reaches overlapping
// callbacks, while the serial gate-bypass mutant exposes the missing gate.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn host_provider_generated_wfst_snapshot_and_admission(
        callers in 2usize..5,
        parallel in any::<bool>(),
        alias_base_vtable in any::<bool>(),
        distinct_snapshot in any::<bool>(),
    ) {
        let mut config = if parallel {
            TestWfstConfig::default()
        } else {
            TestWfstConfig::serial()
        };
        if alias_base_vtable {
            config = config.with_snapshot_base_vtable_alias();
        }
        if distinct_snapshot {
            config = config.with_snapshot_distinct_context();
        }
        assert_capture_overlap(config, parallel, callers, distinct_snapshot);
    }

    #[test]
    fn host_provider_generated_conflicting_claim_rejection(
        live_parallel in any::<bool>(),
        alias_base_vtable in any::<bool>(),
        distinct_snapshot in any::<bool>(),
    ) {
        let mut config = if live_parallel {
            TestWfstConfig::default()
        } else {
            TestWfstConfig::serial()
        };
        let snapshot_flags = if live_parallel { 0 } else { wfst_flags::PARALLEL_REENTRANT };
        config = config.with_snapshot_flags(snapshot_flags);
        if alias_base_vtable {
            config = config.with_snapshot_base_vtable_alias();
        }
        if distinct_snapshot {
            config = config.with_snapshot_distinct_context();
        }
        let source = TestWfst::new(
            vec![TestState::accepting(0.0, Vec::new())],
            0,
            config,
        );
        let metrics = source.metrics();
        let mut composed: *mut LlingWfst = ptr::null_mut();
        let status = lling_wfst_compose(source.as_raw(), source.as_raw(), &mut composed);
        if distinct_snapshot {
            prop_assert_eq!(status, LlingLlangStatus::Ok);
            prop_assert!(!composed.is_null());
            unsafe { lling_wfst_free(composed) };
        } else {
            prop_assert_eq!(status, LlingLlangStatus::ProviderError);
            prop_assert!(composed.is_null());
        }
        drop(source);
        prop_assert_eq!(metrics.balance(), 0);
        prop_assert_eq!(metrics.callbacks_in_flight(), 0);
    }
}

#[test]
fn contradictory_live_and_snapshot_parallel_claims_are_rejected() {
    for (live, snapshot) in [
        (TestWfstConfig::serial(), wfst_flags::PARALLEL_REENTRANT),
        (TestWfstConfig::default(), 0),
    ] {
        let source = TestWfst::new(
            vec![TestState::accepting(0.0, Vec::new())],
            0,
            live.with_snapshot_flags(snapshot)
                .with_snapshot_base_vtable_alias(),
        );
        let metrics = source.metrics();
        let mut composed: *mut LlingWfst = ptr::null_mut();
        assert_eq!(
            lling_wfst_compose(source.as_raw(), source.as_raw(), &mut composed),
            LlingLlangStatus::ProviderError
        );
        assert!(composed.is_null(), "failed capture cannot publish a graph");
        drop(source);
        assert_eq!(metrics.balance(), 0, "failed capture settles all retains");
        assert_eq!(metrics.callbacks_in_flight(), 0);
    }
}
