//! Representation-sensitive replay and separately timed cache reclamation.
//!
//! This generic-cache control complements the exported-ABI benchmarks; it does
//! not measure dictionary expansion or whole-query throughput. Setup, oracle
//! checks, and untimed cleanup stay outside each timed operation. Per-iteration
//! batching bounds live fixtures to one populated cache, not a Criterion batch.

use std::convert::Infallible;
use std::hint::black_box;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use lling_llang::wfst::{SharedCachePolicy, SharedStateCache};

fn request(cache: &SharedStateCache<Vec<u64>>, index: usize, units: usize) -> Arc<Vec<u64>> {
    let id = index as u64 * 1_000_000_007;
    cache
        .get_or_try_insert_with(id, || Ok::<_, Infallible>(vec![id; units]), |_| true)
        .expect("deterministic immutable state")
}

#[path = "support/cache_trace.rs"]
mod cache_trace;
use cache_trace::access_order;

fn replay(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("shared_cache_residency");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    group.throughput(criterion::Throughput::Elements(1));
    for capacity in [1, 2, 64, 1024] {
        for units in [0, 640] {
            for (miss, trace) in [
                (false, "cyclic"),
                (false, "shuffled"),
                (false, "interior"),
                (true, "cyclic"),
                (true, "shuffled"),
            ] {
                let cache = SharedStateCache::new(SharedCachePolicy::Lru {
                    capacity: NonZeroUsize::new(capacity).expect("positive capacity"),
                });
                let order = access_order(capacity, miss, trace);
                for index in 0..capacity + usize::from(miss) {
                    let value = request(&cache, index, units);
                    assert_eq!(&*value, &vec![index as u64 * 1_000_000_007; units]);
                }
                for &index in &order {
                    drop(request(&cache, index, units));
                }
                let before = cache.statistics();
                let (mut position, mut count) = (0, 0u64);
                let phase = if miss { "miss" } else { "hot" };
                group.bench_function(
                    BenchmarkId::new(format!("{phase}_{trace}"), format!("{capacity}/{units}")),
                    |b| {
                        b.iter(|| {
                            black_box(request(&cache, black_box(order[position]), units));
                            position = (position + 1) % order.len();
                            count += 1;
                        });
                    },
                );
                let after = cache.statistics();
                assert_eq!(after.misses - before.misses, if miss { count } else { 0 });
                assert_eq!(after.hits - before.hits, if miss { 0 } else { count });
                assert_eq!(
                    after.evictions - before.evictions,
                    if miss { count } else { 0 }
                );
                assert_eq!(
                    (after.resident_states, after.recency_records),
                    (capacity, capacity)
                );
            }
        }
    }
    group.finish();
}

struct Retirement {
    cache: SharedStateCache<Vec<u64>>,
    held: Vec<Arc<Vec<u64>>>,
}

impl Retirement {
    fn new(policy: SharedCachePolicy, capacity: usize, units: usize, holders: usize) -> Self {
        assert!(holders <= capacity);
        let cache = SharedStateCache::new(policy);
        let mut held = Vec::with_capacity(holders);
        for index in 0..capacity {
            let value = request(&cache, index, units);
            if index < holders {
                held.push(value);
            }
        }
        assert_eq!(cache.statistics().resident_states, capacity);
        Self { cache, held }
    }

    fn verify(&self, units: usize) {
        assert_eq!(self.cache.statistics().resident_states, 0);
        for (index, value) in self.held.iter().enumerate() {
            assert_eq!(&**value, &vec![index as u64 * 1_000_000_007; units]);
            assert_eq!(
                Arc::strong_count(value),
                1,
                "clear released cache ownership"
            );
        }
    }
}

fn reclamation(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("shared_cache_reclamation");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for capacity in [1, 2, 64, 1024] {
        for units in [0, 640] {
            for (policy_name, policy) in [
                ("all", SharedCachePolicy::CacheAll),
                (
                    "lru",
                    SharedCachePolicy::Lru {
                        capacity: NonZeroUsize::new(capacity).expect("positive capacity"),
                    },
                ),
            ] {
                let mut holder_counts = vec![0, 1, capacity.min(32)];
                holder_counts.dedup();
                for holders in holder_counts {
                    // Validate the public ownership contract outside timing.
                    let checked = Retirement::new(policy, capacity, units, holders);
                    checked.cache.clear();
                    checked.verify(units);
                    drop(checked);
                    let case = format!("{policy_name}/{capacity}/{units}/{holders}");
                    group.bench_function(BenchmarkId::new("clear", &case), |b| {
                        b.iter_batched_ref(
                            || Retirement::new(policy, capacity, units, holders),
                            |fixture| fixture.cache.clear(),
                            BatchSize::PerIteration,
                        );
                    });
                    if holders > 0 {
                        group.bench_function(BenchmarkId::new("last_reader_drop", &case), |b| {
                            b.iter_batched(
                                || {
                                    let fixture = Retirement::new(policy, capacity, units, holders);
                                    fixture.cache.clear();
                                    fixture.held
                                },
                                drop,
                                BatchSize::PerIteration,
                            );
                        });
                    }
                }
            }
        }
    }
    group.finish();
}

criterion_group!(benches, replay, reclamation);
criterion_main!(benches);
