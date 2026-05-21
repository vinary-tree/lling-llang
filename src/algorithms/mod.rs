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
    compute_accessible, compute_coaccessible, connect, count_useful_states, is_connected,
    ConnectConfig,
};

pub use epsilon_removal::{
    has_epsilon_transitions, remove_epsilon, remove_epsilon_star, EpsilonRemovalConfig,
    EpsilonRemovalError,
};

pub use push::{is_stochastic, push_weights, PushConfig, PushDirection, PushError};

pub use queue::{
    AutoQueue, FifoQueue, QueueType, ShortestDistanceQueue, ShortestFirstQueue, TopologicalQueue,
};

pub use shortest_distance::{
    all_pairs_shortest_distance, reverse_shortest_distance, shortest_distance_to_final,
    single_source_shortest_distance, single_source_shortest_distance_with_queue,
    ShortestDistanceConfig,
};

pub use determinize::{
    determinize, is_deterministic, non_determinism_degree, DeterminizeConfig, DeterminizeError,
};

pub use minimize::{estimate_reduction, minimize, MinimizeConfig, MinimizeError};

pub use sample::{
    estimate_expected_weight, sample_path, sample_paths, sample_paths_until, SampleConfig,
    SampleError, SampleStrategy, SampledPath,
};

pub use rrwm::{Rrwm, RrwmBuilder, RrwmConfig, RrwmError, RrwmStatistics};
