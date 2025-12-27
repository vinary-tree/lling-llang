//! WFST algorithms for shortest-distance, weight pushing, and optimization.
//!
//! This module provides core algorithms from Mohri's weighted automata theory:
//!
//! - **Queue Disciplines**: Different traversal strategies for shortest-distance
//!   - [`FifoQueue`]: General-purpose for k-closed semirings
//!   - [`TopologicalQueue`]: Optimal for acyclic graphs O(|Q| + |E|)
//!   - [`ShortestFirstQueue`]: Dijkstra-style for tropical semiring
//!
//! - **Shortest-Distance Algorithms**:
//!   - [`single_source_shortest_distance`]: From start to all states
//!   - [`all_pairs_shortest_distance`]: Between all state pairs
//!
//! - **Weight Pushing**:
//!   - [`push_weights`]: Redistribute weights toward initial/final states
//!
//! - **Epsilon Removal**:
//!   - [`remove_epsilon`]: Remove epsilon transitions preserving language
//!
//! - **Connect (Trim)**:
//!   - [`connect`]: Remove non-useful states
//!
//! # Queue Selection Guide
//!
//! | Graph Type | Semiring | Recommended Queue | Complexity |
//! |------------|----------|-------------------|------------|
//! | Acyclic | Any | TopologicalQueue | O(|Q| + |E|) |
//! | General | Tropical | ShortestFirstQueue | O(|E| + |Q| log |Q|) |
//! | General | Log/k-closed | FifoQueue | O(C·|E|) |
//!
//! # References
//!
//! - Mohri, M. (2009). "Weighted Automata Algorithms"
//! - Mohri, M., Pereira, F., & Riley, M. (2002). "WFSTs in Speech Recognition"

mod connect;
mod epsilon_removal;
mod push;
mod queue;
mod shortest_distance;

pub use connect::{
    connect,
    compute_accessible,
    compute_coaccessible,
    is_connected,
    count_useful_states,
    ConnectConfig,
};

pub use epsilon_removal::{
    remove_epsilon,
    remove_epsilon_star,
    has_epsilon_transitions,
    EpsilonRemovalConfig,
    EpsilonRemovalError,
};

pub use push::{
    push_weights,
    is_stochastic,
    PushConfig,
    PushDirection,
    PushError,
};

pub use queue::{
    ShortestDistanceQueue,
    FifoQueue,
    TopologicalQueue,
    ShortestFirstQueue,
    AutoQueue,
    QueueType,
};

pub use shortest_distance::{
    single_source_shortest_distance,
    single_source_shortest_distance_with_queue,
    all_pairs_shortest_distance,
    reverse_shortest_distance,
    shortest_distance_to_final,
    ShortestDistanceConfig,
};
