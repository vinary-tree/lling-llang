//! Non-blocking residency for immutable, independently recomputable states.
//!
//! One atomically published persistent snapshot contains both lookup and exact
//! least-recently-used order. Provider code is never run inside an update loop.
//! See `docs/architecture/shared-state-cache.md` for linearization and memory bounds.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use imbl::GenericHashMap;

mod lru;
use lru::{LruStorage, Slot};

#[cfg(test)]
std::thread_local! {
    // One-shot deterministic scheduling at the real publication boundary.
    // This hook and its storage do not exist in production builds.
    static BEFORE_PUBLICATION: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn before_publication() {
    let hook = BEFORE_PUBLICATION.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

/// Residency policy for a shared immutable-state cache.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SharedCachePolicy {
    /// Retain every valid successful result until cleared.
    #[default]
    CacheAll,
    /// Recompute every request; retain no entries or per-state metadata.
    NoCache,
    /// Retain at most this many states, using exact global access order.
    /// Resolve application-specific zero-capacity heuristics before construction.
    Lru {
        /// Strict positive bound on resident state payloads.
        capacity: NonZeroUsize,
    },
}

/// Cumulative request counters and a coherent current-residency observation.
///
/// Counters saturate rather than wrap. Concurrent requests can advance between
/// counter reads; this is not an atomic snapshot of all in-flight operations.
/// `resident_states` and `recency_records` come from the same published root.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SharedCacheStatistics {
    /// Requests finding an entry at their first lookup.
    pub hits: u64,
    /// Requests invoking their supplied computation exactly once.
    pub misses: u64,
    /// Completed computations returning an error; errors are not retained.
    pub faults: u64,
    /// Successful results rejected by the caller's admission predicate.
    pub uncacheable_results: u64,
    /// New entries admitted, including a hit reinserted after concurrent eviction.
    pub insertions: u64,
    /// Entries displaced to respect an LRU capacity (not clear operations).
    pub evictions: u64,
    /// Cold computations that found a competing publication of the same state.
    pub raced_publications: u64,
    /// Explicit clears and policy changes, each replacing the generation.
    pub clears: u64,
    /// Entries in the current generation; excludes active returned values.
    pub resident_states: usize,
    /// Exact LRU records; zero for other policies, never a stale access log.
    pub recency_records: usize,
}

#[derive(Default)]
struct Counters {
    hits: AtomicU64,
    misses: AtomicU64,
    faults: AtomicU64,
    uncacheable_results: AtomicU64,
    insertions: AtomicU64,
    evictions: AtomicU64,
    raced_publications: AtomicU64,
    clears: AtomicU64,
}

fn increment(counter: &AtomicU64) {
    let mut previous = counter.load(Ordering::Relaxed);
    loop {
        let next = previous.saturating_add(1);
        match counter.compare_exchange_weak(previous, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => previous = actual,
        }
    }
}

type StateIndex<T> = GenericHashMap<u64, T, ahash::RandomState, imbl::shared_ptr::DefaultSharedPtr>;

struct Snapshot<T> {
    // Hold only this small marker across external computation. Its retained
    // identity prevents ABA without pinning the whole pre-clear cache.
    generation: Arc<()>,
    storage: Storage<T>,
}

enum Storage<T> {
    All(StateIndex<Arc<T>>),
    None,
    Lru(LruStorage<T>),
}

struct Cached<'a, T> {
    value: &'a Arc<T>,
    // Resolved only within the immutable root that owns this lookup.
    // None denotes a no-op access (CacheAll or the exact LRU tail).
    touch: Option<Slot>,
}

impl<T> Clone for Snapshot<T> {
    fn clone(&self) -> Self {
        Self {
            generation: Arc::clone(&self.generation),
            storage: match &self.storage {
                Storage::All(entries) => Storage::All(entries.clone()),
                Storage::None => Storage::None,
                Storage::Lru(storage) => Storage::Lru(storage.clone()),
            },
        }
    }
}

impl<T> Snapshot<T> {
    fn policy(&self) -> SharedCachePolicy {
        match &self.storage {
            Storage::All(_) => SharedCachePolicy::CacheAll,
            Storage::None => SharedCachePolicy::NoCache,
            Storage::Lru(storage) => SharedCachePolicy::Lru {
                capacity: storage.capacity,
            },
        }
    }

    #[inline(always)]
    fn new(policy: SharedCachePolicy) -> Self {
        Self {
            generation: Arc::new(()),
            storage: match policy {
                SharedCachePolicy::CacheAll => {
                    Storage::All(StateIndex::with_hasher(ahash::RandomState::new()))
                }
                SharedCachePolicy::NoCache => Storage::None,
                SharedCachePolicy::Lru { capacity } => Storage::Lru(LruStorage::new(capacity)),
            },
        }
    }

    fn lookup(&self, id: u64) -> Option<Cached<'_, T>> {
        match &self.storage {
            Storage::All(entries) => entries.get(&id).map(|value| Cached { value, touch: None }),
            Storage::None => None,
            Storage::Lru(storage) => storage.lookup(id),
        }
    }

    fn resident_count(&self) -> usize {
        match &self.storage {
            Storage::All(entries) => entries.len(),
            Storage::None => 0,
            Storage::Lru(storage) => storage.entries.len(),
        }
    }

    fn record(&mut self, id: u64, value: &Arc<T>, touch: Option<Slot>) -> bool {
        match &mut self.storage {
            Storage::All(entries) => {
                entries.insert(id, Arc::clone(value));
                false
            }
            Storage::None => unreachable!("NoCache never publishes a resident"),
            Storage::Lru(storage) => {
                if let Some(slot) = touch {
                    storage.touch(slot);
                    false
                } else {
                    storage.insert(id, Arc::clone(value))
                }
            }
        }
    }
}

/// Concurrent cache for immutable results indexed by stable state identifiers.
///
/// Wrap this in an `Arc` to share ownership. `clear` and policy replacement
/// affect every owner. Independently constructed caches never share entries.
/// Values for a valid ID must remain semantically identical for this cache's
/// lifetime; the cache is not a substitute for a source snapshot or registry.
///
/// CacheAll hits and already-most-recent LRU hits do not update the residency
/// or recency root; they do update the atomic hit counter. Other exact LRU updates use persistent
/// ID and linked-slot indexes and a compare-and-swap retry loop: synchronization is lock-free,
/// not wait-free. Allocation, user computations and destructors have their own
/// progress characteristics. No user computation is retried by this cache.
///
/// ```
/// use std::convert::Infallible;
/// use lling_llang::wfst::{SharedCachePolicy, SharedStateCache};
/// let cache = SharedStateCache::new(SharedCachePolicy::CacheAll);
/// let value = cache.get_or_try_insert_with(7,
///     || Ok::<_, Infallible>("immutable result"), |_| true).expect("infallible");
/// cache.clear();
/// assert_eq!(*value, "immutable result");
/// assert_eq!(cache.statistics().resident_states, 0);
/// ```
pub struct SharedStateCache<T> {
    root: ArcSwap<Snapshot<T>>,
    counters: Counters,
}

impl<T> SharedStateCache<T> {
    /// Construct an empty independent cache with explicit residency semantics.
    pub fn new(policy: SharedCachePolicy) -> Self {
        Self {
            root: ArcSwap::from_pointee(Snapshot::new(policy)),
            counters: Counters::default(),
        }
    }

    /// Observe the current policy.
    pub fn policy(&self) -> SharedCachePolicy {
        self.root.load().policy()
    }

    /// Evict the current generation, preserving its policy and cumulative counters.
    /// A computation already in flight can return, but cannot repopulate it.
    pub fn clear(&self) {
        loop {
            let current = self.root.load();
            let next = Arc::new(Snapshot::new(current.policy()));
            #[cfg(test)]
            before_publication();
            let previous = self.root.compare_and_swap(&*current, next);
            if Arc::ptr_eq(&current, &previous) {
                break;
            }
        }
        increment(&self.counters.clears);
    }

    /// Atomically replace both policy and resident generation.
    pub fn set_policy(&self, policy: SharedCachePolicy) {
        self.root.store(Arc::new(Snapshot::new(policy)));
        increment(&self.counters.clears);
    }

    /// Observe cumulative counters and current resident metadata sizes.
    pub fn statistics(&self) -> SharedCacheStatistics {
        let root = self.root.load();
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        SharedCacheStatistics {
            hits: read(&self.counters.hits),
            misses: read(&self.counters.misses),
            faults: read(&self.counters.faults),
            uncacheable_results: read(&self.counters.uncacheable_results),
            insertions: read(&self.counters.insertions),
            evictions: read(&self.counters.evictions),
            raced_publications: read(&self.counters.raced_publications),
            clears: read(&self.counters.clears),
            resident_states: root.resident_count(),
            recency_records: match &root.storage {
                Storage::Lru(storage) => storage.entries.len(),
                _ => 0,
            },
        }
    }

    /// Look up a state or compute it once outside all cache guards.
    ///
    /// `admit` classifies successful complete results. Return false for an
    /// invalid-but-potentially-discoverable ID. Errors and rejected results are
    /// returned to their caller, never hidden by another request's success.
    /// Racing successful misses can compute redundantly; publication selects
    /// one canonical immutable value for the current generation.
    pub fn get_or_try_insert_with<E>(
        &self,
        id: u64,
        compute: impl FnOnce() -> Result<T, E>,
        admit: impl FnOnce(&T) -> bool,
    ) -> Result<Arc<T>, E> {
        let current = self.root.load();
        // Keep the CacheAll hit on a direct first-branch path. This avoids the
        // policy-generic lookup dispatch on the most common read-only path.
        if let Storage::All(entries) = &current.storage {
            if let Some(value) = entries.get(&id) {
                let value = Arc::clone(value);
                drop(current);
                increment(&self.counters.hits);
                return Ok(value);
            }
        } else if let Some(entry) = current.lookup(id) {
            let value = Arc::clone(entry.value);
            if entry.touch.is_none() {
                // Touching the tail changes no order. Linearize this access at
                // the snapshot read, before any overlapping eviction or clear.
                drop(current);
                increment(&self.counters.hits);
                return Ok(value);
            }
            let generation = Arc::clone(&current.generation);
            drop(current);
            increment(&self.counters.hits);
            return Ok(self.publish(id, value, &generation, false));
        }
        let generation = (current.policy() != SharedCachePolicy::NoCache)
            .then(|| Arc::clone(&current.generation));
        drop(current);
        increment(&self.counters.misses);
        let value = match compute() {
            Ok(value) => value,
            Err(error) => {
                increment(&self.counters.faults);
                return Err(error);
            }
        };
        let cacheable = admit(&value);
        let value = Arc::new(value);
        if !cacheable {
            increment(&self.counters.uncacheable_results);
            return Ok(value);
        }
        Ok(match generation {
            Some(generation) => self.publish(id, value, &generation, true),
            None => value,
        })
    }

    fn publish(&self, id: u64, value: Arc<T>, generation: &Arc<()>, cold: bool) -> Arc<T> {
        loop {
            let current = self.root.load();
            if !Arc::ptr_eq(generation, &current.generation)
                || current.policy() == SharedCachePolicy::NoCache
            {
                return value;
            }
            let existing = current.lookup(id);
            let canonical = existing
                .as_ref()
                .map_or_else(|| Arc::clone(&value), |entry| Arc::clone(entry.value));
            if existing.as_ref().is_some_and(|entry| entry.touch.is_none()) {
                if cold {
                    increment(&self.counters.raced_publications);
                }
                return canonical;
            }
            let inserted = existing.is_none();
            let mut next = (**current).clone();
            // Resolve a slot from this root only. After failed CAS, the next
            // iteration must look up the ID again: eviction can reuse its slot.
            let evicted = next.record(id, &canonical, existing.and_then(|entry| entry.touch));
            #[cfg(test)]
            before_publication();
            let previous = self.root.compare_and_swap(&*current, Arc::new(next));
            if Arc::ptr_eq(&current, &previous) {
                if inserted {
                    increment(&self.counters.insertions);
                }
                if evicted {
                    increment(&self.counters.evictions);
                }
                if cold && !inserted {
                    increment(&self.counters.raced_publications);
                }
                return canonical;
            }
            // Retry metadata around the already computed value. A racing
            // eviction does not erase this access from exact LRU order.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::convert::Infallible;
    use std::sync::{Barrier, Weak};

    fn lru(capacity: usize) -> SharedCachePolicy {
        SharedCachePolicy::Lru {
            capacity: NonZeroUsize::new(capacity).expect("positive capacity"),
        }
    }

    fn get(cache: &SharedStateCache<u64>, id: u64) -> Arc<u64> {
        cache
            .get_or_try_insert_with(id, || Ok::<_, Infallible>(id), |_| true)
            .expect("infallible")
    }

    fn assert_indexes<T>(cache: &SharedStateCache<T>) {
        let root = cache.root.load();
        match &root.storage {
            Storage::None => assert_eq!(root.resident_count(), 0),
            Storage::All(entries) => assert_eq!(root.resident_count(), entries.len()),
            Storage::Lru(storage) => {
                storage.checked_order();
            }
        }
    }

    #[test]
    fn all_none_and_exact_capacity_one_two() {
        for (policy, misses, evictions, residents) in [
            (SharedCachePolicy::CacheAll, 3, 0, 3),
            (SharedCachePolicy::NoCache, 5, 0, 0),
            (lru(1), 5, 4, 1),
            (lru(2), 4, 2, 2),
        ] {
            let cache = SharedStateCache::new(policy);
            for id in [0, 1, 0, 2, 1] {
                assert_eq!(*get(&cache, id), id);
                assert_indexes(&cache);
            }
            let stats = cache.statistics();
            assert_eq!(stats.misses, misses);
            assert_eq!(stats.evictions, evictions);
            assert_eq!(stats.resident_states, residents);
            assert_eq!(stats.hits + stats.misses, 5);
        }
    }

    #[test]
    fn failed_and_uncacheable_results_are_returned_not_admitted() {
        let cache = SharedStateCache::new(SharedCachePolicy::CacheAll);
        assert_eq!(
            cache.get_or_try_insert_with(7, || Err::<u64, _>("fault"), |_| true),
            Err("fault")
        );
        assert_eq!(
            *cache
                .get_or_try_insert_with(7, || Ok::<_, Infallible>(99), |_| false)
                .expect("value"),
            99
        );
        assert_eq!(cache.statistics().resident_states, 0);
        assert_eq!(*get(&cache, 7), 7);
        assert_eq!(cache.statistics().faults, 1);
        assert_eq!(cache.statistics().uncacheable_results, 1);
    }

    #[test]
    fn same_id_reentry_never_waits_for_its_own_initialization() {
        let cache = SharedStateCache::new(lru(2));
        let result = cache
            .get_or_try_insert_with(
                4,
                || {
                    let nested = get(&cache, 4);
                    assert_eq!(*nested, 4);
                    Ok::<_, Infallible>(4)
                },
                |_| true,
            )
            .expect("reentrant computation");
        assert_eq!(*result, 4);
        let stats = cache.statistics();
        assert_eq!(
            (stats.misses, stats.insertions, stats.raced_publications),
            (2, 1, 1)
        );
        assert_indexes(&cache);
    }

    #[test]
    fn failing_reentrant_attempt_is_not_hidden_by_nested_success() {
        let cache = SharedStateCache::new(SharedCachePolicy::CacheAll);
        let result = cache.get_or_try_insert_with(
            4,
            || {
                get(&cache, 4);
                Err::<u64, _>("late failure")
            },
            |_| true,
        );
        assert_eq!(result, Err("late failure"));
        assert_eq!(*get(&cache, 4), 4);
        assert_eq!(cache.statistics().faults, 1);
    }

    struct Dropped(Arc<AtomicU64>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            increment(&self.0);
        }
    }

    #[test]
    fn blocked_computation_pins_only_generation_and_cannot_repopulate_after_clear() {
        let cache = Arc::new(SharedStateCache::new(SharedCachePolicy::CacheAll));
        let retired = Arc::new(AtomicU64::new(0));
        drop(
            cache
                .get_or_try_insert_with(
                    0,
                    || Ok::<_, Infallible>(Dropped(retired.clone())),
                    |_| true,
                )
                .expect("seed"),
        );
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker = {
            let (cache, entered, release) = (cache.clone(), entered.clone(), release.clone());
            std::thread::spawn(move || {
                cache
                    .get_or_try_insert_with(
                        1,
                        || {
                            entered.wait();
                            release.wait();
                            Ok::<_, Infallible>(Dropped(Arc::new(AtomicU64::new(0))))
                        },
                        |_| true,
                    )
                    .expect("in-flight value")
            })
        };
        entered.wait();
        cache.clear();
        assert_eq!(
            retired.load(Ordering::Relaxed),
            1,
            "blocked callback must not retain the old cache"
        );
        release.wait();
        drop(worker.join().expect("worker completes"));
        assert_eq!(cache.statistics().resident_states, 0);
        assert_eq!(cache.statistics().clears, 1);
    }

    #[test]
    fn concurrent_same_id_misses_compute_once_each_and_publish_one_value() {
        let cache = Arc::new(SharedStateCache::new(lru(2)));
        let barrier = Arc::new(Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (cache, barrier) = (cache.clone(), barrier.clone());
                std::thread::spawn(move || {
                    cache
                        .get_or_try_insert_with(
                            9,
                            || {
                                barrier.wait();
                                Ok::<_, Infallible>(9)
                            },
                            |_| true,
                        )
                        .expect("value")
                })
            })
            .collect();
        let values: Vec<_> = threads
            .into_iter()
            .map(|t| t.join().expect("thread"))
            .collect();
        assert!(values.iter().all(|v| Arc::ptr_eq(v, &values[0])));
        let stats = cache.statistics();
        assert_eq!(
            (stats.misses, stats.insertions, stats.raced_publications),
            (8, 1, 7)
        );
        assert_indexes(&cache);
    }

    #[test]
    fn retained_hit_is_reinserted_if_evicted_before_publication() {
        let cache = SharedStateCache::new(lru(1));
        let first = get(&cache, 0);
        let generation = cache.root.load().generation.clone();
        get(&cache, 1);
        assert_eq!(*cache.publish(0, first, &generation, false), 0);
        assert!(cache.root.load().lookup(0).is_some());
        assert_indexes(&cache);
    }

    #[test]
    fn repeated_hot_accesses_preserve_order_without_growing_metadata() {
        let cache = SharedStateCache::new(lru(2));
        get(&cache, 0);
        get(&cache, 1);
        for _ in 0..10_000 {
            get(&cache, 0);
        }
        get(&cache, 2);
        assert!(cache.root.load().lookup(1).is_none());
        assert_eq!(cache.statistics().recency_records, 2);
        assert_indexes(&cache);
    }

    struct ReentrantDrop {
        cache: Weak<SharedStateCache<ReentrantDrop>>,
        drops: Arc<AtomicU64>,
    }
    impl Drop for ReentrantDrop {
        fn drop(&mut self) {
            if let Some(cache) = self.cache.upgrade() {
                let _ = cache.statistics();
            }
            increment(&self.drops);
        }
    }

    #[test]
    fn retired_payload_destructor_can_reenter_cache() {
        let cache = Arc::new(SharedStateCache::new(lru(1)));
        let drops = Arc::new(AtomicU64::new(0));
        for id in 0..3 {
            drop(
                cache
                    .get_or_try_insert_with(
                        id,
                        || {
                            Ok::<_, Infallible>(ReentrantDrop {
                                cache: Arc::downgrade(&cache),
                                drops: drops.clone(),
                            })
                        },
                        |_| true,
                    )
                    .expect("value"),
            );
        }
        cache.clear();
        assert_eq!(drops.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn concurrent_distinct_and_hot_ids_preserve_exact_index_bounds() {
        let cache = Arc::new(SharedStateCache::new(lru(2)));
        std::thread::scope(|scope| {
            for thread in 0..8 {
                let cache = &cache;
                scope.spawn(move || {
                    for request in 0..200 {
                        let id = (request + thread) % 7;
                        assert_eq!(*get(cache, id), id);
                        assert_indexes(cache);
                    }
                });
            }
        });
        let stats = cache.statistics();
        assert_eq!(stats.hits + stats.misses, 1600);
        assert_indexes(&cache);
    }

    #[test]
    fn actual_failed_hit_cas_retries_eviction_but_cannot_cross_clear() {
        for clear in [false, true] {
            let cache = Arc::new(SharedStateCache::new(lru(2)));
            get(&cache, 0);
            get(&cache, 1); // The paused hit must really change recency.
            let paused = Arc::new(Barrier::new(2));
            let resume = Arc::new(Barrier::new(2));
            let worker = {
                let (cache, paused, resume) = (cache.clone(), paused.clone(), resume.clone());
                std::thread::spawn(move || {
                    BEFORE_PUBLICATION.with(|slot| {
                        *slot.borrow_mut() = Some(Box::new(move || {
                            paused.wait();
                            resume.wait();
                        }))
                    });
                    cache
                        .get_or_try_insert_with(
                            0,
                            || Err::<u64, _>("a held hit must not recompute"),
                            |_| true,
                        )
                        .expect("held hit")
                })
            };
            paused.wait();
            if clear {
                cache.clear();
            } else {
                get(&cache, 2); // Evict the paused non-MRU state 0.
            }
            resume.wait();
            assert_eq!(*worker.join().expect("racing hit"), 0);
            assert_eq!(cache.root.load().lookup(0).is_some(), !clear);
            assert_indexes(&cache);
        }
    }

    #[test]
    fn most_recent_hits_and_racing_canonical_results_need_no_root_publication() {
        for capacity in [1, 2] {
            let cache = SharedStateCache::new(lru(capacity));
            let canonical = get(&cache, 7);
            let root = cache.root.load_full();
            let value = cache
                .get_or_try_insert_with(
                    7,
                    || Err::<u64, _>("most-recent hit must not recompute"),
                    |_| true,
                )
                .expect("hit");
            assert!(Arc::ptr_eq(&value, &canonical));
            assert!(Arc::ptr_eq(&root, &cache.root.load_full()));
            let raced = cache.publish(7, Arc::new(7), &root.generation, true);
            assert!(Arc::ptr_eq(&raced, &canonical));
            assert!(Arc::ptr_eq(&root, &cache.root.load_full()));
            assert_eq!(cache.statistics().raced_publications, 1);
            assert_indexes(&cache);
        }
    }

    #[test]
    fn failed_cas_resolves_an_id_that_moved_to_a_different_reused_slot() {
        let cache = Arc::new(SharedStateCache::new(lru(2)));
        let held = get(&cache, 0);
        get(&cache, 1);
        let paused = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let worker = {
            let (cache, paused, resume) = (cache.clone(), paused.clone(), resume.clone());
            std::thread::spawn(move || {
                BEFORE_PUBLICATION.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move || {
                        paused.wait();
                        resume.wait();
                    }));
                });
                cache
                    .get_or_try_insert_with(
                        0,
                        || Err::<u64, _>("held hit must not recompute"),
                        |_| true,
                    )
                    .expect("hit resolves after reuse")
            })
        };
        paused.wait();
        get(&cache, 2); // Reuse 0's old slot for 2.
        let canonical = get(&cache, 0); // Reuse 1's old slot for a new resident 0.
        assert!(!Arc::ptr_eq(&held, &canonical));
        resume.wait();
        assert!(Arc::ptr_eq(&worker.join().expect("worker"), &canonical));
        assert_eq!(*held, 0);
        let root = cache.root.load();
        let Storage::Lru(storage) = &root.storage else {
            panic!("LRU");
        };
        assert_eq!(storage.checked_order(), [2, 0]);
    }

    #[test]
    fn failed_cas_retries_promotion_without_reusing_a_stale_inline_slot() {
        let cache = Arc::new(SharedStateCache::new(lru(3)));
        let held = get(&cache, 0);
        get(&cache, 1);
        let old = cache.root.load_full();
        let nested = Arc::clone(&cache);
        BEFORE_PUBLICATION.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                // This publication promotes the winning root while the outer
                // request still owns a staged promotion of the old inline root.
                assert_eq!(*get(&nested, 2), 2);
            }));
        });
        assert_eq!(*get(&cache, 3), 3);
        let Storage::Lru(old_storage) = &old.storage else {
            panic!("LRU");
        };
        assert_eq!(old_storage.checked_order(), [0, 1]);
        let root = cache.root.load();
        let Storage::Lru(storage) = &root.storage else {
            panic!("LRU");
        };
        assert_eq!(storage.checked_order(), [1, 2, 3]);
        assert_eq!(*held, 0);
        assert_eq!(cache.statistics().evictions, 1);
        assert_indexes(&cache);
    }

    #[test]
    fn shuffled_touches_preserve_exact_order_across_vector_boundaries() {
        for capacity in [
            1, 2, 3, 15, 16, 17, 63, 64, 65, 127, 128, 129, 1023, 1024, 1025,
        ] {
            let cache = SharedStateCache::new(lru(capacity));
            let mut reference: Vec<_> = (0..capacity as u64).collect();
            for id in &reference {
                get(&cache, *id);
            }
            let mut sequence = 0x6a09_e667_f3bc_c909u64;
            for _ in 0..512 {
                sequence = sequence
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                let id = (sequence >> 32) % capacity as u64;
                get(&cache, id);
                reference.retain(|entry| *entry != id);
                reference.push(id);
                let root = cache.root.load();
                let Storage::Lru(storage) = &root.storage else {
                    panic!("LRU");
                };
                assert_eq!(storage.checked_order(), reference);
            }
        }
    }

    #[test]
    fn huge_capacity_grows_with_residents_and_counters_saturate() {
        let cache = SharedStateCache::new(lru(usize::MAX));
        get(&cache, u64::MAX);
        assert_eq!(cache.statistics().resident_states, 1);
        assert_indexes(&cache);
        let counter = AtomicU64::new(u64::MAX - 1);
        increment(&counter);
        increment(&counter);
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
    }

    #[test]
    fn non_mru_hits_share_payloads_and_the_entire_id_index() {
        for capacity in [2, 3, 7, 8, 15, 16, 17, 64, 65, 128, 129, 1023, 1024, 1025] {
            for cold in [false, true] {
                let cache = SharedStateCache::new(lru(capacity));
                for id in 0..capacity as u64 {
                    get(&cache, id);
                }
                let before = cache.root.load_full();
                if cold {
                    drop(cache.publish(0, Arc::new(0), &before.generation, true));
                } else {
                    get(&cache, 0);
                }
                let after = cache.root.load_full();
                assert!(
                    !Arc::ptr_eq(&before, &after),
                    "non-MRU touch publishes order"
                );
                let (Storage::Lru(old), Storage::Lru(new)) = (&before.storage, &after.storage)
                else {
                    panic!("both roots retain the LRU policy");
                };
                assert!(
                    old.entries.ptr_eq(&new.entries),
                    "ownership HAMT must not copy"
                );
                assert_eq!(new.checked_order().last(), Some(&0));
                for payload in old.entries.values() {
                    assert_eq!(
                        Arc::strong_count(&payload.value),
                        1,
                        "retaining both roots must not clone payload references"
                    );
                }
                assert_indexes(&cache);
                get(&cache, capacity as u64);
                assert!(cache.root.load().lookup(1).is_none());
                assert_indexes(&cache);
            }
        }
    }

    #[test]
    fn full_sparse_id_domain_and_policy_resets_leave_no_stale_metadata() {
        let cache = SharedStateCache::new(lru(2));
        for id in [0, u64::MAX, 1 << 63, u64::MAX, 0] {
            assert_eq!(*get(&cache, id), id);
            assert_indexes(&cache);
        }
        assert!(cache.root.load().lookup(1 << 63).is_none());
        for policy in [
            SharedCachePolicy::CacheAll,
            SharedCachePolicy::NoCache,
            lru(1),
        ] {
            cache.set_policy(policy);
            assert_indexes(&cache);
            for id in [u64::MAX, 0, u64::MAX] {
                assert_eq!(*get(&cache, id), id);
                assert_indexes(&cache);
            }
        }
    }

    #[test]
    fn clear_retries_a_policy_replacement_without_restoring_the_old_policy() {
        let cache = Arc::new(SharedStateCache::new(SharedCachePolicy::CacheAll));
        get(&cache, 0);
        let paused = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let worker = {
            let (cache, paused, resume) = (cache.clone(), paused.clone(), resume.clone());
            std::thread::spawn(move || {
                BEFORE_PUBLICATION.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move || {
                        paused.wait();
                        resume.wait();
                    }));
                });
                cache.clear();
            })
        };
        paused.wait();
        cache.set_policy(lru(2));
        get(&cache, 1);
        resume.wait();
        worker.join().expect("clear retries and completes");
        assert_eq!(cache.policy(), lru(2));
        assert_eq!(cache.statistics().resident_states, 0);
        assert_eq!(cache.statistics().clears, 2);
        get(&cache, 2);
        assert_indexes(&cache);
    }

    struct PublishingDrop {
        cache: Weak<SharedStateCache<PublishingDrop>>,
        publish_once: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Drop for PublishingDrop {
        fn drop(&mut self) {
            if self.publish_once.swap(false, Ordering::Relaxed) {
                if let Some(cache) = self.cache.upgrade() {
                    cache.set_policy(lru(2));
                    cache
                        .get_or_try_insert_with(
                            77,
                            || {
                                Ok::<_, Infallible>(Self {
                                    cache: self.cache.clone(),
                                    publish_once: self.publish_once.clone(),
                                })
                            },
                            |_| true,
                        )
                        .expect("destructor may publish into the new generation");
                }
            }
        }
    }

    #[test]
    fn clear_does_not_erase_publication_from_retired_payload_destructor() {
        let cache = Arc::new(SharedStateCache::new(SharedCachePolicy::CacheAll));
        let publish_once = Arc::new(std::sync::atomic::AtomicBool::new(true));
        drop(
            cache
                .get_or_try_insert_with(
                    0,
                    || {
                        Ok::<_, Infallible>(PublishingDrop {
                            cache: Arc::downgrade(&cache),
                            publish_once: publish_once.clone(),
                        })
                    },
                    |_| true,
                )
                .expect("initial payload"),
        );
        cache.clear();
        assert_eq!(cache.policy(), lru(2));
        assert_eq!(cache.statistics().resident_states, 1);
        assert!(cache.root.load().lookup(77).is_some());
        assert_indexes(&cache);
    }

    #[test]
    fn blocked_admission_does_not_pin_old_cache_and_old_no_cache_does_not_admit() {
        let cache = Arc::new(SharedStateCache::new(SharedCachePolicy::CacheAll));
        let retired = Arc::new(AtomicU64::new(0));
        drop(
            cache
                .get_or_try_insert_with(
                    0,
                    || Ok::<_, Infallible>(Dropped(retired.clone())),
                    |_| true,
                )
                .expect("seed"),
        );
        let paused = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let worker = {
            let (cache, paused, resume) = (cache.clone(), paused.clone(), resume.clone());
            std::thread::spawn(move || {
                cache
                    .get_or_try_insert_with(
                        1,
                        || Ok::<_, Infallible>(Dropped(Arc::new(AtomicU64::new(0)))),
                        |_| {
                            paused.wait();
                            resume.wait();
                            true
                        },
                    )
                    .expect("result")
            })
        };
        paused.wait();
        cache.clear();
        assert_eq!(retired.load(Ordering::Relaxed), 1);
        resume.wait();
        drop(worker.join().expect("admission finishes"));
        assert_eq!(cache.statistics().resident_states, 0);

        let cache = SharedStateCache::new(SharedCachePolicy::NoCache);
        assert_eq!(
            *cache
                .get_or_try_insert_with(
                    0,
                    || {
                        cache.set_policy(SharedCachePolicy::CacheAll);
                        Ok::<_, Infallible>(0)
                    },
                    |_| true
                )
                .expect("result"),
            0
        );
        assert_eq!(cache.statistics().resident_states, 0);
    }

    #[test]
    fn repeated_slot_reuse_preserves_held_values_and_exact_victims() {
        for capacity in [1, 2] {
            let cache = SharedStateCache::new(lru(capacity));
            let first = get(&cache, 0);
            for id in 1..2000 {
                get(&cache, id);
                assert_indexes(&cache);
                assert_eq!(*first, 0, "slot reuse cannot mutate a retained payload");
                let root = cache.root.load();
                let Storage::Lru(storage) = &root.storage else {
                    panic!("LRU");
                };
                let expected: Vec<_> = ((id + 1).saturating_sub(capacity as u64)..=id).collect();
                assert_eq!(storage.checked_order(), expected);
            }
            cache.clear();
            assert_eq!(*first, 0);
            assert_indexes(&cache);
        }
    }

    proptest! {
        #[test]
        fn histories_match_a_simple_exact_recency_reference(actions in prop::collection::vec(0u8..16, 0..200)) {
            let cache = SharedStateCache::new(lru(2));
            let mut policy = lru(2);
            let mut order = Vec::<u64>::new();
            for action in actions {
                match action {
                    0..=9 => {
                        let id = u64::from(action % 5) * 1_000_000_007;
                        prop_assert_eq!(*get(&cache, id), id);
                        if policy != SharedCachePolicy::NoCache {
                            order.retain(|entry| *entry != id);
                            order.push(id);
                            if let SharedCachePolicy::Lru { capacity } = policy {
                                if order.len() > capacity.get() { order.remove(0); }
                            }
                        }
                    }
                    10 => { cache.clear(); order.clear(); }
                    _ => {
                        policy = match action {
                            11 => SharedCachePolicy::NoCache,
                            12 => SharedCachePolicy::CacheAll,
                            13 => lru(1),
                            14 => lru(2),
                            _ => lru(3),
                        };
                        cache.set_policy(policy);
                        order.clear();
                    }
                }
                let root = cache.root.load();
                prop_assert_eq!(root.resident_count(), order.len());
                for id in &order { prop_assert!(root.lookup(*id).is_some()); }
                if let Storage::Lru(storage) = &root.storage {
                    let actual = storage.checked_order();
                    prop_assert_eq!(&actual, &order);
                }
                drop(root);
                assert_indexes(&cache);
            }
        }
    }
}
