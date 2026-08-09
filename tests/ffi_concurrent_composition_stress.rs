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
//! No TSan CI leg exists in this repository yet, so these run native-only;
//! the assertions are pure correctness (no timing dependence).
//!
//! Formal-model correspondence (invariant registry owned by the coordinator):
//! - `// INVARIANT-HOOK: LLING-GATE-1` — serial (non-PARALLEL_REENTRANT)
//!   providers are safely serialized by the per-captured-provider call gate
//!   even under concurrent product traversal (no deadlock, identical views).
//! - `// INVARIANT-HOOK: LLING-GATE-2` — PARALLEL_REENTRANT providers run
//!   gate-free with genuinely concurrent callbacks and identical results.
//! - `// INVARIANT-HOOK: LLING-COMP-1` — the concurrently traversed product
//!   is the same machine the single-threaded composition denotes.
#![cfg(feature = "ffi")]

mod support;

use lling_llang::ffi::{
    lling_resource_release, lling_wfst_compose, lling_wfst_free, lling_wfst_resource, LlingStatus,
    LlingWfst,
};
use std::collections::BTreeMap;
use std::ptr;
use support::interop_wfst::{
    canonical_of_walk, discover_scalar_wfst, walk_reachable, TestArc, TestState, TestWfst,
    TestWfstConfig, WalkedState,
};
use vinary_tree_interop::{VtResource, VtStatus, VtWfstArc};

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
        LlingStatus::Ok
    );
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(composed, &mut resource) },
        LlingStatus::Ok
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
        LlingStatus::Ok
    );
    let mut fresh_resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(fresh_composed, &mut fresh_resource) },
        LlingStatus::Ok
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
// serialized by the per-captured-provider gate while the product layer stays
// gate-free; no deadlock, identical views, balanced ledger.
#[test]
fn concurrent_walkers_agree_over_serial_inputs() {
    run_concurrent_stress(TestWfstConfig::serial());
}
