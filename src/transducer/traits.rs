//! Core traits for Neural Transducer components.

use crate::semiring::Semiring;
use crate::wfst::{StateId, VectorWfst};
use std::fmt::{self, Debug};

/// Label type for transducer output vocabulary.
pub type Label = u32;

/// Time frame index.
pub type FrameIndex = usize;

/// Blank token constant (typically 0).
pub const BLANK: Label = 0;

/// Output tensor kind used in [`TransducerOutputError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransducerOutputKind {
    /// Acoustic encoder output tensor.
    Encoder,
    /// Autoregressive predictor output tensor.
    Predictor,
}

impl TransducerOutputKind {
    #[inline]
    fn name(self) -> &'static str {
        match self {
            Self::Encoder => "encoder output",
            Self::Predictor => "predictor output",
        }
    }

    #[inline]
    fn row_name(self) -> &'static str {
        match self {
            Self::Encoder => "frames",
            Self::Predictor => "positions",
        }
    }
}

/// Error returned by checked transducer output tensor constructors and accessors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransducerOutputError {
    /// `rows * dim` overflowed `usize`.
    ShapeSizeOverflow {
        /// Tensor kind.
        kind: TransducerOutputKind,
        /// Number of rows: frames for encoder output, positions for predictor output.
        rows: usize,
        /// Width of each row.
        dim: usize,
    },
    /// The flat data buffer does not match the declared tensor shape.
    DataLengthMismatch {
        /// Tensor kind.
        kind: TransducerOutputKind,
        /// Number of rows: frames for encoder output, positions for predictor output.
        rows: usize,
        /// Width of each row.
        dim: usize,
        /// Required flattened value count.
        expected: usize,
        /// Actual flattened value count.
        actual: usize,
    },
    /// Requested encoder frame is outside the tensor.
    FrameOutOfBounds {
        /// Requested frame index.
        frame: FrameIndex,
        /// Number of available frames.
        num_frames: usize,
    },
    /// Requested predictor position is outside the tensor.
    PositionOutOfBounds {
        /// Requested predictor position.
        position: usize,
        /// Number of available positions.
        num_positions: usize,
    },
}

impl fmt::Display for TransducerOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShapeSizeOverflow { kind, rows, dim } => write!(
                f,
                "{} shape overflows usize: {} {} x {} dimensions",
                kind.name(),
                rows,
                kind.row_name(),
                dim
            ),
            Self::DataLengthMismatch {
                kind,
                rows,
                dim,
                expected,
                actual,
            } => write!(
                f,
                "{} data length {} does not match shape {} {} x {} dimensions = {}",
                kind.name(),
                actual,
                rows,
                kind.row_name(),
                dim,
                expected
            ),
            Self::FrameOutOfBounds { frame, num_frames } => write!(
                f,
                "encoder frame {} is out of bounds for {} frames",
                frame, num_frames
            ),
            Self::PositionOutOfBounds {
                position,
                num_positions,
            } => write!(
                f,
                "predictor position {} is out of bounds for {} positions",
                position, num_positions
            ),
        }
    }
}

impl std::error::Error for TransducerOutputError {}

/// Error returned by checked transducer lattice constructors and accessors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransducerLatticeError {
    /// `num_frames * num_positions * vocab_size` overflowed `usize`.
    ShapeSizeOverflow {
        /// Number of time frames.
        num_frames: usize,
        /// Number of label positions.
        num_positions: usize,
        /// Vocabulary size including blank.
        vocab_size: usize,
    },
    /// The vocabulary cannot be represented by the [`Label`] type.
    VocabularyTooLarge {
        /// Supplied vocabulary size.
        vocab_size: usize,
        /// Largest supported vocabulary size.
        maximum: usize,
    },
    /// The flat log-probability buffer does not match the lattice shape.
    LogProbLengthMismatch {
        /// Required flattened value count.
        expected: usize,
        /// Actual flattened value count.
        actual: usize,
    },
    /// Requested time frame is outside the lattice.
    FrameOutOfBounds {
        /// Requested frame index.
        frame: usize,
        /// Number of available frames.
        num_frames: usize,
    },
    /// Requested label position is outside the lattice.
    PositionOutOfBounds {
        /// Requested label position.
        position: usize,
        /// Number of available label positions.
        num_positions: usize,
    },
    /// Requested label is outside the lattice vocabulary.
    LabelOutOfBounds {
        /// Requested label.
        label: Label,
        /// Vocabulary size including blank.
        vocab_size: usize,
    },
    /// A WFST conversion needs at least one label position.
    EmptyPositionAxis,
    /// WFST conversion would exceed the representable [`StateId`] range.
    StateCountOverflow {
        /// Number of time frames.
        num_frames: usize,
        /// Number of label positions.
        num_positions: usize,
    },
}

impl fmt::Display for TransducerLatticeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShapeSizeOverflow {
                num_frames,
                num_positions,
                vocab_size,
            } => write!(
                f,
                "transducer lattice shape overflows usize: {} frames x {} positions x {} labels",
                num_frames, num_positions, vocab_size
            ),
            Self::VocabularyTooLarge {
                vocab_size,
                maximum,
            } => write!(
                f,
                "transducer lattice vocabulary size {} exceeds maximum {}",
                vocab_size, maximum
            ),
            Self::LogProbLengthMismatch { expected, actual } => write!(
                f,
                "transducer lattice log-probability length {} does not match expected {}",
                actual, expected
            ),
            Self::FrameOutOfBounds { frame, num_frames } => write!(
                f,
                "transducer lattice frame {} is out of bounds for {} frames",
                frame, num_frames
            ),
            Self::PositionOutOfBounds {
                position,
                num_positions,
            } => write!(
                f,
                "transducer lattice position {} is out of bounds for {} positions",
                position, num_positions
            ),
            Self::LabelOutOfBounds { label, vocab_size } => write!(
                f,
                "transducer lattice label {} is out of bounds for vocabulary size {}",
                label, vocab_size
            ),
            Self::EmptyPositionAxis => {
                write!(
                    f,
                    "transducer lattice cannot convert zero positions to WFST"
                )
            }
            Self::StateCountOverflow {
                num_frames,
                num_positions,
            } => write!(
                f,
                "transducer lattice state count exceeds StateId range: {} frames x {} positions",
                num_frames, num_positions
            ),
        }
    }
}

impl std::error::Error for TransducerLatticeError {}

#[inline]
fn max_label_count() -> usize {
    Label::MAX as usize + 1
}

#[inline]
fn validate_vocab_size(vocab_size: usize) -> Result<(), TransducerLatticeError> {
    let maximum = max_label_count();
    if vocab_size > maximum {
        return Err(TransducerLatticeError::VocabularyTooLarge {
            vocab_size,
            maximum,
        });
    }

    Ok(())
}

#[inline]
fn checked_lattice_len(
    num_frames: usize,
    num_positions: usize,
    vocab_size: usize,
) -> Result<usize, TransducerLatticeError> {
    validate_vocab_size(vocab_size)?;

    let frame_positions =
        num_frames
            .checked_mul(num_positions)
            .ok_or(TransducerLatticeError::ShapeSizeOverflow {
                num_frames,
                num_positions,
                vocab_size,
            })?;

    frame_positions
        .checked_mul(vocab_size)
        .ok_or(TransducerLatticeError::ShapeSizeOverflow {
            num_frames,
            num_positions,
            vocab_size,
        })
}

#[inline]
fn validate_output_shape(
    kind: TransducerOutputKind,
    rows: usize,
    dim: usize,
    actual: usize,
) -> Result<(), TransducerOutputError> {
    let expected = rows
        .checked_mul(dim)
        .ok_or(TransducerOutputError::ShapeSizeOverflow { kind, rows, dim })?;

    if actual != expected {
        return Err(TransducerOutputError::DataLengthMismatch {
            kind,
            rows,
            dim,
            expected,
            actual,
        });
    }

    Ok(())
}

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
        Self::try_new(data, num_frames, dim).unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to create a new encoder output.
    ///
    /// The flattened data buffer must contain exactly `num_frames * dim`
    /// values. Empty tensors such as `0 x D` remain valid.
    pub fn try_new(
        data: Vec<f32>,
        num_frames: usize,
        dim: usize,
    ) -> Result<Self, TransducerOutputError> {
        validate_output_shape(TransducerOutputKind::Encoder, num_frames, dim, data.len())?;

        Ok(Self {
            data,
            num_frames,
            dim,
        })
    }

    /// Get the encoder output at time frame `t`.
    #[inline]
    pub fn frame(&self, t: FrameIndex) -> &[f32] {
        self.try_frame(t).unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to get the encoder output at time frame `t`.
    #[inline]
    pub fn try_frame(&self, t: FrameIndex) -> Result<&[f32], TransducerOutputError> {
        if t >= self.num_frames {
            return Err(TransducerOutputError::FrameOutOfBounds {
                frame: t,
                num_frames: self.num_frames,
            });
        }

        let start = t * self.dim;
        Ok(&self.data[start..start + self.dim])
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
    fn get_output<'a>(&self, predictor_out: &'a PredictorOutput, u: usize) -> &'a [f32];
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
        Self::try_new(data, num_positions, dim).unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to create a new predictor output.
    ///
    /// The flattened data buffer must contain exactly `num_positions * dim`
    /// values. Empty tensors such as `0 x D` remain valid.
    pub fn try_new(
        data: Vec<f32>,
        num_positions: usize,
        dim: usize,
    ) -> Result<Self, TransducerOutputError> {
        validate_output_shape(
            TransducerOutputKind::Predictor,
            num_positions,
            dim,
            data.len(),
        )?;

        Ok(Self {
            data,
            num_positions,
            dim,
        })
    }

    /// Get the predictor output at label position `u`.
    #[inline]
    pub fn position(&self, u: usize) -> &[f32] {
        self.try_position(u).unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to get the predictor output at label position `u`.
    #[inline]
    pub fn try_position(&self, u: usize) -> Result<&[f32], TransducerOutputError> {
        if u >= self.num_positions {
            return Err(TransducerOutputError::PositionOutOfBounds {
                position: u,
                num_positions: self.num_positions,
            });
        }

        let start = u * self.dim;
        Ok(&self.data[start..start + self.dim])
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
        Self::try_new(num_frames, num_positions, vocab_size).unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to create a new transducer lattice.
    ///
    /// The lattice stores `num_frames * num_positions * vocab_size`
    /// log-probabilities in row-major order. Empty shapes remain valid as data
    /// containers, but conversion to a WFST requires at least one position.
    pub fn try_new(
        num_frames: usize,
        num_positions: usize,
        vocab_size: usize,
    ) -> Result<Self, TransducerLatticeError> {
        let size = checked_lattice_len(num_frames, num_positions, vocab_size)?;

        Ok(Self {
            num_frames,
            num_positions,
            vocab_size,
            log_probs: vec![f64::NEG_INFINITY; size],
            _phantom: std::marker::PhantomData,
        })
    }

    /// Set log-probability at (t, u, label).
    #[inline]
    pub fn set(&mut self, t: usize, u: usize, label: Label, log_prob: f64) {
        self.try_set(t, u, label, log_prob)
            .unwrap_or_else(|err| panic!("{err}"));
    }

    /// Try to set log-probability at (t, u, label).
    #[inline]
    pub fn try_set(
        &mut self,
        t: usize,
        u: usize,
        label: Label,
        log_prob: f64,
    ) -> Result<(), TransducerLatticeError> {
        let idx = self.try_index(t, u, label)?;
        let Some(slot) = self.log_probs.get_mut(idx) else {
            let expected =
                checked_lattice_len(self.num_frames, self.num_positions, self.vocab_size)?;
            return Err(TransducerLatticeError::LogProbLengthMismatch {
                expected,
                actual: self.log_probs.len(),
            });
        };

        *slot = log_prob;
        Ok(())
    }

    /// Get log-probability at (t, u, label).
    #[inline]
    pub fn get(&self, t: usize, u: usize, label: Label) -> f64 {
        self.try_get(t, u, label)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to get log-probability at (t, u, label).
    #[inline]
    pub fn try_get(&self, t: usize, u: usize, label: Label) -> Result<f64, TransducerLatticeError> {
        let idx = self.try_index(t, u, label)?;
        match self.log_probs.get(idx).copied() {
            Some(log_prob) => Ok(log_prob),
            None => {
                let expected =
                    checked_lattice_len(self.num_frames, self.num_positions, self.vocab_size)?;
                Err(TransducerLatticeError::LogProbLengthMismatch {
                    expected,
                    actual: self.log_probs.len(),
                })
            }
        }
    }

    #[inline]
    fn try_index(&self, t: usize, u: usize, label: Label) -> Result<usize, TransducerLatticeError> {
        if t >= self.num_frames {
            return Err(TransducerLatticeError::FrameOutOfBounds {
                frame: t,
                num_frames: self.num_frames,
            });
        }
        if u >= self.num_positions {
            return Err(TransducerLatticeError::PositionOutOfBounds {
                position: u,
                num_positions: self.num_positions,
            });
        }
        validate_vocab_size(self.vocab_size)?;
        if (label as usize) >= self.vocab_size {
            return Err(TransducerLatticeError::LabelOutOfBounds {
                label,
                vocab_size: self.vocab_size,
            });
        }

        let position = t
            .checked_mul(self.num_positions)
            .and_then(|base| base.checked_add(u))
            .ok_or(TransducerLatticeError::ShapeSizeOverflow {
                num_frames: self.num_frames,
                num_positions: self.num_positions,
                vocab_size: self.vocab_size,
            })?;

        position
            .checked_mul(self.vocab_size)
            .and_then(|base| base.checked_add(label as usize))
            .ok_or(TransducerLatticeError::ShapeSizeOverflow {
                num_frames: self.num_frames,
                num_positions: self.num_positions,
                vocab_size: self.vocab_size,
            })
    }

    /// Get all log-probs at position (t, u).
    pub fn get_position(&self, t: usize, u: usize) -> &[f64] {
        self.try_get_position(t, u)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to get all log-probs at position (t, u).
    pub fn try_get_position(&self, t: usize, u: usize) -> Result<&[f64], TransducerLatticeError> {
        let start = self.try_index(t, u, BLANK)?;
        let end = start.checked_add(self.vocab_size).ok_or(
            TransducerLatticeError::ShapeSizeOverflow {
                num_frames: self.num_frames,
                num_positions: self.num_positions,
                vocab_size: self.vocab_size,
            },
        )?;
        match self.log_probs.get(start..end) {
            Some(log_probs) => Ok(log_probs),
            None => {
                let expected =
                    checked_lattice_len(self.num_frames, self.num_positions, self.vocab_size)?;
                Err(TransducerLatticeError::LogProbLengthMismatch {
                    expected,
                    actual: self.log_probs.len(),
                })
            }
        }
    }

    /// Convert to explicit WFST representation.
    ///
    /// Creates a WFST with states (t, u) and transitions labeled
    /// with vocabulary symbols and weighted by log-probabilities.
    pub fn to_wfst(&self) -> VectorWfst<Label, W>
    where
        W: From<f64> + Clone,
    {
        self.try_to_wfst().unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to convert to explicit WFST representation.
    pub fn try_to_wfst(&self) -> Result<VectorWfst<Label, W>, TransducerLatticeError>
    where
        W: From<f64> + Clone,
    {
        use crate::wfst::{MutableWfst, WeightedTransition};

        let mut fst: VectorWfst<Label, W> = VectorWfst::new();

        let num_states = self.checked_wfst_state_count()?;

        // Create states for each (t, u) position
        // State id = t * (num_positions) + u
        fst.add_states(num_states);

        // Start state is (0, 0)
        fst.set_start(0);

        // Final state is (T, U) - after processing all frames and labels
        let final_state = self.try_state_id(self.num_frames, self.num_positions - 1)?;
        fst.set_final(final_state, W::one());

        // Add transitions
        for t in 0..self.num_frames {
            for u in 0..self.num_positions {
                let from_state = self.try_state_id(t, u)?;

                // Blank transition: (t, u) -> (t+1, u)
                let blank_prob = self.try_get(t, u, BLANK)?;
                if blank_prob > f64::NEG_INFINITY {
                    let to_state = self.try_state_id(t + 1, u)?;
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
                    for label_idx in 1..self.vocab_size {
                        let label = Label::try_from(label_idx).map_err(|_| {
                            TransducerLatticeError::VocabularyTooLarge {
                                vocab_size: self.vocab_size,
                                maximum: max_label_count(),
                            }
                        })?;
                        let label_prob = self.try_get(t, u, label)?;
                        if label_prob > f64::NEG_INFINITY {
                            let to_state = self.try_state_id(t + 1, u + 1)?;
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

        Ok(fst)
    }

    #[inline]
    fn checked_wfst_state_count(&self) -> Result<usize, TransducerLatticeError> {
        if self.num_positions == 0 {
            return Err(TransducerLatticeError::EmptyPositionAxis);
        }

        let rows =
            self.num_frames
                .checked_add(1)
                .ok_or(TransducerLatticeError::StateCountOverflow {
                    num_frames: self.num_frames,
                    num_positions: self.num_positions,
                })?;
        let num_states = rows.checked_mul(self.num_positions).ok_or(
            TransducerLatticeError::StateCountOverflow {
                num_frames: self.num_frames,
                num_positions: self.num_positions,
            },
        )?;

        if num_states > max_label_count() {
            return Err(TransducerLatticeError::StateCountOverflow {
                num_frames: self.num_frames,
                num_positions: self.num_positions,
            });
        }

        Ok(num_states)
    }

    #[inline]
    fn try_state_id(&self, t: usize, u: usize) -> Result<StateId, TransducerLatticeError> {
        let state = t
            .checked_mul(self.num_positions)
            .and_then(|base| base.checked_add(u))
            .ok_or(TransducerLatticeError::StateCountOverflow {
                num_frames: self.num_frames,
                num_positions: self.num_positions,
            })?;

        StateId::try_from(state).map_err(|_| TransducerLatticeError::StateCountOverflow {
            num_frames: self.num_frames,
            num_positions: self.num_positions,
        })
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
    fn test_encoder_output_try_new_rejects_data_length_mismatch() {
        let err = EncoderOutput::try_new(vec![1.0], 2, 2).unwrap_err();

        assert_eq!(
            err,
            TransducerOutputError::DataLengthMismatch {
                kind: TransducerOutputKind::Encoder,
                rows: 2,
                dim: 2,
                expected: 4,
                actual: 1,
            }
        );
    }

    #[test]
    fn test_encoder_output_try_new_rejects_shape_overflow() {
        let err = EncoderOutput::try_new(Vec::new(), usize::MAX, 2).unwrap_err();

        assert_eq!(
            err,
            TransducerOutputError::ShapeSizeOverflow {
                kind: TransducerOutputKind::Encoder,
                rows: usize::MAX,
                dim: 2,
            }
        );
    }

    #[test]
    fn test_encoder_output_try_frame_rejects_out_of_bounds() {
        let encoder_out = EncoderOutput::new(vec![0.1, 0.2], 1, 2);

        assert_eq!(
            encoder_out.try_frame(1),
            Err(TransducerOutputError::FrameOutOfBounds {
                frame: 1,
                num_frames: 1,
            })
        );
    }

    #[test]
    #[should_panic(expected = "encoder frame 1 is out of bounds for 1 frames")]
    fn test_encoder_output_infallible_frame_preserves_panic_contract() {
        let encoder_out = EncoderOutput::new(vec![0.1, 0.2], 1, 2);

        encoder_out.frame(1);
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
    fn test_predictor_output_try_new_rejects_data_length_mismatch() {
        let err = PredictorOutput::try_new(vec![1.0], 2, 2).unwrap_err();

        assert_eq!(
            err,
            TransducerOutputError::DataLengthMismatch {
                kind: TransducerOutputKind::Predictor,
                rows: 2,
                dim: 2,
                expected: 4,
                actual: 1,
            }
        );
    }

    #[test]
    fn test_predictor_output_try_new_rejects_shape_overflow() {
        let err = PredictorOutput::try_new(Vec::new(), usize::MAX, 2).unwrap_err();

        assert_eq!(
            err,
            TransducerOutputError::ShapeSizeOverflow {
                kind: TransducerOutputKind::Predictor,
                rows: usize::MAX,
                dim: 2,
            }
        );
    }

    #[test]
    fn test_predictor_output_try_position_rejects_out_of_bounds() {
        let predictor_out = PredictorOutput::new(vec![0.1, 0.2], 1, 2);

        assert_eq!(
            predictor_out.try_position(1),
            Err(TransducerOutputError::PositionOutOfBounds {
                position: 1,
                num_positions: 1,
            })
        );
    }

    #[test]
    #[should_panic(expected = "predictor position 1 is out of bounds for 1 positions")]
    fn test_predictor_output_infallible_position_preserves_panic_contract() {
        let predictor_out = PredictorOutput::new(vec![0.1, 0.2], 1, 2);

        predictor_out.position(1);
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
    fn test_transducer_lattice_try_new_rejects_shape_overflow() {
        let err = TransducerLattice::<TropicalWeight>::try_new(usize::MAX, 2, 1).unwrap_err();

        assert_eq!(
            err,
            TransducerLatticeError::ShapeSizeOverflow {
                num_frames: usize::MAX,
                num_positions: 2,
                vocab_size: 1,
            }
        );
    }

    #[test]
    fn test_transducer_lattice_try_new_rejects_vocabulary_too_large() {
        let err =
            TransducerLattice::<TropicalWeight>::try_new(1, 1, max_label_count() + 1).unwrap_err();

        assert_eq!(
            err,
            TransducerLatticeError::VocabularyTooLarge {
                vocab_size: max_label_count() + 1,
                maximum: max_label_count(),
            }
        );
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
    fn test_transducer_lattice_try_set_get_rejects_out_of_bounds() {
        let mut lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(1, 1, 2);

        assert_eq!(
            lattice.try_set(1, 0, BLANK, -1.0),
            Err(TransducerLatticeError::FrameOutOfBounds {
                frame: 1,
                num_frames: 1,
            })
        );
        assert_eq!(
            lattice.try_get(0, 1, BLANK),
            Err(TransducerLatticeError::PositionOutOfBounds {
                position: 1,
                num_positions: 1,
            })
        );
        assert_eq!(
            lattice.try_get(0, 0, 2),
            Err(TransducerLatticeError::LabelOutOfBounds {
                label: 2,
                vocab_size: 2,
            })
        );
    }

    #[test]
    #[should_panic(expected = "transducer lattice label 2 is out of bounds for vocabulary size 2")]
    fn test_transducer_lattice_infallible_get_preserves_panic_contract() {
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(1, 1, 2);

        lattice.get(0, 0, 2);
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
    fn test_transducer_lattice_try_get_position_rejects_out_of_bounds() {
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(1, 1, 2);

        assert_eq!(
            lattice.try_get_position(0, 1),
            Err(TransducerLatticeError::PositionOutOfBounds {
                position: 1,
                num_positions: 1,
            })
        );
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
    fn test_transducer_lattice_try_to_wfst_rejects_empty_positions() {
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(0, 0, 0);

        assert_eq!(
            lattice.try_to_wfst().unwrap_err(),
            TransducerLatticeError::EmptyPositionAxis
        );
    }

    #[test]
    #[should_panic(expected = "transducer lattice cannot convert zero positions to WFST")]
    fn test_transducer_lattice_to_wfst_preserves_panic_contract() {
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(0, 0, 0);

        lattice.to_wfst();
    }

    #[test]
    fn test_transducer_lattice_try_to_wfst_rejects_state_count_overflow() {
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice {
            num_frames: StateId::MAX as usize + 1,
            num_positions: 1,
            vocab_size: 1,
            log_probs: Vec::new(),
            _phantom: std::marker::PhantomData,
        };

        assert_eq!(
            lattice.try_to_wfst().unwrap_err(),
            TransducerLatticeError::StateCountOverflow {
                num_frames: StateId::MAX as usize + 1,
                num_positions: 1,
            }
        );
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
