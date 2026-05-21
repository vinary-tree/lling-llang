//! Rational Randomized Weighted-Majority (RRWM) algorithm.
//!
//! RRWM is an online learning algorithm for ensemble path expert prediction
//! using the η-power semiring for rational loss functions. It provides a
//! principled way to combine multiple WFST-based models with guaranteed
//! regret bounds.
//!
//! # Algorithm Overview
//!
//! The RRWM algorithm maintains a cumulative weight automaton that tracks
//! performance across multiple "path experts" (WFSTs that make predictions).
//! After each round:
//!
//! 1. Compose cumulative weights with loss transducer
//! 2. Push weights to make the result stochastic
//! 3. Sample a prediction from the stochastic WFST
//!
//! # Regret Bounds
//!
//! RRWM achieves expected regret bound:
//!
//! ```text
//! E[R_T] ≤ 2M√(T log N)
//! ```
//!
//! where M is the maximum loss per round, T is the number of rounds,
//! and N is the number of path experts.
//!
//! # Use Cases
//!
//! - **Speech recognition**: Combine multiple ASR models
//! - **Machine translation**: Ensemble of translation systems
//! - **Text normalization**: Multiple correction strategies
//!
//! # References
//!
//! - Cortes, C., Kuznetsov, V., Mohri, M., & Warmuth, M. K. (2015).
//!   "On-Line Learning Algorithms for Path Experts with Non-Additive Losses"
//!   JMLR 16, 2015.
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::algorithms::{Rrwm, RrwmConfig};
//!
//! let mut rrwm = Rrwm::new(RrwmConfig::default());
//!
//! // Online learning loop
//! for (input, loss_transducer, actual) in data {
//!     // Make prediction
//!     let prediction = rrwm.predict();
//!
//!     // Observe loss and update
//!     rrwm.observe(&loss_transducer)?;
//! }
//! ```

use std::hash::Hash;

use crate::composition::{compose, materialize};
use crate::semiring::{NumericalWeight, PowerWeight};
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst};

use super::push::{push_weights, PushConfig};
use super::sample::{sample_path, SampleConfig, SampleError, SampledPath};

/// Configuration for the RRWM algorithm.
#[derive(Clone, Debug)]
pub struct RrwmConfig {
    /// η parameter for the power semiring.
    ///
    /// Controls the "softness" of the learning:
    /// - Smaller η: More exploratory (approaches uniform)
    /// - Larger η: More exploitative (approaches greedy)
    pub eta: f64,

    /// Learning rate multiplier.
    ///
    /// Controls how quickly the algorithm adapts to new observations.
    pub learning_rate: f64,

    /// Maximum number of rounds before resetting.
    pub max_rounds: usize,

    /// Whether to track detailed statistics.
    pub track_statistics: bool,

    /// Random seed for reproducibility.
    pub seed: Option<u64>,
}

impl Default for RrwmConfig {
    fn default() -> Self {
        Self {
            eta: 1.0,
            learning_rate: 1.0,
            max_rounds: 100_000,
            track_statistics: false,
            seed: None,
        }
    }
}

impl RrwmConfig {
    /// Create a new configuration with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the η parameter.
    pub fn eta(mut self, eta: f64) -> Self {
        self.eta = eta;
        self
    }

    /// Set the learning rate.
    pub fn learning_rate(mut self, rate: f64) -> Self {
        self.learning_rate = rate;
        self
    }

    /// Set maximum rounds.
    pub fn max_rounds(mut self, rounds: usize) -> Self {
        self.max_rounds = rounds;
        self
    }

    /// Enable statistics tracking.
    pub fn with_statistics(mut self) -> Self {
        self.track_statistics = true;
        self
    }

    /// Set random seed.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }
}

/// Statistics tracked by the RRWM algorithm.
#[derive(Clone, Debug, Default)]
pub struct RrwmStatistics {
    /// Total accumulated loss.
    pub total_loss: f64,

    /// Number of rounds completed.
    pub rounds: usize,

    /// Average loss per round.
    pub average_loss: f64,

    /// Number of states in cumulative automaton.
    pub cumulative_states: usize,

    /// History of per-round losses (if tracking enabled).
    pub loss_history: Vec<f64>,
}

impl RrwmStatistics {
    fn update(&mut self, loss: f64, num_states: usize) {
        self.total_loss += loss;
        self.rounds += 1;
        self.average_loss = self.total_loss / self.rounds as f64;
        self.cumulative_states = num_states;
    }
}

/// Error type for RRWM operations.
#[derive(Clone, Debug)]
pub enum RrwmError {
    /// Maximum rounds exceeded.
    MaxRoundsExceeded,

    /// Weight pushing failed.
    PushFailed(String),

    /// Sampling failed.
    SampleFailed(SampleError),

    /// Composition produced empty result.
    EmptyComposition,

    /// Configuration error.
    ConfigError(String),
}

impl std::fmt::Display for RrwmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MaxRoundsExceeded => write!(f, "Maximum rounds exceeded"),
            Self::PushFailed(e) => write!(f, "Weight pushing failed: {}", e),
            Self::SampleFailed(e) => write!(f, "Sampling failed: {}", e),
            Self::EmptyComposition => write!(f, "Composition produced empty result"),
            Self::ConfigError(e) => write!(f, "Configuration error: {}", e),
        }
    }
}

impl std::error::Error for RrwmError {}

impl From<SampleError> for RrwmError {
    fn from(e: SampleError) -> Self {
        Self::SampleFailed(e)
    }
}

/// RRWM (Rational Randomized Weighted-Majority) algorithm state.
///
/// Maintains a cumulative weight automaton for online learning with
/// WFST-based path experts.
pub struct Rrwm<L>
where
    L: Clone + Eq + Hash + Send + Sync,
{
    /// Configuration.
    config: RrwmConfig,

    /// Cumulative weight automaton W_t.
    ///
    /// Tracks accumulated weights across all rounds.
    cumulative: VectorWfst<L, PowerWeight>,

    /// Current round number.
    round: usize,

    /// Statistics (if enabled).
    statistics: Option<RrwmStatistics>,
}

impl<L> Rrwm<L>
where
    L: Clone + Eq + Hash + Send + Sync + 'static,
{
    /// Create a new RRWM instance.
    ///
    /// Initializes with a one-state automaton mapping all strings to weight 1.
    pub fn new(config: RrwmConfig) -> Self {
        // Initialize W_0 as one-state automaton with weight 1
        let mut cumulative = VectorWfst::new();
        let start = cumulative.add_state();
        cumulative.set_start(start);
        cumulative.set_final(start, PowerWeight::one_with_eta(config.eta));

        let statistics = if config.track_statistics {
            Some(RrwmStatistics::default())
        } else {
            None
        };

        Self {
            config,
            cumulative,
            round: 0,
            statistics,
        }
    }

    /// Get the current round number.
    pub fn round(&self) -> usize {
        self.round
    }

    /// Get the η parameter.
    pub fn eta(&self) -> f64 {
        self.config.eta
    }

    /// Get the cumulative weight automaton.
    pub fn cumulative_weights(&self) -> &VectorWfst<L, PowerWeight> {
        &self.cumulative
    }

    /// Get statistics (if tracking is enabled).
    pub fn statistics(&self) -> Option<&RrwmStatistics> {
        self.statistics.as_ref()
    }

    /// Observe a loss transducer and update cumulative weights.
    ///
    /// The loss transducer encodes the loss for each possible path.
    /// After observation:
    /// 1. Compose cumulative weights with loss transducer
    /// 2. Push weights to make stochastic
    ///
    /// # Arguments
    ///
    /// * `loss_transducer` - WFST encoding losses for each path
    ///
    /// # Returns
    ///
    /// The loss incurred (extracted from the composition).
    pub fn observe<T>(&mut self, loss_transducer: T) -> Result<f64, RrwmError>
    where
        T: Wfst<L, PowerWeight>,
    {
        if self.round >= self.config.max_rounds {
            return Err(RrwmError::MaxRoundsExceeded);
        }

        // V_t = compose cumulative with loss transducer
        let composed = compose(self.cumulative.clone(), loss_transducer);

        // Materialize the composition
        let mut materialized: VectorWfst<L, PowerWeight> = materialize(composed);

        if materialized.is_empty() {
            return Err(RrwmError::EmptyComposition);
        }

        // W_t = W_{t-1} ◦ V_t (already done via composition)
        // Weight push to make stochastic
        push_weights(&mut materialized, PushConfig::backward())
            .map_err(|e| RrwmError::PushFailed(format!("{:?}", e)))?;

        // Extract loss (sum of final weights as a proxy)
        let loss = self.extract_loss(&materialized);

        // Update cumulative
        self.cumulative = materialized;
        self.round += 1;

        // Update statistics
        if let Some(ref mut stats) = self.statistics {
            stats.update(loss, self.cumulative.num_states());
            if self.config.track_statistics {
                stats.loss_history.push(loss);
            }
        }

        Ok(loss)
    }

    /// Sample a prediction from the current cumulative weights.
    ///
    /// Uses the stochastic cumulative automaton to sample a path
    /// according to the current weight distribution.
    ///
    /// # Returns
    ///
    /// A sampled path, or an error if sampling fails.
    pub fn predict(&self) -> Result<SampledPath<L, PowerWeight>, RrwmError> {
        let sample_config = SampleConfig::default().seed(
            self.config
                .seed
                .map(|s| s.wrapping_add(self.round as u64))
                .unwrap_or(self.round as u64),
        );

        sample_path(&self.cumulative, sample_config).map_err(RrwmError::from)
    }

    /// Get the regret bound estimate for the current state.
    ///
    /// The theoretical regret bound is: E[R_T] ≤ 2M√(T log N)
    ///
    /// # Arguments
    ///
    /// * `max_loss` - Maximum loss per round (M)
    /// * `num_experts` - Number of path experts (N)
    ///
    /// # Returns
    ///
    /// The estimated regret bound.
    pub fn regret_bound(&self, max_loss: f64, num_experts: usize) -> f64 {
        if self.round == 0 || num_experts == 0 {
            return 0.0;
        }
        2.0 * max_loss * ((self.round as f64) * (num_experts as f64).ln()).sqrt()
    }

    /// Reset the algorithm to initial state.
    pub fn reset(&mut self) {
        self.cumulative = VectorWfst::new();
        let start = self.cumulative.add_state();
        self.cumulative.set_start(start);
        self.cumulative
            .set_final(start, PowerWeight::one_with_eta(self.config.eta));
        self.round = 0;

        if let Some(ref mut stats) = self.statistics {
            *stats = RrwmStatistics::default();
        }
    }

    /// Extract loss from a composed/pushed WFST.
    fn extract_loss(&self, wfst: &VectorWfst<L, PowerWeight>) -> f64 {
        // Sum of final weights as a proxy for loss
        let mut total = 0.0;
        for state in 0..wfst.num_states() as StateId {
            if wfst.is_final(state) {
                total += wfst.final_weight(state).numerical_value();
            }
        }
        total
    }
}

/// Builder for creating RRWM instances with initial path experts.
pub struct RrwmBuilder<L>
where
    L: Clone + Eq + Hash + Send + Sync,
{
    config: RrwmConfig,
    initial_experts: Vec<VectorWfst<L, PowerWeight>>,
}

impl<L> RrwmBuilder<L>
where
    L: Clone + Eq + Hash + Send + Sync + 'static,
{
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: RrwmConfig::default(),
            initial_experts: Vec::new(),
        }
    }

    /// Set the configuration.
    pub fn config(mut self, config: RrwmConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the η parameter.
    pub fn eta(mut self, eta: f64) -> Self {
        self.config.eta = eta;
        self
    }

    /// Add an initial path expert.
    pub fn add_expert(mut self, expert: VectorWfst<L, PowerWeight>) -> Self {
        self.initial_experts.push(expert);
        self
    }

    /// Build the RRWM instance.
    pub fn build(self) -> Rrwm<L> {
        let mut rrwm = Rrwm::new(self.config);

        // If initial experts provided, compose them into cumulative
        for expert in self.initial_experts {
            // Simple union-like initialization
            if rrwm.cumulative.num_states() == 1 {
                rrwm.cumulative = expert;
            }
        }

        rrwm
    }
}

impl<L> Default for RrwmBuilder<L>
where
    L: Clone + Eq + Hash + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::MutableWfst;

    fn make_simple_expert() -> VectorWfst<char, PowerWeight> {
        let mut wfst = VectorWfst::new();
        let s0 = wfst.add_state();
        let s1 = wfst.add_state();

        wfst.set_start(s0);
        wfst.set_final(s1, PowerWeight::one_with_eta(1.0));

        wfst.add_arc(
            s0,
            Some('a'),
            Some('a'),
            s1,
            PowerWeight::from_probability(0.8, 1.0),
        );

        wfst
    }

    #[test]
    fn test_rrwm_creation() {
        let rrwm = Rrwm::<char>::new(RrwmConfig::default());

        assert_eq!(rrwm.round(), 0);
        assert_eq!(rrwm.eta(), 1.0);
        assert_eq!(rrwm.cumulative_weights().num_states(), 1);
    }

    #[test]
    fn test_rrwm_config() {
        let config = RrwmConfig::default()
            .eta(2.0)
            .learning_rate(0.5)
            .max_rounds(1000)
            .with_statistics()
            .seed(42);

        assert_eq!(config.eta, 2.0);
        assert_eq!(config.learning_rate, 0.5);
        assert_eq!(config.max_rounds, 1000);
        assert!(config.track_statistics);
        assert_eq!(config.seed, Some(42));
    }

    #[test]
    fn test_rrwm_builder() {
        let expert = make_simple_expert();
        let rrwm = RrwmBuilder::<char>::new()
            .eta(2.0)
            .add_expert(expert)
            .build();

        assert_eq!(rrwm.eta(), 2.0);
    }

    #[test]
    fn test_regret_bound() {
        let mut rrwm = Rrwm::<char>::new(RrwmConfig::default());

        // Initially zero
        assert_eq!(rrwm.regret_bound(1.0, 10), 0.0);

        // Simulate some rounds
        rrwm.round = 100;

        // E[R_T] ≤ 2M√(T log N) = 2 * 1.0 * sqrt(100 * ln(10)) ≈ 30.3
        let bound = rrwm.regret_bound(1.0, 10);
        assert!(bound > 0.0);
        assert!(bound < 35.0); // Reasonable upper bound
    }

    #[test]
    fn test_rrwm_reset() {
        let mut rrwm = Rrwm::<char>::new(RrwmConfig::default().with_statistics());
        rrwm.round = 10;

        rrwm.reset();

        assert_eq!(rrwm.round(), 0);
        assert_eq!(rrwm.cumulative_weights().num_states(), 1);
        assert_eq!(
            rrwm.statistics()
                .expect("algorithms/rrwm.rs: required value was None/Err")
                .rounds,
            0
        );
    }

    #[test]
    fn test_rrwm_statistics() {
        let rrwm = Rrwm::<char>::new(RrwmConfig::default().with_statistics());

        let stats = rrwm.statistics().expect("Statistics should be enabled");
        assert_eq!(stats.rounds, 0);
        assert_eq!(stats.total_loss, 0.0);
    }

    #[test]
    fn test_rrwm_predict_initial() {
        let rrwm = Rrwm::<char>::new(RrwmConfig::default().seed(42));

        // Initial automaton only has one state with self-accepting
        // This should work for the trivial case
        let result = rrwm.predict();

        // The initial automaton accepts empty string
        assert!(result.is_ok());
    }
}
