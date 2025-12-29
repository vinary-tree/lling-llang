//! WFST algorithms for shortest-distance, weight pushing, sampling, and optimization.
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
//! - **Path Sampling**:
//!   - [`sample_path`]: Sample random accepting paths from a WFST
//!   - [`sample_paths`]: Sample multiple paths
//!   - [`estimate_expected_weight`]: Monte Carlo weight estimation
//!
//! - **Online Learning**:
//!   - [`Rrwm`]: Rational Randomized Weighted-Majority algorithm
//!
//! - **Epsilon Removal**:
//!   - [`remove_epsilon`]: Remove epsilon transitions preserving language
//!
//! - **Connect (Trim)**:
//!   - [`connect`]: Remove non-useful states
//!
//! - **Determinization**:
//!   - [`determinize`]: Produce deterministic WFST via powerset construction
//!
//! - **Minimization**:
//!   - [`minimize`]: Produce minimal WFST via partition refinement
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
mod determinize;
mod epsilon_removal;
mod minimize;
mod push;
mod queue;
mod rrwm;
mod sample;
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

pub use determinize::{
    determinize,
    is_deterministic,
    non_determinism_degree,
    DeterminizeConfig,
    DeterminizeError,
};

pub use minimize::{
    minimize,
    estimate_reduction,
    MinimizeConfig,
    MinimizeError,
};

pub use sample::{
    sample_path,
    sample_paths,
    sample_paths_until,
    estimate_expected_weight,
    SampleConfig,
    SampleStrategy,
    SampleError,
    SampledPath,
};

pub use rrwm::{
    Rrwm,
    RrwmBuilder,
    RrwmConfig,
    RrwmError,
    RrwmStatistics,
};
