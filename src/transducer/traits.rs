//! Core traits for Neural Transducer components.

use crate::semiring::Semiring;
use crate::wfst::{StateId, VectorWfst};
use std::fmt::Debug;

/// Label type for transducer output vocabulary.
pub type Label = u32;

/// Time frame index.
pub type FrameIndex = usize;

/// Blank token constant (typically 0).
pub const BLANK: Label = 0;

/// Acoustic encoder output representation.
///
/// The encoder processes input features (e.g., mel spectrograms) and produces
/// high-level representations at each time frame.
pub trait AcousticEncoder: Send + Sync + Debug {
    /// Output dimension of the encoder.
    fn output_dim(&self) -> usize;

    /// Number of output frames given input length.
    fn output_length(&self, input_length: usize) -> usize;

    /// Get encoder output at a specific time frame.
    ///
    /// Returns a slice of `output_dim()` values representing the encoder
    /// hidden state at time `t`.
    fn get_frame(&self, encoder_out: &EncoderOutput, t: FrameIndex) -> &[f32];
}

/// Encoder output container.
#[derive(Debug, Clone)]
pub struct EncoderOutput {
    /// Shape: [T, D] where T is frames and D is encoder dimension.
    pub data: Vec<f32>,
    /// Number of time frames.
    pub num_frames: usize,
    /// Encoder output dimension.
    pub dim: usize,
}

impl EncoderOutput {
    /// Create a new encoder output.
    pub fn new(data: Vec<f32>, num_frames: usize, dim: usize) -> Self {
        debug_assert_eq!(data.len(), num_frames * dim);
        Self {
            data,
            num_frames,
            dim,
        }
    }

    /// Get the encoder output at time frame `t`.
    #[inline]
    pub fn frame(&self, t: FrameIndex) -> &[f32] {
        let start = t * self.dim;
        &self.data[start..start + self.dim]
    }

    /// Number of time frames.
    #[inline]
    pub fn len(&self) -> usize {
        self.num_frames
    }

    /// Check if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.num_frames == 0
    }
}

/// Autoregressive predictor (language model component).
///
/// The predictor generates representations conditioned on previously
/// emitted non-blank tokens.
pub trait AutoregressivePredictor: Send + Sync + Debug {
    /// Output dimension of the predictor.
    fn output_dim(&self) -> usize;

    /// Initial hidden state for start of sequence.
    fn initial_state(&self) -> PredictorState;

    /// Step the predictor with a new token.
    ///
    /// Returns the new state and output representation.
    fn step(&self, state: &PredictorState, token: Label) -> (PredictorState, Vec<f32>);

    /// Get predictor output at a specific label position.
    fn get_output(&self, predictor_out: &PredictorOutput, u: usize) -> &[f32];
}

/// Predictor hidden state.
#[derive(Debug, Clone, Default)]
pub struct PredictorState {
    /// Hidden state data (LSTM cell state, etc.).
    pub hidden: Vec<f32>,
    /// Cell state for LSTM-based predictors.
    pub cell: Vec<f32>,
    /// Number of tokens emitted so far.
    pub num_tokens: usize,
}

impl PredictorState {
    /// Create a new predictor state.
    pub fn new(hidden: Vec<f32>, cell: Vec<f32>) -> Self {
        Self {
            hidden,
            cell,
            num_tokens: 0,
        }
    }

    /// Create an empty initial state with given dimension.
    pub fn zeros(dim: usize) -> Self {
        Self {
            hidden: vec![0.0; dim],
            cell: vec![0.0; dim],
            num_tokens: 0,
        }
    }
}

/// Predictor output container.
#[derive(Debug, Clone)]
pub struct PredictorOutput {
    /// Shape: [U+1, D] where U is target length (includes start token).
    pub data: Vec<f32>,
    /// Number of label positions (U+1).
    pub num_positions: usize,
    /// Predictor output dimension.
    pub dim: usize,
}

impl PredictorOutput {
    /// Create a new predictor output.
    pub fn new(data: Vec<f32>, num_positions: usize, dim: usize) -> Self {
        debug_assert_eq!(data.len(), num_positions * dim);
        Self {
            data,
            num_positions,
            dim,
        }
    }

    /// Get the predictor output at label position `u`.
    #[inline]
    pub fn position(&self, u: usize) -> &[f32] {
        let start = u * self.dim;
        &self.data[start..start + self.dim]
    }

    /// Number of label positions.
    #[inline]
    pub fn len(&self) -> usize {
        self.num_positions
    }

    /// Check if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.num_positions == 0
    }
}

/// Joint network that combines encoder and predictor outputs.
///
/// The joiner produces log-probabilities over the vocabulary (including blank)
/// given encoder output at time `t` and predictor output at label position `u`.
pub trait JointNetwork: Send + Sync + Debug {
    /// Vocabulary size (including blank).
    fn vocab_size(&self) -> usize;

    /// Compute log-probabilities for a single (t, u) position.
    ///
    /// Returns a vector of `vocab_size()` log-probabilities.
    fn forward(&self, encoder_frame: &[f32], predictor_output: &[f32]) -> Vec<f32>;

    /// Batch computation for efficiency.
    ///
    /// Computes log-probs for multiple (t, u) positions at once.
    fn forward_batch(
        &self,
        encoder_frames: &[&[f32]],
        predictor_outputs: &[&[f32]],
    ) -> Vec<Vec<f32>> {
        encoder_frames
            .iter()
            .zip(predictor_outputs.iter())
            .map(|(enc, pred)| self.forward(enc, pred))
            .collect()
    }
}

/// Neural Transducer combining encoder, predictor, and joiner.
///
/// This trait provides the interface for WFST-based operations on
/// neural transducer models.
pub trait NeuralTransducer: Send + Sync + Debug {
    /// Encoder type.
    type Encoder: AcousticEncoder;
    /// Predictor type.
    type Predictor: AutoregressivePredictor;
    /// Joiner type.
    type Joiner: JointNetwork;

    /// Get the encoder.
    fn encoder(&self) -> &Self::Encoder;

    /// Get the predictor.
    fn predictor(&self) -> &Self::Predictor;

    /// Get the joiner.
    fn joiner(&self) -> &Self::Joiner;

    /// Vocabulary size (including blank).
    fn vocab_size(&self) -> usize {
        self.joiner().vocab_size()
    }

    /// Build the transducer lattice for WFST decoding.
    ///
    /// Creates a WFST representing all possible alignments between
    /// acoustic frames and output labels.
    fn build_lattice<W: Semiring + From<f64>>(
        &self,
        encoder_out: &EncoderOutput,
        predictor_out: &PredictorOutput,
    ) -> TransducerLattice<W>;
}

/// Configuration for neural transducer operations.
#[derive(Debug, Clone)]
pub struct TransducerConfig {
    /// Beam width for decoding.
    pub beam_width: usize,
    /// Maximum number of active hypotheses.
    pub max_active: usize,
    /// Pruning threshold (log-prob difference from best).
    pub pruning_threshold: f32,
    /// Whether to use batched joiner computation.
    pub use_batch_joiner: bool,
    /// Maximum symbols per frame (for streaming).
    pub max_symbols_per_frame: usize,
}

impl Default for TransducerConfig {
    fn default() -> Self {
        Self {
            beam_width: 10,
            max_active: 1000,
            pruning_threshold: 10.0,
            use_batch_joiner: true,
            max_symbols_per_frame: 10,
        }
    }
}

/// Transducer lattice representation.
///
/// This is a T × (U+1) grid WFST where:
/// - Rows correspond to time frames (T)
/// - Columns correspond to label positions (U+1)
/// - Horizontal arcs emit blank (stay at same label position)
/// - Diagonal arcs emit non-blank (advance label position)
#[derive(Debug, Clone)]
pub struct TransducerLattice<W: Semiring> {
    /// Number of time frames.
    pub num_frames: usize,
    /// Number of label positions (target length + 1).
    pub num_positions: usize,
    /// Vocabulary size (including blank).
    pub vocab_size: usize,
    /// Log-probabilities at each (t, u, label) position.
    /// Shape: [T, U+1, V] stored as flat vector.
    pub log_probs: Vec<f64>,
    /// Phantom for weight type.
    _phantom: std::marker::PhantomData<W>,
}

impl<W: Semiring> TransducerLattice<W> {
    /// Create a new transducer lattice.
    pub fn new(num_frames: usize, num_positions: usize, vocab_size: usize) -> Self {
        let size = num_frames * num_positions * vocab_size;
        Self {
            num_frames,
            num_positions,
            vocab_size,
            log_probs: vec![f64::NEG_INFINITY; size],
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set log-probability at (t, u, label).
    #[inline]
    pub fn set(&mut self, t: usize, u: usize, label: Label, log_prob: f64) {
        let idx = self.index(t, u, label as usize);
        self.log_probs[idx] = log_prob;
    }

    /// Get log-probability at (t, u, label).
    #[inline]
    pub fn get(&self, t: usize, u: usize, label: Label) -> f64 {
        let idx = self.index(t, u, label as usize);
        self.log_probs[idx]
    }

    /// Compute flat index from (t, u, label).
    #[inline]
    fn index(&self, t: usize, u: usize, label: usize) -> usize {
        (t * self.num_positions + u) * self.vocab_size + label
    }

    /// Get all log-probs at position (t, u).
    pub fn get_position(&self, t: usize, u: usize) -> &[f64] {
        let start = (t * self.num_positions + u) * self.vocab_size;
        &self.log_probs[start..start + self.vocab_size]
    }

    /// Convert to explicit WFST representation.
    ///
    /// Creates a WFST with states (t, u) and transitions labeled
    /// with vocabulary symbols and weighted by log-probabilities.
    pub fn to_wfst(&self) -> VectorWfst<Label, W>
    where
        W: From<f64> + Clone,
    {
        use crate::wfst::{MutableWfst, WeightedTransition};

        let mut fst: VectorWfst<Label, W> = VectorWfst::new();

        // Create states for each (t, u) position
        // State id = t * (num_positions) + u
        let num_states = (self.num_frames + 1) * self.num_positions;
        fst.add_states(num_states);

        // Start state is (0, 0)
        fst.set_start(0);

        // Final state is (T, U) - after processing all frames and labels
        let final_state =
            (self.num_frames * self.num_positions + (self.num_positions - 1)) as StateId;
        fst.set_final(final_state, W::one());

        // Add transitions
        for t in 0..self.num_frames {
            for u in 0..self.num_positions {
                let from_state = (t * self.num_positions + u) as StateId;

                // Blank transition: (t, u) -> (t+1, u)
                let blank_prob = self.get(t, u, BLANK);
                if blank_prob > f64::NEG_INFINITY {
                    let to_state = ((t + 1) * self.num_positions + u) as StateId;
                    fst.add_transition(WeightedTransition {
                        from: from_state,
                        input: Some(BLANK),
                        output: Some(BLANK),
                        to: to_state,
                        weight: W::from(-blank_prob), // Convert to tropical/log weight
                    });
                }

                // Non-blank transitions: (t, u) -> (t+1, u+1) for u < U
                if u + 1 < self.num_positions {
                    for label in 1..self.vocab_size as Label {
                        let label_prob = self.get(t, u, label);
                        if label_prob > f64::NEG_INFINITY {
                            let to_state = ((t + 1) * self.num_positions + u + 1) as StateId;
                            fst.add_transition(WeightedTransition {
                                from: from_state,
                                input: Some(label),
                                output: Some(label),
                                to: to_state,
                                weight: W::from(-label_prob),
                            });
                        }
                    }
                }
            }
        }

        fst
    }
}

/// Statistics from transducer operations.
#[derive(Debug, Clone, Default)]
pub struct TransducerStats {
    /// Number of encoder frames processed.
    pub num_frames: usize,
    /// Number of label positions.
    pub num_positions: usize,
    /// Number of non-pruned hypotheses.
    pub num_hypotheses: usize,
    /// Time spent in encoder (ms).
    pub encoder_time_ms: f64,
    /// Time spent in predictor (ms).
    pub predictor_time_ms: f64,
    /// Time spent in joiner (ms).
    pub joiner_time_ms: f64,
    /// Time spent in beam search (ms).
    pub search_time_ms: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    // =========================================================================
    // EncoderOutput Tests
    // =========================================================================

    #[test]
    fn test_encoder_output_creation() {
        let data = vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        let num_frames = 2;
        let dim = 3;

        let encoder_out = EncoderOutput::new(data.clone(), num_frames, dim);

        assert_eq!(encoder_out.num_frames, 2);
        assert_eq!(encoder_out.dim, 3);
        assert_eq!(encoder_out.len(), 2);
        assert!(!encoder_out.is_empty());
    }

    #[test]
    fn test_encoder_output_frame_access() {
        // Create encoder output with 3 frames, dimension 2
        let data = vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        let encoder_out = EncoderOutput::new(data, 3, 2);

        // Access each frame
        assert_eq!(encoder_out.frame(0), &[1.0f32, 2.0]);
        assert_eq!(encoder_out.frame(1), &[3.0f32, 4.0]);
        assert_eq!(encoder_out.frame(2), &[5.0f32, 6.0]);
    }

    #[test]
    fn test_encoder_output_empty() {
        let encoder_out = EncoderOutput::new(vec![], 0, 4);

        assert_eq!(encoder_out.len(), 0);
        assert!(encoder_out.is_empty());
    }

    #[test]
    fn test_encoder_output_single_frame() {
        let data = vec![0.1f32, 0.2, 0.3, 0.4];
        let encoder_out = EncoderOutput::new(data, 1, 4);

        assert_eq!(encoder_out.len(), 1);
        assert_eq!(encoder_out.frame(0), &[0.1f32, 0.2, 0.3, 0.4]);
    }

    // =========================================================================
    // PredictorState Tests
    // =========================================================================

    #[test]
    fn test_predictor_state_creation() {
        let hidden = vec![0.1, 0.2, 0.3];
        let cell = vec![0.4, 0.5, 0.6];

        let state = PredictorState::new(hidden.clone(), cell.clone());

        assert_eq!(state.hidden, hidden);
        assert_eq!(state.cell, cell);
        assert_eq!(state.num_tokens, 0);
    }

    #[test]
    fn test_predictor_state_zeros() {
        let state = PredictorState::zeros(4);

        assert_eq!(state.hidden, vec![0.0; 4]);
        assert_eq!(state.cell, vec![0.0; 4]);
        assert_eq!(state.num_tokens, 0);
    }

    #[test]
    fn test_predictor_state_default() {
        let state = PredictorState::default();

        assert!(state.hidden.is_empty());
        assert!(state.cell.is_empty());
        assert_eq!(state.num_tokens, 0);
    }

    // =========================================================================
    // PredictorOutput Tests
    // =========================================================================

    #[test]
    fn test_predictor_output_creation() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let num_positions = 4;
        let dim = 2;

        let predictor_out = PredictorOutput::new(data.clone(), num_positions, dim);

        assert_eq!(predictor_out.num_positions, 4);
        assert_eq!(predictor_out.dim, 2);
        assert_eq!(predictor_out.len(), 4);
        assert!(!predictor_out.is_empty());
    }

    #[test]
    fn test_predictor_output_position_access() {
        // Create predictor output with 3 positions, dimension 2
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let predictor_out = PredictorOutput::new(data, 3, 2);

        // Access each position
        assert_eq!(predictor_out.position(0), &[1.0, 2.0]);
        assert_eq!(predictor_out.position(1), &[3.0, 4.0]);
        assert_eq!(predictor_out.position(2), &[5.0, 6.0]);
    }

    #[test]
    fn test_predictor_output_empty() {
        let predictor_out = PredictorOutput::new(vec![], 0, 4);

        assert_eq!(predictor_out.len(), 0);
        assert!(predictor_out.is_empty());
    }

    // =========================================================================
    // TransducerConfig Tests
    // =========================================================================

    #[test]
    fn test_transducer_config_default() {
        let config = TransducerConfig::default();

        assert_eq!(config.beam_width, 10);
        assert_eq!(config.max_active, 1000);
        assert!((config.pruning_threshold - 10.0).abs() < f32::EPSILON);
        assert!(config.use_batch_joiner);
        assert_eq!(config.max_symbols_per_frame, 10);
    }

    #[test]
    fn test_transducer_config_custom() {
        let config = TransducerConfig {
            beam_width: 20,
            max_active: 500,
            pruning_threshold: 5.0,
            use_batch_joiner: false,
            max_symbols_per_frame: 5,
        };

        assert_eq!(config.beam_width, 20);
        assert_eq!(config.max_active, 500);
        assert!(!config.use_batch_joiner);
    }

    // =========================================================================
    // TransducerLattice Tests
    // =========================================================================

    #[test]
    fn test_transducer_lattice_creation() {
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(5, 3, 10);

        assert_eq!(lattice.num_frames, 5);
        assert_eq!(lattice.num_positions, 3);
        assert_eq!(lattice.vocab_size, 10);
        assert_eq!(lattice.log_probs.len(), 5 * 3 * 10);
    }

    #[test]
    fn test_transducer_lattice_set_get() {
        let mut lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(3, 2, 5);

        // Set some values
        lattice.set(0, 0, 0, -1.0);
        lattice.set(0, 0, 1, -2.0);
        lattice.set(1, 1, 2, -3.0);
        lattice.set(2, 0, BLANK, -0.5);

        // Get the values back
        assert!((lattice.get(0, 0, 0) - (-1.0)).abs() < 1e-10);
        assert!((lattice.get(0, 0, 1) - (-2.0)).abs() < 1e-10);
        assert!((lattice.get(1, 1, 2) - (-3.0)).abs() < 1e-10);
        assert!((lattice.get(2, 0, BLANK) - (-0.5)).abs() < 1e-10);

        // Unset values should be NEG_INFINITY
        assert!(lattice.get(0, 1, 3) == f64::NEG_INFINITY);
    }

    #[test]
    fn test_transducer_lattice_get_position() {
        let mut lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(2, 2, 3);

        // Set values at position (0, 0)
        lattice.set(0, 0, 0, -1.0);
        lattice.set(0, 0, 1, -2.0);
        lattice.set(0, 0, 2, -3.0);

        // Get all log-probs at position (0, 0)
        let position_probs = lattice.get_position(0, 0);
        assert_eq!(position_probs.len(), 3);
        assert!((position_probs[0] - (-1.0)).abs() < 1e-10);
        assert!((position_probs[1] - (-2.0)).abs() < 1e-10);
        assert!((position_probs[2] - (-3.0)).abs() < 1e-10);
    }

    #[test]
    fn test_transducer_lattice_to_wfst_structure() {
        use crate::wfst::Wfst;

        // Create a small lattice: 2 frames, 2 positions, vocab size 3
        let mut lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(2, 2, 3);

        // Set some log-probs (negative log probabilities)
        // Frame 0, position 0: blank and labels
        lattice.set(0, 0, BLANK, -0.1); // blank stays at (0,0) -> (1,0)
        lattice.set(0, 0, 1, -0.5); // label 1 goes (0,0) -> (1,1)

        // Frame 1, position 0: blank
        lattice.set(1, 0, BLANK, -0.2); // blank stays at (1,0) -> (2,0)

        // Frame 0, position 1: not used in this test
        // Frame 1, position 1: blank goes to final
        lattice.set(1, 1, BLANK, -0.3); // blank stays at (1,1) -> (2,1)

        // Convert to WFST
        let wfst = lattice.to_wfst();

        // Verify WFST structure - start() returns StateId, check num_states > 0
        assert!(wfst.num_states() > 0);
        let start_state = wfst.start();
        assert!(
            wfst.is_valid_state(start_state),
            "Start state should be valid"
        );

        // Verify there is at least one final state
        let has_final = (0..wfst.num_states()).any(|s| wfst.is_final(s as u32));
        assert!(has_final, "WFST should have at least one final state");
    }

    #[test]
    fn test_transducer_lattice_empty() {
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(0, 0, 0);

        assert_eq!(lattice.num_frames, 0);
        assert_eq!(lattice.num_positions, 0);
        assert_eq!(lattice.vocab_size, 0);
        assert!(lattice.log_probs.is_empty());
    }

    #[test]
    fn test_transducer_lattice_indexing() {
        // Verify the indexing formula: (t * num_positions + u) * vocab_size + label
        // Manually compute expected indices
        // (t=0, u=0, label=0) -> (0 * 4 + 0) * 5 + 0 = 0
        // (t=0, u=0, label=4) -> (0 * 4 + 0) * 5 + 4 = 4
        // (t=0, u=1, label=0) -> (0 * 4 + 1) * 5 + 0 = 5
        // (t=1, u=0, label=0) -> (1 * 4 + 0) * 5 + 0 = 20
        // (t=2, u=3, label=4) -> (2 * 4 + 3) * 5 + 4 = 59

        // The lattice uses private index() method, but we can verify through set/get
        let mut lattice2: TransducerLattice<TropicalWeight> = TransducerLattice::new(3, 4, 5);

        // Set specific positions and verify they don't interfere
        lattice2.set(0, 0, 0, 1.0);
        lattice2.set(0, 0, 4, 2.0);
        lattice2.set(0, 1, 0, 3.0);
        lattice2.set(1, 0, 0, 4.0);
        lattice2.set(2, 3, 4, 5.0);

        assert!((lattice2.get(0, 0, 0) - 1.0).abs() < 1e-10);
        assert!((lattice2.get(0, 0, 4) - 2.0).abs() < 1e-10);
        assert!((lattice2.get(0, 1, 0) - 3.0).abs() < 1e-10);
        assert!((lattice2.get(1, 0, 0) - 4.0).abs() < 1e-10);
        assert!((lattice2.get(2, 3, 4) - 5.0).abs() < 1e-10);
    }

    // =========================================================================
    // TransducerStats Tests
    // =========================================================================

    #[test]
    fn test_transducer_stats_default() {
        let stats = TransducerStats::default();

        assert_eq!(stats.num_frames, 0);
        assert_eq!(stats.num_positions, 0);
        assert_eq!(stats.num_hypotheses, 0);
        assert!((stats.encoder_time_ms - 0.0).abs() < f64::EPSILON);
        assert!((stats.predictor_time_ms - 0.0).abs() < f64::EPSILON);
        assert!((stats.joiner_time_ms - 0.0).abs() < f64::EPSILON);
        assert!((stats.search_time_ms - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_transducer_stats_custom() {
        let stats = TransducerStats {
            num_frames: 100,
            num_positions: 50,
            num_hypotheses: 200,
            encoder_time_ms: 10.5,
            predictor_time_ms: 5.2,
            joiner_time_ms: 15.8,
            search_time_ms: 3.1,
        };

        assert_eq!(stats.num_frames, 100);
        assert_eq!(stats.num_positions, 50);
        assert_eq!(stats.num_hypotheses, 200);
        assert!((stats.encoder_time_ms - 10.5).abs() < 1e-10);
    }

    // =========================================================================
    // Constants Tests
    // =========================================================================

    #[test]
    fn test_blank_constant() {
        assert_eq!(BLANK, 0);
    }

    // =========================================================================
    // Type Alias Tests
    // =========================================================================

    #[test]
    fn test_label_type() {
        let label: Label = 42;
        assert_eq!(label, 42u32);
    }

    #[test]
    fn test_frame_index_type() {
        let frame: FrameIndex = 100;
        assert_eq!(frame, 100usize);
    }

    // =========================================================================
    // Clone Tests
    // =========================================================================

    #[test]
    fn test_encoder_output_clone() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let encoder_out = EncoderOutput::new(data.clone(), 2, 2);
        let cloned = encoder_out.clone();

        assert_eq!(encoder_out.data, cloned.data);
        assert_eq!(encoder_out.num_frames, cloned.num_frames);
        assert_eq!(encoder_out.dim, cloned.dim);
    }

    #[test]
    fn test_predictor_state_clone() {
        let state = PredictorState::new(vec![1.0, 2.0], vec![3.0, 4.0]);
        let cloned = state.clone();

        assert_eq!(state.hidden, cloned.hidden);
        assert_eq!(state.cell, cloned.cell);
        assert_eq!(state.num_tokens, cloned.num_tokens);
    }

    #[test]
    fn test_predictor_output_clone() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let predictor_out = PredictorOutput::new(data.clone(), 2, 2);
        let cloned = predictor_out.clone();

        assert_eq!(predictor_out.data, cloned.data);
        assert_eq!(predictor_out.num_positions, cloned.num_positions);
        assert_eq!(predictor_out.dim, cloned.dim);
    }

    #[test]
    fn test_transducer_lattice_clone() {
        let mut lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(2, 2, 3);
        lattice.set(0, 0, 0, -1.0);
        lattice.set(1, 1, 2, -2.0);

        let cloned = lattice.clone();

        assert_eq!(lattice.num_frames, cloned.num_frames);
        assert_eq!(lattice.num_positions, cloned.num_positions);
        assert_eq!(lattice.vocab_size, cloned.vocab_size);
        assert!((lattice.get(0, 0, 0) - cloned.get(0, 0, 0)).abs() < 1e-10);
        assert!((lattice.get(1, 1, 2) - cloned.get(1, 1, 2)).abs() < 1e-10);
    }
}
