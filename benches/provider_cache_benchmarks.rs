//! Paired retained-provider cache measurements through the real resource ABI.
//!
//! The deterministic provider isolates cache lookup from dictionary expansion.
//! Run the same source and workload before and after cache changes; report
//! affinity, host load, CPU policy, source commits, and resident-state counts.
use std::collections::HashMap;
use std::ffi::c_void;
use std::hint::black_box;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::{Barrier, RwLock};
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use lling_llang::bindings::{OwnedWfstResource, ScalarWfstProvider, ScalarWfstState};
use lling_llang::wfst::{SharedCachePolicy, SharedStateCache};
use vinary_tree_interop::{
    VtStatus, VtWfstArc, VtWfstVTable, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
};

struct Provider {
    degree: usize,
}

impl ScalarWfstProvider for Provider {
    fn start(&self) -> Result<u64, VtStatus> {
        Ok(0)
    }
    fn num_states(&self) -> Result<Option<usize>, VtStatus> {
        Ok(None)
    }
    fn state(&self, state: u64) -> Result<ScalarWfstState, VtStatus> {
        Ok(ScalarWfstState {
            valid: true,
            is_final: true,
            final_weight: 0.0,
            arcs: (0..self.degree)
                .map(|label| VtWfstArc {
                    input_label: label as u64,
                    output_label: label as u64,
                    target_state: state + 1,
                    weight: 1.0,
                    has_input: 1,
                    has_output: 1,
                    reserved: [0; 6],
                })
                .collect(),
        })
    }
}

fn table(resource: &OwnedWfstResource) -> &VtWfstVTable {
    let raw = resource.as_raw();
    let mut interface: *const c_void = std::ptr::null();
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

fn retained_provider(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("provider_cache_abi");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for degree in [0, 8, 128] {
        let resource = OwnedWfstResource::from_provider(Arc::new(Provider { degree }));
        let raw = resource.as_raw();
        let vtable = table(&resource);
        let info = vtable.state_info.expect("state_info");
        let arcs = vtable.state_arcs.expect("state_arcs");
        let (mut valid, mut finality, mut weight) = (0, 0, 0.0);
        for state in 0..4096 {
            assert_eq!(
                unsafe { info(raw.context, state, &mut valid, &mut finality, &mut weight) },
                VtStatus::Ok.to_raw()
            );
        }
        group.bench_with_input(BenchmarkId::new("warm_info", degree), &degree, |b, _| {
            let mut state = 0;
            b.iter(|| {
                state = (state + 1) % 4096;
                black_box(unsafe {
                    info(
                        raw.context,
                        black_box(state),
                        &mut valid,
                        &mut finality,
                        &mut weight,
                    )
                })
            });
        });
        let mut buffer = vec![VtWfstArc::default(); degree.max(1)];
        let (mut written, mut total) = (0, 0);
        group.bench_with_input(BenchmarkId::new("warm_arcs", degree), &degree, |b, _| {
            let mut state = 0;
            b.iter(|| {
                state = (state + 1) % 4096;
                black_box(unsafe {
                    arcs(
                        raw.context,
                        black_box(state),
                        0,
                        buffer.as_mut_ptr(),
                        buffer.len(),
                        &mut written,
                        &mut total,
                    )
                })
            });
        });
    }
    group.finish();
}

/// An instrumented version of the old warm-cache kernel. This is a benchmark
/// control, not another production cache: all entries are seeded before timing.
struct CountedBaseline {
    entries: RwLock<HashMap<u64, Arc<u64>>>,
    hits: AtomicU64,
}

trait WarmLookup: Send + Sync {
    fn warm_lookup(&self, id: u64) -> Arc<u64>;
}

impl WarmLookup for CountedBaseline {
    fn warm_lookup(&self, id: u64) -> Arc<u64> {
        let value = self
            .entries
            .read()
            .expect("unpoisoned benchmark cache")
            .get(&id)
            .expect("seeded state")
            .clone();
        let _ = self
            .hits
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_add(1))
            });
        value
    }
}

impl WarmLookup for SharedStateCache<u64> {
    fn warm_lookup(&self, id: u64) -> Arc<u64> {
        self.get_or_try_insert_with(id, || Err::<u64, _>("seeded state missing"), |_| true)
            .expect("warm lookup")
    }
}

fn timed_parallel_reads(
    cache: &impl WarmLookup,
    iterations: u64,
    threads: usize,
    sparse: bool,
) -> Duration {
    let start = std::time::Instant::now();
    let barrier = Barrier::new(threads);
    std::thread::scope(|scope| {
        let barrier = &barrier;
        let workers: Vec<_> = (0..threads)
            .map(|thread| {
                scope.spawn(move || {
                    barrier.wait();
                    for index in 0..iterations {
                        let id = (index + thread as u64 * 97) % 4096;
                        let id = if sparse { id * 1_000_000_007 } else { id };
                        black_box(cache.warm_lookup(black_box(id)));
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("benchmark worker");
        }
    });
    start.elapsed()
}

fn counted_cache_kernels(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("counted_cache_kernel");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for sparse in [false, true] {
        let candidate = SharedStateCache::new(SharedCachePolicy::CacheAll);
        let mut entries = HashMap::with_capacity(4096);
        for id in 0..4096 {
            let id = if sparse { id * 1_000_000_007 } else { id };
            entries.insert(id, Arc::new(id));
            candidate
                .get_or_try_insert_with(id, || Ok::<_, std::convert::Infallible>(id), |_| true)
                .expect("seed");
        }
        let baseline = CountedBaseline {
            entries: RwLock::new(entries),
            hits: AtomicU64::new(0),
        };
        for threads in [1, 8] {
            let case = format!("{}/{threads}", if sparse { "sparse" } else { "dense" });
            group.throughput(criterion::Throughput::Elements(threads as u64));
            group.bench_function(BenchmarkId::new("rwlock", &case), |b| {
                b.iter_custom(|iterations| {
                    timed_parallel_reads(&baseline, iterations, threads, sparse)
                });
            });
            group.bench_function(BenchmarkId::new("persistent", &case), |b| {
                b.iter_custom(|iterations| {
                    timed_parallel_reads(&candidate, iterations, threads, sparse)
                });
            });
        }
    }
    group.finish();
}

/// Characterize policy costs without pretending the old unbounded exporter
/// implemented equivalent LRU/NoCache contracts. Cold batches include clear
/// and reclamation; hot sets are seeded outside the timed request loop.
fn policy_workloads(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("provider_cache_policy");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for degree in [0, 128] {
        for (name, policy) in [
            ("all", SharedCachePolicy::CacheAll),
            ("none", SharedCachePolicy::NoCache),
            (
                "lru64",
                SharedCachePolicy::Lru {
                    capacity: NonZeroUsize::new(64).expect("positive capacity"),
                },
            ),
        ] {
            let resource =
                OwnedWfstResource::from_provider_with_cache(Arc::new(Provider { degree }), policy);
            let control = resource.provider_cache().expect("provider cache");
            let raw = resource.as_raw();
            let info = table(&resource).state_info.expect("state_info");
            let (mut valid, mut finality, mut weight) = (0, 0, 0.0);
            group.throughput(criterion::Throughput::Elements(256));
            group.bench_function(
                BenchmarkId::new("cold_batch", format!("{name}/{degree}")),
                |b| {
                    b.iter(|| {
                        control.clear();
                        for state in 0..256 {
                            black_box(unsafe {
                                info(
                                    raw.context,
                                    black_box(state * 1_000_000_007),
                                    &mut valid,
                                    &mut finality,
                                    &mut weight,
                                )
                            });
                        }
                    });
                },
            );
            control.clear();
            for state in 0..64 {
                assert_eq!(
                    unsafe {
                        info(
                            raw.context,
                            state * 1_000_000_007,
                            &mut valid,
                            &mut finality,
                            &mut weight,
                        )
                    },
                    VtStatus::Ok.to_raw()
                );
            }
            group.throughput(criterion::Throughput::Elements(1));
            group.bench_function(
                BenchmarkId::new("hot_set", format!("{name}/{degree}")),
                |b| {
                    let mut state = 0;
                    b.iter(|| {
                        state = (state + 1) % 64;
                        black_box(unsafe {
                            info(
                                raw.context,
                                black_box(state * 1_000_000_007),
                                &mut valid,
                                &mut finality,
                                &mut weight,
                            )
                        })
                    });
                },
            );
            let arcs = table(&resource).state_arcs.expect("state_arcs");
            let mut buffer = vec![VtWfstArc::default(); degree.max(1)];
            let (mut written, mut total) = (0, 0);
            group.throughput(criterion::Throughput::Elements(2));
            group.bench_function(
                BenchmarkId::new("info_arc_pair", format!("{name}/{degree}")),
                |b| {
                    let mut state = 0;
                    b.iter(|| {
                        state = (state + 1) % 64;
                        let id = black_box(state * 1_000_000_007);
                        black_box(unsafe {
                            info(raw.context, id, &mut valid, &mut finality, &mut weight)
                        });
                        black_box(unsafe {
                            arcs(
                                raw.context,
                                id,
                                0,
                                buffer.as_mut_ptr(),
                                buffer.len(),
                                &mut written,
                                &mut total,
                            )
                        });
                    });
                },
            );
            let expected_residents = if policy == SharedCachePolicy::NoCache {
                0
            } else {
                64
            };
            assert_eq!(control.statistics().resident_states, expected_residents);
        }
    }
    group.finish();
}

/// A cyclic working set one larger than capacity produces steady misses without
/// clear/reclamation batches. The resident bound is independent of sparse IDs.
fn bounded_workloads(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("provider_cache_bounded");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    group.throughput(criterion::Throughput::Elements(1));
    for degree in [0, 128] {
        for capacity in [1, 2, 64, 1024] {
            for (name, working_set) in [("hot", capacity), ("steady_miss", capacity + 1)] {
                let resource = OwnedWfstResource::from_provider_with_cache(
                    Arc::new(Provider { degree }),
                    SharedCachePolicy::Lru {
                        capacity: NonZeroUsize::new(capacity).expect("positive capacity"),
                    },
                );
                let control = resource.provider_cache().expect("provider cache");
                let raw = resource.as_raw();
                let info = table(&resource).state_info.expect("state_info");
                let (mut valid, mut finality, mut weight) = (0, 0, 0.0);
                for state in 0..working_set {
                    assert_eq!(
                        unsafe {
                            info(
                                raw.context,
                                state as u64 * 1_000_000_007,
                                &mut valid,
                                &mut finality,
                                &mut weight,
                            )
                        },
                        VtStatus::Ok.to_raw()
                    );
                }
                let before = control.statistics();
                let mut requests = 0u64;
                let mut state = 0;
                group.bench_function(
                    BenchmarkId::new(name, format!("{capacity}/{degree}")),
                    |b| {
                        // Continue across Criterion iterations and sample calls,
                        // keeping every steady-miss access a real miss.
                        b.iter(|| {
                            black_box(unsafe {
                                info(
                                    raw.context,
                                    black_box(state as u64 * 1_000_000_007),
                                    &mut valid,
                                    &mut finality,
                                    &mut weight,
                                )
                            });
                            state = (state + 1) % working_set;
                            requests += 1;
                        });
                    },
                );
                let after = control.statistics();
                assert_eq!(after.resident_states, capacity);
                if name == "steady_miss" {
                    assert_eq!(after.misses - before.misses, requests);
                    assert_eq!(after.evictions - before.evictions, requests);
                } else {
                    assert_eq!(after.hits - before.hits, requests);
                }
            }
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    retained_provider,
    counted_cache_kernels,
    policy_workloads,
    bounded_workloads
);
criterion_main!(benches);
