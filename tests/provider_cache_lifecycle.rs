//! Cache ownership observed through the exported scalar-WFST ABI.
#![cfg(feature = "bindings-core")]

use std::ffi::c_void;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, OnceLock};

use lling_llang::bindings::{OwnedWfstResource, ScalarWfstProvider, ScalarWfstState};
use lling_llang::wfst::SharedCachePolicy;
use vinary_tree_interop::{
    VtResource, VtStatus, VtWfstArc, VtWfstVTable, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
};

#[derive(Default)]
struct DiscoveringProvider {
    discovered: AtomicBool,
    calls: AtomicUsize,
}

impl ScalarWfstProvider for DiscoveringProvider {
    fn start(&self) -> Result<u64, VtStatus> {
        Ok(0)
    }

    fn num_states(&self) -> Result<Option<usize>, VtStatus> {
        Ok(None)
    }

    fn state(&self, state: u64) -> Result<ScalarWfstState, VtStatus> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match state {
            0 => {
                self.discovered.store(true, Ordering::Release);
                Ok(ScalarWfstState {
                    valid: true,
                    is_final: false,
                    final_weight: f64::INFINITY,
                    arcs: vec![VtWfstArc {
                        input_label: u64::from('a'),
                        output_label: u64::from('a'),
                        target_state: 1,
                        weight: 0.0,
                        has_input: 1,
                        has_output: 1,
                        reserved: [0; 6],
                    }],
                })
            }
            1 if self.discovered.load(Ordering::Acquire) => Ok(ScalarWfstState {
                valid: true,
                is_final: true,
                final_weight: 0.0,
                arcs: vec![],
            }),
            _ => Ok(ScalarWfstState {
                valid: false,
                is_final: false,
                final_weight: f64::INFINITY,
                arcs: vec![],
            }),
        }
    }
}

fn table(resource: &OwnedWfstResource) -> &VtWfstVTable {
    let raw = resource.as_raw();
    let mut interface: *const c_void = std::ptr::null();
    // The owned retain keeps both the immutable vtable and context alive.
    unsafe {
        assert_eq!(
            (*raw.vtable).query_interface.expect("query_interface")(
                raw.context,
                &VT_WFST_INTERFACE_ID,
                VT_WFST_INTERFACE_VERSION,
                &mut interface,
            ),
            VtStatus::Ok.to_raw()
        );
        interface
            .cast::<VtWfstVTable>()
            .as_ref()
            .expect("WFST table")
    }
}

fn info(resource: &OwnedWfstResource, state: u64) -> (u8, u8, f64) {
    info_result(resource, state).expect("successful state_info")
}

fn info_result(resource: &OwnedWfstResource, state: u64) -> Result<(u8, u8, f64), u32> {
    let (mut valid, mut is_final, mut weight) = (0, 0, f64::NAN);
    let status = unsafe {
        table(resource).state_info.expect("state_info")(
            resource.as_raw().context,
            state,
            &mut valid,
            &mut is_final,
            &mut weight,
        )
    };
    match status {
        0 => Ok((valid, is_final, weight)),
        error => Err(error),
    }
}

fn page_result(
    resource: &OwnedWfstResource,
    state: u64,
    offset: usize,
    capacity: usize,
) -> Result<(Vec<VtWfstArc>, usize), u32> {
    let mut arcs = vec![VtWfstArc::default(); capacity];
    let (mut written, mut total) = (0, 0);
    let status = unsafe {
        table(resource).state_arcs.expect("state_arcs")(
            resource.as_raw().context,
            state,
            offset,
            arcs.as_mut_ptr(),
            capacity,
            &mut written,
            &mut total,
        )
    };
    if status != VtStatus::Ok.to_raw() {
        return Err(status);
    }
    assert!(written <= capacity, "provider must respect page capacity");
    assert_eq!(written, capacity.min(total.saturating_sub(offset)));
    arcs.truncate(written);
    Ok((arcs, total))
}

struct CallbackProvider<F>(F);

impl<F> ScalarWfstProvider for CallbackProvider<F>
where
    F: Fn(u64) -> Result<ScalarWfstState, VtStatus> + Send + Sync + 'static,
{
    fn start(&self) -> Result<u64, VtStatus> {
        Ok(0)
    }
    fn num_states(&self) -> Result<Option<usize>, VtStatus> {
        Ok(None)
    }
    fn state(&self, id: u64) -> Result<ScalarWfstState, VtStatus> {
        (self.0)(id)
    }
}

fn complete_state(id: u64, degree: usize) -> ScalarWfstState {
    ScalarWfstState {
        valid: true,
        is_final: true,
        final_weight: 0.0,
        arcs: (0..degree)
            .map(|ordinal| VtWfstArc {
                input_label: ordinal as u64 + 65,
                output_label: ordinal as u64 + 65,
                target_state: id,
                weight: ordinal as f64,
                has_input: 1,
                has_output: 1,
                reserved: [0; 6],
            })
            .collect(),
    }
}

fn lru(capacity: usize) -> SharedCachePolicy {
    SharedCachePolicy::Lru {
        capacity: NonZeroUsize::new(capacity).expect("positive capacity"),
    }
}

fn first_arc(resource: &OwnedWfstResource, state: u64) -> VtWfstArc {
    let VtResource { context, .. } = resource.as_raw();
    let mut arc = VtWfstArc::default();
    let (mut written, mut total) = (0, 0);
    unsafe {
        assert_eq!(
            table(resource).state_arcs.expect("state_arcs")(
                context,
                state,
                0,
                &mut arc,
                1,
                &mut written,
                &mut total,
            ),
            VtStatus::Ok.to_raw()
        );
    }
    assert_eq!((written, total), (1, 1));
    arc
}

#[test]
fn warm_state_is_reused_across_info_arcs_and_resource_clones() {
    let provider = Arc::new(DiscoveringProvider::default());
    let resource = OwnedWfstResource::from_provider(provider.clone());
    assert_eq!(info(&resource, 0), (1, 0, f64::INFINITY));
    let retained = resource.clone();
    drop(resource);
    for _ in 0..8 {
        assert_eq!(info(&retained, 0), (1, 0, f64::INFINITY));
        assert_eq!(first_arc(&retained, 0).target_state, 1);
    }
    assert_eq!(provider.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn an_early_invalid_probe_does_not_poison_a_later_discovered_state() {
    let provider = Arc::new(DiscoveringProvider::default());
    let resource = OwnedWfstResource::from_provider(provider.clone());
    assert_eq!(info(&resource, 1), (0, 0, f64::INFINITY));
    assert_eq!(first_arc(&resource, 0).target_state, 1);
    assert_eq!(info(&resource, 1), (1, 1, 0.0));
    assert_eq!(provider.calls.load(Ordering::Relaxed), 3);
}

#[test]
fn no_cache_recomputes_every_abi_callback_and_controls_share_one_owner() {
    let provider = Arc::new(DiscoveringProvider::default());
    let resource =
        OwnedWfstResource::from_provider_with_cache(provider.clone(), SharedCachePolicy::NoCache);
    let control = resource.provider_cache().expect("provider cache");
    let clone = resource.clone();
    for _ in 0..4 {
        assert_eq!(info(&resource, 0), (1, 0, f64::INFINITY));
        assert_eq!(first_arc(&clone, 0).target_state, 1);
    }
    assert_eq!(provider.calls.load(Ordering::Relaxed), 8);
    assert_eq!(control.statistics().resident_states, 0);
    control.set_policy(SharedCachePolicy::CacheAll);
    info(&clone, 0);
    first_arc(&resource, 0);
    assert_eq!(provider.calls.load(Ordering::Relaxed), 9);
    clone.provider_cache().expect("shared control").clear();
    first_arc(&resource, 0);
    assert_eq!(provider.calls.load(Ordering::Relaxed), 10);
    assert_eq!(control.statistics().clears, 2);
    drop(clone);
    drop(resource);
    assert_eq!(
        Arc::strong_count(&provider),
        1,
        "control does not retain provider"
    );
    assert_eq!(control.statistics().resident_states, 1);
    control.clear();
    assert_eq!(control.statistics().resident_states, 0);
}

#[test]
fn independent_constructions_never_share_a_state_id_namespace() {
    let first = Arc::new(DiscoveringProvider::default());
    let second = Arc::new(DiscoveringProvider::default());
    let a = OwnedWfstResource::from_provider(first.clone());
    let b = OwnedWfstResource::from_provider(second.clone());
    info(&a, 0);
    assert_eq!(info(&b, 1), (0, 0, f64::INFINITY));
    assert_eq!(first.calls.load(Ordering::Relaxed), 1);
    assert_eq!(second.calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        b.provider_cache()
            .expect("control")
            .statistics()
            .resident_states,
        0
    );
}

#[test]
fn provider_fault_statuses_survive_both_abi_callbacks_and_are_not_retained() {
    // VtStatus has no cancellation discriminant: native expansion cancellation
    // maps to ProviderError at the existing duallity boundary.
    for fault in [
        VtStatus::IoError,
        VtStatus::ProviderError,
        VtStatus::LimitExceeded,
        VtStatus::Closed,
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let provider = Arc::new(CallbackProvider(move |id| {
            if observed.fetch_add(1, Ordering::Relaxed) < 2 {
                Err(fault)
            } else {
                Ok(complete_state(id, 3))
            }
        }));
        let resource = OwnedWfstResource::from_provider(provider);
        let control = resource.provider_cache().expect("control");
        assert_eq!(info_result(&resource, 0), Err(fault.to_raw()));
        assert_eq!(page_result(&resource, 0, 0, 2).err(), Some(fault.to_raw()));
        assert_eq!(control.statistics().resident_states, 0);
        assert_eq!(info(&resource, 0), (1, 1, 0.0));
        assert_eq!(page_result(&resource, 0, 0, 2).expect("retry").1, 3);
        assert_eq!(calls.load(Ordering::Relaxed), 3);
        let stats = control.statistics();
        assert_eq!(
            (stats.misses, stats.faults, stats.insertions, stats.hits),
            (3, 2, 1, 1)
        );
    }
}

#[test]
fn domain_invalid_provider_output_is_rejected_before_shared_cache_publication() {
    // Merge regression: the generic-domain provider validation must run inside
    // the shared-cache miss callback, not after an invalid state is published.
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let provider = Arc::new(CallbackProvider(move |id| {
        let mut state = complete_state(id, 1);
        if observed.fetch_add(1, Ordering::Relaxed) == 0 {
            state.arcs[0].input_label = 0x11_0000;
        }
        Ok(state)
    }));
    let resource = OwnedWfstResource::from_provider(provider);
    let cache = resource.provider_cache().expect("provider cache");
    assert_eq!(
        info_result(&resource, 0),
        Err(VtStatus::ProviderError.to_raw())
    );
    assert_eq!(cache.statistics().resident_states, 0);
    assert_eq!(info(&resource, 0), (1, 1, 0.0));
    assert_eq!(info(&resource, 0), (1, 1, 0.0));
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let stats = cache.statistics();
    assert_eq!(
        (stats.misses, stats.faults, stats.insertions, stats.hits),
        (2, 1, 1, 1)
    );
}

#[test]
fn failing_outer_abi_call_is_not_hidden_by_successful_same_id_reentry() {
    let owner = Arc::new(OnceLock::<OwnedWfstResource>::new());
    let weak_owner = Arc::downgrade(&owner);
    let first = AtomicBool::new(true);
    let provider = Arc::new(CallbackProvider(move |id| {
        if first.swap(false, Ordering::Relaxed) {
            let owner = weak_owner.upgrade().expect("caller owns resource");
            assert_eq!(info(owner.get().expect("initialized"), id), (1, 1, 0.0));
            Err(VtStatus::IoError)
        } else {
            Ok(complete_state(id, 0))
        }
    }));
    let resource = OwnedWfstResource::from_provider(provider);
    owner
        .set(resource.clone())
        .unwrap_or_else(|_| panic!("one initialization"));
    assert_eq!(info_result(&resource, 0), Err(VtStatus::IoError.to_raw()));
    assert_eq!(info(&resource, 0), (1, 1, 0.0));
    let stats = resource.provider_cache().expect("control").statistics();
    assert_eq!(
        (stats.misses, stats.faults, stats.insertions, stats.hits),
        (2, 1, 1, 1)
    );
}

#[test]
fn exact_lru_retention_and_recomputation_are_observable_through_the_abi() {
    for (capacity, computations, evictions, hits) in [(1, 5, 4, 1), (2, 4, 2, 2)] {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let provider = Arc::new(CallbackProvider(move |id| {
            observed.fetch_add(1, Ordering::Relaxed);
            Ok(complete_state(id, 1))
        }));
        let resource = OwnedWfstResource::from_provider_with_cache(provider, lru(capacity));
        for id in [0, 1, 0, 2, 1] {
            assert_eq!(info(&resource, id), (1, 1, 0.0));
        }
        assert_eq!(page_result(&resource, 1, 0, 1).expect("warm page").1, 1);
        assert_eq!(calls.load(Ordering::Relaxed), computations);
        let stats = resource.provider_cache().expect("control").statistics();
        assert_eq!(stats.resident_states, capacity);
        assert_eq!(stats.recency_records, capacity);
        assert_eq!((stats.evictions, stats.hits), (evictions, hits));
    }
}

#[test]
fn in_flight_abi_result_cannot_populate_a_replaced_policy_generation() {
    for initial in [
        SharedCachePolicy::CacheAll,
        SharedCachePolicy::NoCache,
        lru(2),
    ] {
        for replacement in [
            SharedCachePolicy::CacheAll,
            SharedCachePolicy::NoCache,
            lru(1),
        ] {
            let entered = Arc::new(Barrier::new(2));
            let resume = Arc::new(Barrier::new(2));
            let first = AtomicBool::new(true);
            let (provider_entered, provider_resume) = (entered.clone(), resume.clone());
            let provider = Arc::new(CallbackProvider(move |id| {
                if first.swap(false, Ordering::Relaxed) {
                    provider_entered.wait();
                    provider_resume.wait();
                }
                Ok(complete_state(id, 0))
            }));
            let resource = OwnedWfstResource::from_provider_with_cache(provider, initial);
            let control = resource.provider_cache().expect("control");
            let retained = resource.clone();
            let worker = std::thread::spawn(move || info(&retained, 0));
            entered.wait();
            control.set_policy(replacement);
            resume.wait();
            assert_eq!(worker.join().expect("old callback completes"), (1, 1, 0.0));
            assert_eq!(control.policy(), replacement);
            assert_eq!(control.statistics().resident_states, 0);
            assert_eq!(info(&resource, 0), (1, 1, 0.0));
            assert_eq!(
                control.statistics().resident_states,
                usize::from(replacement != SharedCachePolicy::NoCache)
            );
        }
    }
}

#[test]
fn multiple_arc_pages_stay_stable_across_clear_and_policy_replacement() {
    for policy in [SharedCachePolicy::NoCache, lru(1), lru(2)] {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let provider = Arc::new(CallbackProvider(move |id| {
            observed.fetch_add(1, Ordering::Relaxed);
            Ok(complete_state(id, 9))
        }));
        let resource = OwnedWfstResource::from_provider_with_cache(provider, policy);
        let control = resource.provider_cache().expect("control");
        let mut all_arcs = Vec::with_capacity(9);
        for offset in [0, 3, 6] {
            match offset {
                3 => control.clear(),
                6 => control.set_policy(policy),
                _ => {}
            }
            let (arcs, total) = page_result(&resource, 0, offset, 3).expect("page");
            assert_eq!(total, 9, "reported total cannot change across pages");
            all_arcs.extend(arcs);
        }
        assert_eq!(all_arcs.len(), 9);
        for (ordinal, arc) in all_arcs.iter().enumerate() {
            assert_eq!(arc.input_label, ordinal as u64 + 65);
            assert_eq!(arc.output_label, ordinal as u64 + 65);
            assert_eq!(arc.weight, ordinal as f64);
            assert_eq!(arc.target_state, 0);
        }
        assert_eq!(calls.load(Ordering::Relaxed), 3);
        let (end, total) = page_result(&resource, 0, 9, 3).expect("exhausted page");
        assert!(end.is_empty());
        assert_eq!(total, 9);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            if policy == SharedCachePolicy::NoCache {
                4
            } else {
                3
            }
        );
    }
}

#[test]
fn concurrent_retained_abi_readers_preserve_pages_and_release_the_provider() {
    const READERS: usize = 8;
    const REQUESTS: usize = 128;
    const STATE_COUNT: usize = 65;
    for policy in [
        SharedCachePolicy::CacheAll,
        SharedCachePolicy::NoCache,
        lru(1),
        lru(2),
        lru(17),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let provider = Arc::new(CallbackProvider(move |id| {
            observed.fetch_add(1, Ordering::Relaxed);
            Ok(complete_state(id, 9))
        }));
        let weak_provider = Arc::downgrade(&provider);
        let resource = OwnedWfstResource::from_provider_with_cache(provider.clone(), policy);
        let control = resource.provider_cache().expect("shared cache control");
        let retained: Vec<_> = (0..READERS).map(|_| resource.clone()).collect();
        drop(resource);
        drop(provider);

        let start = Barrier::new(READERS + 1);
        std::thread::scope(|scope| {
            let workers: Vec<_> = retained
                .into_iter()
                .enumerate()
                .map(|(reader, resource)| {
                    let start = &start;
                    scope.spawn(move || {
                        start.wait();
                        for request in 0..REQUESTS {
                            let ordinal = (request * 17 + reader * 7) % STATE_COUNT;
                            let id = if ordinal + 1 == STATE_COUNT {
                                u64::MAX
                            } else {
                                ordinal as u64 * 1_000_000_007
                            };
                            assert_eq!(info(&resource, id), (1, 1, 0.0));
                            for offset in [0, 3, 6] {
                                let (arcs, total) = page_result(&resource, id, offset, 3)
                                    .expect("concurrent page succeeds");
                                assert_eq!(total, 9);
                                assert_eq!(arcs.len(), 3);
                                for (index, arc) in arcs.iter().enumerate() {
                                    let ordinal = offset + index;
                                    assert_eq!(arc.input_label, ordinal as u64 + 65);
                                    assert_eq!(arc.output_label, ordinal as u64 + 65);
                                    assert_eq!(arc.target_state, id);
                                    assert_eq!(arc.weight.to_bits(), (ordinal as f64).to_bits());
                                    assert_eq!((arc.has_input, arc.has_output), (1, 1));
                                    assert_eq!(arc.reserved, [0; 6]);
                                }
                            }
                        }
                    })
                })
                .collect();
            start.wait();
            // Readers hold independent ABI retains while the control publishes
            // replacement generations. This schedule does not assume which
            // request overlaps which clear or wins a residency publication.
            for _ in 0..REQUESTS {
                control.clear();
                control.set_policy(policy);
                let stats = control.statistics();
                match policy {
                    SharedCachePolicy::CacheAll => {
                        assert!(stats.resident_states <= STATE_COUNT);
                        assert_eq!(stats.recency_records, 0);
                    }
                    SharedCachePolicy::NoCache => {
                        assert_eq!((stats.resident_states, stats.recency_records), (0, 0));
                    }
                    SharedCachePolicy::Lru { capacity } => {
                        assert!(stats.resident_states <= capacity.get());
                        assert_eq!(stats.recency_records, stats.resident_states);
                    }
                }
            }
            for worker in workers {
                worker.join().expect("concurrent retained reader");
            }
        });

        let stats = control.statistics();
        assert_eq!(stats.hits + stats.misses, (READERS * REQUESTS * 4) as u64);
        assert_eq!(stats.misses, calls.load(Ordering::Relaxed) as u64);
        assert_eq!((stats.faults, stats.uncacheable_results), (0, 0));
        assert_eq!(stats.clears, (REQUESTS * 2) as u64);
        if policy == SharedCachePolicy::NoCache {
            assert_eq!(stats.hits, 0);
        }
        assert_eq!(
            weak_provider.strong_count(),
            0,
            "all ABI retains were released"
        );
        control.clear();
        assert_eq!(control.statistics().resident_states, 0);
    }
}
