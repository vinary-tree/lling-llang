//! Dynamic load balancing for parallel WFST processing.
//!
//! This module provides load balancing abstractions that mirror GPU cooperative
//! group patterns, enabling efficient work distribution across parallel workers.
//!
//! ## Problem
//!
//! WFST states have varying numbers of outgoing arcs. Static work assignment
//! causes load imbalance - some threads finish quickly while others struggle
//! with high-degree states.
//!
//! ## Solution: Cooperative Groups with Dispatcher
//!
//! ```text
//! procedure DYNAMIC_LOAD_BALANCING(tokens):
//!     group = cooperative_groups::tiled_partition<32>()
//!     if group.thread_rank() == 0:
//!         i = atomic_add(global_d, 1)  // request new token
//!     i = group.shfl(i, 0)  // broadcast to whole group
//!     if i >= sizeof(tokens):
//!         return
//!     for arc in token_to_arcs(tokens[i]):  // thread parallelism
//!         call Process(arc)
//! ```
//!
//! ## Key Concepts
//!
//! - **Work Group**: N threads (typically 32, CUDA warp size) working together
//! - **Dispatcher**: Thread 0 requests work items via atomic counter
//! - **Broadcast**: All threads receive the same work item
//! - **Thread Parallelism**: Each thread processes one arc from the work item
//!
//! ## Benefits
//!
//! - **No WFST restructuring**: Works with any graph structure
//! - **Dynamic adaptation**: Automatically balances varying workloads
//! - **Minimal synchronization**: Only atomic add and warp shuffle

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Size of a work group (matches CUDA warp size).
pub const WORK_GROUP_SIZE: usize = 32;

/// A work item to be processed.
#[derive(Clone, Debug)]
pub struct WorkItem<T> {
    /// The item data.
    pub data: T,
    /// Number of sub-tasks (e.g., number of arcs).
    pub num_subtasks: usize,
    /// Priority (lower is higher priority).
    pub priority: f32,
}

impl<T> WorkItem<T> {
    /// Create a new work item.
    pub fn new(data: T, num_subtasks: usize) -> Self {
        Self {
            data,
            num_subtasks,
            priority: 0.0,
        }
    }

    /// Create with priority.
    pub fn with_priority(data: T, num_subtasks: usize, priority: f32) -> Self {
        Self {
            data,
            num_subtasks,
            priority,
        }
    }
}

/// A queue of work items with atomic dispatch.
#[derive(Debug)]
pub struct WorkQueue<T> {
    /// Work items to process.
    items: Vec<WorkItem<T>>,
    /// Current dispatch index.
    dispatch_index: AtomicUsize,
    /// Total number of subtasks.
    total_subtasks: usize,
}

impl<T> WorkQueue<T> {
    /// Create a new work queue.
    pub fn new(mut items: Vec<WorkItem<T>>) -> Self {
        items.sort_by(|a, b| priority_order(a.priority).total_cmp(&priority_order(b.priority)));
        let total_subtasks = items.iter().fold(0usize, |total, item| {
            total.saturating_add(item.num_subtasks)
        });
        Self {
            items,
            dispatch_index: AtomicUsize::new(0),
            total_subtasks,
        }
    }

    /// Get the number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get total number of subtasks.
    pub fn total_subtasks(&self) -> usize {
        self.total_subtasks
    }

    /// Atomically request the next work item index.
    ///
    /// Returns `None` if all items have been dispatched.
    pub fn request_next(&self) -> Option<usize> {
        let batch = self.request_batch(1);
        (batch.start < batch.end).then_some(batch.start)
    }

    /// Atomically request a contiguous batch of work item indices.
    ///
    /// `max_items == 0` is treated as one item so a caller cannot accidentally
    /// livelock on empty claims. The returned range is empty when all items have
    /// already been dispatched.
    pub fn request_batch(&self, max_items: usize) -> Range<usize> {
        let len = self.items.len();
        let max_items = max_items.max(1);

        loop {
            let start = self.dispatch_index.load(Ordering::Acquire);
            if start >= len {
                return len..len;
            }

            let end = start.saturating_add(max_items).min(len);
            if self
                .dispatch_index
                .compare_exchange_weak(start, end, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return start..end;
            }
        }
    }

    /// Get a work item by index.
    pub fn get(&self, index: usize) -> Option<&WorkItem<T>> {
        self.items.get(index)
    }

    /// Reset the dispatch index for reuse.
    pub fn reset(&self) {
        self.dispatch_index.store(0, Ordering::Release);
    }

    /// Get the current dispatch progress.
    pub fn progress(&self) -> (usize, usize) {
        let dispatched = self.dispatch_index.load(Ordering::Acquire);
        (dispatched.min(self.items.len()), self.items.len())
    }
}

/// A work group of N threads processing items together.
#[derive(Debug)]
pub struct WorkGroup {
    /// Group size (number of threads).
    size: usize,
    /// Group ID.
    id: usize,
    /// Thread rank within group (0 = dispatcher).
    thread_rank: usize,
}

impl WorkGroup {
    /// Create a new work group.
    pub fn new(size: usize, id: usize, thread_rank: usize) -> Self {
        let size = size.max(1);
        Self {
            size,
            id,
            thread_rank: thread_rank % size,
        }
    }

    /// Get the group size.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get the group ID.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Get the thread rank within the group.
    pub fn thread_rank(&self) -> usize {
        self.thread_rank
    }

    /// Check if this thread is the dispatcher (rank 0).
    pub fn is_dispatcher(&self) -> bool {
        self.thread_rank == 0
    }

    /// Calculate which subtask this thread should handle.
    ///
    /// # Arguments
    ///
    /// * `num_subtasks` - Total number of subtasks in the work item
    ///
    /// # Returns
    ///
    /// Iterator over subtask indices this thread should process.
    pub fn subtask_range(&self, num_subtasks: usize) -> impl Iterator<Item = usize> {
        let start = self.thread_rank;
        let step = self.size;
        (start..num_subtasks).step_by(step)
    }
}

fn priority_order(priority: f32) -> f32 {
    if priority.is_nan() {
        f32::INFINITY
    } else {
        priority
    }
}

/// Work dispatcher for dynamic load balancing.
///
/// Coordinates multiple work groups processing a shared work queue.
#[derive(Debug)]
pub struct WorkDispatcher<T> {
    /// Shared work queue.
    queue: Arc<WorkQueue<T>>,
    /// Number of work groups.
    num_groups: usize,
    /// Size of each group.
    group_size: usize,
}

impl<T> WorkDispatcher<T> {
    /// Create a new work dispatcher.
    pub fn new(items: Vec<WorkItem<T>>, num_groups: usize, group_size: usize) -> Self {
        Self {
            queue: Arc::new(WorkQueue::new(items)),
            num_groups,
            group_size: group_size.max(1),
        }
    }

    /// Create with default group size (32).
    pub fn with_default_group_size(items: Vec<WorkItem<T>>, num_groups: usize) -> Self {
        Self::new(items, num_groups, WORK_GROUP_SIZE)
    }

    /// Get the work queue.
    pub fn queue(&self) -> &WorkQueue<T> {
        &self.queue
    }

    /// Get the number of work groups.
    pub fn num_groups(&self) -> usize {
        self.num_groups
    }

    /// Get the group size.
    pub fn group_size(&self) -> usize {
        self.group_size
    }

    /// Create a work group handle for a specific group and thread.
    pub fn create_group(&self, group_id: usize, thread_rank: usize) -> WorkGroup {
        WorkGroup::new(self.group_size, group_id, thread_rank)
    }

    /// Reset the dispatcher for reuse.
    pub fn reset(&self) {
        self.queue.reset();
    }

    /// Get dispatch statistics.
    pub fn stats(&self) -> DispatchStats {
        let (dispatched, total) = self.queue.progress();
        DispatchStats {
            total_items: total,
            dispatched_items: dispatched,
            total_subtasks: self.queue.total_subtasks(),
            num_groups: self.num_groups,
            group_size: self.group_size,
        }
    }
}

impl<T> WorkDispatcher<T> {
    /// Get a clone of the queue for sharing with threads.
    pub fn queue_handle(&self) -> Arc<WorkQueue<T>> {
        Arc::clone(&self.queue)
    }
}

/// Statistics about work dispatch.
#[derive(Clone, Debug)]
pub struct DispatchStats {
    /// Total number of work items.
    pub total_items: usize,
    /// Number of items dispatched.
    pub dispatched_items: usize,
    /// Total number of subtasks.
    pub total_subtasks: usize,
    /// Number of work groups.
    pub num_groups: usize,
    /// Size of each group.
    pub group_size: usize,
}

impl DispatchStats {
    /// Get the completion ratio.
    pub fn completion_ratio(&self) -> f64 {
        if self.total_items == 0 {
            1.0
        } else {
            self.dispatched_items.min(self.total_items) as f64 / self.total_items as f64
        }
    }

    /// Get the total number of worker threads.
    pub fn total_workers(&self) -> usize {
        self.num_groups.saturating_mul(self.group_size)
    }

    /// Estimate average subtasks per worker.
    pub fn avg_subtasks_per_worker(&self) -> f64 {
        if self.total_workers() == 0 {
            0.0
        } else {
            self.total_subtasks as f64 / self.total_workers() as f64
        }
    }
}

/// Load balancer for distributing work across workers.
///
/// This is a higher-level abstraction that manages work distribution
/// and provides utilities for parallel processing.
pub struct LoadBalancer {
    /// Number of worker threads.
    num_workers: usize,
    /// Group size.
    group_size: usize,
}

impl LoadBalancer {
    /// Create a new load balancer.
    pub fn new(num_workers: usize) -> Self {
        let group_size = if num_workers == 0 {
            1
        } else {
            WORK_GROUP_SIZE.min(num_workers).max(1)
        };
        Self {
            num_workers,
            group_size,
        }
    }

    /// Create with custom group size.
    pub fn with_group_size(num_workers: usize, group_size: usize) -> Self {
        Self {
            num_workers,
            group_size: group_size.max(1),
        }
    }

    /// Get the number of workers.
    pub fn num_workers(&self) -> usize {
        self.num_workers
    }

    /// Get the number of work groups.
    pub fn num_groups(&self) -> usize {
        if self.num_workers == 0 {
            0
        } else {
            self.num_workers.div_ceil(self.group_size)
        }
    }

    /// Create a dispatcher for a set of work items.
    pub fn create_dispatcher<T>(&self, items: Vec<WorkItem<T>>) -> WorkDispatcher<T> {
        WorkDispatcher::new(items, self.num_groups(), self.group_size)
    }

    /// Estimate the optimal number of workers for a workload.
    ///
    /// # Arguments
    ///
    /// * `num_items` - Number of work items
    /// * `avg_subtasks` - Average number of subtasks per item
    ///
    /// # Returns
    ///
    /// Recommended number of workers.
    pub fn estimate_workers(num_items: usize, avg_subtasks: usize) -> usize {
        if num_items == 0 || avg_subtasks == 0 {
            return 0;
        }

        let total_work = num_items.saturating_mul(avg_subtasks);
        // Aim for at least 4 items per group for amortization
        let min_groups = num_items.div_ceil(4);
        let max_groups = total_work.div_ceil(WORK_GROUP_SIZE);
        min_groups
            .max(1)
            .min(max_groups.max(1))
            .saturating_mul(WORK_GROUP_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_work_item_creation() {
        let item = WorkItem::new(42, 10);
        assert_eq!(item.data, 42);
        assert_eq!(item.num_subtasks, 10);
        assert_eq!(item.priority, 0.0);
    }

    #[test]
    fn test_work_item_with_priority() {
        let item = WorkItem::with_priority("data", 5, 1.5);
        assert_eq!(item.data, "data");
        assert_eq!(item.num_subtasks, 5);
        assert_eq!(item.priority, 1.5);
    }

    #[test]
    fn test_work_queue_creation() {
        let items = vec![
            WorkItem::new(1, 10),
            WorkItem::new(2, 20),
            WorkItem::new(3, 30),
        ];
        let queue = WorkQueue::new(items);

        assert_eq!(queue.len(), 3);
        assert_eq!(queue.total_subtasks(), 60);
        assert!(!queue.is_empty());
    }

    #[test]
    fn test_work_queue_dispatches_priority_order() {
        let items = vec![
            WorkItem::with_priority("late", 1, 10.0),
            WorkItem::new("default", 1),
            WorkItem::with_priority("first", 1, -1.0),
            WorkItem::with_priority("nan", 1, f32::NAN),
        ];
        let queue = WorkQueue::new(items);

        let mut dispatched = Vec::new();
        while let Some(index) = queue.request_next() {
            dispatched.push(queue.get(index).map(|item| item.data));
        }

        assert_eq!(
            dispatched,
            vec![Some("first"), Some("default"), Some("late"), Some("nan")]
        );
    }

    #[test]
    fn test_work_queue_total_subtasks_saturates() {
        let items = vec![WorkItem::new(1, usize::MAX), WorkItem::new(2, 1)];
        let queue = WorkQueue::new(items);

        assert_eq!(queue.total_subtasks(), usize::MAX);
    }

    #[test]
    fn test_work_queue_dispatch() {
        let items = vec![WorkItem::new(1, 10), WorkItem::new(2, 20)];
        let queue = WorkQueue::new(items);

        assert_eq!(queue.request_next(), Some(0));
        assert_eq!(queue.request_next(), Some(1));
        assert_eq!(queue.request_next(), None);
    }

    #[test]
    fn test_work_queue_dispatch_index_stops_at_length() {
        let items = vec![WorkItem::new(1, 10), WorkItem::new(2, 20)];
        let queue = WorkQueue::new(items);

        assert_eq!(queue.request_next(), Some(0));
        assert_eq!(queue.request_next(), Some(1));
        for _ in 0..8 {
            assert_eq!(queue.request_next(), None);
        }

        assert_eq!(
            queue
                .dispatch_index
                .load(std::sync::atomic::Ordering::Acquire),
            queue.len()
        );
        assert_eq!(queue.progress(), (queue.len(), queue.len()));
    }

    #[test]
    fn test_work_queue_batch_dispatch_clamps_to_remaining_items() {
        let items: Vec<_> = (0..5).map(|i| WorkItem::new(i, 1)).collect();
        let queue = WorkQueue::new(items);

        assert_eq!(queue.request_batch(2), 0..2);
        assert_eq!(queue.progress(), (2, 5));
        assert_eq!(queue.request_batch(16), 2..5);
        assert_eq!(queue.progress(), (5, 5));
        assert_eq!(queue.request_batch(1), 5..5);
    }

    #[test]
    fn test_work_queue_zero_sized_batch_claims_one_item() {
        let items = vec![WorkItem::new(1, 10), WorkItem::new(2, 20)];
        let queue = WorkQueue::new(items);

        assert_eq!(queue.request_batch(0), 0..1);
        assert_eq!(queue.request_next(), Some(1));
        assert_eq!(queue.request_batch(0), 2..2);
    }

    #[test]
    fn test_work_queue_reset() {
        let items = vec![WorkItem::new(1, 10)];
        let queue = WorkQueue::new(items);

        assert_eq!(queue.request_next(), Some(0));
        assert_eq!(queue.request_next(), None);

        queue.reset();
        assert_eq!(queue.request_next(), Some(0));
    }

    #[test]
    fn test_work_group() {
        let group = WorkGroup::new(32, 0, 0);

        assert_eq!(group.size(), 32);
        assert_eq!(group.id(), 0);
        assert!(group.is_dispatcher());

        let non_dispatcher = WorkGroup::new(32, 0, 5);
        assert!(!non_dispatcher.is_dispatcher());
    }

    #[test]
    fn test_work_group_subtask_range() {
        let group = WorkGroup::new(4, 0, 1); // Thread 1 of 4

        let subtasks: Vec<_> = group.subtask_range(10).collect();
        assert_eq!(subtasks, vec![1, 5, 9]); // 1, 1+4, 1+8
    }

    #[test]
    fn test_zero_sized_work_group_is_normalized() {
        let group = WorkGroup::new(0, 0, 0);

        assert_eq!(group.size(), 1);
        assert_eq!(group.subtask_range(3).collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn test_out_of_range_work_group_rank_is_normalized() {
        let group = WorkGroup::new(4, 0, 5);

        assert_eq!(group.thread_rank(), 1);
        assert_eq!(group.subtask_range(10).collect::<Vec<_>>(), vec![1, 5, 9]);
    }

    #[test]
    fn test_work_dispatcher() {
        let items = vec![WorkItem::new(1, 10), WorkItem::new(2, 20)];
        let dispatcher = WorkDispatcher::with_default_group_size(items, 4);

        assert_eq!(dispatcher.num_groups(), 4);
        assert_eq!(dispatcher.group_size(), 32);
    }

    #[test]
    fn test_work_dispatcher_normalizes_zero_group_size() {
        let dispatcher = WorkDispatcher::new(vec![WorkItem::new(1, 1)], 1, 0);

        assert_eq!(dispatcher.group_size(), 1);
    }

    #[test]
    fn test_work_dispatcher_queue_handle_does_not_require_clone_items() {
        #[derive(Debug)]
        struct NonClone(usize);

        let dispatcher = WorkDispatcher::new(vec![WorkItem::new(NonClone(7), 1)], 1, 1);
        let queue = dispatcher.queue_handle();

        assert_eq!(queue.get(0).map(|item| item.data.0), Some(7));
    }

    #[test]
    fn test_dispatch_stats() {
        let items = vec![WorkItem::new(1, 10), WorkItem::new(2, 20)];
        let dispatcher = WorkDispatcher::new(items, 4, 8);

        let stats = dispatcher.stats();
        assert_eq!(stats.total_items, 2);
        assert_eq!(stats.total_subtasks, 30);
        assert_eq!(stats.total_workers(), 32);
        assert!((stats.avg_subtasks_per_worker() - 0.9375).abs() < 0.01);
    }

    #[test]
    fn test_load_balancer() {
        let balancer = LoadBalancer::new(128);

        assert_eq!(balancer.num_workers(), 128);
        assert_eq!(balancer.num_groups(), 4);
    }

    #[test]
    fn test_load_balancer_zero_workers_is_total() {
        let balancer = LoadBalancer::new(0);

        assert_eq!(balancer.num_workers(), 0);
        assert_eq!(balancer.group_size, 1);
        assert_eq!(balancer.num_groups(), 0);
    }

    #[test]
    fn test_load_balancer_zero_custom_group_size_is_normalized() {
        let balancer = LoadBalancer::with_group_size(8, 0);

        assert_eq!(balancer.num_groups(), 8);
    }

    #[test]
    fn test_load_balancer_create_dispatcher() {
        let balancer = LoadBalancer::new(64);
        let items = vec![WorkItem::new(1, 10)];
        let dispatcher = balancer.create_dispatcher(items);

        assert_eq!(dispatcher.num_groups(), 2);
    }

    #[test]
    fn test_estimate_workers() {
        // Small workload
        let workers = LoadBalancer::estimate_workers(10, 5);
        assert!(workers >= 32); // At least one group

        // Large workload
        let workers = LoadBalancer::estimate_workers(1000, 100);
        assert!(workers >= 32);
    }

    #[test]
    fn test_estimate_workers_zero_work_and_overflow_are_total() {
        assert_eq!(LoadBalancer::estimate_workers(0, 100), 0);
        assert_eq!(LoadBalancer::estimate_workers(100, 0), 0);

        let workers = LoadBalancer::estimate_workers(usize::MAX, usize::MAX);
        assert_eq!(workers, usize::MAX);
    }

    #[test]
    fn test_dispatch_stats_saturates_total_workers_and_clamps_completion() {
        let stats = DispatchStats {
            total_items: 10,
            dispatched_items: 20,
            total_subtasks: 100,
            num_groups: usize::MAX,
            group_size: 2,
        };

        assert_eq!(stats.total_workers(), usize::MAX);
        assert_eq!(stats.completion_ratio(), 1.0);
    }

    #[test]
    fn test_concurrent_dispatch() {
        use std::thread;

        let items: Vec<_> = (0..100).map(|i| WorkItem::new(i, 1)).collect();
        let dispatcher = WorkDispatcher::with_default_group_size(items, 4);
        let queue = dispatcher.queue_handle();

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let q = Arc::clone(&queue);
                thread::spawn(move || {
                    let mut count = 0;
                    while q.request_next().is_some() {
                        count += 1;
                    }
                    count
                })
            })
            .collect();

        let total: usize = handles
            .into_iter()
            .map(|h| {
                h.join()
                    .expect("gpu/load_balance.rs: required value was None/Err")
            })
            .sum();
        assert_eq!(total, 100);
    }

    #[test]
    fn test_concurrent_batch_dispatch() {
        use std::sync::Mutex;
        use std::thread;

        let item_count = 257;
        let items: Vec<_> = (0..item_count).map(|i| WorkItem::new(i, 1)).collect();
        let queue = Arc::new(WorkQueue::new(items));
        let seen = Arc::new(Mutex::new(vec![false; item_count]));

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let q = Arc::clone(&queue);
                let seen = Arc::clone(&seen);
                thread::spawn(move || {
                    let mut count = 0;
                    loop {
                        let batch = q.request_batch(7);
                        if batch.start == batch.end {
                            break;
                        }

                        let mut seen = seen
                            .lock()
                            .expect("gpu/load_balance.rs: seen mutex was poisoned");
                        for index in batch {
                            assert!(!seen[index], "work item {index} dispatched twice");
                            seen[index] = true;
                            count += 1;
                        }
                    }
                    count
                })
            })
            .collect();

        let total: usize = handles
            .into_iter()
            .map(|h| {
                h.join()
                    .expect("gpu/load_balance.rs: batch worker thread panicked")
            })
            .sum();

        assert_eq!(total, item_count);
        assert!(seen
            .lock()
            .expect("gpu/load_balance.rs: seen mutex was poisoned")
            .iter()
            .all(|dispatched| *dispatched));
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
        // WorkItem Properties
        // =====================================================================

        /// WorkItem stores data correctly.
        #[test]
        fn work_item_stores_data(data in 0i32..1000, num_subtasks in 0usize..100) {
            let item = WorkItem::new(data, num_subtasks);
            prop_assert_eq!(item.data, data);
            prop_assert_eq!(item.num_subtasks, num_subtasks);
            prop_assert_eq!(item.priority, 0.0);
        }

        /// WorkItem with_priority stores all fields.
        #[test]
        fn work_item_priority(data in 0i32..1000, num_subtasks in 0usize..100, priority in -10.0f32..10.0) {
            let item = WorkItem::with_priority(data, num_subtasks, priority);
            prop_assert_eq!(item.data, data);
            prop_assert_eq!(item.num_subtasks, num_subtasks);
            prop_assert!((item.priority - priority).abs() < 1e-6);
        }

        // =====================================================================
        // WorkQueue Properties
        // =====================================================================

        /// WorkQueue tracks total subtasks correctly.
        #[test]
        fn work_queue_subtask_sum(subtasks in proptest::collection::vec(0usize..50, 1..20)) {
            let items: Vec<_> = subtasks.iter().map(|&s| WorkItem::new(s, s)).collect();
            let expected_total: usize = subtasks.iter().sum();
            let queue = WorkQueue::new(items);

            prop_assert_eq!(queue.total_subtasks(), expected_total);
            prop_assert_eq!(queue.len(), subtasks.len());
        }

        /// WorkQueue dispatches all items exactly once.
        #[test]
        fn work_queue_dispatch_all(num_items in 1usize..50) {
            let items: Vec<_> = (0..num_items).map(|i| WorkItem::new(i, 1)).collect();
            let queue = WorkQueue::new(items);

            let mut dispatched = Vec::new();
            while let Some(idx) = queue.request_next() {
                dispatched.push(idx);
            }

            prop_assert_eq!(dispatched.len(), num_items);

            // All indices 0..num_items should be present exactly once
            dispatched.sort();
            let expected: Vec<_> = (0..num_items).collect();
            prop_assert_eq!(dispatched, expected);
        }

        /// WorkQueue batch dispatches all items exactly once.
        #[test]
        fn work_queue_batch_dispatch_all(num_items in 1usize..50, batch_size in 0usize..16) {
            let items: Vec<_> = (0..num_items).map(|i| WorkItem::new(i, 1)).collect();
            let queue = WorkQueue::new(items);

            let mut dispatched = Vec::new();
            loop {
                let batch = queue.request_batch(batch_size);
                if batch.start == batch.end {
                    break;
                }
                dispatched.extend(batch);
            }

            prop_assert_eq!(dispatched.len(), num_items);
            dispatched.sort();
            let expected: Vec<_> = (0..num_items).collect();
            prop_assert_eq!(dispatched, expected);
            prop_assert_eq!(queue.request_batch(batch_size), num_items..num_items);
        }

        /// WorkQueue reset allows re-dispatch.
        #[test]
        fn work_queue_reset_enables_redispatch(num_items in 1usize..20) {
            let items: Vec<_> = (0..num_items).map(|i| WorkItem::new(i, 1)).collect();
            let queue = WorkQueue::new(items);

            // First dispatch
            while queue.request_next().is_some() {}
            prop_assert!(queue.request_next().is_none());

            // Reset and redispatch
            queue.reset();
            let mut count = 0;
            while queue.request_next().is_some() {
                count += 1;
            }
            prop_assert_eq!(count, num_items);
        }

        /// WorkQueue progress reflects dispatch state.
        #[test]
        fn work_queue_progress_accurate(num_items in 2usize..20, dispatch_count in 0usize..20) {
            let items: Vec<_> = (0..num_items).map(|i| WorkItem::new(i, 1)).collect();
            let queue = WorkQueue::new(items);

            for _ in 0..dispatch_count {
                queue.request_next();
            }

            let (dispatched, total) = queue.progress();
            prop_assert_eq!(total, num_items);
            prop_assert_eq!(dispatched, dispatch_count.min(num_items));
        }

        // =====================================================================
        // WorkGroup Properties
        // =====================================================================

        /// WorkGroup thread 0 is dispatcher.
        #[test]
        fn work_group_thread_zero_is_dispatcher(size in 1usize..64, id in 0usize..100) {
            let group = WorkGroup::new(size, id, 0);
            prop_assert!(group.is_dispatcher());

            for rank in 1..size {
                let non_dispatch = WorkGroup::new(size, id, rank);
                prop_assert!(!non_dispatch.is_dispatcher());
            }
        }

        /// WorkGroup subtask_range covers all subtasks with correct stride.
        #[test]
        fn work_group_subtask_range_covers_all(
            size in 2usize..8,
            num_subtasks in 1usize..50
        ) {
            let mut all_subtasks = std::collections::HashSet::new();

            for rank in 0..size {
                let group = WorkGroup::new(size, 0, rank);
                for subtask in group.subtask_range(num_subtasks) {
                    all_subtasks.insert(subtask);
                }
            }

            // All subtasks 0..num_subtasks should be covered
            let expected: std::collections::HashSet<_> = (0..num_subtasks).collect();
            prop_assert_eq!(all_subtasks, expected);
        }

        /// WorkGroup subtask_range has correct step size.
        #[test]
        fn work_group_subtask_step(size in 2usize..16, rank in 0usize..16, num_subtasks in 10usize..50) {
            prop_assume!(rank < size);
            let group = WorkGroup::new(size, 0, rank);

            let subtasks: Vec<_> = group.subtask_range(num_subtasks).collect();

            // First element should be rank
            if !subtasks.is_empty() {
                prop_assert_eq!(subtasks[0], rank);
            }

            // Consecutive elements should differ by size
            for i in 1..subtasks.len() {
                prop_assert_eq!(subtasks[i] - subtasks[i-1], size);
            }
        }

        // =====================================================================
        // WorkDispatcher Properties
        // =====================================================================

        /// WorkDispatcher stats are consistent.
        #[test]
        fn work_dispatcher_stats_consistent(
            num_items in 1usize..30,
            num_groups in 1usize..8,
            group_size in 1usize..32
        ) {
            let items: Vec<_> = (0..num_items).map(|i| WorkItem::new(i, i + 1)).collect();
            let dispatcher = WorkDispatcher::new(items, num_groups, group_size);

            let stats = dispatcher.stats();
            prop_assert_eq!(stats.total_items, num_items);
            prop_assert_eq!(stats.num_groups, num_groups);
            prop_assert_eq!(stats.group_size, group_size);
            prop_assert_eq!(stats.total_workers(), num_groups * group_size);

            let expected_subtasks: usize = (1..=num_items).sum();
            prop_assert_eq!(stats.total_subtasks, expected_subtasks);
        }

        /// WorkDispatcher completion_ratio progresses correctly.
        #[test]
        fn work_dispatcher_completion_ratio(num_items in 2usize..20) {
            let items: Vec<_> = (0..num_items).map(|i| WorkItem::new(i, 1)).collect();
            let dispatcher = WorkDispatcher::with_default_group_size(items, 1);

            let initial = dispatcher.stats().completion_ratio();
            prop_assert!((initial - 0.0).abs() < 0.01);

            // Dispatch half
            for _ in 0..num_items/2 {
                dispatcher.queue().request_next();
            }

            let half = dispatcher.stats().completion_ratio();
            let expected_half = (num_items / 2) as f64 / num_items as f64;
            prop_assert!((half - expected_half).abs() < 0.01);
        }

        // =====================================================================
        // LoadBalancer Properties
        // =====================================================================

        /// LoadBalancer num_groups is ceiling division.
        #[test]
        fn load_balancer_num_groups(num_workers in 1usize..1000) {
            let balancer = LoadBalancer::new(num_workers);
            let group_size = WORK_GROUP_SIZE.min(num_workers);
            let expected_groups = num_workers.div_ceil(group_size);

            prop_assert_eq!(balancer.num_groups(), expected_groups);
        }

        /// LoadBalancer estimate_workers returns reasonable values.
        #[test]
        fn load_balancer_estimate_reasonable(num_items in 1usize..1000, avg_subtasks in 1usize..100) {
            let workers = LoadBalancer::estimate_workers(num_items, avg_subtasks);
            let total_work = num_items.saturating_mul(avg_subtasks);

            // Should be at least one warp
            prop_assert!(workers >= WORK_GROUP_SIZE.min(total_work));

            // Should be a multiple of group size or at least 1
            prop_assert!(workers >= 1);
        }

        // =====================================================================
        // DispatchStats Properties
        // =====================================================================

        /// DispatchStats avg_subtasks_per_worker is correct.
        #[test]
        fn dispatch_stats_avg_subtasks(
            total_subtasks in 1usize..1000,
            num_groups in 1usize..10,
            group_size in 1usize..32
        ) {
            let stats = DispatchStats {
                total_items: 10,
                dispatched_items: 5,
                total_subtasks,
                num_groups,
                group_size,
            };

            let total_workers = num_groups * group_size;
            let expected_avg = total_subtasks as f64 / total_workers as f64;
            prop_assert!((stats.avg_subtasks_per_worker() - expected_avg).abs() < 1e-10);
        }
    }
}
