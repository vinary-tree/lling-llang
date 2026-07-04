//! Joint network implementations for Neural Transducers.
//!
//! The joiner combines encoder and predictor outputs to produce
//! log-probabilities over the vocabulary.

use super::JointNetwork;
use std::fmt::{self, Debug};

/// Built-in joiner implementation kind used in [`JoinerError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinerKind {
    /// Feed-forward encoder/predictor fusion joiner.
    FeedForward,
    /// Factorized blank/vocabulary joiner.
    Factorized,
    /// Additive vocabulary-space joiner.
    Additive,
}

impl JoinerKind {
    #[inline]
    fn name(self) -> &'static str {
        match self {
            Self::FeedForward => "feed-forward joiner",
            Self::Factorized => "factorized joiner",
            Self::Additive => "additive joiner",
        }
    }
}

/// Joiner parameter tensor used in [`JoinerError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinerTensor {
    /// Feed-forward encoder projection weights.
    FeedForwardEncoderWeights,
    /// Feed-forward predictor projection weights.
    FeedForwardPredictorWeights,
    /// Feed-forward hidden bias.
    FeedForwardHiddenBias,
    /// Feed-forward output projection weights.
    FeedForwardOutputWeights,
    /// Feed-forward output bias.
    FeedForwardOutputBias,
    /// Factorized blank projection weights.
    FactorizedBlankWeights,
    /// Factorized vocabulary projection weights.
    FactorizedVocabularyWeights,
    /// Factorized vocabulary bias.
    FactorizedVocabularyBias,
}

impl JoinerTensor {
    #[inline]
    fn name(self) -> &'static str {
        match self {
            Self::FeedForwardEncoderWeights => "feed-forward encoder weights",
            Self::FeedForwardPredictorWeights => "feed-forward predictor weights",
            Self::FeedForwardHiddenBias => "feed-forward hidden bias",
            Self::FeedForwardOutputWeights => "feed-forward output weights",
            Self::FeedForwardOutputBias => "feed-forward output bias",
            Self::FactorizedBlankWeights => "factorized blank weights",
            Self::FactorizedVocabularyWeights => "factorized vocabulary weights",
            Self::FactorizedVocabularyBias => "factorized vocabulary bias",
        }
    }
}

/// Joiner input vector used in [`JoinerError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinerInput {
    /// Encoder frame vector.
    EncoderFrame,
    /// Predictor output vector.
    PredictorOutput,
}

impl JoinerInput {
    #[inline]
    fn name(self) -> &'static str {
        match self {
            Self::EncoderFrame => "encoder frame",
            Self::PredictorOutput => "predictor output",
        }
    }
}

/// Error returned by checked built-in joiner constructors and forward passes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinerError {
    /// `rows * cols` overflowed while sizing a parameter tensor.
    TensorSizeOverflow {
        /// Tensor being sized.
        tensor: JoinerTensor,
        /// Number of rows.
        rows: usize,
        /// Number of columns.
        cols: usize,
    },
    /// A supplied parameter vector does not match the declared shape.
    TensorLengthMismatch {
        /// Tensor being validated.
        tensor: JoinerTensor,
        /// Required value count.
        expected: usize,
        /// Actual value count.
        actual: usize,
    },
    /// A joiner was constructed with an invalid vocabulary size.
    VocabSizeTooSmall {
        /// Joiner kind.
        joiner: JoinerKind,
        /// Supplied vocabulary size.
        vocab_size: usize,
        /// Minimum valid vocabulary size.
        minimum: usize,
    },
    /// A forward input vector does not match the joiner dimension.
    InputLengthMismatch {
        /// Input being validated.
        input: JoinerInput,
        /// Required vector length.
        expected: usize,
        /// Actual vector length.
        actual: usize,
    },
    /// Batched forward input slices have different batch sizes.
    BatchLengthMismatch {
        /// Number of encoder frames.
        encoder_frames: usize,
        /// Number of predictor outputs.
        predictor_outputs: usize,
    },
}

impl fmt::Display for JoinerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TensorSizeOverflow { tensor, rows, cols } => write!(
                f,
                "{} shape overflows usize: {} x {}",
                tensor.name(),
                rows,
                cols
            ),
            Self::TensorLengthMismatch {
                tensor,
                expected,
                actual,
            } => write!(
                f,
                "{} length {} does not match expected {}",
                tensor.name(),
                actual,
                expected
            ),
            Self::VocabSizeTooSmall {
                joiner,
                vocab_size,
                minimum,
            } => write!(
                f,
                "{} vocabulary size {} is below minimum {}",
                joiner.name(),
                vocab_size,
                minimum
            ),
            Self::InputLengthMismatch {
                input,
                expected,
                actual,
            } => write!(
                f,
                "{} length {} does not match expected {}",
                input.name(),
                actual,
                expected
            ),
            Self::BatchLengthMismatch {
                encoder_frames,
                predictor_outputs,
            } => write!(
                f,
                "joiner batch has {} encoder frames but {} predictor outputs",
                encoder_frames, predictor_outputs
            ),
        }
    }
}

impl std::error::Error for JoinerError {}

#[inline]
fn checked_tensor_len(
    tensor: JoinerTensor,
    rows: usize,
    cols: usize,
) -> Result<usize, JoinerError> {
    rows.checked_mul(cols)
        .ok_or(JoinerError::TensorSizeOverflow { tensor, rows, cols })
}

#[inline]
fn validate_tensor_len(
    tensor: JoinerTensor,
    expected: usize,
    actual: usize,
) -> Result<(), JoinerError> {
    if actual != expected {
        return Err(JoinerError::TensorLengthMismatch {
            tensor,
            expected,
            actual,
        });
    }

    Ok(())
}

#[inline]
fn validate_input_len(
    input: JoinerInput,
    expected: usize,
    actual: usize,
) -> Result<(), JoinerError> {
    if actual != expected {
        return Err(JoinerError::InputLengthMismatch {
            input,
            expected,
            actual,
        });
    }

    Ok(())
}

#[inline]
fn validate_batch_len(
    encoder_frames: &[&[f32]],
    predictor_outputs: &[&[f32]],
) -> Result<(), JoinerError> {
    if encoder_frames.len() != predictor_outputs.len() {
        return Err(JoinerError::BatchLengthMismatch {
            encoder_frames: encoder_frames.len(),
            predictor_outputs: predictor_outputs.len(),
        });
    }

    Ok(())
}

/// Simple feedforward joiner network.
///
/// Computes: log_softmax(W * tanh(W_enc * enc + W_pred * pred + b) + b_out)
#[derive(Debug, Clone)]
pub struct FeedForwardJoiner {
    /// Vocabulary size (including blank).
    pub vocab_size: usize,
    /// Hidden dimension.
    pub hidden_dim: usize,
    /// Encoder projection weights [hidden_dim, enc_dim].
    pub w_enc: Vec<f32>,
    /// Predictor projection weights [hidden_dim, pred_dim].
    pub w_pred: Vec<f32>,
    /// Hidden bias [hidden_dim].
    pub b_hidden: Vec<f32>,
    /// Output projection weights [vocab_size, hidden_dim].
    pub w_out: Vec<f32>,
    /// Output bias [vocab_size].
    pub b_out: Vec<f32>,
    /// Encoder input dimension.
    pub enc_dim: usize,
    /// Predictor input dimension.
    pub pred_dim: usize,
}

impl FeedForwardJoiner {
    /// Create a new feedforward joiner with random initialization.
    pub fn new(vocab_size: usize, enc_dim: usize, pred_dim: usize, hidden_dim: usize) -> Self {
        Self::try_new(vocab_size, enc_dim, pred_dim, hidden_dim)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to create a new feedforward joiner with random initialization.
    ///
    /// All parameter tensor lengths are checked before allocation so impossible
    /// shapes fail deterministically instead of overflowing in release builds.
    pub fn try_new(
        vocab_size: usize,
        enc_dim: usize,
        pred_dim: usize,
        hidden_dim: usize,
    ) -> Result<Self, JoinerError> {
        let w_enc_len =
            checked_tensor_len(JoinerTensor::FeedForwardEncoderWeights, hidden_dim, enc_dim)?;
        let w_pred_len = checked_tensor_len(
            JoinerTensor::FeedForwardPredictorWeights,
            hidden_dim,
            pred_dim,
        )?;
        let w_out_len = checked_tensor_len(
            JoinerTensor::FeedForwardOutputWeights,
            vocab_size,
            hidden_dim,
        )?;

        Ok(Self {
            vocab_size,
            hidden_dim,
            w_enc: vec![0.0; w_enc_len],
            w_pred: vec![0.0; w_pred_len],
            b_hidden: vec![0.0; hidden_dim],
            w_out: vec![0.0; w_out_len],
            b_out: vec![0.0; vocab_size],
            enc_dim,
            pred_dim,
        })
    }

    /// Create from pre-trained weights.
    #[allow(clippy::too_many_arguments)]
    pub fn from_weights(
        vocab_size: usize,
        enc_dim: usize,
        pred_dim: usize,
        hidden_dim: usize,
        w_enc: Vec<f32>,
        w_pred: Vec<f32>,
        b_hidden: Vec<f32>,
        w_out: Vec<f32>,
        b_out: Vec<f32>,
    ) -> Self {
        Self::try_from_weights(
            vocab_size, enc_dim, pred_dim, hidden_dim, w_enc, w_pred, b_hidden, w_out, b_out,
        )
        .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to create a feedforward joiner from pre-trained weights.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_weights(
        vocab_size: usize,
        enc_dim: usize,
        pred_dim: usize,
        hidden_dim: usize,
        w_enc: Vec<f32>,
        w_pred: Vec<f32>,
        b_hidden: Vec<f32>,
        w_out: Vec<f32>,
        b_out: Vec<f32>,
    ) -> Result<Self, JoinerError> {
        let w_enc_len =
            checked_tensor_len(JoinerTensor::FeedForwardEncoderWeights, hidden_dim, enc_dim)?;
        let w_pred_len = checked_tensor_len(
            JoinerTensor::FeedForwardPredictorWeights,
            hidden_dim,
            pred_dim,
        )?;
        let w_out_len = checked_tensor_len(
            JoinerTensor::FeedForwardOutputWeights,
            vocab_size,
            hidden_dim,
        )?;

        validate_tensor_len(
            JoinerTensor::FeedForwardEncoderWeights,
            w_enc_len,
            w_enc.len(),
        )?;
        validate_tensor_len(
            JoinerTensor::FeedForwardPredictorWeights,
            w_pred_len,
            w_pred.len(),
        )?;
        validate_tensor_len(
            JoinerTensor::FeedForwardHiddenBias,
            hidden_dim,
            b_hidden.len(),
        )?;
        validate_tensor_len(
            JoinerTensor::FeedForwardOutputWeights,
            w_out_len,
            w_out.len(),
        )?;
        validate_tensor_len(JoinerTensor::FeedForwardOutputBias, vocab_size, b_out.len())?;

        Ok(Self {
            vocab_size,
            hidden_dim,
            w_enc,
            w_pred,
            b_hidden,
            w_out,
            b_out,
            enc_dim,
            pred_dim,
        })
    }

    /// Try to compute log-probabilities for a single `(t, u)` position.
    pub fn try_forward(
        &self,
        encoder_frame: &[f32],
        predictor_output: &[f32],
    ) -> Result<Vec<f32>, JoinerError> {
        validate_input_len(JoinerInput::EncoderFrame, self.enc_dim, encoder_frame.len())?;
        validate_input_len(
            JoinerInput::PredictorOutput,
            self.pred_dim,
            predictor_output.len(),
        )?;

        let mut hidden = self.b_hidden.clone();

        for (i, h) in hidden.iter_mut().enumerate() {
            let row_start = i * self.enc_dim;
            for (j, &enc) in encoder_frame.iter().enumerate() {
                *h += self.w_enc[row_start + j] * enc;
            }
        }

        for (i, h) in hidden.iter_mut().enumerate() {
            let row_start = i * self.pred_dim;
            for (j, &pred) in predictor_output.iter().enumerate() {
                *h += self.w_pred[row_start + j] * pred;
            }
        }

        for h in &mut hidden {
            *h = h.tanh();
        }

        let mut logits = self.b_out.clone();
        for (i, logit) in logits.iter_mut().enumerate() {
            let row_start = i * self.hidden_dim;
            for (j, &h) in hidden.iter().enumerate() {
                *logit += self.w_out[row_start + j] * h;
            }
        }

        Ok(log_softmax(&logits))
    }

    /// Try to compute log-probabilities for a batch of `(t, u)` positions.
    pub fn try_forward_batch(
        &self,
        encoder_frames: &[&[f32]],
        predictor_outputs: &[&[f32]],
    ) -> Result<Vec<Vec<f32>>, JoinerError> {
        validate_batch_len(encoder_frames, predictor_outputs)?;

        encoder_frames
            .iter()
            .zip(predictor_outputs.iter())
            .map(|(enc, pred)| self.try_forward(enc, pred))
            .collect()
    }
}

impl JointNetwork for FeedForwardJoiner {
    fn vocab_size(&self) -> usize {
        self.vocab_size
    }

    fn forward(&self, encoder_frame: &[f32], predictor_output: &[f32]) -> Vec<f32> {
        self.try_forward(encoder_frame, predictor_output)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    fn forward_batch(
        &self,
        encoder_frames: &[&[f32]],
        predictor_outputs: &[&[f32]],
    ) -> Vec<Vec<f32>> {
        self.try_forward_batch(encoder_frames, predictor_outputs)
            .unwrap_or_else(|err| panic!("{err}"))
    }
}

/// Factorized joiner for Factorized Neural Transducer (FNT).
///
/// In FNT, blank and vocabulary predictions are separated:
/// - Blank probability: sigmoid(W_blank * enc)
/// - Vocab probabilities: softmax(W_vocab * pred)
///
/// This allows the predictor to function as a pure language model.
#[derive(Debug, Clone)]
pub struct FactorizedJoiner {
    /// Vocabulary size (including blank).
    pub vocab_size: usize,
    /// Encoder dimension.
    pub enc_dim: usize,
    /// Predictor dimension.
    pub pred_dim: usize,
    /// Blank projection weights [enc_dim].
    pub w_blank: Vec<f32>,
    /// Blank bias.
    pub b_blank: f32,
    /// Vocabulary projection weights [(vocab_size-1), pred_dim].
    pub w_vocab: Vec<f32>,
    /// Vocabulary bias [vocab_size-1].
    pub b_vocab: Vec<f32>,
}

impl FactorizedJoiner {
    /// Create a new factorized joiner.
    pub fn new(vocab_size: usize, enc_dim: usize, pred_dim: usize) -> Self {
        Self::try_new(vocab_size, enc_dim, pred_dim).unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to create a new factorized joiner.
    ///
    /// The factorized form needs at least one blank label and one non-blank
    /// label so the blank/non-blank mixture remains normalized.
    pub fn try_new(
        vocab_size: usize,
        enc_dim: usize,
        pred_dim: usize,
    ) -> Result<Self, JoinerError> {
        if vocab_size < 2 {
            return Err(JoinerError::VocabSizeTooSmall {
                joiner: JoinerKind::Factorized,
                vocab_size,
                minimum: 2,
            });
        }

        let non_blank = vocab_size - 1;
        let w_vocab_len = checked_tensor_len(
            JoinerTensor::FactorizedVocabularyWeights,
            non_blank,
            pred_dim,
        )?;

        Ok(Self {
            vocab_size,
            enc_dim,
            pred_dim,
            w_blank: vec![0.0; enc_dim],
            b_blank: 0.0,
            w_vocab: vec![0.0; w_vocab_len],
            b_vocab: vec![0.0; non_blank],
        })
    }

    /// Create from pre-trained weights.
    pub fn from_weights(
        vocab_size: usize,
        enc_dim: usize,
        pred_dim: usize,
        w_blank: Vec<f32>,
        b_blank: f32,
        w_vocab: Vec<f32>,
        b_vocab: Vec<f32>,
    ) -> Self {
        Self::try_from_weights(
            vocab_size, enc_dim, pred_dim, w_blank, b_blank, w_vocab, b_vocab,
        )
        .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to create a factorized joiner from pre-trained weights.
    pub fn try_from_weights(
        vocab_size: usize,
        enc_dim: usize,
        pred_dim: usize,
        w_blank: Vec<f32>,
        b_blank: f32,
        w_vocab: Vec<f32>,
        b_vocab: Vec<f32>,
    ) -> Result<Self, JoinerError> {
        if vocab_size < 2 {
            return Err(JoinerError::VocabSizeTooSmall {
                joiner: JoinerKind::Factorized,
                vocab_size,
                minimum: 2,
            });
        }

        let non_blank = vocab_size - 1;
        let w_vocab_len = checked_tensor_len(
            JoinerTensor::FactorizedVocabularyWeights,
            non_blank,
            pred_dim,
        )?;

        validate_tensor_len(JoinerTensor::FactorizedBlankWeights, enc_dim, w_blank.len())?;
        validate_tensor_len(
            JoinerTensor::FactorizedVocabularyWeights,
            w_vocab_len,
            w_vocab.len(),
        )?;
        validate_tensor_len(
            JoinerTensor::FactorizedVocabularyBias,
            non_blank,
            b_vocab.len(),
        )?;

        Ok(Self {
            vocab_size,
            enc_dim,
            pred_dim,
            w_blank,
            b_blank,
            w_vocab,
            b_vocab,
        })
    }

    /// Compute blank probability from encoder.
    fn blank_prob(&self, encoder_frame: &[f32]) -> f32 {
        let mut logit = self.b_blank;
        for (w, &enc) in self.w_blank.iter().zip(encoder_frame.iter()) {
            logit += w * enc;
        }
        sigmoid(logit)
    }

    /// Compute vocabulary log-probabilities from predictor.
    fn vocab_log_probs(&self, predictor_output: &[f32]) -> Vec<f32> {
        let mut logits = self.b_vocab.clone();
        for (i, logit) in logits.iter_mut().enumerate() {
            for (j, &pred) in predictor_output.iter().enumerate() {
                *logit += self.w_vocab[i * self.pred_dim + j] * pred;
            }
        }
        log_softmax(&logits)
    }

    /// Try to compute log-probabilities for a single `(t, u)` position.
    pub fn try_forward(
        &self,
        encoder_frame: &[f32],
        predictor_output: &[f32],
    ) -> Result<Vec<f32>, JoinerError> {
        validate_input_len(JoinerInput::EncoderFrame, self.enc_dim, encoder_frame.len())?;
        validate_input_len(
            JoinerInput::PredictorOutput,
            self.pred_dim,
            predictor_output.len(),
        )?;

        let blank_p = self.blank_prob(encoder_frame);
        let vocab_log_probs = self.vocab_log_probs(predictor_output);
        let mut result = Vec::with_capacity(self.vocab_size);

        result.push(blank_p.ln());

        let non_blank_log = (1.0 - blank_p).ln();
        for lp in vocab_log_probs {
            result.push(non_blank_log + lp);
        }

        Ok(result)
    }

    /// Try to compute log-probabilities for a batch of `(t, u)` positions.
    pub fn try_forward_batch(
        &self,
        encoder_frames: &[&[f32]],
        predictor_outputs: &[&[f32]],
    ) -> Result<Vec<Vec<f32>>, JoinerError> {
        validate_batch_len(encoder_frames, predictor_outputs)?;

        encoder_frames
            .iter()
            .zip(predictor_outputs.iter())
            .map(|(enc, pred)| self.try_forward(enc, pred))
            .collect()
    }
}

impl JointNetwork for FactorizedJoiner {
    fn vocab_size(&self) -> usize {
        self.vocab_size
    }

    fn forward(&self, encoder_frame: &[f32], predictor_output: &[f32]) -> Vec<f32> {
        self.try_forward(encoder_frame, predictor_output)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    fn forward_batch(
        &self,
        encoder_frames: &[&[f32]],
        predictor_outputs: &[&[f32]],
    ) -> Vec<Vec<f32>> {
        self.try_forward_batch(encoder_frames, predictor_outputs)
            .unwrap_or_else(|err| panic!("{err}"))
    }
}

/// Additive joiner (simple addition of encoder and predictor).
///
/// Computes: log_softmax(enc + pred)
#[derive(Debug, Clone)]
pub struct AdditiveJoiner {
    /// Vocabulary size.
    pub vocab_size: usize,
}

impl AdditiveJoiner {
    /// Create a new additive joiner.
    pub fn new(vocab_size: usize) -> Self {
        Self { vocab_size }
    }

    /// Try to compute log-probabilities for a single `(t, u)` position.
    pub fn try_forward(
        &self,
        encoder_frame: &[f32],
        predictor_output: &[f32],
    ) -> Result<Vec<f32>, JoinerError> {
        validate_input_len(
            JoinerInput::EncoderFrame,
            self.vocab_size,
            encoder_frame.len(),
        )?;
        validate_input_len(
            JoinerInput::PredictorOutput,
            self.vocab_size,
            predictor_output.len(),
        )?;

        let logits: Vec<f32> = encoder_frame
            .iter()
            .zip(predictor_output.iter())
            .map(|(e, p)| e + p)
            .collect();
        Ok(log_softmax(&logits))
    }

    /// Try to compute log-probabilities for a batch of `(t, u)` positions.
    pub fn try_forward_batch(
        &self,
        encoder_frames: &[&[f32]],
        predictor_outputs: &[&[f32]],
    ) -> Result<Vec<Vec<f32>>, JoinerError> {
        validate_batch_len(encoder_frames, predictor_outputs)?;

        encoder_frames
            .iter()
            .zip(predictor_outputs.iter())
            .map(|(enc, pred)| self.try_forward(enc, pred))
            .collect()
    }
}

impl JointNetwork for AdditiveJoiner {
    fn vocab_size(&self) -> usize {
        self.vocab_size
    }

    fn forward(&self, encoder_frame: &[f32], predictor_output: &[f32]) -> Vec<f32> {
        self.try_forward(encoder_frame, predictor_output)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    fn forward_batch(
        &self,
        encoder_frames: &[&[f32]],
        predictor_outputs: &[&[f32]],
    ) -> Vec<Vec<f32>> {
        self.try_forward_batch(encoder_frames, predictor_outputs)
            .unwrap_or_else(|err| panic!("{err}"))
    }
}

/// Compute log-softmax of logits.
fn log_softmax(logits: &[f32]) -> Vec<f32> {
    if logits.is_empty() {
        return Vec::new();
    }

    // Find max for numerical stability
    let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

    // Compute log-sum-exp
    let log_sum_exp: f32 = logits
        .iter()
        .map(|&x| (x - max_logit).exp())
        .sum::<f32>()
        .ln()
        + max_logit;

    // Compute log-softmax
    logits.iter().map(|&x| x - log_sum_exp).collect()
}

/// Sigmoid activation.
#[inline]
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_softmax() {
        let logits = vec![1.0, 2.0, 3.0];
        let result = log_softmax(&logits);

        // Sum of exp(log_softmax) should be 1
        let sum: f32 = result.iter().map(|x| x.exp()).sum();
        assert!((sum - 1.0).abs() < 1e-5);

        // Should preserve ordering
        assert!(result[2] > result[1]);
        assert!(result[1] > result[0]);
    }

    #[test]
    fn test_sigmoid() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
        assert!(sigmoid(10.0) > 0.99);
        assert!(sigmoid(-10.0) < 0.01);
    }

    #[test]
    fn test_feedforward_joiner() {
        let joiner = FeedForwardJoiner::new(10, 256, 256, 128);
        let enc = vec![0.1; 256];
        let pred = vec![0.2; 256];

        let result = joiner.forward(&enc, &pred);
        assert_eq!(result.len(), 10);

        // Should be valid log-probs
        let sum: f32 = result.iter().map(|x| x.exp()).sum();
        assert!((sum - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_feedforward_joiner_try_new_rejects_shape_overflow() {
        let err = FeedForwardJoiner::try_new(2, 2, 1, usize::MAX).unwrap_err();

        assert_eq!(
            err,
            JoinerError::TensorSizeOverflow {
                tensor: JoinerTensor::FeedForwardEncoderWeights,
                rows: usize::MAX,
                cols: 2,
            }
        );
    }

    #[test]
    fn test_feedforward_joiner_try_from_weights_rejects_bad_lengths() {
        let err = FeedForwardJoiner::try_from_weights(
            2,
            2,
            2,
            2,
            vec![0.0; 3],
            vec![0.0; 4],
            vec![0.0; 2],
            vec![0.0; 4],
            vec![0.0; 2],
        )
        .unwrap_err();

        assert_eq!(
            err,
            JoinerError::TensorLengthMismatch {
                tensor: JoinerTensor::FeedForwardEncoderWeights,
                expected: 4,
                actual: 3,
            }
        );
    }

    #[test]
    fn test_feedforward_joiner_try_forward_rejects_input_length_mismatch() {
        let joiner = FeedForwardJoiner::new(2, 2, 3, 4);

        assert_eq!(
            joiner.try_forward(&[0.0], &[0.0, 0.0, 0.0]),
            Err(JoinerError::InputLengthMismatch {
                input: JoinerInput::EncoderFrame,
                expected: 2,
                actual: 1,
            })
        );
        assert_eq!(
            joiner.try_forward(&[0.0, 0.0], &[0.0, 0.0]),
            Err(JoinerError::InputLengthMismatch {
                input: JoinerInput::PredictorOutput,
                expected: 3,
                actual: 2,
            })
        );
    }

    #[test]
    #[should_panic(expected = "encoder frame length 1 does not match expected 2")]
    fn test_feedforward_joiner_forward_preserves_panic_contract() {
        let joiner = FeedForwardJoiner::new(2, 2, 3, 4);

        joiner.forward(&[0.0], &[0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_feedforward_joiner_try_forward_batch_rejects_batch_mismatch() {
        let joiner = FeedForwardJoiner::new(2, 1, 1, 1);
        let enc = [vec![0.0]];
        let pred = [vec![0.0], vec![0.0]];
        let enc_refs: Vec<&[f32]> = enc.iter().map(Vec::as_slice).collect();
        let pred_refs: Vec<&[f32]> = pred.iter().map(Vec::as_slice).collect();

        assert_eq!(
            joiner.try_forward_batch(&enc_refs, &pred_refs),
            Err(JoinerError::BatchLengthMismatch {
                encoder_frames: 1,
                predictor_outputs: 2,
            })
        );
    }

    #[test]
    fn test_factorized_joiner() {
        let joiner = FactorizedJoiner::new(10, 256, 256);
        let enc = vec![0.1; 256];
        let pred = vec![0.2; 256];

        let result = joiner.forward(&enc, &pred);
        assert_eq!(result.len(), 10);

        // Should be valid log-probs (approximately, due to factorization)
        for &lp in &result {
            assert!(lp <= 0.0);
            assert!(lp.is_finite());
        }
    }

    #[test]
    fn test_factorized_joiner_try_new_rejects_too_small_vocabulary() {
        let err = FactorizedJoiner::try_new(1, 2, 2).unwrap_err();

        assert_eq!(
            err,
            JoinerError::VocabSizeTooSmall {
                joiner: JoinerKind::Factorized,
                vocab_size: 1,
                minimum: 2,
            }
        );
    }

    #[test]
    fn test_factorized_joiner_try_new_rejects_shape_overflow() {
        let err = FactorizedJoiner::try_new(usize::MAX, 1, 2).unwrap_err();

        assert_eq!(
            err,
            JoinerError::TensorSizeOverflow {
                tensor: JoinerTensor::FactorizedVocabularyWeights,
                rows: usize::MAX - 1,
                cols: 2,
            }
        );
    }

    #[test]
    fn test_factorized_joiner_try_from_weights_rejects_bad_lengths() {
        let err = FactorizedJoiner::try_from_weights(
            3,
            2,
            2,
            vec![0.0; 1],
            0.0,
            vec![0.0; 4],
            vec![0.0; 2],
        )
        .unwrap_err();

        assert_eq!(
            err,
            JoinerError::TensorLengthMismatch {
                tensor: JoinerTensor::FactorizedBlankWeights,
                expected: 2,
                actual: 1,
            }
        );
    }

    #[test]
    fn test_factorized_joiner_try_forward_rejects_input_length_mismatch() {
        let joiner = FactorizedJoiner::new(3, 2, 3);

        assert_eq!(
            joiner.try_forward(&[0.0, 0.0], &[0.0, 0.0]),
            Err(JoinerError::InputLengthMismatch {
                input: JoinerInput::PredictorOutput,
                expected: 3,
                actual: 2,
            })
        );
    }

    #[test]
    fn test_additive_joiner_try_forward_rejects_truncated_inputs() {
        let joiner = AdditiveJoiner::new(3);

        assert_eq!(
            joiner.try_forward(&[0.0, 0.0], &[0.0, 0.0, 0.0]),
            Err(JoinerError::InputLengthMismatch {
                input: JoinerInput::EncoderFrame,
                expected: 3,
                actual: 2,
            })
        );
        assert_eq!(
            joiner.try_forward(&[0.0, 0.0, 0.0], &[0.0, 0.0]),
            Err(JoinerError::InputLengthMismatch {
                input: JoinerInput::PredictorOutput,
                expected: 3,
                actual: 2,
            })
        );
    }

    #[test]
    fn test_additive_joiner_try_forward_returns_full_vocabulary() {
        let joiner = AdditiveJoiner::new(3);
        let result = joiner
            .try_forward(&[1.0, 2.0, 3.0], &[0.5, 0.5, 0.5])
            .unwrap();

        assert_eq!(result.len(), 3);
        let sum: f32 = result.iter().map(|x| x.exp()).sum();
        assert!((sum - 1.0).abs() < 1e-5);
    }
}
