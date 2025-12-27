//! Shortest-distance algorithms for WFSTs.
//!
//! This module implements generalized shortest-distance algorithms from Mohri's
//! weighted automata theory. These algorithms compute the total weight of all
//! paths from one state to another, combining path weights using the semiring's
//! ⊕ (plus) operation.
//!
//! # Single-Source Shortest Distance
//!
//! Computes the shortest distance from the initial state to all reachable states.
//! The algorithm generalizes relaxation-based shortest path algorithms like
//! Dijkstra's and Bellman-Ford to arbitrary semirings.
//!
//! # All-Pairs Shortest Distance
//!
//! Computes shortest distances between all pairs of states using a generalized
//! Floyd-Warshall algorithm. Requires a star semiring for handling cycles.
//!
//! # References
//!
//! - Mohri, M. (2002). "Semiring Frameworks and Algorithms for Shortest-Distance Problems"
//! - Mohri, M. (2009). "Weighted Automata Algorithms" in Handbook of Weighted Automata

use crate::semiring::{Semiring, StarSemiring};
use crate::wfst::{StateId, Wfst, NO_STATE};

use super::queue::{
    FifoQueue, QueueType, ShortestDistanceQueue, ShortestFirstQueue, TopologicalQueue,
};

/// Configuration for shortest-distance computation.
#[derive(Clone, Debug)]
pub struct ShortestDistanceConfig {
    /// Queue type to use for state processing.
    pub queue_type: QueueType,
    /// Maximum number of iterations (for convergence checking).
    pub max_iterations: Option<usize>,
    /// Whether the graph is known to be acyclic.
    pub is_acyclic: Option<bool>,
    /// Convergence threshold for approximate equality.
    pub epsilon: f64,
}

impl Default for ShortestDistanceConfig {
    fn default() -> Self {
        Self {
            queue_type: QueueType::Auto,
            max_iterations: None,
            is_acyclic: None,
            epsilon: 1e-10,
        }
    }
}

impl ShortestDistanceConfig {
    /// Create a configuration for acyclic graphs.
    pub fn acyclic() -> Self {
        Self {
            queue_type: QueueType::Topological,
            is_acyclic: Some(true),
            ..Default::default()
        }
    }

    /// Create a configuration for tropical semiring (Dijkstra).
    pub fn tropical() -> Self {
        Self {
            queue_type: QueueType::ShortestFirst,
            ..Default::default()
        }
    }

    /// Create a configuration for general semirings.
    pub fn general() -> Self {
        Self {
            queue_type: QueueType::Fifo,
            ..Default::default()
        }
    }
}

/// Compute single-source shortest distances from the initial state.
///
/// Returns a vector where `result[s]` is the total weight of all paths from
/// the start state to state `s`, combined using the semiring's ⊕ operation.
///
/// # Arguments
///
/// * `fst` - The WFST to compute distances on
/// * `config` - Configuration controlling queue selection and convergence
///
/// # Returns
///
/// A vector of shortest distances indexed by state ID, or `None` if the
/// computation did not converge (e.g., negative cycles in tropical semiring).
///
/// # Example
///
/// ```ignore
/// use lling_llang::algorithms::{single_source_shortest_distance, ShortestDistanceConfig};
///
/// let distances = single_source_shortest_distance(&fst, ShortestDistanceConfig::default());
/// if let Some(dists) = distances {
///     println!("Distance to final state: {:?}", dists[final_state]);
/// }
/// ```
///
/// # Complexity
///
/// - Acyclic + TopologicalQueue: O(|Q| + |E|)
/// - Tropical + ShortestFirstQueue: O(|E| + |Q| log |Q|)
/// - General + FifoQueue: O(C · |E|) where C is path length bound
pub fn single_source_shortest_distance<L, W, F>(
    fst: &F,
    config: ShortestDistanceConfig,
) -> Option<Vec<W>>
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let num_states = fst.num_states();
    if num_states == 0 {
        return Some(Vec::new());
    }

    let start = fst.start();
    if start == NO_STATE || start as usize >= num_states {
        return Some(vec![W::zero(); num_states]);
    }

    // Select queue based on configuration
    match config.queue_type {
        QueueType::Fifo => {
            let queue = FifoQueue::with_capacity(num_states);
            single_source_shortest_distance_impl(fst, queue, &config)
        }
        QueueType::ShortestFirst => {
            let mut queue = ShortestFirstQueue::with_capacity(num_states);
            queue.init_distances(num_states);
            single_source_shortest_distance_impl(fst, queue, &config)
        }
        QueueType::Topological => {
            // Need to compute topological order first
            if let Some(order) = compute_topological_order(fst) {
                let queue = TopologicalQueue::from_order(order);
                single_source_shortest_distance_impl(fst, queue, &config)
            } else {
                // Graph has cycles, fall back to FIFO
                let queue = FifoQueue::with_capacity(num_states);
                single_source_shortest_distance_impl(fst, queue, &config)
            }
        }
        QueueType::Auto => {
            // Try topological first, fall back to appropriate queue
            if let Some(order) = compute_topological_order(fst) {
                let queue = TopologicalQueue::from_order(order);
                single_source_shortest_distance_impl(fst, queue, &config)
            } else {
                // Cyclic graph: use FIFO for general semirings
                let queue = FifoQueue::with_capacity(num_states);
                single_source_shortest_distance_impl(fst, queue, &config)
            }
        }
    }
}

/// Compute single-source shortest distances with an explicit queue type.
///
/// This is a lower-level interface allowing direct queue selection.
pub fn single_source_shortest_distance_with_queue<L, W, F, Q>(
    fst: &F,
    queue: Q,
) -> Option<Vec<W>>
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
    Q: ShortestDistanceQueue<W>,
{
    single_source_shortest_distance_impl(fst, queue, &ShortestDistanceConfig::default())
}

/// Internal implementation of single-source shortest distance.
fn single_source_shortest_distance_impl<L, W, F, Q>(
    fst: &F,
    mut queue: Q,
    config: &ShortestDistanceConfig,
) -> Option<Vec<W>>
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
    Q: ShortestDistanceQueue<W>,
{
    let num_states = fst.num_states();
    let start = fst.start();

    // Initialize distances: all states start at ⊕-identity (zero)
    let mut distance: Vec<W> = vec![W::zero(); num_states];

    // Track the "remainder" - weight to be propagated from each state
    // This is the key insight from Mohri's algorithm: we track what's
    // left to propagate separately from the accumulated distance
    let mut remainder: Vec<W> = vec![W::zero(); num_states];

    // Start state has distance = ⊗-identity (one)
    distance[start as usize] = W::one();
    remainder[start as usize] = W::one();

    // Enqueue start state
    queue.insert(start, &distance[start as usize]);

    let max_iterations = config.max_iterations.unwrap_or(usize::MAX);
    let mut iterations = 0;

    while let Some(state) = queue.pop() {
        iterations += 1;
        if iterations > max_iterations {
            // Did not converge within iteration limit
            return None;
        }

        let state_idx = state as usize;
        let r = remainder[state_idx];

        // Clear remainder for this state
        remainder[state_idx] = W::zero();

        // Skip if no remainder to propagate
        if r.is_zero() {
            continue;
        }

        // Relax all outgoing edges
        for transition in fst.transitions(state) {
            let next_state = transition.to;
            let next_idx = next_state as usize;

            // Compute contribution: remainder ⊗ edge_weight
            let contribution = r.times(&transition.weight);

            // Update distance and remainder for next state
            let old_distance = distance[next_idx];
            let new_distance = old_distance.plus(&contribution);

            // Check if distance actually changed
            if !new_distance.approx_eq(&old_distance, config.epsilon) {
                // Update remainder: add the contribution
                remainder[next_idx] = remainder[next_idx].plus(&contribution);
                distance[next_idx] = new_distance;

                // Enqueue if not already in queue
                queue.update(next_state, &distance[next_idx]);
            }
        }
    }

    Some(distance)
}

/// Compute all-pairs shortest distances using Floyd-Warshall generalization.
///
/// Returns a 2D matrix where `result[i][j]` is the shortest distance from
/// state `i` to state `j`.
///
/// # Requirements
///
/// Requires a star semiring for handling cycles. The star operation
/// computes: `a* = 1̄ ⊕ a ⊕ a² ⊕ a³ ⊕ ...`
///
/// # Arguments
///
/// * `fst` - The WFST to compute distances on
///
/// # Returns
///
/// A 2D vector of shortest distances, or `None` if the star operation
/// does not converge for some cycle weight.
///
/// # Complexity
///
/// Time: Θ(|Q|³(T⊕ + T⊗ + T*))
/// Space: Θ(|Q|²)
pub fn all_pairs_shortest_distance<L, W, F>(fst: &F) -> Option<Vec<Vec<W>>>
where
    L: Clone,
    W: StarSemiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return Some(Vec::new());
    }

    // Initialize distance matrix
    // d[i][j] = weight of direct edge from i to j, or zero if no edge
    let mut d: Vec<Vec<W>> = vec![vec![W::zero(); n]; n];

    // Self-loops have distance one (identity for ⊗)
    for i in 0..n {
        d[i][i] = W::one();
    }

    // Add edge weights
    for state in 0..n as StateId {
        for transition in fst.transitions(state) {
            let from = state as usize;
            let to = transition.to as usize;
            // Combine parallel edges with ⊕
            d[from][to] = d[from][to].plus(&transition.weight);
        }
    }

    // Floyd-Warshall with star operation for cycles
    // d[i][j] = d[i][j] ⊕ (d[i][k] ⊗ d[k][k]* ⊗ d[k][j])
    for k in 0..n {
        // Compute star of d[k][k] for handling cycles through k
        let star_kk = d[k][k].star()?;

        for i in 0..n {
            if d[i][k].is_zero() {
                continue; // No path from i to k
            }

            for j in 0..n {
                if d[k][j].is_zero() {
                    continue; // No path from k to j
                }

                // Contribution through k: d[i][k] ⊗ d[k][k]* ⊗ d[k][j]
                let through_k = d[i][k].times(&star_kk).times(&d[k][j]);
                d[i][j] = d[i][j].plus(&through_k);
            }
        }
    }

    Some(d)
}

/// Compute topological order for a WFST if it's acyclic.
///
/// Returns `None` if the graph contains cycles.
fn compute_topological_order<L, W, F>(fst: &F) -> Option<Vec<StateId>>
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return Some(Vec::new());
    }

    // Compute in-degrees
    let mut in_degree: Vec<usize> = vec![0; n];
    for state in 0..n as StateId {
        for transition in fst.transitions(state) {
            let to = transition.to as usize;
            if to < n {
                in_degree[to] += 1;
            }
        }
    }

    // Kahn's algorithm
    let mut queue: Vec<StateId> = Vec::with_capacity(n);
    let mut result: Vec<StateId> = Vec::with_capacity(n);

    // Start with nodes that have no incoming edges
    for (state, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push(state as StateId);
        }
    }

    while let Some(state) = queue.pop() {
        result.push(state);

        for transition in fst.transitions(state) {
            let next = transition.to as usize;
            if next < n {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    queue.push(next as StateId);
                }
            }
        }
    }

    if result.len() == n {
        Some(result)
    } else {
        // Cycle detected
        None
    }
}

/// Compute the shortest distance to all final states.
///
/// This is a convenience function that computes single-source shortest
/// distances and then combines the distances to all final states.
///
/// # Returns
///
/// The total weight of all paths from the start state to any final state.
pub fn shortest_distance_to_final<L, W, F>(fst: &F, config: ShortestDistanceConfig) -> Option<W>
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let distances = single_source_shortest_distance(fst, config)?;

    let mut total = W::zero();
    for (state, dist) in distances.iter().enumerate() {
        if fst.is_final(state as StateId) {
            // Combine with final weight
            let final_weight = fst.final_weight(state as StateId);
            total = total.plus(&dist.times(&final_weight));
        }
    }

    Some(total)
}

/// Compute reverse shortest distances (from each state to final states).
///
/// This is useful for weight pushing algorithms that need backward distances.
///
/// # Returns
///
/// A vector where `result[s]` is the total weight of all paths from state `s`
/// to any final state.
pub fn reverse_shortest_distance<L, W, F>(fst: &F, config: ShortestDistanceConfig) -> Option<Vec<W>>
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return Some(Vec::new());
    }

    // Build reverse graph adjacency
    let mut reverse_adj: Vec<Vec<(StateId, W)>> = vec![Vec::new(); n];
    for state in 0..n as StateId {
        for transition in fst.transitions(state) {
            let to = transition.to as usize;
            if to < n {
                reverse_adj[to].push((state, transition.weight));
            }
        }
    }

    // Initialize: final states have distance = final_weight
    let mut distance: Vec<W> = vec![W::zero(); n];
    let mut queue = FifoQueue::with_capacity(n);

    for state in 0..n as StateId {
        if fst.is_final(state) {
            distance[state as usize] = fst.final_weight(state);
            queue.insert(state, &distance[state as usize]);
        }
    }

    // Propagate backwards
    let mut remainder: Vec<W> = distance.clone();

    while let Some(state) = queue.pop() {
        let state_idx = state as usize;
        let r = remainder[state_idx];
        remainder[state_idx] = W::zero();

        if r.is_zero() {
            continue;
        }

        // Relax reverse edges
        for &(prev_state, ref weight) in &reverse_adj[state_idx] {
            let prev_idx = prev_state as usize;
            let contribution = weight.times(&r);
            let old_distance = distance[prev_idx];
            let new_distance = old_distance.plus(&contribution);

            if !new_distance.approx_eq(&old_distance, config.epsilon) {
                remainder[prev_idx] = remainder[prev_idx].plus(&contribution);
                distance[prev_idx] = new_distance;
                queue.update(prev_state, &distance[prev_idx]);
            }
        }
    }

    Some(distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::{LogWeight, TropicalWeight};
    use crate::wfst::{MutableWfst, VectorWfst, VectorWfstBuilder};

    fn build_linear_fst(n: usize) -> VectorWfst<char, TropicalWeight> {
        let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::with_capacity(n + 1);
        fst.reserve_states(n + 1);

        for _ in 0..=n {
            fst.add_state();
        }
        fst.set_start(0);
        fst.set_final(n as StateId, TropicalWeight::one());

        for i in 0..n {
            fst.add_arc(
                i as StateId,
                Some('a'),
                Some('a'),
                (i + 1) as StateId,
                TropicalWeight::new(1.0),
            );
        }

        fst
    }

    fn build_diamond_fst() -> VectorWfst<char, TropicalWeight> {
        // Diamond: 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3
        VectorWfstBuilder::new()
            .add_states(4)
            .start(0)
            .final_state(3, TropicalWeight::one())
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
            .arc(0, Some('b'), Some('b'), 2, TropicalWeight::new(2.0))
            .arc(1, Some('c'), Some('c'), 3, TropicalWeight::new(1.0))
            .arc(2, Some('d'), Some('d'), 3, TropicalWeight::new(1.0))
            .build()
    }

    #[test]
    fn test_single_source_linear() {
        let fst = build_linear_fst(3);
        let distances =
            single_source_shortest_distance(&fst, ShortestDistanceConfig::default()).unwrap();

        assert_eq!(distances.len(), 4);
        assert!(distances[0].approx_eq(&TropicalWeight::one(), 1e-10)); // Start
        assert!(distances[1].approx_eq(&TropicalWeight::new(1.0), 1e-10));
        assert!(distances[2].approx_eq(&TropicalWeight::new(2.0), 1e-10));
        assert!(distances[3].approx_eq(&TropicalWeight::new(3.0), 1e-10)); // Final
    }

    #[test]
    fn test_single_source_diamond() {
        let fst = build_diamond_fst();
        let distances =
            single_source_shortest_distance(&fst, ShortestDistanceConfig::default()).unwrap();

        assert_eq!(distances.len(), 4);
        assert!(distances[0].approx_eq(&TropicalWeight::one(), 1e-10));
        assert!(distances[1].approx_eq(&TropicalWeight::new(1.0), 1e-10)); // Via 'a'
        assert!(distances[2].approx_eq(&TropicalWeight::new(2.0), 1e-10)); // Via 'b'
        // State 3: min(1+1, 2+1) = min(2, 3) = 2
        assert!(distances[3].approx_eq(&TropicalWeight::new(2.0), 1e-10));
    }

    #[test]
    fn test_single_source_with_topological_queue() {
        let fst = build_linear_fst(5);
        let distances =
            single_source_shortest_distance(&fst, ShortestDistanceConfig::acyclic()).unwrap();

        assert_eq!(distances.len(), 6);
        for i in 0..6 {
            assert!(distances[i].approx_eq(&TropicalWeight::new(i as f64), 1e-10));
        }
    }

    #[test]
    fn test_shortest_distance_to_final() {
        let fst = build_diamond_fst();
        let total = shortest_distance_to_final(&fst, ShortestDistanceConfig::default()).unwrap();

        // Shortest path weight = 2 (0 -> 1 -> 3)
        // Final weight = 1 (one())
        // Total = 2 + 0 = 2
        assert!(total.approx_eq(&TropicalWeight::new(2.0), 1e-10));
    }

    #[test]
    fn test_all_pairs_simple() {
        // Simple 3-state chain: 0 -> 1 -> 2
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .final_state(2, TropicalWeight::one())
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('b'), Some('b'), 2, TropicalWeight::new(2.0))
            .build();

        let distances = all_pairs_shortest_distance(&fst).unwrap();

        assert_eq!(distances.len(), 3);

        // d[0][0] = 0 (self)
        assert!(distances[0][0].approx_eq(&TropicalWeight::one(), 1e-10));
        // d[0][1] = 1
        assert!(distances[0][1].approx_eq(&TropicalWeight::new(1.0), 1e-10));
        // d[0][2] = 3 (1 + 2)
        assert!(distances[0][2].approx_eq(&TropicalWeight::new(3.0), 1e-10));
        // d[1][2] = 2
        assert!(distances[1][2].approx_eq(&TropicalWeight::new(2.0), 1e-10));
        // d[2][0] = infinity (unreachable)
        assert!(distances[2][0].is_zero());
    }

    #[test]
    fn test_reverse_shortest_distance() {
        let fst = build_linear_fst(3);
        let distances =
            reverse_shortest_distance(&fst, ShortestDistanceConfig::default()).unwrap();

        // Reverse distances from each state to final state 3
        // d[0] = 3 (3 edges)
        // d[1] = 2 (2 edges)
        // d[2] = 1 (1 edge)
        // d[3] = 0 (final state)
        assert!(distances[0].approx_eq(&TropicalWeight::new(3.0), 1e-10));
        assert!(distances[1].approx_eq(&TropicalWeight::new(2.0), 1e-10));
        assert!(distances[2].approx_eq(&TropicalWeight::new(1.0), 1e-10));
        assert!(distances[3].approx_eq(&TropicalWeight::one(), 1e-10));
    }

    #[test]
    fn test_empty_fst() {
        let builder: VectorWfstBuilder<char, TropicalWeight> = VectorWfstBuilder::new();
        let fst = builder.build();

        let distances =
            single_source_shortest_distance(&fst, ShortestDistanceConfig::default()).unwrap();
        assert!(distances.is_empty());

        let all_pairs = all_pairs_shortest_distance(&fst).unwrap();
        assert!(all_pairs.is_empty());
    }

    #[test]
    fn test_log_semiring_shortest_distance() {
        // Test with log semiring for probabilistic interpretation
        // Two paths: 0->1->3 with weight 2, 0->2->3 with weight 3
        let fst: VectorWfst<char, LogWeight> = VectorWfstBuilder::new()
            .add_states(4)
            .start(0)
            .final_state(3, LogWeight::one())
            .arc(0, Some('a'), Some('a'), 1, LogWeight::new(1.0))
            .arc(0, Some('b'), Some('b'), 2, LogWeight::new(2.0))
            .arc(1, Some('c'), Some('c'), 3, LogWeight::new(1.0))
            .arc(2, Some('d'), Some('d'), 3, LogWeight::new(1.0))
            .build();

        let distances =
            single_source_shortest_distance(&fst, ShortestDistanceConfig::default()).unwrap();

        // In log semiring, plus combines probabilities (log-sum-exp)
        // State 3 receives contributions from both paths
        assert!(distances[3].value() < 2.0); // Should be less than either individual path
    }
}
