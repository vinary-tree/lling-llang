//! Path sampling from WFSTs.
//!
//! This module provides algorithms for randomly sampling accepting paths from
//! weighted finite-state transducers. Sampling is particularly useful for:
//!
//! - **Monte Carlo methods**: Approximate expectations over path distributions
//! - **Online learning**: RRWM and FPTL algorithms that sample predictions
//! - **Beam search alternatives**: Random exploration of hypothesis space
//! - **Data augmentation**: Generate diverse outputs from WFSTs
//!
//! # Sampling Strategies
//!
//! The module supports different sampling strategies:
//!
//! - **Proportional**: Sample transitions proportional to their weights (requires
//!   a [`StochasticSemiring`] that can be converted to probabilities)
//! - **Uniform**: Sample uniformly from available transitions (ignores weights)
//!
//! # Stochastic vs Non-Stochastic WFSTs
//!
//! For best results, use weight-pushed WFSTs where outgoing weights sum to 1:
//!
//! ```rust,ignore
//! use lling_llang::algorithms::{push_weights, sample_path, PushConfig};
//!
//! // Push weights to make WFST stochastic
//! push_weights(&mut wfst, PushConfig::backward())?;
//!
//! // Sample from the stochastic WFST
//! let path = sample_path(&wfst, SampleConfig::default())?;
//! ```
//!
//! # References
//!
//! - Cortes, C., et al. (2015). "On-Line Learning for Path Experts with
//!   Non-Additive Losses" - RRWM algorithm using path sampling

use rand::{Rng, SeedableRng};
use smallvec::SmallVec;

use crate::semiring::{Semiring, StochasticSemiring};
use crate::wfst::{StateId, WeightedTransition, Wfst, NO_STATE};

/// Configuration for path sampling.
#[derive(Clone, Debug)]
pub struct SampleConfig {
    /// Maximum path length before giving up (prevents infinite loops).
    pub max_length: usize,

    /// Sampling strategy to use.
    pub strategy: SampleStrategy,

    /// Whether to include epsilon labels in the output path.
    pub include_epsilon: bool,

    /// Random seed (None for random seed from entropy).
    pub seed: Option<u64>,
}

impl Default for SampleConfig {
    fn default() -> Self {
        Self {
            max_length: 10_000,
            strategy: SampleStrategy::Proportional,
            include_epsilon: false,
            seed: None,
        }
    }
}

impl SampleConfig {
    /// Create a new config with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum path length.
    pub fn max_length(mut self, length: usize) -> Self {
        self.max_length = length;
        self
    }

    /// Set the sampling strategy.
    pub fn strategy(mut self, strategy: SampleStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set whether to include epsilon labels.
    pub fn include_epsilon(mut self, include: bool) -> Self {
        self.include_epsilon = include;
        self
    }

    /// Set a fixed random seed for reproducibility.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }
}

/// Sampling strategy for choosing transitions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SampleStrategy {
    /// Sample proportional to weights (requires StochasticSemiring).
    ///
    /// For a stochastic WFST (weight-pushed), this gives proper probability sampling.
    /// For non-stochastic WFSTs, weights are normalized on-the-fly.
    #[default]
    Proportional,

    /// Sample uniformly from available transitions (ignores weights).
    ///
    /// Useful for exploration or when weights don't represent probabilities.
    Uniform,
}

/// Error type for sampling operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SampleError {
    /// The WFST is empty (no states).
    EmptyWfst,

    /// No accepting path was found within the maximum length.
    MaxLengthExceeded,

    /// The WFST has no accepting paths (no reachable final states).
    NoAcceptingPaths,

    /// A state has no outgoing transitions and is not final (dead state).
    DeadState(StateId),

    /// All weights are zero at a state (can't sample).
    ZeroWeights(StateId),
}

impl std::fmt::Display for SampleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyWfst => write!(f, "WFST is empty"),
            Self::MaxLengthExceeded => write!(f, "Maximum path length exceeded"),
            Self::NoAcceptingPaths => write!(f, "WFST has no accepting paths"),
            Self::DeadState(s) => write!(f, "Dead state encountered: {}", s),
            Self::ZeroWeights(s) => write!(f, "All weights are zero at state {}", s),
        }
    }
}

impl std::error::Error for SampleError {}

/// A sampled path from a WFST.
#[derive(Clone, Debug)]
pub struct SampledPath<L, W> {
    /// The sequence of input labels along the path.
    pub input_labels: Vec<Option<L>>,

    /// The sequence of output labels along the path.
    pub output_labels: Vec<Option<L>>,

    /// The accumulated weight along the path.
    pub weight: W,

    /// The sequence of states visited.
    pub states: Vec<StateId>,

    /// The number of transitions in the path.
    pub length: usize,
}

impl<L, W: Semiring> SampledPath<L, W> {
    /// Create a new empty path starting at a state.
    fn new(start: StateId) -> Self {
        Self {
            input_labels: Vec::new(),
            output_labels: Vec::new(),
            weight: W::one(),
            states: vec![start],
            length: 0,
        }
    }

    /// Add a transition to the path.
    fn extend(&mut self, trans: &WeightedTransition<L, W>, include_epsilon: bool)
    where
        L: Clone,
    {
        if include_epsilon || trans.input.is_some() {
            self.input_labels.push(trans.input.clone());
        }
        if include_epsilon || trans.output.is_some() {
            self.output_labels.push(trans.output.clone());
        }
        self.weight = self.weight.times(&trans.weight);
        self.states.push(trans.to);
        self.length += 1;
    }

    /// Finalize with the final weight.
    fn finalize(&mut self, final_weight: &W) {
        self.weight = self.weight.times(final_weight);
    }

    /// Get non-epsilon input labels.
    pub fn input_string(&self) -> Vec<&L> {
        self.input_labels
            .iter()
            .filter_map(|l| l.as_ref())
            .collect()
    }

    /// Get non-epsilon output labels.
    pub fn output_string(&self) -> Vec<&L> {
        self.output_labels
            .iter()
            .filter_map(|l| l.as_ref())
            .collect()
    }
}

/// Sample a single random accepting path from a WFST.
///
/// The path is sampled according to the configured strategy:
/// - [`SampleStrategy::Proportional`]: Transitions are chosen proportional to weights
/// - [`SampleStrategy::Uniform`]: Transitions are chosen uniformly at random
///
/// # Arguments
///
/// * `wfst` - The WFST to sample from
/// * `config` - Sampling configuration
///
/// # Returns
///
/// A sampled accepting path, or an error if no accepting path could be found.
///
/// # Example
///
/// ```rust,ignore
/// use lling_llang::algorithms::{sample_path, SampleConfig};
///
/// let path = sample_path(&wfst, SampleConfig::default())?;
/// println!("Sampled output: {:?}", path.output_string());
/// ```
pub fn sample_path<L, W, F>(
    wfst: &F,
    config: SampleConfig,
) -> Result<SampledPath<L, W>, SampleError>
where
    L: Clone,
    W: Semiring + StochasticSemiring,
    F: Wfst<L, W>,
{
    if wfst.is_empty() {
        return Err(SampleError::EmptyWfst);
    }

    let mut rng: Box<dyn rand::RngCore> = match config.seed {
        Some(seed) => Box::new(rand::rngs::StdRng::seed_from_u64(seed)),
        None => Box::new(rand::rng()),
    };

    sample_path_with_rng(wfst, &config, &mut *rng)
}

/// Sample a path using a provided RNG.
fn sample_path_with_rng<L, W, F, R>(
    wfst: &F,
    config: &SampleConfig,
    rng: &mut R,
) -> Result<SampledPath<L, W>, SampleError>
where
    L: Clone,
    W: Semiring + StochasticSemiring,
    F: Wfst<L, W>,
    R: Rng + ?Sized,
{
    let start = wfst.start();
    if start == NO_STATE || !wfst.is_valid_state(start) {
        return Err(SampleError::NoAcceptingPaths);
    }

    let mut path = SampledPath::new(start);
    let mut current = start;
    let num_states = wfst.num_states();

    for _ in 0..config.max_length {
        let transitions = wfst.transitions(current);
        let is_final = wfst.is_final(current);
        let final_weight = wfst.final_weight(current);
        let valid_transition_count = transitions
            .iter()
            .filter(|transition| valid_state_index(transition.to, num_states).is_some())
            .count();

        // If there are no valid outgoing transitions, accept only at a final state.
        if valid_transition_count == 0 {
            if is_final {
                path.finalize(&final_weight);
                return Ok(path);
            } else {
                return Err(SampleError::DeadState(current));
            }
        }

        // Decide whether to stop (if final) or continue
        // We treat stopping as an additional "transition" with the final weight
        let should_stop = if is_final {
            sample_stop_decision(
                transitions,
                &final_weight,
                config.strategy,
                num_states,
                valid_transition_count,
                rng,
            )?
        } else {
            false
        };

        if should_stop {
            path.finalize(&final_weight);
            return Ok(path);
        }

        // Sample a transition
        let trans = sample_transition(current, transitions, config.strategy, num_states, rng)?;
        path.extend(trans, config.include_epsilon);
        current = trans.to;
    }

    if wfst.is_final(current) {
        let final_weight = wfst.final_weight(current);
        path.finalize(&final_weight);
        return Ok(path);
    }

    Err(SampleError::MaxLengthExceeded)
}

/// Decide whether to stop at a final state.
///
/// The stop probability is proportional to the final weight (converted via
/// `StochasticSemiring::to_probability()`) relative to the sum of transition
/// weights plus the final weight.
fn sample_stop_decision<L, W, R>(
    transitions: &[WeightedTransition<L, W>],
    final_weight: &W,
    strategy: SampleStrategy,
    num_states: usize,
    valid_transition_count: usize,
    rng: &mut R,
) -> Result<bool, SampleError>
where
    W: Semiring + StochasticSemiring,
    R: Rng + ?Sized,
{
    match strategy {
        SampleStrategy::Uniform => {
            // Equal chance of stopping vs each transition
            let total_options = valid_transition_count + 1; // +1 for stop
            let stop_index: usize = rng.random_range(0..total_options);
            Ok(stop_index == 0) // Stop if we picked index 0
        }
        SampleStrategy::Proportional => {
            let final_prob = positive_probability(final_weight.to_probability());
            let infinite_transitions = transitions
                .iter()
                .filter(|transition| valid_state_index(transition.to, num_states).is_some())
                .filter(|transition| {
                    matches!(
                        positive_probability(transition.weight.to_probability()),
                        ProbabilityMass::Infinite
                    )
                })
                .count();

            if matches!(final_prob, ProbabilityMass::Infinite) || infinite_transitions > 0 {
                let total_options = infinite_transitions
                    + usize::from(matches!(final_prob, ProbabilityMass::Infinite));
                return Ok(matches!(final_prob, ProbabilityMass::Infinite)
                    && rng.random_range(0..total_options) == 0);
            }

            let final_prob = match final_prob {
                ProbabilityMass::Finite(probability) => probability,
                ProbabilityMass::Infinite | ProbabilityMass::Unavailable => 0.0,
            };
            let trans_sum: f64 = transitions
                .iter()
                .filter(|transition| valid_state_index(transition.to, num_states).is_some())
                .filter_map(|transition| {
                    match positive_probability(transition.weight.to_probability()) {
                        ProbabilityMass::Finite(probability) => Some(probability),
                        ProbabilityMass::Infinite | ProbabilityMass::Unavailable => None,
                    }
                })
                .sum();

            let total = final_prob + trans_sum;
            if total <= 0.0 || !total.is_finite() {
                // Can't sample - this shouldn't happen for well-formed WFSTs
                return Ok(true); // Default to stopping if everything is zero
            }

            let threshold = final_prob / total;
            let r: f64 = rng.random();
            Ok(r < threshold)
        }
    }
}

/// Sample a transition from available transitions.
///
/// Uses `StochasticSemiring::to_probability()` to convert weights for
/// proportional sampling.
fn sample_transition<'a, L, W, R>(
    state: StateId,
    transitions: &'a [WeightedTransition<L, W>],
    strategy: SampleStrategy,
    num_states: usize,
    rng: &mut R,
) -> Result<&'a WeightedTransition<L, W>, SampleError>
where
    W: Semiring + StochasticSemiring,
    R: Rng + ?Sized,
{
    if transitions.is_empty() {
        return Err(SampleError::DeadState(state));
    }

    match strategy {
        SampleStrategy::Uniform => sample_uniform_transition(state, transitions, num_states, rng),
        SampleStrategy::Proportional => {
            let mut infinite_weight_indices: SmallVec<[usize; 8]> = SmallVec::new();
            let mut finite_weights: SmallVec<[(usize, f64); 8]> = SmallVec::new();
            let mut total = 0.0;

            for (index, transition) in transitions.iter().enumerate() {
                if valid_state_index(transition.to, num_states).is_none() {
                    continue;
                }

                match positive_probability(transition.weight.to_probability()) {
                    ProbabilityMass::Finite(probability) => {
                        total += probability;
                        finite_weights.push((index, probability));
                    }
                    ProbabilityMass::Infinite => infinite_weight_indices.push(index),
                    ProbabilityMass::Unavailable => {}
                }
            }

            if !infinite_weight_indices.is_empty() {
                let idx = rng.random_range(0..infinite_weight_indices.len());
                return Ok(&transitions[infinite_weight_indices[idx]]);
            }

            if total <= 0.0 || !total.is_finite() {
                // All usable weights are zero or numerically unusable - fall back to uniform.
                return sample_uniform_transition(state, transitions, num_states, rng);
            }

            // Sample from cumulative distribution
            let r: f64 = rng.random::<f64>() * total;
            let mut cumulative = 0.0;

            for &(index, probability) in &finite_weights {
                cumulative += probability;
                if r < cumulative {
                    return Ok(&transitions[index]);
                }
            }

            // Due to floating point, might reach here - return last
            finite_weights
                .last()
                .map(|(index, _)| &transitions[*index])
                .ok_or(SampleError::DeadState(state))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ProbabilityMass {
    Finite(f64),
    Infinite,
    Unavailable,
}

fn positive_probability(probability: f64) -> ProbabilityMass {
    if probability.is_finite() && probability > 0.0 {
        ProbabilityMass::Finite(probability)
    } else if probability.is_infinite() && probability.is_sign_positive() {
        ProbabilityMass::Infinite
    } else {
        ProbabilityMass::Unavailable
    }
}

fn sample_uniform_transition<'a, L, W, R>(
    state: StateId,
    transitions: &'a [WeightedTransition<L, W>],
    num_states: usize,
    rng: &mut R,
) -> Result<&'a WeightedTransition<L, W>, SampleError>
where
    W: Semiring,
    R: Rng + ?Sized,
{
    let valid_count = transitions
        .iter()
        .filter(|transition| valid_state_index(transition.to, num_states).is_some())
        .count();
    if valid_count == 0 {
        return Err(SampleError::DeadState(state));
    }

    let selected = rng.random_range(0..valid_count);
    transitions
        .iter()
        .filter(|transition| valid_state_index(transition.to, num_states).is_some())
        .nth(selected)
        .ok_or(SampleError::DeadState(state))
}

#[inline]
fn valid_state_index(state: StateId, num_states: usize) -> Option<usize> {
    let index = state as usize;
    (index < num_states).then_some(index)
}

/// Sample multiple random accepting paths from a WFST.
///
/// # Arguments
///
/// * `wfst` - The WFST to sample from
/// * `count` - Number of paths to sample
/// * `config` - Sampling configuration
///
/// # Returns
///
/// A vector of sampled paths. May contain fewer than `count` paths if sampling
/// fails for some attempts.
pub fn sample_paths<L, W, F>(
    wfst: &F,
    count: usize,
    config: SampleConfig,
) -> Vec<Result<SampledPath<L, W>, SampleError>>
where
    L: Clone,
    W: Semiring + StochasticSemiring,
    F: Wfst<L, W>,
{
    if wfst.is_empty() {
        return vec![Err(SampleError::EmptyWfst); count];
    }

    let mut rng: Box<dyn rand::RngCore> = match config.seed {
        Some(seed) => Box::new(rand::rngs::StdRng::seed_from_u64(seed)),
        None => Box::new(rand::rng()),
    };

    (0..count)
        .map(|_| sample_path_with_rng(wfst, &config, &mut *rng))
        .collect()
}

/// Sample paths until a specified number of successful samples are obtained.
///
/// Unlike [`sample_paths`], this continues sampling until the desired number
/// of successful paths is obtained or a maximum number of attempts is exceeded.
///
/// # Arguments
///
/// * `wfst` - The WFST to sample from
/// * `target` - Target number of successful samples
/// * `max_attempts` - Maximum number of sampling attempts
/// * `config` - Sampling configuration
///
/// # Returns
///
/// A vector of successfully sampled paths (at most `target` paths).
pub fn sample_paths_until<L, W, F>(
    wfst: &F,
    target: usize,
    max_attempts: usize,
    config: SampleConfig,
) -> Vec<SampledPath<L, W>>
where
    L: Clone,
    W: Semiring + StochasticSemiring,
    F: Wfst<L, W>,
{
    if wfst.is_empty() {
        return Vec::new();
    }

    let mut rng: Box<dyn rand::RngCore> = match config.seed {
        Some(seed) => Box::new(rand::rngs::StdRng::seed_from_u64(seed)),
        None => Box::new(rand::rng()),
    };

    let mut paths = Vec::with_capacity(target);
    let mut attempts = 0;

    while paths.len() < target && attempts < max_attempts {
        if let Ok(path) = sample_path_with_rng(wfst, &config, &mut *rng) {
            paths.push(path);
        }
        attempts += 1;
    }

    paths
}

/// Estimate the expected weight of accepting paths via Monte Carlo sampling.
///
/// This is useful for approximating the total weight (partition function) or
/// expected path weights when exact computation is intractable.
///
/// # Arguments
///
/// * `wfst` - The WFST to sample from
/// * `num_samples` - Number of samples for the estimate
/// * `config` - Sampling configuration
///
/// # Returns
///
/// The estimated expected weight, or None if no samples could be obtained.
pub fn estimate_expected_weight<L, W, F>(
    wfst: &F,
    num_samples: usize,
    config: SampleConfig,
) -> Option<f64>
where
    L: Clone,
    W: Semiring + StochasticSemiring,
    F: Wfst<L, W>,
{
    if wfst.is_empty() || num_samples == 0 {
        return None;
    }

    let paths = sample_paths_until(wfst, num_samples, num_samples * 10, config);

    if paths.is_empty() {
        return None;
    }

    let total: f64 = paths.iter().map(|p| p.weight.to_probability()).sum();
    Some(total / paths.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::{MutableWfst, VectorWfst};

    fn make_simple_wfst() -> VectorWfst<char, TropicalWeight> {
        // Simple WFST: 0 --a:a/1.0--> 1 --b:b/1.0--> 2 (final, weight 0)
        let mut wfst = VectorWfst::new();
        let s0 = wfst.add_state();
        let s1 = wfst.add_state();
        let s2 = wfst.add_state();

        wfst.set_start(s0);
        wfst.set_final(s2, TropicalWeight::new(0.0));

        wfst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));
        wfst.add_arc(s1, Some('b'), Some('b'), s2, TropicalWeight::new(1.0));

        wfst
    }

    fn make_branching_wfst() -> VectorWfst<char, TropicalWeight> {
        // Branching WFST with two paths:
        // 0 --a:x/1.0--> 1 (final)
        // 0 --b:y/2.0--> 2 (final)
        let mut wfst = VectorWfst::new();
        let s0 = wfst.add_state();
        let s1 = wfst.add_state();
        let s2 = wfst.add_state();

        wfst.set_start(s0);
        wfst.set_final(s1, TropicalWeight::new(0.0));
        wfst.set_final(s2, TropicalWeight::new(0.0));

        wfst.add_arc(s0, Some('a'), Some('x'), s1, TropicalWeight::new(1.0));
        wfst.add_arc(s0, Some('b'), Some('y'), s2, TropicalWeight::new(2.0));

        wfst
    }

    fn make_epsilon_wfst() -> VectorWfst<char, TropicalWeight> {
        let mut wfst = VectorWfst::new();
        let s0 = wfst.add_state();
        let s1 = wfst.add_state();
        let s2 = wfst.add_state();

        wfst.set_start(s0);
        wfst.set_final(s2, TropicalWeight::one());
        wfst.add_arc(s0, None, None, s1, TropicalWeight::one());
        wfst.add_arc(s1, Some('a'), Some('b'), s2, TropicalWeight::one());

        wfst
    }

    fn make_start_final_wfst() -> VectorWfst<char, TropicalWeight> {
        let mut wfst = VectorWfst::new();
        let s0 = wfst.add_state();

        wfst.set_start(s0);
        wfst.set_final(s0, TropicalWeight::one());

        wfst
    }

    #[test]
    fn test_sample_simple_path() {
        let wfst = make_simple_wfst();
        let config = SampleConfig::default().seed(42);

        let path = sample_path(&wfst, config).expect("Should sample a path");

        assert_eq!(path.input_string(), vec![&'a', &'b']);
        assert_eq!(path.output_string(), vec![&'a', &'b']);
        assert_eq!(path.length, 2);
        assert_eq!(path.states.len(), 3);
    }

    #[test]
    fn test_sample_accepts_start_final_with_zero_max_length() {
        let wfst = make_start_final_wfst();
        let config = SampleConfig::default().max_length(0).seed(42);

        let path = sample_path(&wfst, config).expect("zero-length accepting path should sample");

        assert_eq!(path.length, 0);
        assert_eq!(path.states, vec![0]);
        assert!(path.input_labels.is_empty());
        assert!(path.output_labels.is_empty());
    }

    #[test]
    fn test_sample_accepts_final_reached_at_max_length() {
        let wfst = make_simple_wfst();
        let config = SampleConfig::default().max_length(2).seed(42);

        let path = sample_path(&wfst, config).expect("path at max length should sample");

        assert_eq!(path.length, 2);
        assert_eq!(path.states, vec![0, 1, 2]);
        assert_eq!(path.input_string(), vec![&'a', &'b']);
    }

    #[test]
    fn test_sample_rejects_non_final_after_max_length() {
        let wfst = make_simple_wfst();
        let config = SampleConfig::default().max_length(1).seed(42);

        let result = sample_path(&wfst, config);

        assert!(matches!(result, Err(SampleError::MaxLengthExceeded)));
    }

    #[test]
    fn test_sample_uniform() {
        let wfst = make_branching_wfst();
        let config = SampleConfig::default()
            .strategy(SampleStrategy::Uniform)
            .seed(42);

        // Sample many paths and check distribution
        let paths = sample_paths_until(&wfst, 100, 1000, config);

        let a_count = paths
            .iter()
            .filter(|p| p.input_string() == vec![&'a'])
            .count();
        let b_count = paths
            .iter()
            .filter(|p| p.input_string() == vec![&'b'])
            .count();

        assert!(a_count > 0, "Should sample 'a' path");
        assert!(b_count > 0, "Should sample 'b' path");
    }

    #[test]
    fn test_sample_reproducible() {
        let wfst = make_branching_wfst();

        let config1 = SampleConfig::default().seed(12345);
        let config2 = SampleConfig::default().seed(12345);

        let path1 = sample_path(&wfst, config1).expect("Should sample");
        let path2 = sample_path(&wfst, config2).expect("Should sample");

        assert_eq!(path1.input_string(), path2.input_string());
    }

    #[test]
    fn test_sample_empty_wfst() {
        let wfst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        let config = SampleConfig::default();

        let result = sample_path(&wfst, config);
        assert!(matches!(result, Err(SampleError::EmptyWfst)));
    }

    #[test]
    fn test_sample_dead_state() {
        let mut wfst = VectorWfst::<char, TropicalWeight>::new();
        let s0 = wfst.add_state();
        let s1 = wfst.add_state();

        wfst.set_start(s0);
        // s1 is not final and has no transitions - dead state
        wfst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));

        let config = SampleConfig::default().seed(42);
        let result = sample_path(&wfst, config);

        assert!(matches!(result, Err(SampleError::DeadState(_))));
    }

    #[test]
    fn test_sample_transition_empty_slice_reports_dead_state() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let transitions: Vec<WeightedTransition<char, TropicalWeight>> = Vec::new();

        let result = sample_transition(
            42,
            &transitions,
            SampleStrategy::Proportional,
            100,
            &mut rng,
        );

        assert!(matches!(result, Err(SampleError::DeadState(42))));
    }

    #[test]
    fn test_sample_invalid_start_reports_no_accepting_paths() {
        let mut wfst = VectorWfst::<char, TropicalWeight>::new();
        wfst.add_state();

        let result = sample_path(&wfst, SampleConfig::default().seed(42));

        assert!(matches!(result, Err(SampleError::NoAcceptingPaths)));
    }

    #[test]
    fn test_sample_ignores_invalid_transition_targets() {
        let mut wfst = make_simple_wfst();
        wfst.add_arc(0, Some('x'), Some('x'), 99, TropicalWeight::new(0.0));

        for seed in 0..32 {
            let path = sample_path(&wfst, SampleConfig::default().seed(seed))
                .expect("valid path should remain sampleable");

            assert_eq!(path.states, vec![0, 1, 2]);
            assert_eq!(path.input_string(), vec![&'a', &'b']);
        }
    }

    #[test]
    fn test_sample_all_invalid_transitions_from_non_final_state_is_dead_state() {
        let mut wfst = VectorWfst::<char, TropicalWeight>::new();
        let s0 = wfst.add_state();
        wfst.set_start(s0);
        wfst.add_arc(s0, Some('x'), Some('x'), 99, TropicalWeight::one());

        let result = sample_path(&wfst, SampleConfig::default().seed(42));

        assert!(matches!(result, Err(SampleError::DeadState(0))));
    }

    #[test]
    fn test_sample_excludes_epsilon_labels_by_default() {
        let wfst = make_epsilon_wfst();

        let path = sample_path(&wfst, SampleConfig::default().seed(42))
            .expect("epsilon path should be sampleable");

        assert_eq!(path.length, 2);
        assert_eq!(path.input_labels, vec![Some('a')]);
        assert_eq!(path.output_labels, vec![Some('b')]);
    }

    #[test]
    fn test_sample_can_include_epsilon_labels() {
        let wfst = make_epsilon_wfst();

        let path = sample_path(
            &wfst,
            SampleConfig::default().include_epsilon(true).seed(42),
        )
        .expect("epsilon path should be sampleable");

        assert_eq!(path.length, 2);
        assert_eq!(path.input_labels, vec![None, Some('a')]);
        assert_eq!(path.output_labels, vec![None, Some('b')]);
    }

    #[test]
    fn test_sample_multiple_paths() {
        let wfst = make_branching_wfst();
        let config = SampleConfig::default().seed(42);

        let results = sample_paths(&wfst, 10, config);

        assert_eq!(results.len(), 10);
        let success_count = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(success_count, 10);
    }

    #[test]
    fn test_estimate_expected_weight() {
        let wfst = make_simple_wfst();
        let config = SampleConfig::default().seed(42);

        let expected = estimate_expected_weight(&wfst, 100, config);

        assert!(expected.is_some());
        // The only path has weight 1.0 + 1.0 + 0.0 = 2.0 (tropical semiring)
        // But in tropical, times is +, so the weight accumulates as 0+1+1+0 = 2
        let e = expected.expect("algorithms/sample.rs: required value was None/Err");
        assert!(e > 0.0, "Expected weight should be positive");
    }

    #[test]
    fn test_sampled_path_methods() {
        let wfst = make_simple_wfst();
        let config = SampleConfig::default().seed(42);

        let path = sample_path(&wfst, config).expect("Should sample");

        // Test convenience methods
        let input = path.input_string();
        let output = path.output_string();

        assert_eq!(input.len(), 2);
        assert_eq!(output.len(), 2);
        assert_eq!(*input[0], 'a');
        assert_eq!(*output[1], 'b');
    }
}
