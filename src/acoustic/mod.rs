//! Acoustic model traits and utilities for ASR integration.
//!
//! This module provides the core abstraction layer for integrating acoustic models
//! with WFST-based speech recognition. It defines traits for:
//!
//! - **Emission probability computation**: Converting audio features to unit posteriors
//! - **HMM topology**: State transition matrices for hybrid HMM-DNN systems
//! - **Acoustic-LM fusion**: Combining acoustic and language model scores
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Acoustic Model Pipeline                          │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │                                                                         │
//! │   Audio Frames     ──────►  AcousticModel::forward()  ──────►  Posteriors
//! │   [B, T, F]                                                   [B, T, U]
//! │                                                                         │
//! │   Where:                                                                │
//! │     B = batch size                                                      │
//! │     T = time steps                                                      │
//! │     F = feature_dim (e.g., 40 for filterbank)                          │
//! │     U = num_units (e.g., senones, phonemes, or characters)             │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Integration with WFST Decoding
//!
//! The acoustic model outputs are used in the H transducer of the ASR cascade:
//!
//! ```text
//! N = π(min(det(H̃ ∘ det(C̃ ∘ det(L̃ ∘ G)))))
//!                 ↑
//!                 H̃ uses emission probabilities from AcousticModel
//! ```
//!
//! # Example
//!
//! ```ignore
//! use lling_llang::acoustic::{AcousticModel, TransitionMatrix};
//!
//! struct MyNeuralModel {
//!     // Neural network weights...
//! }
//!
//! impl AcousticModel for MyNeuralModel {
//!     fn feature_dim(&self) -> usize { 40 }  // filterbank features
//!     fn num_units(&self) -> usize { 4096 }  // senone outputs
//!
//!     fn forward(&self, frames: &[Vec<f32>]) -> Vec<Vec<f32>> {
//!         // Run neural network inference
//!         unimplemented!()
//!     }
//! }
//! ```
//!
//! # Semiring Integration
//!
//! Acoustic scores integrate with WFST semirings via the ProductWeight:
//!
//! | Semiring | Use Case |
//! |----------|----------|
//! | `LogWeight` | Native log probability space for acoustic scores |
//! | `TropicalWeight` | Best-path decoding with negative log costs |
//! | `ProductWeight<LogWeight, LogWeight>` | Separate AM and LM score tracking |

use std::sync::Arc;

use crate::semiring::LogWeight;

/// State ID in the HMM topology.
pub type HmmStateId = u32;

/// Unit ID (senone, phoneme, or character).
pub type UnitId = u32;

/// Transition probability in log space.
pub type TransitionLogProb = f32;

/// HMM state transition matrix.
///
/// Represents the transition probabilities between HMM states.
/// Used for hybrid HMM-DNN acoustic models where the DNN provides
/// emission probabilities and the HMM provides topology.
#[derive(Clone, Debug)]
pub struct TransitionMatrix {
    /// Number of HMM states.
    num_states: usize,

    /// Transition log probabilities: [from_state][to_state] -> log_prob
    /// Sparse representation: only non-zero transitions are stored.
    transitions: Vec<Vec<(HmmStateId, TransitionLogProb)>>,

    /// Initial state distribution (log probabilities).
    initial_probs: Vec<TransitionLogProb>,

    /// Final state indicators (which states can end utterances).
    is_final: Vec<bool>,
}

impl TransitionMatrix {
    /// Create a new transition matrix with the given number of states.
    pub fn new(num_states: usize) -> Self {
        Self {
            num_states,
            transitions: vec![Vec::new(); num_states],
            initial_probs: vec![f32::NEG_INFINITY; num_states],
            is_final: vec![false; num_states],
        }
    }

    /// Create a simple left-to-right HMM topology.
    ///
    /// Each state transitions to itself (self-loop) and to the next state.
    pub fn left_to_right(num_states: usize, self_loop_prob: f32) -> Self {
        let mut tm = Self::new(num_states);

        // First state is initial
        tm.initial_probs[0] = 0.0; // log(1.0)

        // Each state has self-loop and forward transition
        let forward_prob = 1.0 - self_loop_prob;
        let log_self = self_loop_prob.ln();
        let log_forward = forward_prob.ln();

        for i in 0..num_states {
            // Self-loop
            tm.add_transition(i as HmmStateId, i as HmmStateId, log_self);

            // Forward transition (if not last state)
            if i + 1 < num_states {
                tm.add_transition(i as HmmStateId, (i + 1) as HmmStateId, log_forward);
            }
        }

        // Last state is final
        tm.is_final[num_states - 1] = true;

        tm
    }

    /// Create a Bakis (left-to-right with skip) topology.
    ///
    /// Each state can transition to itself, next state, or skip one state.
    pub fn bakis(num_states: usize, self_prob: f32, forward_prob: f32) -> Self {
        let mut tm = Self::new(num_states);

        // First state is initial
        tm.initial_probs[0] = 0.0;

        let skip_prob = 1.0 - self_prob - forward_prob;
        let log_self = self_prob.ln();
        let log_forward = forward_prob.ln();
        let log_skip = skip_prob.ln();

        for i in 0..num_states {
            // Self-loop
            tm.add_transition(i as HmmStateId, i as HmmStateId, log_self);

            // Forward transition
            if i + 1 < num_states {
                tm.add_transition(i as HmmStateId, (i + 1) as HmmStateId, log_forward);
            }

            // Skip transition
            if i + 2 < num_states {
                tm.add_transition(i as HmmStateId, (i + 2) as HmmStateId, log_skip);
            }
        }

        // Last two states are final (to allow skip to end)
        if num_states > 0 {
            tm.is_final[num_states - 1] = true;
        }
        if num_states > 1 {
            tm.is_final[num_states - 2] = true;
        }

        tm
    }

    /// Add a transition between states.
    pub fn add_transition(
        &mut self,
        from: HmmStateId,
        to: HmmStateId,
        log_prob: TransitionLogProb,
    ) {
        if (from as usize) < self.num_states && (to as usize) < self.num_states {
            self.transitions[from as usize].push((to, log_prob));
        }
    }

    /// Set initial probability for a state.
    pub fn set_initial(&mut self, state: HmmStateId, log_prob: TransitionLogProb) {
        if (state as usize) < self.num_states {
            self.initial_probs[state as usize] = log_prob;
        }
    }

    /// Mark a state as final (accepting).
    pub fn set_final(&mut self, state: HmmStateId, is_final: bool) {
        if (state as usize) < self.num_states {
            self.is_final[state as usize] = is_final;
        }
    }

    /// Get the number of states.
    pub fn num_states(&self) -> usize {
        self.num_states
    }

    /// Get transitions from a state.
    pub fn transitions_from(&self, state: HmmStateId) -> &[(HmmStateId, TransitionLogProb)] {
        if (state as usize) < self.num_states {
            &self.transitions[state as usize]
        } else {
            &[]
        }
    }

    /// Get initial log probability for a state.
    pub fn initial_prob(&self, state: HmmStateId) -> TransitionLogProb {
        if (state as usize) < self.num_states {
            self.initial_probs[state as usize]
        } else {
            f32::NEG_INFINITY
        }
    }

    /// Check if a state is final.
    pub fn is_final(&self, state: HmmStateId) -> bool {
        if (state as usize) < self.num_states {
            self.is_final[state as usize]
        } else {
            false
        }
    }

    /// Get all initial states (with non-negative-infinity probability).
    pub fn initial_states(&self) -> Vec<HmmStateId> {
        self.initial_probs
            .iter()
            .enumerate()
            .filter(|(_, &p)| p > f32::NEG_INFINITY)
            .map(|(i, _)| i as HmmStateId)
            .collect()
    }

    /// Get all final states.
    pub fn final_states(&self) -> Vec<HmmStateId> {
        self.is_final
            .iter()
            .enumerate()
            .filter(|(_, &f)| f)
            .map(|(i, _)| i as HmmStateId)
            .collect()
    }
}

/// Core trait for acoustic models that compute emission probabilities.
///
/// Implementations of this trait wrap neural networks (DNNs, RNNs, Transformers)
/// that convert audio features to posterior probabilities over output units.
///
/// # Output Units
///
/// The output units depend on the acoustic model architecture:
///
/// | Model Type | Output Units |
/// |------------|--------------|
/// | Hybrid HMM-DNN | Senones (tied HMM states) |
/// | CTC | Characters or phonemes + blank |
/// | Attention | Characters or subwords |
///
/// # Thread Safety
///
/// Implementations must be thread-safe (`Send + Sync`) to allow parallel decoding.
pub trait AcousticModel: Send + Sync {
    /// Frame feature dimensionality.
    ///
    /// Common values:
    /// - 40: Mel filterbank features
    /// - 13: MFCC features
    /// - 80: Extended filterbank for modern models
    fn feature_dim(&self) -> usize;

    /// Number of output units (e.g., senones, phonemes, characters).
    ///
    /// For CTC models, this includes the blank token.
    fn num_units(&self) -> usize;

    /// Compute log posteriors for a batch of frames.
    ///
    /// # Arguments
    ///
    /// * `frames` - Batch of frames: `[batch_size][feature_dim]`
    ///
    /// # Returns
    ///
    /// Log posteriors: `[batch_size][num_units]`
    ///
    /// Each output is `log P(unit | frame)`.
    fn forward(&self, frames: &[Vec<f32>]) -> Vec<Vec<f32>>;

    /// Compute log posteriors for a sequence of frames.
    ///
    /// This is a convenience method that processes frames one at a time.
    /// Override for more efficient batched processing.
    fn forward_sequence(&self, frames: &[Vec<f32>]) -> Vec<Vec<f32>> {
        frames
            .iter()
            .map(|f| self.forward(std::slice::from_ref(f))[0].clone())
            .collect()
    }

    /// Optional: Get the HMM transition matrix for hybrid models.
    ///
    /// Returns `None` for CTC or attention-based models that don't use HMM topology.
    fn transition_matrix(&self) -> Option<&TransitionMatrix> {
        None
    }

    /// Optional: Get the blank token ID for CTC models.
    ///
    /// Returns `None` for non-CTC models.
    fn blank_id(&self) -> Option<UnitId> {
        None
    }

    /// Optional: Get human-readable name for a unit.
    fn unit_name(&self, unit: UnitId) -> Option<String> {
        let _ = unit;
        None
    }
}

/// Configuration for combining acoustic and language model scores.
#[derive(Clone, Debug)]
pub struct FusionConfig {
    /// Weight for acoustic model scores (typically 1.0).
    pub acoustic_weight: f64,

    /// Weight for language model scores.
    ///
    /// Typical values range from 0.1 to 1.0.
    /// Higher values give more influence to the language model.
    pub lm_weight: f64,

    /// Word insertion penalty (in log space).
    ///
    /// Negative values encourage shorter hypotheses.
    /// Positive values encourage longer hypotheses.
    pub word_insertion_penalty: f64,

    /// Blank penalty for CTC models (in log space).
    ///
    /// Encourages or discourages blank emissions.
    pub blank_penalty: f64,
}

impl Default for FusionConfig {
    fn default() -> Self {
        Self {
            acoustic_weight: 1.0,
            lm_weight: 0.5,
            word_insertion_penalty: 0.0,
            blank_penalty: 0.0,
        }
    }
}

/// Wrapper combining acoustic and language model scores.
///
/// This struct enables joint decoding by combining:
/// - Acoustic scores: P(observation | state) from the acoustic model
/// - Language model scores: P(word | context) from the language model
///
/// The combined score is:
/// ```text
/// score = λ_am * log P(obs | state) + λ_lm * log P(word | context) + penalty
/// ```
#[derive(Clone)]
pub struct AcousticLanguageModel<A: AcousticModel, L> {
    /// The acoustic model.
    acoustic: Arc<A>,

    /// The language model (implements scoring interface).
    language: Arc<L>,

    /// Fusion configuration.
    config: FusionConfig,
}

impl<A: AcousticModel, L> AcousticLanguageModel<A, L> {
    /// Create a new acoustic-language model wrapper.
    pub fn new(acoustic: Arc<A>, language: Arc<L>, config: FusionConfig) -> Self {
        Self {
            acoustic,
            language,
            config,
        }
    }

    /// Get the acoustic model.
    pub fn acoustic(&self) -> &A {
        &self.acoustic
    }

    /// Get the language model.
    pub fn language(&self) -> &L {
        &self.language
    }

    /// Get the fusion configuration.
    pub fn config(&self) -> &FusionConfig {
        &self.config
    }

    /// Get mutable fusion configuration.
    pub fn config_mut(&mut self) -> &mut FusionConfig {
        &mut self.config
    }

    /// Compute acoustic posteriors for frames.
    pub fn acoustic_forward(&self, frames: &[Vec<f32>]) -> Vec<Vec<f32>> {
        self.acoustic.forward(frames)
    }

    /// Apply acoustic weight to log probabilities.
    pub fn weight_acoustic(&self, log_prob: f64) -> f64 {
        self.config.acoustic_weight * log_prob
    }

    /// Apply LM weight to log probabilities.
    pub fn weight_lm(&self, log_prob: f64) -> f64 {
        self.config.lm_weight * log_prob
    }

    /// Combine acoustic and LM scores.
    pub fn combine_scores(&self, acoustic_log_prob: f64, lm_log_prob: f64) -> f64 {
        self.weight_acoustic(acoustic_log_prob) + self.weight_lm(lm_log_prob)
    }

    /// Convert combined score to LogWeight.
    pub fn to_log_weight(&self, acoustic_log_prob: f64, lm_log_prob: f64) -> LogWeight {
        let combined = self.combine_scores(acoustic_log_prob, lm_log_prob);
        // LogWeight uses negative log, so negate
        LogWeight::new(-combined)
    }
}

/// Frame-level posterior with metadata.
#[derive(Clone, Debug)]
pub struct FramePosterior {
    /// Frame index in the sequence.
    pub frame_idx: usize,

    /// Log posteriors for each unit.
    pub log_probs: Vec<f32>,

    /// Optional: Top-k unit indices (for sparse representation).
    pub top_k_units: Option<Vec<UnitId>>,
}

impl FramePosterior {
    /// Create a new frame posterior.
    pub fn new(frame_idx: usize, log_probs: Vec<f32>) -> Self {
        Self {
            frame_idx,
            log_probs,
            top_k_units: None,
        }
    }

    /// Get the best (highest probability) unit.
    pub fn best_unit(&self) -> Option<UnitId> {
        self.log_probs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i as UnitId)
    }

    /// Get the log probability for a unit.
    pub fn log_prob(&self, unit: UnitId) -> f32 {
        self.log_probs
            .get(unit as usize)
            .copied()
            .unwrap_or(f32::NEG_INFINITY)
    }

    /// Compute top-k units and store them.
    pub fn compute_top_k(&mut self, k: usize) {
        let mut indexed: Vec<(usize, f32)> = self.log_probs.iter().copied().enumerate().collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        self.top_k_units = Some(
            indexed
                .into_iter()
                .take(k)
                .map(|(i, _)| i as UnitId)
                .collect(),
        );
    }
}

/// Sequence of frame posteriors.
#[derive(Clone, Debug)]
pub struct PosteriorSequence {
    /// Posteriors for each frame.
    pub frames: Vec<FramePosterior>,

    /// Number of units.
    pub num_units: usize,
}

impl PosteriorSequence {
    /// Create a new posterior sequence from raw posteriors.
    pub fn from_raw(posteriors: Vec<Vec<f32>>) -> Self {
        let num_units = posteriors.first().map(|f| f.len()).unwrap_or(0);
        let frames = posteriors
            .into_iter()
            .enumerate()
            .map(|(i, probs)| FramePosterior::new(i, probs))
            .collect();
        Self { frames, num_units }
    }

    /// Get the number of frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Get posterior for a specific frame.
    pub fn frame(&self, idx: usize) -> Option<&FramePosterior> {
        self.frames.get(idx)
    }

    /// Get the best path (greedy decoding).
    pub fn greedy_path(&self) -> Vec<UnitId> {
        self.frames.iter().filter_map(|f| f.best_unit()).collect()
    }

    /// Compute top-k for all frames.
    pub fn compute_all_top_k(&mut self, k: usize) {
        for frame in &mut self.frames {
            frame.compute_top_k(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transition_matrix_left_to_right() {
        let tm = TransitionMatrix::left_to_right(3, 0.5);

        assert_eq!(tm.num_states(), 3);
        assert_eq!(tm.initial_states(), vec![0]);
        assert_eq!(tm.final_states(), vec![2]);

        // Check transitions from state 0
        let trans = tm.transitions_from(0);
        assert_eq!(trans.len(), 2); // self-loop + forward
    }

    #[test]
    fn test_transition_matrix_bakis() {
        let tm = TransitionMatrix::bakis(4, 0.3, 0.4);

        assert_eq!(tm.num_states(), 4);

        // Check state 0 has self, forward, and skip
        let trans = tm.transitions_from(0);
        assert_eq!(trans.len(), 3);
    }

    #[test]
    fn test_frame_posterior() {
        let mut fp = FramePosterior::new(0, vec![-1.0, -0.5, -2.0, -0.1]);

        assert_eq!(fp.best_unit(), Some(3)); // -0.1 is highest
        assert!((fp.log_prob(1) - (-0.5)).abs() < 1e-6);

        fp.compute_top_k(2);
        assert!(fp.top_k_units.is_some());
        assert_eq!(
            fp.top_k_units
                .as_ref()
                .expect("acoustic/mod.rs: required value was None/Err")
                .len(),
            2
        );
        assert_eq!(
            fp.top_k_units
                .as_ref()
                .expect("acoustic/mod.rs: required value was None/Err")[0],
            3
        ); // -0.1
        assert_eq!(
            fp.top_k_units
                .as_ref()
                .expect("acoustic/mod.rs: required value was None/Err")[1],
            1
        ); // -0.5
    }

    #[test]
    fn test_posterior_sequence() {
        let posteriors = vec![
            vec![-1.0, -0.5, -2.0],
            vec![-0.1, -0.8, -1.5],
            vec![-0.3, -0.2, -0.9],
        ];

        let seq = PosteriorSequence::from_raw(posteriors);

        assert_eq!(seq.len(), 3);
        assert_eq!(seq.num_units, 3);

        let greedy = seq.greedy_path();
        assert_eq!(greedy, vec![1, 0, 1]); // Best units per frame
    }

    #[test]
    fn test_fusion_config_default() {
        let config = FusionConfig::default();

        assert!((config.acoustic_weight - 1.0).abs() < 1e-6);
        assert!((config.lm_weight - 0.5).abs() < 1e-6);
        assert!((config.word_insertion_penalty - 0.0).abs() < 1e-6);
    }

    // Mock acoustic model for testing
    struct MockAcousticModel {
        feature_dim: usize,
        num_units: usize,
    }

    impl AcousticModel for MockAcousticModel {
        fn feature_dim(&self) -> usize {
            self.feature_dim
        }

        fn num_units(&self) -> usize {
            self.num_units
        }

        fn forward(&self, frames: &[Vec<f32>]) -> Vec<Vec<f32>> {
            // Return uniform distribution for testing
            let log_prob = (-(self.num_units as f32)).ln();
            frames
                .iter()
                .map(|_| vec![log_prob; self.num_units])
                .collect()
        }
    }

    #[test]
    fn test_acoustic_model_trait() {
        let model = MockAcousticModel {
            feature_dim: 40,
            num_units: 100,
        };

        assert_eq!(model.feature_dim(), 40);
        assert_eq!(model.num_units(), 100);

        let frames = vec![vec![0.0f32; 40]; 5];
        let posteriors = model.forward(&frames);

        assert_eq!(posteriors.len(), 5);
        assert_eq!(posteriors[0].len(), 100);
    }

    #[test]
    fn test_acoustic_language_model() {
        let acoustic = Arc::new(MockAcousticModel {
            feature_dim: 40,
            num_units: 100,
        });

        // Use a placeholder for LM (we don't need actual scoring for this test)
        let language: Arc<()> = Arc::new(());

        let config = FusionConfig {
            acoustic_weight: 1.0,
            lm_weight: 0.5,
            ..Default::default()
        };

        let alm = AcousticLanguageModel::new(acoustic, language, config);

        // Test score combination
        let am_score = -2.0; // log prob
        let lm_score = -1.0;

        let combined = alm.combine_scores(am_score, lm_score);
        // Expected: 1.0 * -2.0 + 0.5 * -1.0 = -2.5
        assert!((combined - (-2.5)).abs() < 1e-6);

        // Test LogWeight conversion
        let lw = alm.to_log_weight(am_score, lm_score);
        // LogWeight stores negative log, so 2.5
        assert!((lw.value() - 2.5).abs() < 1e-6);
    }
}
