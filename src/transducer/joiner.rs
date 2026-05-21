//! Joint network implementations for Neural Transducers.
//!
//! The joiner combines encoder and predictor outputs to produce
//! log-probabilities over the vocabulary.

use super::JointNetwork;
use std::fmt::Debug;

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
        Self {
            vocab_size,
            hidden_dim,
            w_enc: vec![0.0; hidden_dim * enc_dim],
            w_pred: vec![0.0; hidden_dim * pred_dim],
            b_hidden: vec![0.0; hidden_dim],
            w_out: vec![0.0; vocab_size * hidden_dim],
            b_out: vec![0.0; vocab_size],
            enc_dim,
            pred_dim,
        }
    }

    /// Create from pre-trained weights.
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
        debug_assert_eq!(w_enc.len(), hidden_dim * enc_dim);
        debug_assert_eq!(w_pred.len(), hidden_dim * pred_dim);
        debug_assert_eq!(b_hidden.len(), hidden_dim);
        debug_assert_eq!(w_out.len(), vocab_size * hidden_dim);
        debug_assert_eq!(b_out.len(), vocab_size);

        Self {
            vocab_size,
            hidden_dim,
            w_enc,
            w_pred,
            b_hidden,
            w_out,
            b_out,
            enc_dim,
            pred_dim,
        }
    }
}

impl JointNetwork for FeedForwardJoiner {
    fn vocab_size(&self) -> usize {
        self.vocab_size
    }

    fn forward(&self, encoder_frame: &[f32], predictor_output: &[f32]) -> Vec<f32> {
        // Hidden layer: h = tanh(W_enc * enc + W_pred * pred + b)
        let mut hidden = self.b_hidden.clone();

        // Add encoder projection
        for (i, h) in hidden.iter_mut().enumerate() {
            for (j, &enc) in encoder_frame.iter().enumerate() {
                *h += self.w_enc[i * self.enc_dim + j] * enc;
            }
        }

        // Add predictor projection
        for (i, h) in hidden.iter_mut().enumerate() {
            for (j, &pred) in predictor_output.iter().enumerate() {
                *h += self.w_pred[i * self.pred_dim + j] * pred;
            }
        }

        // Apply tanh
        for h in &mut hidden {
            *h = h.tanh();
        }

        // Output layer: logits = W_out * h + b_out
        let mut logits = self.b_out.clone();
        for (i, logit) in logits.iter_mut().enumerate() {
            for (j, &h) in hidden.iter().enumerate() {
                *logit += self.w_out[i * self.hidden_dim + j] * h;
            }
        }

        // Log-softmax
        log_softmax(&logits)
    }

    fn forward_batch(
        &self,
        encoder_frames: &[&[f32]],
        predictor_outputs: &[&[f32]],
    ) -> Vec<Vec<f32>> {
        // Simple batched implementation (could be optimized with SIMD)
        encoder_frames
            .iter()
            .zip(predictor_outputs.iter())
            .map(|(enc, pred)| self.forward(enc, pred))
            .collect()
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
        Self {
            vocab_size,
            enc_dim,
            pred_dim,
            w_blank: vec![0.0; enc_dim],
            b_blank: 0.0,
            w_vocab: vec![0.0; (vocab_size - 1) * pred_dim],
            b_vocab: vec![0.0; vocab_size - 1],
        }
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
        debug_assert_eq!(w_blank.len(), enc_dim);
        debug_assert_eq!(w_vocab.len(), (vocab_size - 1) * pred_dim);
        debug_assert_eq!(b_vocab.len(), vocab_size - 1);

        Self {
            vocab_size,
            enc_dim,
            pred_dim,
            w_blank,
            b_blank,
            w_vocab,
            b_vocab,
        }
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
}

impl JointNetwork for FactorizedJoiner {
    fn vocab_size(&self) -> usize {
        self.vocab_size
    }

    fn forward(&self, encoder_frame: &[f32], predictor_output: &[f32]) -> Vec<f32> {
        let blank_p = self.blank_prob(encoder_frame);
        let vocab_log_probs = self.vocab_log_probs(predictor_output);

        // Combine: P(y) = P(blank) if y=blank else P(~blank) * P(y|~blank)
        let mut result = Vec::with_capacity(self.vocab_size);

        // Blank probability
        result.push(blank_p.ln());

        // Vocabulary probabilities (scaled by 1-blank_p)
        let non_blank_log = (1.0 - blank_p).ln();
        for lp in vocab_log_probs {
            result.push(non_blank_log + lp);
        }

        result
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
}

impl JointNetwork for AdditiveJoiner {
    fn vocab_size(&self) -> usize {
        self.vocab_size
    }

    fn forward(&self, encoder_frame: &[f32], predictor_output: &[f32]) -> Vec<f32> {
        // Assume encoder and predictor outputs are already vocab-sized
        let logits: Vec<f32> = encoder_frame
            .iter()
            .zip(predictor_output.iter())
            .map(|(e, p)| e + p)
            .collect();
        log_softmax(&logits)
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
}
