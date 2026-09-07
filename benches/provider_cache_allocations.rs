//! Single-threaded allocation accounting, deliberately separate from timing.
//!
//! Reports allocator-requested bytes, not malloc usable size or process RSS.
//! The payload is a vector of u64 units; this isolates the generic cache, not
//! the complete foreign exporter. No allocator replacement enters library code.
use std::alloc::{GlobalAlloc, Layout, System};
use std::convert::Infallible;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use lling_llang::wfst::{SharedCachePolicy, SharedStateCache};

#[path = "support/cache_trace.rs"]
mod cache_trace;
use cache_trace::access_order;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static DEALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
// Exact requested sizes for attribution, not an allocator replacement policy.
// Large requests have a separately reported count and byte total. Everything
// is static and allocation-free inside GlobalAlloc, including histogram reset.
const MAX_RECORDED_SIZE: usize = 8192;
static HISTOGRAM_ENABLED: AtomicBool = AtomicBool::new(false);
static HISTOGRAM: [AtomicUsize; MAX_RECORDED_SIZE + 1] =
    [const { AtomicUsize::new(0) }; MAX_RECORDED_SIZE + 1];
static LARGE_COUNT: AtomicUsize = AtomicUsize::new(0);
static LARGE_BYTES: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

fn allocated(size: usize) {
    if HISTOGRAM_ENABLED.load(Ordering::Relaxed) {
        match HISTOGRAM.get(size) {
            Some(bucket) => {
                bucket.fetch_add(1, Ordering::Relaxed);
            }
            None => {
                LARGE_COUNT.fetch_add(1, Ordering::Relaxed);
                LARGE_BYTES.fetch_add(size, Ordering::Relaxed);
            }
        }
    }
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    ALLOCATED.fetch_add(size, Ordering::Relaxed);
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

fn deallocated(size: usize) {
    DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    FREED.fetch_add(size, Ordering::Relaxed);
    LIVE.fetch_sub(size, Ordering::Relaxed);
}

// SAFETY: all allocation and layout contracts are delegated unchanged to
// System. Accounting uses only allocation-free atomics, including in drop.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            allocated(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            allocated(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        deallocated(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let resized = unsafe { System.realloc(ptr, layout, new_size) };
        if !resized.is_null() {
            deallocated(layout.size());
            allocated(new_size);
        }
        resized
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Clone, Copy)]
struct Sample {
    allocations: usize,
    bytes: usize,
    deallocations: usize,
    freed: usize,
    live: usize,
}

impl Sample {
    fn take() -> Self {
        Self {
            allocations: ALLOCATIONS.load(Ordering::Relaxed),
            bytes: ALLOCATED.load(Ordering::Relaxed),
            deallocations: DEALLOCATIONS.load(Ordering::Relaxed),
            freed: FREED.load(Ordering::Relaxed),
            live: LIVE.load(Ordering::Relaxed),
        }
    }

    fn start() -> Self {
        let sample = Self::take();
        PEAK.store(sample.live, Ordering::Relaxed);
        sample
    }

    fn report(self, policy: &str, capacity: usize, units: usize, phase: &str, requests: usize) {
        let after = Self::take();
        let allocations = after.allocations - self.allocations;
        let bytes = after.bytes - self.bytes;
        let deallocations = after.deallocations - self.deallocations;
        let freed = after.freed - self.freed;
        let retained = after.live as i128 - self.live as i128;
        let peak_extra = PEAK.load(Ordering::Relaxed).saturating_sub(self.live);
        println!("{policy}\t{capacity}\t{units}\t{phase}\t{requests}\t{allocations}\t{deallocations}\t{bytes}\t{freed}\t{retained}\t{peak_extra}");
    }
}

fn retained_request(
    cache: &SharedStateCache<Vec<u64>>,
    index: usize,
    units: usize,
) -> Arc<Vec<u64>> {
    let id = index as u64 * 1_000_000_007;
    let result = cache
        .get_or_try_insert_with(id, || Ok::<_, Infallible>(vec![id; units]), |_| true)
        .expect("deterministic provider");
    assert_eq!(result.len(), units);
    if let Some(first) = result.first() {
        assert_eq!(*first, id);
    }
    result
}

fn request(cache: &SharedStateCache<Vec<u64>>, index: usize, units: usize) {
    drop(retained_request(cache, index, units));
}

fn main() {
    let mut histogram = false;
    let mut retirement = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--histogram" => histogram = true,
            "--retirement" => retirement = true,
            // Cargo supplies this marker to harness-free benchmark binaries.
            "--bench" => {}
            _ => panic!(
                "unexpected argument {argument}; use --histogram, --retirement or no arguments"
            ),
        }
    }
    assert!(!(histogram && retirement), "select one diagnostic mode");
    if retirement {
        retirement_workloads();
        return;
    }
    if histogram {
        histogram_workloads();
        return;
    }
    println!("policy\tcapacity\tpayload_u64s\tphase\trequests\tallocations\tdeallocations\tallocated_bytes\tfreed_bytes\tlive_delta_bytes\tpeak_extra_bytes");
    // Initialize ArcSwap's per-thread machinery before counted scenarios.
    let warmup = SharedStateCache::new(SharedCachePolicy::CacheAll);
    request(&warmup, 0, 0);
    drop(warmup);
    for units in [0, 640] {
        for capacity in [1, 2, 64, 1024] {
            for (name, policy) in [
                ("all", SharedCachePolicy::CacheAll),
                ("none", SharedCachePolicy::NoCache),
                (
                    "lru",
                    SharedCachePolicy::Lru {
                        capacity: NonZeroUsize::new(capacity).expect("positive capacity"),
                    },
                ),
            ] {
                let empty = Sample::start();
                let cache = SharedStateCache::new(policy);
                empty.report(name, capacity, units, "empty", 0);
                let seed = Sample::start();
                for id in 0..capacity {
                    request(&cache, id, units);
                }
                seed.report(name, capacity, units, "seed", capacity);
                let hot = Sample::start();
                for step in 0..10_000 {
                    request(&cache, step % capacity, units);
                }
                hot.report(name, capacity, units, "hot", 10_000);
                let expected = if policy == SharedCachePolicy::NoCache {
                    0
                } else {
                    capacity
                };
                assert_eq!(cache.statistics().resident_states, expected);
                cache.clear();
                for id in 0..=capacity {
                    request(&cache, id, units);
                }
                let misses = Sample::start();
                for step in 0..10_000 {
                    request(&cache, step % (capacity + 1), units);
                }
                misses.report(name, capacity, units, "capacity_plus_one", 10_000);
                let clear = Sample::start();
                cache.clear();
                clear.report(name, capacity, units, "clear", 1);
                assert_eq!(cache.statistics().resident_states, 0);
            }
        }
    }
}

/// RSS includes the allocator, executable, runtime and this procfs read. It is
/// neither live requested bytes nor a promise that freed pages return to the OS.
#[cfg(target_os = "linux")]
fn report_resident_memory(
    policy: &str,
    capacity: usize,
    units: usize,
    holders: usize,
    phase: &str,
) {
    let status = std::fs::read_to_string("/proc/self/status").expect("Linux process memory status");
    let kib = |name| {
        status
            .lines()
            .find_map(|line| {
                let suffix = line.strip_prefix(name)?;
                let mut fields = suffix.split_whitespace();
                let value = fields
                    .next()
                    .expect("memory value")
                    .parse::<usize>()
                    .expect("KiB value");
                assert_eq!(fields.next(), Some("kB"));
                Some(value)
            })
            .expect("process memory field")
    };
    eprintln!(
        "resident_memory\t{policy}\t{capacity}\t{units}\t{holders}\t{phase}\t{}\t{}",
        kib("VmRSS:"),
        kib("VmHWM:")
    );
}

#[cfg(not(target_os = "linux"))]
fn report_resident_memory(
    policy: &str,
    capacity: usize,
    units: usize,
    holders: usize,
    phase: &str,
) {
    eprintln!("resident_memory\t{policy}\t{capacity}\t{units}\t{holders}\t{phase}\tunavailable\tunavailable");
}

/// Separate clear, delayed last-owner release, and empty-cache destruction.
/// Only bounded returned payloads survive clear; no private root is exposed.
fn retirement_workloads() {
    println!("policy\tcapacity\tpayload_u64s\tphase\trequests\tallocations\tdeallocations\tallocated_bytes\tfreed_bytes\tlive_delta_bytes\tpeak_extra_bytes");
    eprintln!("memory_kind\tpolicy\tcapacity\tpayload_u64s\tholders\tphase\tprocess_rss_kib\tprocess_peak_rss_kib");
    let warmup = SharedStateCache::new(SharedCachePolicy::CacheAll);
    request(&warmup, 0, 0);
    drop(warmup);
    for capacity in [1, 2, 64, 1024] {
        for units in [0, 640] {
            for (name, policy) in [
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
                    let seed_phase = format!("seed_hold_{holders}");
                    let seed = Sample::start();
                    let cache = SharedStateCache::new(policy);
                    let mut held = Vec::with_capacity(holders);
                    for index in 0..capacity {
                        let value = retained_request(&cache, index, units);
                        if index < holders {
                            held.push(value);
                        }
                    }
                    seed.report(name, capacity, units, &seed_phase, capacity);
                    report_resident_memory(name, capacity, units, holders, "seed");
                    assert_eq!(cache.statistics().resident_states, capacity);

                    let clear_phase = format!("clear_hold_{holders}");
                    let cleared = Sample::start();
                    cache.clear();
                    cleared.report(name, capacity, units, &clear_phase, 1);
                    report_resident_memory(name, capacity, units, holders, "clear");
                    assert_eq!(cache.statistics().resident_states, 0);
                    for (index, value) in held.iter().enumerate() {
                        assert_eq!(&**value, &vec![index as u64 * 1_000_000_007; units]);
                        assert_eq!(Arc::strong_count(value), 1, "cache relinquished ownership");
                    }

                    let release_phase = format!("last_reader_drop_{holders}");
                    let released = Sample::start();
                    drop(held);
                    released.report(name, capacity, units, &release_phase, holders);
                    report_resident_memory(name, capacity, units, holders, "last_reader_drop");

                    let drop_phase = format!("drop_empty_cache_hold_{holders}");
                    let dropped = Sample::start();
                    drop(cache);
                    dropped.report(name, capacity, units, &drop_phase, 1);
                    report_resident_memory(name, capacity, units, holders, "drop_empty_cache");
                }
            }
        }
    }
}

fn histogram_start() {
    for bucket in &HISTOGRAM {
        bucket.store(0, Ordering::Relaxed);
    }
    LARGE_COUNT.store(0, Ordering::Relaxed);
    LARGE_BYTES.store(0, Ordering::Relaxed);
    HISTOGRAM_ENABLED.store(true, Ordering::Relaxed);
}

fn histogram_report(repetition: usize, capacity: usize, phase: &str) {
    assert!(!HISTOGRAM_ENABLED.load(Ordering::Relaxed));
    for (size, bucket) in HISTOGRAM.iter().enumerate() {
        let count = bucket.load(Ordering::Relaxed);
        if count != 0 {
            println!(
                "{repetition}\t{capacity}\t{phase}\t{size}\t{count}\t{}",
                size * count
            );
        }
    }
    let count = LARGE_COUNT.load(Ordering::Relaxed);
    if count != 0 {
        println!(
            "{repetition}\t{capacity}\t{phase}\t>8192\t{count}\t{}",
            LARGE_BYTES.load(Ordering::Relaxed)
        );
    }
}

/// Diagnostic replay: three independently keyed ownership maps, with the same
/// deterministic access permutation in each repetition. No timing is reported.
fn histogram_workloads() {
    println!("repetition\tcapacity\tphase\trequested_size\tallocations\tallocated_bytes");
    let warmup = SharedStateCache::new(SharedCachePolicy::CacheAll);
    request(&warmup, 0, 0);
    drop(warmup);
    for repetition in 0..3 {
        for capacity in [64, 1024] {
            for miss in [false, true] {
                for trace in ["cyclic", "shuffled", "interior"] {
                    if miss && trace == "interior" {
                        continue;
                    }
                    let cache = SharedStateCache::new(SharedCachePolicy::Lru {
                        capacity: NonZeroUsize::new(capacity).expect("positive capacity"),
                    });
                    let working_set = capacity + usize::from(miss);
                    let order = access_order(capacity, miss, trace);
                    // Seed in numerical order, then establish the access order.
                    // Shuffled touches therefore span nonadjacent physical slots.
                    for id in 0..working_set {
                        request(&cache, id, 0);
                    }
                    for &id in &order {
                        request(&cache, id, 0);
                    }
                    let before = cache.statistics();
                    histogram_start();
                    for step in 0..10_000 {
                        request(&cache, order[step % order.len()], 0);
                    }
                    HISTOGRAM_ENABLED.store(false, Ordering::Relaxed);
                    let after = cache.statistics();
                    assert_eq!(after.misses - before.misses, if miss { 10_000 } else { 0 });
                    assert_eq!(after.hits - before.hits, if miss { 0 } else { 10_000 });
                    assert_eq!(
                        after.evictions - before.evictions,
                        if miss { 10_000 } else { 0 }
                    );
                    assert_eq!(after.resident_states, capacity);
                    let phase = match (miss, trace) {
                        (false, "cyclic") => "hot_cyclic",
                        (false, "shuffled") => "hot_shuffled",
                        (false, "interior") => "hot_interior",
                        (true, "cyclic") => "miss_cyclic",
                        (true, "shuffled") => "miss_shuffled",
                        _ => unreachable!("enumerated trace"),
                    };
                    histogram_report(repetition, capacity, phase);
                }
            }
        }
    }
}
