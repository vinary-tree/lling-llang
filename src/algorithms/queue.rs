//! Queue disciplines for shortest-distance algorithms.
//!
//! The choice of queue discipline significantly impacts the efficiency of
//! shortest-distance computation:
//!
//! - **TopologicalQueue**: Best for acyclic graphs, O(|Q| + |E|)
//! - **ShortestFirstQueue**: Best for tropical semiring (Dijkstra), O(|E| + |Q| log |Q|)
//! - **FifoQueue**: General-purpose for k-closed semirings
//!
//! # Theory
//!
//! The generalized shortest-distance algorithm (Mohri, 2002) works by:
//! 1. Maintaining a queue of states to process
//! 2. For each state, relaxing all outgoing edges
//! 3. Enqueueing states whose distances improved
//!
//! The queue discipline determines the order of state processing, which
//! affects both correctness (for non-idempotent semirings) and efficiency.

use std::cmp::Ordering;
use std::collections::VecDeque;
use std::collections::BinaryHeap;

use ordered_float::OrderedFloat;
use rustc_hash::FxHashSet;

use crate::semiring::Semiring;
use crate::wfst::StateId;

/// Queue type enumeration for configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QueueType {
    /// Automatic selection based on graph structure and semiring.
    #[default]
    Auto,
    /// FIFO queue for general k-closed semirings.
    Fifo,
    /// Topological order for acyclic graphs.
    Topological,
    /// Shortest-first (Dijkstra) for tropical semiring.
    ShortestFirst,
}

/// Trait for queue disciplines in shortest-distance algorithms.
///
/// Different queue implementations provide different performance characteristics
/// depending on the semiring and graph structure.
///
/// # Type Parameters
///
/// - `W`: Weight type (must implement [`Semiring`])
pub trait ShortestDistanceQueue<W: Semiring> {
    /// Create a new queue with the given capacity hint.
    fn with_capacity(capacity: usize) -> Self;

    /// Create a new empty queue.
    fn new() -> Self where Self: Sized {
        Self::with_capacity(0)
    }

    /// Insert a state into the queue with its current distance.
    fn insert(&mut self, state: StateId, distance: &W);

    /// Extract the next state to process.
    fn pop(&mut self) -> Option<StateId>;

    /// Update the priority of a state after distance relaxation.
    fn update(&mut self, state: StateId, distance: &W);

    /// Check if the queue is empty.
    fn is_empty(&self) -> bool;

    /// Get the number of states currently in the queue.
    fn len(&self) -> usize;

    /// Check if a state is currently in the queue.
    fn contains(&self, state: StateId) -> bool;

    /// Clear all states from the queue.
    fn clear(&mut self);
}

/// FIFO queue for general k-closed semirings.
///
/// Simple but correct for any semiring. Not optimal for specific cases
/// but guarantees termination for k-closed semirings.
///
/// # Complexity
///
/// - Insert: O(1)
/// - Pop: O(1) amortized
/// - Update: O(1)
/// - Overall shortest-distance: O(C · |E|) where C is path length bound
#[derive(Clone, Debug)]
pub struct FifoQueue {
    queue: VecDeque<StateId>,
    in_queue: FxHashSet<StateId>,
}

impl FifoQueue {
    /// Create a new empty FIFO queue.
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// Create a FIFO queue with the given capacity hint.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity),
            in_queue: FxHashSet::with_capacity_and_hasher(capacity, Default::default()),
        }
    }

    /// Insert a state into the queue.
    pub fn insert_state(&mut self, state: StateId) {
        if self.in_queue.insert(state) {
            self.queue.push_back(state);
        }
    }

    /// Extract the next state to process.
    pub fn pop(&mut self) -> Option<StateId> {
        let state = self.queue.pop_front()?;
        self.in_queue.remove(&state);
        Some(state)
    }

    /// Re-enqueue a state if not already in queue.
    pub fn update_state(&mut self, state: StateId) {
        self.insert_state(state);
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Get the number of states currently in the queue.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Check if a state is currently in the queue.
    pub fn contains(&self, state: StateId) -> bool {
        self.in_queue.contains(&state)
    }

    /// Clear all states from the queue.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.in_queue.clear();
    }
}

impl Default for FifoQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Semiring> ShortestDistanceQueue<W> for FifoQueue {
    fn with_capacity(capacity: usize) -> Self {
        FifoQueue::with_capacity(capacity)
    }

    fn insert(&mut self, state: StateId, _distance: &W) {
        self.insert_state(state);
    }

    fn pop(&mut self) -> Option<StateId> {
        FifoQueue::pop(self)
    }

    fn update(&mut self, state: StateId, _distance: &W) {
        self.update_state(state);
    }

    fn is_empty(&self) -> bool {
        FifoQueue::is_empty(self)
    }

    fn len(&self) -> usize {
        FifoQueue::len(self)
    }

    fn contains(&self, state: StateId) -> bool {
        FifoQueue::contains(self, state)
    }

    fn clear(&mut self) {
        FifoQueue::clear(self)
    }
}

/// Topological queue for acyclic graphs.
///
/// Processes states in topological order, which is optimal for acyclic graphs
/// as each state is visited exactly once.
///
/// # Requirements
///
/// - Graph must be acyclic
/// - Topological order must be computed beforehand
///
/// # Complexity
///
/// - Insert: O(1)
/// - Pop: O(1)
/// - Overall shortest-distance: O(|Q| + |E|)
#[derive(Clone, Debug)]
pub struct TopologicalQueue {
    /// States indexed by topological order
    order: Vec<StateId>,
    /// Current position in the topological order
    current_pos: usize,
    /// Reverse mapping: state -> position in order
    state_to_pos: Vec<usize>,
    /// States that have been enqueued (ready to process)
    enqueued: Vec<bool>,
    /// Number of states currently in queue
    count: usize,
}

impl TopologicalQueue {
    /// Create a new empty topological queue.
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// Create a topological queue with the given capacity hint.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            order: Vec::with_capacity(capacity),
            current_pos: 0,
            state_to_pos: Vec::with_capacity(capacity),
            enqueued: Vec::with_capacity(capacity),
            count: 0,
        }
    }

    /// Create a topological queue initialized with the given order.
    pub fn from_order(order: Vec<StateId>) -> Self {
        let n = order.iter().map(|&s| s as usize + 1).max().unwrap_or(0);
        let mut state_to_pos = vec![usize::MAX; n];
        for (pos, &state) in order.iter().enumerate() {
            state_to_pos[state as usize] = pos;
        }

        Self {
            enqueued: vec![false; order.len()],
            order,
            current_pos: 0,
            state_to_pos,
            count: 0,
        }
    }

    /// Insert a state into the queue.
    pub fn insert_state(&mut self, state: StateId) {
        let idx = state as usize;
        if idx < self.state_to_pos.len() {
            let pos = self.state_to_pos[idx];
            if pos < self.enqueued.len() && !self.enqueued[pos] {
                self.enqueued[pos] = true;
                self.count += 1;
            }
        }
    }

    /// Extract the next state to process.
    pub fn pop(&mut self) -> Option<StateId> {
        while self.current_pos < self.order.len() {
            if self.enqueued[self.current_pos] {
                self.enqueued[self.current_pos] = false;
                self.count -= 1;
                let state = self.order[self.current_pos];
                self.current_pos += 1;
                return Some(state);
            }
            self.current_pos += 1;
        }
        None
    }

    /// Update (re-enqueue) a state.
    pub fn update_state(&mut self, state: StateId) {
        self.insert_state(state);
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Get the number of states currently in the queue.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if a state is currently in the queue.
    pub fn contains(&self, state: StateId) -> bool {
        let idx = state as usize;
        if idx < self.state_to_pos.len() {
            let pos = self.state_to_pos[idx];
            pos < self.enqueued.len() && self.enqueued[pos]
        } else {
            false
        }
    }

    /// Clear all states from the queue.
    pub fn clear(&mut self) {
        for e in &mut self.enqueued {
            *e = false;
        }
        self.current_pos = 0;
        self.count = 0;
    }
}

impl Default for TopologicalQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Semiring> ShortestDistanceQueue<W> for TopologicalQueue {
    fn with_capacity(capacity: usize) -> Self {
        TopologicalQueue::with_capacity(capacity)
    }

    fn insert(&mut self, state: StateId, _distance: &W) {
        self.insert_state(state);
    }

    fn pop(&mut self) -> Option<StateId> {
        TopologicalQueue::pop(self)
    }

    fn update(&mut self, state: StateId, _distance: &W) {
        self.update_state(state);
    }

    fn is_empty(&self) -> bool {
        TopologicalQueue::is_empty(self)
    }

    fn len(&self) -> usize {
        TopologicalQueue::len(self)
    }

    fn contains(&self, state: StateId) -> bool {
        TopologicalQueue::contains(self, state)
    }

    fn clear(&mut self) {
        TopologicalQueue::clear(self)
    }
}

/// Entry in the shortest-first priority queue.
#[derive(Clone, Debug)]
struct ShortestFirstEntry {
    state: StateId,
    /// Negated distance for max-heap to act as min-heap
    neg_distance: OrderedFloat<f64>,
}

impl PartialEq for ShortestFirstEntry {
    fn eq(&self, other: &Self) -> bool {
        self.neg_distance == other.neg_distance
    }
}

impl Eq for ShortestFirstEntry {}

impl PartialOrd for ShortestFirstEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ShortestFirstEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.neg_distance.cmp(&other.neg_distance)
    }
}

/// Shortest-first (Dijkstra-style) queue for tropical semiring.
///
/// Processes states in order of increasing distance, which is optimal
/// for the tropical semiring (min, +).
///
/// # Requirements
///
/// - Weights must be non-negative (for correctness)
/// - Best with tropical semiring
///
/// # Complexity
///
/// - Insert: O(log |Q|)
/// - Pop: O(log |Q|)
/// - Overall shortest-distance: O(|E| + |Q| log |Q|)
#[derive(Clone, Debug)]
pub struct ShortestFirstQueue {
    heap: BinaryHeap<ShortestFirstEntry>,
    in_queue: FxHashSet<StateId>,
    /// Track current best distance for each state to handle stale entries
    distances: Vec<OrderedFloat<f64>>,
}

impl ShortestFirstQueue {
    /// Create a new empty shortest-first queue.
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// Create a shortest-first queue with the given capacity hint.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            heap: BinaryHeap::with_capacity(capacity),
            in_queue: FxHashSet::with_capacity_and_hasher(capacity, Default::default()),
            distances: Vec::with_capacity(capacity),
        }
    }

    /// Initialize the distance tracking array for a given number of states.
    pub fn init_distances(&mut self, num_states: usize) {
        self.distances.resize(num_states, OrderedFloat(f64::INFINITY));
    }

    /// Insert a state with the given distance (as f64).
    pub fn insert_with_distance(&mut self, state: StateId, dist: f64) {
        let idx = state as usize;
        if idx >= self.distances.len() {
            self.distances.resize(idx + 1, OrderedFloat(f64::INFINITY));
        }

        let ord_dist = OrderedFloat(dist);
        if ord_dist < self.distances[idx] {
            self.distances[idx] = ord_dist;
            self.heap.push(ShortestFirstEntry {
                state,
                neg_distance: OrderedFloat(-dist),
            });
            self.in_queue.insert(state);
        }
    }

    /// Extract the next state to process.
    pub fn pop(&mut self) -> Option<StateId> {
        while let Some(entry) = self.heap.pop() {
            let idx = entry.state as usize;
            let expected_dist = OrderedFloat(-entry.neg_distance.0);

            if idx < self.distances.len() && expected_dist == self.distances[idx] {
                self.in_queue.remove(&entry.state);
                return Some(entry.state);
            }
        }
        None
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.in_queue.is_empty()
    }

    /// Get the number of states currently in the queue.
    pub fn len(&self) -> usize {
        self.in_queue.len()
    }

    /// Check if a state is currently in the queue.
    pub fn contains(&self, state: StateId) -> bool {
        self.in_queue.contains(&state)
    }

    /// Clear all states from the queue.
    pub fn clear(&mut self) {
        self.heap.clear();
        self.in_queue.clear();
        for d in &mut self.distances {
            *d = OrderedFloat(f64::INFINITY);
        }
    }
}

impl Default for ShortestFirstQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Semiring> ShortestDistanceQueue<W> for ShortestFirstQueue {
    fn with_capacity(capacity: usize) -> Self {
        ShortestFirstQueue::with_capacity(capacity)
    }

    fn insert(&mut self, state: StateId, distance: &W) {
        let bytes = distance.to_bytes();
        let dist = if bytes.len() >= 8 {
            f64::from_le_bytes(bytes[..8].try_into().unwrap_or([0; 8]))
        } else {
            0.0
        };
        self.insert_with_distance(state, dist);
    }

    fn pop(&mut self) -> Option<StateId> {
        ShortestFirstQueue::pop(self)
    }

    fn update(&mut self, state: StateId, distance: &W) {
        self.insert(state, distance);
    }

    fn is_empty(&self) -> bool {
        ShortestFirstQueue::is_empty(self)
    }

    fn len(&self) -> usize {
        ShortestFirstQueue::len(self)
    }

    fn contains(&self, state: StateId) -> bool {
        ShortestFirstQueue::contains(self, state)
    }

    fn clear(&mut self) {
        ShortestFirstQueue::clear(self)
    }
}

/// Automatic queue selection based on graph properties.
///
/// This wrapper chooses the appropriate queue implementation at runtime
/// based on whether the graph is acyclic and the semiring type.
#[derive(Clone, Debug)]
pub enum AutoQueue {
    /// FIFO queue (fallback)
    Fifo(FifoQueue),
    /// Topological queue (for acyclic graphs)
    Topological(TopologicalQueue),
    /// Shortest-first queue (for tropical semiring)
    ShortestFirst(ShortestFirstQueue),
}

impl Default for AutoQueue {
    fn default() -> Self {
        AutoQueue::Fifo(FifoQueue::default())
    }
}

impl AutoQueue {
    /// Create an automatic queue using topological order if available.
    pub fn with_topological_order(order: Option<Vec<StateId>>) -> Self {
        match order {
            Some(order) => AutoQueue::Topological(TopologicalQueue::from_order(order)),
            None => AutoQueue::Fifo(FifoQueue::default()),
        }
    }

    /// Create a shortest-first queue for Dijkstra-style processing.
    pub fn shortest_first(num_states: usize) -> Self {
        let mut queue = ShortestFirstQueue::with_capacity(num_states);
        queue.init_distances(num_states);
        AutoQueue::ShortestFirst(queue)
    }

    /// Extract the next state to process.
    pub fn pop(&mut self) -> Option<StateId> {
        match self {
            AutoQueue::Fifo(q) => q.pop(),
            AutoQueue::Topological(q) => q.pop(),
            AutoQueue::ShortestFirst(q) => q.pop(),
        }
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            AutoQueue::Fifo(q) => q.is_empty(),
            AutoQueue::Topological(q) => q.is_empty(),
            AutoQueue::ShortestFirst(q) => q.is_empty(),
        }
    }

    /// Get the number of states currently in the queue.
    pub fn len(&self) -> usize {
        match self {
            AutoQueue::Fifo(q) => q.len(),
            AutoQueue::Topological(q) => q.len(),
            AutoQueue::ShortestFirst(q) => q.len(),
        }
    }

    /// Check if a state is currently in the queue.
    pub fn contains(&self, state: StateId) -> bool {
        match self {
            AutoQueue::Fifo(q) => q.contains(state),
            AutoQueue::Topological(q) => q.contains(state),
            AutoQueue::ShortestFirst(q) => q.contains(state),
        }
    }

    /// Clear all states from the queue.
    pub fn clear(&mut self) {
        match self {
            AutoQueue::Fifo(q) => q.clear(),
            AutoQueue::Topological(q) => q.clear(),
            AutoQueue::ShortestFirst(q) => q.clear(),
        }
    }
}

impl<W: Semiring> ShortestDistanceQueue<W> for AutoQueue {
    fn with_capacity(capacity: usize) -> Self {
        AutoQueue::Fifo(FifoQueue::with_capacity(capacity))
    }

    fn insert(&mut self, state: StateId, distance: &W) {
        match self {
            AutoQueue::Fifo(q) => q.insert(state, distance),
            AutoQueue::Topological(q) => q.insert(state, distance),
            AutoQueue::ShortestFirst(q) => q.insert(state, distance),
        }
    }

    fn pop(&mut self) -> Option<StateId> {
        AutoQueue::pop(self)
    }

    fn update(&mut self, state: StateId, distance: &W) {
        match self {
            AutoQueue::Fifo(q) => q.update(state, distance),
            AutoQueue::Topological(q) => q.update(state, distance),
            AutoQueue::ShortestFirst(q) => q.update(state, distance),
        }
    }

    fn is_empty(&self) -> bool {
        AutoQueue::is_empty(self)
    }

    fn len(&self) -> usize {
        AutoQueue::len(self)
    }

    fn contains(&self, state: StateId) -> bool {
        AutoQueue::contains(self, state)
    }

    fn clear(&mut self) {
        AutoQueue::clear(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_fifo_queue_basic() {
        let mut queue = FifoQueue::new();

        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);

        queue.insert_state(0);
        queue.insert_state(1);
        queue.insert_state(2);

        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 3);
        assert!(queue.contains(0));
        assert!(queue.contains(1));
        assert!(queue.contains(2));

        // FIFO order
        assert_eq!(queue.pop(), Some(0));
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), None);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_fifo_queue_no_duplicates() {
        let mut queue = FifoQueue::new();

        queue.insert_state(0);
        queue.insert_state(0); // Duplicate
        queue.insert_state(1);

        assert_eq!(queue.len(), 2); // Only 2 unique states
        assert_eq!(queue.pop(), Some(0));
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn test_topological_queue_basic() {
        // Order: 0 -> 1 -> 2 -> 3
        let mut queue = TopologicalQueue::from_order(vec![0, 1, 2, 3]);

        queue.insert_state(2);
        queue.insert_state(0);
        queue.insert_state(1);

        // Should pop in topological order, not insertion order
        assert_eq!(queue.pop(), Some(0));
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn test_shortest_first_queue_basic() {
        let mut queue = ShortestFirstQueue::new();
        queue.init_distances(10);

        // Insert out of order
        queue.insert_with_distance(0, 5.0);
        queue.insert_with_distance(1, 1.0); // Smallest
        queue.insert_with_distance(2, 3.0);

        // Should pop in distance order (smallest first)
        assert_eq!(queue.pop(), Some(1)); // 1.0
        assert_eq!(queue.pop(), Some(2)); // 3.0
        assert_eq!(queue.pop(), Some(0)); // 5.0
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn test_shortest_first_queue_update() {
        let mut queue = ShortestFirstQueue::new();
        queue.init_distances(10);

        queue.insert_with_distance(0, 5.0);
        queue.insert_with_distance(1, 10.0);

        // Update state 1 to have smaller distance
        queue.insert_with_distance(1, 2.0);

        // Now state 1 should come first
        assert_eq!(queue.pop(), Some(1)); // 2.0 (updated)
        assert_eq!(queue.pop(), Some(0)); // 5.0
    }

    #[test]
    fn test_auto_queue_fifo_fallback() {
        let mut queue: AutoQueue = AutoQueue::with_topological_order(None);

        <AutoQueue as ShortestDistanceQueue<TropicalWeight>>::insert(
            &mut queue, 0, &TropicalWeight::new(1.0));
        <AutoQueue as ShortestDistanceQueue<TropicalWeight>>::insert(
            &mut queue, 1, &TropicalWeight::new(2.0));

        assert_eq!(queue.pop(), Some(0));
        assert_eq!(queue.pop(), Some(1));
    }

    #[test]
    fn test_auto_queue_topological() {
        let mut queue: AutoQueue = AutoQueue::with_topological_order(Some(vec![2, 0, 1]));

        <AutoQueue as ShortestDistanceQueue<TropicalWeight>>::insert(
            &mut queue, 0, &TropicalWeight::new(1.0));
        <AutoQueue as ShortestDistanceQueue<TropicalWeight>>::insert(
            &mut queue, 1, &TropicalWeight::new(2.0));
        <AutoQueue as ShortestDistanceQueue<TropicalWeight>>::insert(
            &mut queue, 2, &TropicalWeight::new(3.0));

        // Should follow topological order: 2, 0, 1
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(0));
        assert_eq!(queue.pop(), Some(1));
    }

    #[test]
    fn test_queue_clear() {
        let mut queue = FifoQueue::new();
        queue.insert_state(0);
        queue.insert_state(1);

        assert!(!queue.is_empty());
        queue.clear();
        assert!(queue.is_empty());
        assert!(!queue.contains(0));
    }
}
