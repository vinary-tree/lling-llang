//! Property-based round trip over the C ABI: native graph -> C builder ->
//! `lling_wfst_resource` -> `lling_wfst_import` -> exported resource again.
//!
//! For arbitrary Unicode/tropical WFSTs (the `test-utils` strategy
//! `arb_tropical_wfst`, cycles and epsilons included), the reachable
//! subgraph must survive the full boundary crossing bit-for-bit: state count,
//! start, per-state finality and final weights, and the per-state arc
//! sequence (labels, epsilon flags, weights, targets) compared in
//! numbering-invariant canonical (BFS discovery) form.
//!
//! The arc paging law is exercised against the exported resource for every
//! reachable state with capacity probes 0 (pure count probe), 1, an exact
//! fit, and overshoot, plus offsets at and beyond the end: `out_total` must
//! be stable across every call and pages must concatenate losslessly — the
//! producer face of the VT-PAGE contract.
//!
//! Formal-model correspondence (invariant registry owned by the coordinator):
//! - `// INVARIANT-HOOK: LLING-BRIDGE-3` — +inf (the tropical zero) is
//!   representable at every weight position and survives the round trip;
//!   NaN is NEVER emitted by the producer on any observed surface.
//! - `// INVARIANT-HOOK: LLING-BRIDGE-2` — weights cross the bridge
//!   bit-exactly (no re-encoding drift) in both directions.
#![cfg(all(feature = "ffi", feature = "test-utils"))]

mod support;

use lling_llang::ffi::{
    lling_resource_release, lling_wfst_builder_add_arc, lling_wfst_builder_add_state,
    lling_wfst_builder_build, lling_wfst_builder_free, lling_wfst_builder_new,
    lling_wfst_builder_reserve_states, lling_wfst_builder_set_final, lling_wfst_builder_set_start,
    lling_wfst_free, lling_wfst_import, lling_wfst_resource, LlingLlangStatus, LlingWfst,
    LlingWfstBuilder,
};
use lling_llang::semiring::TropicalWeight;
use lling_llang::test_utils::arb_tropical_wfst;
use lling_llang::wfst::{VectorWfst, Wfst};
use proptest::prelude::*;
use std::ptr;
use support::interop_wfst::{
    canonical_of_vector, canonical_of_walk, discover_scalar_wfst, walk_reachable, CanonicalWfst,
};
use vinary_tree_interop::{VtResource, VtStatus, VtWfstArc};

/// Feed a native graph through the 19-function C builder surface.
fn drive_builder(wfst: &VectorWfst<char, TropicalWeight>) -> *mut LlingWfst {
    let mut builder: *mut LlingWfstBuilder = ptr::null_mut();
    assert_eq!(lling_wfst_builder_new(&mut builder), LlingLlangStatus::Ok);
    assert_eq!(
        lling_wfst_builder_reserve_states(builder, wfst.num_states()),
        LlingLlangStatus::Ok
    );
    let mut ids = Vec::with_capacity(wfst.num_states());
    for _ in 0..wfst.num_states() {
        let mut id = u32::MAX;
        assert_eq!(
            lling_wfst_builder_add_state(builder, &mut id),
            LlingLlangStatus::Ok
        );
        ids.push(id);
    }
    assert_eq!(
        lling_wfst_builder_set_start(builder, ids[wfst.start() as usize]),
        LlingLlangStatus::Ok
    );
    for state in 0..wfst.num_states() as u32 {
        if wfst.is_final(state) {
            assert_eq!(
                lling_wfst_builder_set_final(
                    builder,
                    ids[state as usize],
                    wfst.final_weight(state).value()
                ),
                LlingLlangStatus::Ok
            );
        }
        for transition in wfst.transitions(state) {
            let (input_label, has_input) = transition
                .input
                .map_or((0, 0), |label| (u64::from(label), 1));
            let (output_label, has_output) = transition
                .output
                .map_or((0, 0), |label| (u64::from(label), 1));
            assert_eq!(
                lling_wfst_builder_add_arc(
                    builder,
                    ids[state as usize],
                    input_label,
                    has_input,
                    output_label,
                    has_output,
                    ids[transition.to as usize],
                    transition.weight.value(),
                ),
                LlingLlangStatus::Ok
            );
        }
    }
    let mut handle: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_builder_build(builder, &mut handle),
        LlingLlangStatus::Ok
    );
    unsafe { lling_wfst_builder_free(builder) };
    handle
}

/// Take an owned resource retain from a WFST handle.
fn resource_of(handle: *mut LlingWfst) -> VtResource {
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(handle, &mut resource) },
        LlingLlangStatus::Ok
    );
    resource
}

/// Canonicalize the graph published by a live resource.
fn canonical_of_resource(resource: VtResource, page_capacity: usize) -> CanonicalWfst {
    let (start, states) = unsafe { walk_reachable(resource, page_capacity) };
    // INVARIANT-HOOK: LLING-BRIDGE-3 — NaN is never emitted on any surface.
    for (id, state) in &states {
        assert!(
            !state.final_weight.is_nan(),
            "state {id} emitted a NaN final weight"
        );
        for arc in &state.arcs {
            assert!(!arc.weight.is_nan(), "state {id} emitted a NaN arc weight");
        }
    }
    canonical_of_walk(start, &states)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// INVARIANT-HOOK: LLING-BRIDGE-2 — build -> resource -> import -> resource
    /// preserves the reachable subgraph exactly (structure and bit-exact
    /// weights), in canonical numbering-invariant form.
    #[test]
    fn round_trip_preserves_reachable_structure(wfst in arb_tropical_wfst(6, 3)) {
        let native_canonical = canonical_of_vector(&wfst);

        let built = drive_builder(&wfst);
        let built_resource = resource_of(built);
        let built_canonical = canonical_of_resource(built_resource, 2);
        prop_assert_eq!(
            &built_canonical,
            &native_canonical,
            "exported resource must publish the reachable native graph"
        );

        let mut imported: *mut LlingWfst = ptr::null_mut();
        prop_assert_eq!(
            lling_wfst_import(built_resource, &mut imported),
            LlingLlangStatus::Ok
        );
        let imported_resource = resource_of(imported);
        let imported_canonical = canonical_of_resource(imported_resource, 3);
        prop_assert_eq!(
            &imported_canonical,
            &native_canonical,
            "import must reconstruct the same reachable graph"
        );

        lling_resource_release(imported_resource);
        lling_resource_release(built_resource);
        unsafe {
            lling_wfst_free(imported);
            lling_wfst_free(built);
        }
    }

    /// The producer face of VT-PAGE over the exported resource: for every
    /// reachable state, `out_total` is identical for capacity probes
    /// 0 / 1 / exact / overshoot and offsets at or beyond the end, and
    /// capacity-1 pages concatenate to exactly the one-shot fetch.
    #[test]
    fn arc_paging_law_holds_for_every_reachable_state(wfst in arb_tropical_wfst(5, 6)) {
        let built = drive_builder(&wfst);
        let resource = resource_of(built);
        let (_, states) = unsafe { walk_reachable(resource, 256) };

        unsafe {
            let table = &*discover_scalar_wfst(resource);
            let state_arcs = table.state_arcs.expect("state_arcs published");
            for (&state, walked) in &states {
                let expected = &walked.arcs;
                let total_arcs = expected.len();

                // Capacity-0 probe: no buffer needed, count only.
                let mut written = usize::MAX;
                let mut total = usize::MAX;
                prop_assert_eq!(
                    state_arcs(
                        resource.context,
                        state,
                        0,
                        ptr::null_mut(),
                        0,
                        &mut written,
                        &mut total,
                    ),
                    VtStatus::Ok.to_raw()
                );
                prop_assert_eq!((written, total), (0, total_arcs));

                // Capacity-1 pages concatenate losslessly.
                let mut paged = Vec::with_capacity(total_arcs);
                let mut offset = 0usize;
                loop {
                    let mut arc = VtWfstArc::default();
                    let mut page_written = usize::MAX;
                    let mut page_total = usize::MAX;
                    prop_assert_eq!(
                        state_arcs(
                            resource.context,
                            state,
                            offset,
                            &mut arc,
                            1,
                            &mut page_written,
                            &mut page_total,
                        ),
                        VtStatus::Ok.to_raw()
                    );
                    prop_assert_eq!(page_total, total_arcs, "out_total must be stable");
                    prop_assert!(page_written <= 1);
                    if page_written == 0 {
                        break;
                    }
                    paged.push(arc);
                    offset += page_written;
                }
                prop_assert_eq!(paged.len(), total_arcs);
                prop_assert_eq!(offset, total_arcs, "pages must cover exactly the total");
                for (page_arc, walked_arc) in paged.iter().zip(expected.iter()) {
                    prop_assert_eq!(page_arc, walked_arc, "paged arc must match one-shot arc");
                }

                // Exact-fit and overshoot fetches agree with the total.
                for capacity in [total_arcs.max(1), total_arcs + 3] {
                    let mut page = vec![VtWfstArc::default(); capacity];
                    let mut fit_written = usize::MAX;
                    let mut fit_total = usize::MAX;
                    prop_assert_eq!(
                        state_arcs(
                            resource.context,
                            state,
                            0,
                            page.as_mut_ptr(),
                            capacity,
                            &mut fit_written,
                            &mut fit_total,
                        ),
                        VtStatus::Ok.to_raw()
                    );
                    prop_assert_eq!((fit_written, fit_total), (total_arcs, total_arcs));
                }

                // Offsets at and beyond the end: empty page, stable total,
                // still Ok (pinned producer-side leniency).
                for offset in [total_arcs, total_arcs + 5] {
                    let mut tail_written = usize::MAX;
                    let mut tail_total = usize::MAX;
                    prop_assert_eq!(
                        state_arcs(
                            resource.context,
                            state,
                            offset,
                            ptr::null_mut(),
                            0,
                            &mut tail_written,
                            &mut tail_total,
                        ),
                        VtStatus::Ok.to_raw()
                    );
                    prop_assert_eq!((tail_written, tail_total), (0, total_arcs));
                }
            }
        }

        lling_resource_release(resource);
        unsafe { lling_wfst_free(built) };
    }
}

/// Epsilon arcs (absent input, absent output, and both) survive the round
/// trip with their presence flags intact.
#[test]
fn epsilon_arcs_survive_the_round_trip() {
    let mut wfst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    use lling_llang::wfst::MutableWfst;
    let s0 = wfst.add_state();
    let s1 = wfst.add_state();
    let s2 = wfst.add_state();
    wfst.set_start(s0);
    wfst.set_final(s2, TropicalWeight::new(0.5));
    wfst.add_arc(s0, None, None, s1, TropicalWeight::new(0.25));
    wfst.add_arc(s1, None, Some('z'), s2, TropicalWeight::new(1.0));
    wfst.add_arc(s0, Some('q'), None, s2, TropicalWeight::new(2.0));

    let native_canonical = canonical_of_vector(&wfst);
    let built = drive_builder(&wfst);
    let built_resource = resource_of(built);
    let mut imported: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_import(built_resource, &mut imported),
        LlingLlangStatus::Ok
    );
    let imported_resource = resource_of(imported);
    let imported_canonical = canonical_of_resource(imported_resource, 1);
    assert_eq!(imported_canonical, native_canonical);

    // The epsilon flags specifically: state 0 has one ε:ε arc and one q:ε arc.
    assert_eq!(imported_canonical[0].arcs.len(), 2);
    assert_eq!(
        (
            imported_canonical[0].arcs[0].input,
            imported_canonical[0].arcs[0].output
        ),
        (None, None)
    );
    assert_eq!(
        (
            imported_canonical[0].arcs[1].input,
            imported_canonical[0].arcs[1].output
        ),
        (Some(u32::from('q')), None)
    );

    lling_resource_release(imported_resource);
    lling_resource_release(built_resource);
    unsafe {
        lling_wfst_free(imported);
        lling_wfst_free(built);
    }
}

// INVARIANT-HOOK: LLING-BRIDGE-3 — +inf, the tropical semiring zero, is
// representable at both arc- and final-weight positions and survives the
// full round trip; the canonical compare would fail on any NaN substitute.
//
// The fixture is driven through the C builder directly because the wire
// treats `is_final` and the final weight as independent channels: the C
// surface pins `is_final = 1` even at weight +inf, whereas the NATIVE
// `MutableWfst::set_final` normalizes zero-weight finality away — a
// documented layering difference this test also pins.
#[test]
fn positive_infinity_survives_the_round_trip() {
    let mut builder: *mut LlingWfstBuilder = ptr::null_mut();
    assert_eq!(lling_wfst_builder_new(&mut builder), LlingLlangStatus::Ok);
    let mut s0 = u32::MAX;
    let mut s1 = u32::MAX;
    assert_eq!(
        lling_wfst_builder_add_state(builder, &mut s0),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_add_state(builder, &mut s1),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_set_start(builder, s0),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_set_final(builder, s1, f64::INFINITY),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_add_arc(
            builder,
            s0,
            u64::from('a'),
            1,
            u64::from('b'),
            1,
            s1,
            f64::INFINITY
        ),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_add_arc(builder, s0, u64::from('c'), 1, u64::from('d'), 1, s1, 1.5),
        LlingLlangStatus::Ok
    );
    let mut built: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_builder_build(builder, &mut built),
        LlingLlangStatus::Ok
    );
    unsafe { lling_wfst_builder_free(builder) };

    let built_resource = resource_of(built);
    let built_canonical = canonical_of_resource(built_resource, 2);
    assert!(built_canonical[1].is_final, "wire keeps is_final at +inf");
    assert!(built_canonical[1].final_weight.is_infinite());

    let mut imported: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_import(built_resource, &mut imported),
        LlingLlangStatus::Ok
    );
    let imported_resource = resource_of(imported);
    let imported_canonical = canonical_of_resource(imported_resource, 2);
    assert_eq!(imported_canonical, built_canonical);
    assert!(imported_canonical[0].arcs[0].weight.is_infinite());
    assert!(imported_canonical[0].arcs[0].weight.is_sign_positive());
    assert_eq!(imported_canonical[0].arcs[1].weight, 1.5);
    assert!(imported_canonical[1].is_final);
    assert!(imported_canonical[1].final_weight.is_infinite());

    lling_resource_release(imported_resource);
    lling_resource_release(built_resource);
    unsafe {
        lling_wfst_free(imported);
        lling_wfst_free(built);
    }
}
