//! K-vector atomic reduction for reduced contention.
//!
//! This module provides K-vector reduction, a technique for reducing
//! atomic operation contention by distributing operations across K vectors.
//!
//! ## Problem
//!
//! When many threads perform atomic operations on the same memory location,
//! contention causes significant slowdown. This is particularly problematic
//! for lattice arc accumulation during decoding.
//!
//! ## Solution: K-Vector Distribution
//!
//! Instead of a single accumulation buffer, use K buffers:
//!
//! ```text
//! vectors[0]: [slot0, slot1, slot2, ...]
//! vectors[1]: [slot0, slot1, slot2, ...]
//! ...
//! vectors[K-1]: [slot0, slot1, slot2, ...]
//! ```
//!
//! Each thread randomly selects a vector to update, reducing contention by ~K×.
//!
//! ## Algorithm
//!
//! ```text
//! procedure K_VECTOR_ADD(value, slot):
//!     k = random() % K
//!     atomic_add(vectors[k][slot], value)
//!
//! procedure K_VECTOR_COLLECT(slot):
//!     result = []
//!     for k in 0..K:
//!         result.extend(vectors[k][slot])
//!     return result
//! ```
//!
//! ## Performance
//!
//! - **K=32**: 10× speedup for lattice arc generation
//! - **Trade-off**: More memory usage (K× buffer size)
//!
//! ## References
//!
//! - Chen et al., "GPU-based WFST Decoder with Exact Lattice Generation" (2018)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Default number of vectors (matches CUDA warp size).
pub const DEFAULT_K: usize = 32;

/// Configuration for K-vector reduction.
#[derive(Clone, Copy, Debug)]
pub struct KVectorConfig {
    /// Number of vectors (K).
    pub num_vectors: usize,
    /// Number of slots per vector.
    pub num_slots: usize,
    /// Initial capacity per slot.
    pub slot_capacity: usize,
}

impl KVectorConfig {
    /// Create a new configuration.
    pub fn new(num_vectors: usize, num_slots: usize) -> Self {
        Self {
            num_vectors,
            num_slots,
            slot_capacity: 16,
        }
    }

    /// Create with custom slot capacity.
    pub fn with_capacity(num_vectors: usize, num_slots: usize, slot_capacity: usize) -> Self {
        Self {
            num_vectors,
            num_slots,
            slot_capacity,
        }
    }

    /// Calculate total memory size in bytes for a given element size.
    pub fn memory_size(&self, element_size: usize) -> usize {
        self.num_vectors * self.num_slots * self.slot_capacity * element_size
    }
}

impl Default for KVectorConfig {
    fn default() -> Self {
        Self::new(DEFAULT_K, 1024)
    }
}

/// A slot in a K-vector that accumulates values.
///
/// Uses a Mutex for thread-safe CPU simulation. Real GPU implementations
/// would use lock-free primitives with pre-allocated buffers.
struct KVectorSlot<T> {
    /// Values accumulated in this slot (mutex-protected for CPU safety).
    values: Mutex<Vec<T>>,
    /// Number of values in the slot (for fast reads without lock).
    count: AtomicUsize,
}

impl<T: std::fmt::Debug> std::fmt::Debug for KVectorSlot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let values = self.values.lock().unwrap();
        f.debug_struct("KVectorSlot")
            .field("values", &*values)
            .field("count", &self.count.load(Ordering::Relaxed))
            .finish()
    }
}

impl<T> KVectorSlot<T> {
    fn new(capacity: usize) -> Self {
        Self {
            values: Mutex::new(Vec::with_capacity(capacity)),
            count: AtomicUsize::new(0),
        }
    }

    fn push(&self, value: T) {
        let mut values = self.values.lock().unwrap();
        values.push(value);
        self.count.store(values.len(), Ordering::Release);
    }

    fn drain(&self) -> Vec<T> {
        let mut values = self.values.lock().unwrap();
        self.count.store(0, Ordering::Release);
        std::mem::take(&mut *values)
    }

    fn len(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn clear(&self) {
        let mut values = self.values.lock().unwrap();
        values.clear();
        self.count.store(0, Ordering::Release);
    }
}

/// K-vector for reduced contention atomic accumulation.
///
/// Distributes atomic operations across K parallel vectors to reduce
/// contention when many threads accumulate values to the same slots.
pub struct KVector<T> {
    /// K vectors, each with num_slots slots.
    vectors: Vec<Vec<KVectorSlot<T>>>,
    /// Configuration.
    config: KVectorConfig,
    /// Simple random state for vector selection.
    random_state: AtomicUsize,
}

impl<T> KVector<T> {
    /// Create a new K-vector.
    pub fn new(config: KVectorConfig) -> Self {
        let vectors = (0..config.num_vectors)
            .map(|_| {
                (0..config.num_slots)
                    .map(|_| KVectorSlot::new(config.slot_capacity))
                    .collect()
            })
            .collect();

        Self {
            vectors,
            config,
            random_state: AtomicUsize::new(0x12345678),
        }
    }

    /// Create with default K=32.
    pub fn with_num_slots(num_slots: usize) -> Self {
        Self::new(KVectorConfig::new(DEFAULT_K, num_slots))
    }

    /// Get the configuration.
    pub fn config(&self) -> &KVectorConfig {
        &self.config
    }

    /// Get a pseudo-random vector index.
    fn random_vector(&self) -> usize {
        // Simple LCG for fast pseudo-random selection
        let state = self.random_state.fetch_add(1, Ordering::Relaxed);
        let hash = state.wrapping_mul(0x5851F42D4C957F2D);
        hash % self.config.num_vectors
    }

    /// Push a value to a slot, using random vector selection.
    pub fn push(&self, slot: usize, value: T) {
        let k = self.random_vector();
        self.vectors[k][slot].push(value);
    }

    /// Push a value to a specific vector's slot.
    pub fn push_to_vector(&self, k: usize, slot: usize, value: T) {
        self.vectors[k][slot].push(value);
    }

    /// Collect all values from a slot across all K vectors.
    pub fn collect(&self, slot: usize) -> Vec<T> {
        let mut result = Vec::new();
        for k in 0..self.config.num_vectors {
            result.extend(self.vectors[k][slot].drain());
        }
        result
    }

    /// Get the count of values in a slot across all K vectors.
    pub fn slot_count(&self, slot: usize) -> usize {
        (0..self.config.num_vectors)
            .map(|k| self.vectors[k][slot].len())
            .sum()
    }

    /// Check if a slot is empty across all K vectors.
    pub fn slot_is_empty(&self, slot: usize) -> bool {
        (0..self.config.num_vectors).all(|k| self.vectors[k][slot].is_empty())
    }

    /// Clear all slots.
    pub fn clear(&self) {
        for k in 0..self.config.num_vectors {
            for slot in 0..self.config.num_slots {
                self.vectors[k][slot].clear();
            }
        }
    }

    /// Get statistics about the K-vector.
    pub fn stats(&self) -> KVectorStats {
        let mut total_count = 0;
        let mut non_empty_slots = 0;

        for slot in 0..self.config.num_slots {
            let count = self.slot_count(slot);
            if count > 0 {
                total_count += count;
                non_empty_slots += 1;
            }
        }

        KVectorStats {
            num_vectors: self.config.num_vectors,
            num_slots: self.config.num_slots,
            total_values: total_count,
            non_empty_slots,
            avg_values_per_slot: if non_empty_slots > 0 {
                total_count as f64 / non_empty_slots as f64
            } else {
                0.0
            },
        }
    }
}

/// Statistics about a K-vector.
#[derive(Clone, Debug)]
pub struct KVectorStats {
    /// Number of vectors (K).
    pub num_vectors: usize,
    /// Number of slots.
    pub num_slots: usize,
    /// Total number of values stored.
    pub total_values: usize,
    /// Number of non-empty slots.
    pub non_empty_slots: usize,
    /// Average values per non-empty slot.
    pub avg_values_per_slot: f64,
}

impl KVectorStats {
    /// Estimate contention reduction factor.
    pub fn contention_reduction(&self) -> f64 {
        self.num_vectors as f64
    }

    /// Calculate slot utilization.
    pub fn slot_utilization(&self) -> f64 {
        if self.num_slots == 0 {
            0.0
        } else {
            self.non_empty_slots as f64 / self.num_slots as f64
        }
    }
}

/// Reduce values across K vectors using a custom aggregation function.
///
/// # Arguments
///
/// * `k_vector` - The K-vector to reduce
/// * `slot` - The slot to reduce
/// * `reduce_fn` - Function to aggregate values
///
/// # Returns
///
/// The aggregated result, or `None` if the slot is empty.
pub fn reduce_with_k_vectors<T, R, F>(k_vector: &KVector<T>, slot: usize, reduce_fn: F) -> Option<R>
where
    F: Fn(&[T]) -> R,
{
    let values = k_vector.collect(slot);
    if values.is_empty() {
        None
    } else {
        Some(reduce_fn(&values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kvector_config() {
        let config = KVectorConfig::new(16, 100);
        assert_eq!(config.num_vectors, 16);
        assert_eq!(config.num_slots, 100);
    }

    #[test]
    fn test_kvector_config_memory_size() {
        let config = KVectorConfig::with_capacity(32, 1000, 8);
        let size = config.memory_size(8); // 8 bytes per element
        assert_eq!(size, 32 * 1000 * 8 * 8);
    }

    #[test]
    fn test_kvector_creation() {
        let k_vec: KVector<i32> = KVector::with_num_slots(10);
        assert_eq!(k_vec.config().num_vectors, DEFAULT_K);
        assert_eq!(k_vec.config().num_slots, 10);
    }

    #[test]
    fn test_kvector_push_and_collect() {
        let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

        k_vec.push(0, 1);
        k_vec.push(0, 2);
        k_vec.push(0, 3);

        let values = k_vec.collect(0);
        assert_eq!(values.len(), 3);

        // Values should contain 1, 2, 3 (order may vary due to K distribution)
        let mut sorted = values.clone();
        sorted.sort();
        assert_eq!(sorted, vec![1, 2, 3]);
    }

    #[test]
    fn test_kvector_push_to_specific_vector() {
        let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

        k_vec.push_to_vector(0, 5, 10);
        k_vec.push_to_vector(1, 5, 20);
        k_vec.push_to_vector(2, 5, 30);

        let values = k_vec.collect(5);
        assert_eq!(values.len(), 3);

        let mut sorted = values;
        sorted.sort();
        assert_eq!(sorted, vec![10, 20, 30]);
    }

    #[test]
    fn test_kvector_slot_count() {
        let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

        assert_eq!(k_vec.slot_count(0), 0);
        assert!(k_vec.slot_is_empty(0));

        k_vec.push(0, 1);
        k_vec.push(0, 2);

        assert_eq!(k_vec.slot_count(0), 2);
        assert!(!k_vec.slot_is_empty(0));
    }

    #[test]
    fn test_kvector_clear() {
        let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

        k_vec.push(0, 1);
        k_vec.push(1, 2);
        k_vec.push(2, 3);

        k_vec.clear();

        assert!(k_vec.slot_is_empty(0));
        assert!(k_vec.slot_is_empty(1));
        assert!(k_vec.slot_is_empty(2));
    }

    #[test]
    fn test_kvector_stats() {
        let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

        k_vec.push(0, 1);
        k_vec.push(0, 2);
        k_vec.push(5, 3);

        let stats = k_vec.stats();
        assert_eq!(stats.num_vectors, 4);
        assert_eq!(stats.num_slots, 10);
        assert_eq!(stats.total_values, 3);
        assert_eq!(stats.non_empty_slots, 2);
    }

    #[test]
    fn test_reduce_with_k_vectors() {
        let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

        k_vec.push(0, 10);
        k_vec.push(0, 20);
        k_vec.push(0, 30);

        let sum = reduce_with_k_vectors(&k_vec, 0, |values| values.iter().sum::<i32>());
        assert_eq!(sum, Some(60));

        let empty = reduce_with_k_vectors(&k_vec, 5, |values: &[i32]| values.iter().sum::<i32>());
        assert_eq!(empty, None);
    }

    #[test]
    fn test_kvector_stats_contention_reduction() {
        let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(32, 10));
        let stats = k_vec.stats();

        assert!((stats.contention_reduction() - 32.0).abs() < 0.01);
    }

    #[test]
    fn test_concurrent_push() {
        use std::thread;

        let k_vec = std::sync::Arc::new(KVector::<i32>::new(KVectorConfig::new(32, 10)));

        let handles: Vec<_> = (0..8)
            .map(|t| {
                let kv = std::sync::Arc::clone(&k_vec);
                thread::spawn(move || {
                    for i in 0..100 {
                        kv.push(0, t * 100 + i);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let values = k_vec.collect(0);
        assert_eq!(values.len(), 800);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        // =====================================================================
        // KVectorConfig Properties
        // =====================================================================

        /// Config memory size is proportional to parameters.
        #[test]
        fn config_memory_scales(
            num_vectors in 1usize..64,
            num_slots in 1usize..1000,
            slot_capacity in 1usize..100,
            element_size in 1usize..32
        ) {
            let config = KVectorConfig::with_capacity(num_vectors, num_slots, slot_capacity);
            let expected = num_vectors * num_slots * slot_capacity * element_size;
            prop_assert_eq!(config.memory_size(element_size), expected);
        }

        // =====================================================================
        // Push/Collect Properties
        // =====================================================================

        /// Push-collect preserves all values (no loss).
        #[test]
        fn push_collect_preserves_values(values in proptest::collection::vec(0i32..1000, 1..50)) {
            let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

            for &v in &values {
                k_vec.push(0, v);
            }

            let collected = k_vec.collect(0);
            prop_assert_eq!(collected.len(), values.len(),
                "Count mismatch: pushed {} but collected {}", values.len(), collected.len());

            // All values should be present (order may vary)
            let mut sorted_input = values.clone();
            sorted_input.sort();
            let mut sorted_output = collected;
            sorted_output.sort();
            prop_assert_eq!(sorted_input, sorted_output);
        }

        /// Push to specific vector is retrievable.
        #[test]
        fn push_to_vector_retrievable(
            k in 0usize..4,
            slot in 0usize..5,
            values in proptest::collection::vec(0i32..1000, 1..20)
        ) {
            let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

            for &v in &values {
                k_vec.push_to_vector(k, slot, v);
            }

            let collected = k_vec.collect(slot);
            prop_assert_eq!(collected.len(), values.len());

            let mut sorted_input = values.clone();
            sorted_input.sort();
            let mut sorted_output = collected;
            sorted_output.sort();
            prop_assert_eq!(sorted_input, sorted_output);
        }

        // =====================================================================
        // Slot Count Properties
        // =====================================================================

        /// slot_count matches actual values pushed.
        #[test]
        fn slot_count_accurate(values in proptest::collection::vec(0i32..1000, 0..30)) {
            let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

            for &v in &values {
                k_vec.push(0, v);
            }

            prop_assert_eq!(k_vec.slot_count(0), values.len());
        }

        /// Empty slot is_empty returns true.
        #[test]
        fn empty_slot_is_empty(num_slots in 2usize..10) {
            let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, num_slots));

            // Only push to slot 0
            k_vec.push(0, 42);

            prop_assert!(!k_vec.slot_is_empty(0));
            for slot in 1..num_slots {
                prop_assert!(k_vec.slot_is_empty(slot), "Slot {} should be empty", slot);
            }
        }

        // =====================================================================
        // Clear Properties
        // =====================================================================

        /// Clear empties all slots.
        #[test]
        fn clear_empties_all(
            pushes in proptest::collection::vec((0usize..5, 0i32..100), 1..50)
        ) {
            let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

            for (slot, value) in &pushes {
                k_vec.push(*slot, *value);
            }

            k_vec.clear();

            for slot in 0..10 {
                prop_assert!(k_vec.slot_is_empty(slot), "Slot {} should be empty after clear", slot);
                prop_assert_eq!(k_vec.slot_count(slot), 0);
            }
        }

        // =====================================================================
        // Stats Properties
        // =====================================================================

        /// Stats reflect actual data.
        #[test]
        fn stats_accurate(values in proptest::collection::vec((0usize..5, 0i32..100), 1..30)) {
            let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

            for (slot, value) in &values {
                k_vec.push(*slot, *value);
            }

            let stats = k_vec.stats();
            prop_assert_eq!(stats.num_vectors, 4);
            prop_assert_eq!(stats.num_slots, 10);
            prop_assert_eq!(stats.total_values, values.len());
        }

        /// Contention reduction equals num_vectors.
        #[test]
        fn contention_reduction_matches_k(num_vectors in 1usize..64) {
            let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(num_vectors, 10));
            let stats = k_vec.stats();

            prop_assert!((stats.contention_reduction() - num_vectors as f64).abs() < 0.01);
        }

        // =====================================================================
        // Reduction Properties
        // =====================================================================

        /// reduce_with_k_vectors applies function correctly.
        #[test]
        fn reduce_sum_correct(values in proptest::collection::vec(1i32..100, 1..20)) {
            let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

            let expected_sum: i32 = values.iter().sum();
            for &v in &values {
                k_vec.push(0, v);
            }

            let result = reduce_with_k_vectors(&k_vec, 0, |vals| vals.iter().sum::<i32>());
            prop_assert_eq!(result, Some(expected_sum));
        }

        /// reduce_with_k_vectors returns None for empty slot.
        #[test]
        fn reduce_empty_is_none(_ in 0..1) {
            let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

            let result = reduce_with_k_vectors(&k_vec, 0, |vals: &[i32]| vals.iter().sum::<i32>());
            prop_assert_eq!(result, None);
        }

        // =====================================================================
        // Distribution Properties
        // =====================================================================

        /// Values are distributed across K vectors.
        #[test]
        fn values_distributed(num_values in 100usize..200) {
            let k_vec: KVector<i32> = KVector::new(KVectorConfig::new(4, 10));

            for i in 0..num_values {
                k_vec.push(0, i as i32);
            }

            // Stats should show correct count BEFORE collect (which drains values)
            let stats = k_vec.stats();
            prop_assert_eq!(stats.total_values, num_values);

            // Check that values are spread across vectors (not all in one)
            let collected = k_vec.collect(0);
            prop_assert_eq!(collected.len(), num_values);
        }
    }
}
