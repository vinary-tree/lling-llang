//! RNN-T loss computation via WFST (k2-style).
//!
//! This module implements differentiable transducer loss using the WFST framework,
//! following the approach of k2-fsa for efficient forward-backward computation.

use std::collections::{HashMap, VecDeque};

use super::{Label, TransducerLattice, BLANK};
use crate::semiring::Semiring;
use crate::wfst::{StateId, VectorWfst, Wfst};

/// Result of transducer loss computation.
#[derive(Debug, Clone)]
pub struct TransducerLossResult {
    /// Negative log-likelihood loss.
    pub loss: f64,
    /// Gradients with respect to log-probabilities at each (t, u, label).
    pub gradients: TransducerGradients,
    /// Forward scores at each state.
    pub forward_scores: Vec<f64>,
    /// Backward scores at each state.
    pub backward_scores: Vec<f64>,
}

/// Gradients for transducer lattice positions.
#[derive(Debug, Clone)]
pub struct TransducerGradients {
    /// Number of time frames.
    pub num_frames: usize,
    /// Number of label positions.
    pub num_positions: usize,
    /// Vocabulary size.
    pub vocab_size: usize,
    /// Gradient values: [T, U+1, V] flattened.
    pub data: Vec<f64>,
}

impl TransducerGradients {
    /// Create new gradients container.
    pub fn new(num_frames: usize, num_positions: usize, vocab_size: usize) -> Self {
        let size = num_frames * num_positions * vocab_size;
        Self {
            num_frames,
            num_positions,
            vocab_size,
            data: vec![0.0; size],
        }
    }

    /// Get gradient at (t, u, label).
    #[inline]
    pub fn get(&self, t: usize, u: usize, label: Label) -> f64 {
        let idx = (t * self.num_positions + u) * self.vocab_size + label as usize;
        self.data[idx]
    }

    /// Set gradient at (t, u, label).
    #[inline]
    pub fn set(&mut self, t: usize, u: usize, label: Label, value: f64) {
        let idx = (t * self.num_positions + u) * self.vocab_size + label as usize;
        self.data[idx] = value;
    }

    /// Add to gradient at (t, u, label).
    #[inline]
    pub fn add(&mut self, t: usize, u: usize, label: Label, value: f64) {
        let idx = (t * self.num_positions + u) * self.vocab_size + label as usize;
        self.data[idx] += value;
    }
}

/// Compute transducer loss for a single utterance.
///
/// This computes the negative log-likelihood:
///   L = -log P(y|x) = -log Σ_π P(π|x)
///
/// where the sum is over all valid alignments π that produce output y.
///
/// # Arguments
/// * `lattice` - Transducer lattice with log-probabilities
/// * `targets` - Target label sequence (without blank)
///
/// # Returns
/// Loss value and gradients with respect to log-probabilities.
pub fn transducer_loss<W>(lattice: &TransducerLattice<W>, targets: &[Label]) -> TransducerLossResult
where
    W: Semiring + From<f64> + Into<f64>,
{
    let t_len = lattice.num_frames;
    let u_len = targets.len() + 1; // +1 for start position

    // Forward pass: compute α[t, u] = log P(reach state (t,u) from (0,0))
    let mut alpha = vec![vec![f64::NEG_INFINITY; u_len]; t_len + 1];
    alpha[0][0] = 0.0;

    for t in 0..t_len {
        for u in 0..u_len {
            if alpha[t][u] <= f64::NEG_INFINITY {
                continue;
            }

            // Blank transition: (t, u) -> (t+1, u)
            let blank_prob = lattice.get(t, u, BLANK);
            let new_alpha = alpha[t][u] + blank_prob;
            alpha[t + 1][u] = log_add(alpha[t + 1][u], new_alpha);

            // Non-blank transition: (t, u) -> (t+1, u+1)
            if u < targets.len() {
                let label = targets[u];
                let label_prob = lattice.get(t, u, label);
                let new_alpha = alpha[t][u] + label_prob;
                alpha[t + 1][u + 1] = log_add(alpha[t + 1][u + 1], new_alpha);
            }
        }
    }

    // Total log-probability
    let total_log_prob = alpha[t_len][u_len - 1];

    // Backward pass: compute β[t, u] = log P(reach final from (t,u))
    let mut beta = vec![vec![f64::NEG_INFINITY; u_len]; t_len + 1];
    beta[t_len][u_len - 1] = 0.0;

    for t in (0..t_len).rev() {
        for u in (0..u_len).rev() {
            // Blank transition: (t, u) -> (t+1, u)
            if beta[t + 1][u] > f64::NEG_INFINITY {
                let blank_prob = lattice.get(t, u, BLANK);
                let new_beta = blank_prob + beta[t + 1][u];
                beta[t][u] = log_add(beta[t][u], new_beta);
            }

            // Non-blank transition: (t, u) -> (t+1, u+1)
            if u < targets.len() && beta[t + 1][u + 1] > f64::NEG_INFINITY {
                let label = targets[u];
                let label_prob = lattice.get(t, u, label);
                let new_beta = label_prob + beta[t + 1][u + 1];
                beta[t][u] = log_add(beta[t][u], new_beta);
            }
        }
    }

    // Compute gradients
    let mut gradients = TransducerGradients::new(t_len, u_len, lattice.vocab_size);

    for t in 0..t_len {
        for u in 0..u_len {
            if alpha[t][u] <= f64::NEG_INFINITY {
                continue;
            }

            // Gradient for blank
            if beta[t + 1][u] > f64::NEG_INFINITY {
                let blank_prob = lattice.get(t, u, BLANK);
                // grad = exp(α + log_prob + β - total) - exp(α + β - total)
                // For softmax outputs: grad = posterior - target_prob
                let posterior = (alpha[t][u] + blank_prob + beta[t + 1][u] - total_log_prob).exp();
                gradients.set(t, u, BLANK, -posterior);
            }

            // Gradient for target label
            if u < targets.len() && beta[t + 1][u + 1] > f64::NEG_INFINITY {
                let label = targets[u];
                let label_prob = lattice.get(t, u, label);
                let posterior =
                    (alpha[t][u] + label_prob + beta[t + 1][u + 1] - total_log_prob).exp();
                gradients.set(t, u, label, -posterior);
            }
        }
    }

    // Loss is negative log-probability
    let loss = -total_log_prob;

    // Flatten scores for output
    let forward_scores: Vec<f64> = alpha.into_iter().flatten().collect();
    let backward_scores: Vec<f64> = beta.into_iter().flatten().collect();

    TransducerLossResult {
        loss,
        gradients,
        forward_scores,
        backward_scores,
    }
}

/// Compute transducer loss with external language model.
///
/// This enables shallow fusion of neural transducer with n-gram LM:
///   L = -log Σ_π P_AM(π|x) * P_LM(y)^λ
///
/// # Arguments
/// * `lattice` - Transducer lattice with acoustic log-probabilities
/// * `targets` - Target label sequence
/// * `lm` - Language model as WFST
/// * `lm_weight` - Weight for LM scores (λ)
pub fn transducer_loss_with_lm<W>(
    lattice: &TransducerLattice<W>,
    targets: &[Label],
    lm: &VectorWfst<Label, W>,
    lm_weight: f64,
) -> TransducerLossResult
where
    W: Semiring + From<f64> + Into<f64> + Clone,
{
    let mut result = transducer_loss(lattice, targets);

    let lm_score = compute_lm_score(lm, targets);
    result.loss -= lm_weight * lm_score;

    result
}

/// Compute LM score for a target sequence.
fn compute_lm_score<W>(lm: &VectorWfst<Label, W>, targets: &[Label]) -> f64
where
    W: Semiring + Into<f64> + Clone,
{
    const OOV_LOG_SCORE: f64 = -10.0;

    if lm.is_empty() || !lm.is_valid_state(lm.start()) {
        return OOV_LOG_SCORE * targets.len() as f64;
    }

    let mut scores = HashMap::with_capacity(lm.num_states());
    scores.insert(lm.start(), 0.0);
    scores = epsilon_closure_scores(lm, scores);

    for &label in targets {
        let mut next_scores = HashMap::with_capacity(scores.len());

        for (&state, &score) in &scores {
            for tr in lm.transitions(state) {
                if tr.input == Some(label) && lm.is_valid_state(tr.to) {
                    let weight: f64 = tr.weight.into();
                    merge_log_score(&mut next_scores, tr.to, score + weight);
                }
            }
        }

        if next_scores.is_empty() {
            for (&state, &score) in &scores {
                merge_log_score(&mut next_scores, state, score + OOV_LOG_SCORE);
            }
        }

        scores = epsilon_closure_scores(lm, next_scores);
    }

    let mut final_score = f64::NEG_INFINITY;
    for (&state, &score) in &scores {
        if lm.is_final(state) {
            let final_weight: f64 = lm.final_weight(state).into();
            final_score = log_add(final_score, score + final_weight);
        }
    }

    if final_score > f64::NEG_INFINITY {
        final_score
    } else {
        scores.values().copied().fold(f64::NEG_INFINITY, log_add)
    }
}

fn epsilon_closure_scores<W>(
    lm: &VectorWfst<Label, W>,
    mut scores: HashMap<StateId, f64>,
) -> HashMap<StateId, f64>
where
    W: Semiring + Into<f64> + Clone,
{
    let mut queue: VecDeque<StateId> = scores.keys().copied().collect();
    let max_relaxations = lm
        .total_transitions()
        .saturating_add(lm.num_states())
        .saturating_mul(4)
        .max(1);
    let mut relaxations = 0usize;

    while let Some(state) = queue.pop_front() {
        if !lm.is_valid_state(state) {
            continue;
        }

        let state_score = scores.get(&state).copied().unwrap_or(f64::NEG_INFINITY);
        for tr in lm.transitions(state) {
            if tr.input.is_some() || !lm.is_valid_state(tr.to) {
                continue;
            }

            let weight: f64 = tr.weight.into();
            if merge_log_score(&mut scores, tr.to, state_score + weight) {
                relaxations += 1;
                if relaxations <= max_relaxations {
                    queue.push_back(tr.to);
                }
            }
        }
    }

    scores
}

fn merge_log_score(scores: &mut HashMap<StateId, f64>, state: StateId, score: f64) -> bool {
    let old_score = scores.get(&state).copied().unwrap_or(f64::NEG_INFINITY);
    let merged = log_add(old_score, score);
    if merged > old_score + 1e-12 {
        scores.insert(state, merged);
        true
    } else {
        false
    }
}

/// Batched transducer loss for multiple utterances.
pub fn transducer_loss_batch<W>(
    lattices: &[TransducerLattice<W>],
    targets_batch: &[Vec<Label>],
) -> Vec<TransducerLossResult>
where
    W: Semiring + From<f64> + Into<f64>,
{
    lattices
        .iter()
        .zip(targets_batch.iter())
        .map(|(lattice, targets)| transducer_loss(lattice, targets))
        .collect()
}

/// Log-add operation: log(exp(a) + exp(b))
#[inline]
fn log_add(a: f64, b: f64) -> f64 {
    if a == f64::NEG_INFINITY {
        b
    } else if b == f64::NEG_INFINITY {
        a
    } else if a > b {
        a + (1.0 + (b - a).exp()).ln()
    } else {
        b + (1.0 + (a - b).exp()).ln()
    }
}

/// Configuration for transducer loss computation.
#[derive(Debug, Clone)]
pub struct TransducerLossConfig {
    /// Regularization coefficient.
    pub regularization: f64,
    /// Whether to normalize by target length.
    pub normalize_by_length: bool,
    /// Label smoothing factor.
    pub label_smoothing: f64,
    /// LM weight for shallow fusion.
    pub lm_weight: f64,
}

impl Default for TransducerLossConfig {
    fn default() -> Self {
        Self {
            regularization: 0.0,
            normalize_by_length: true,
            label_smoothing: 0.0,
            lm_weight: 0.0,
        }
    }
}

/// Joiner-aware loss computation for factorized transducers.
///
/// In Factorized Neural Transducer (FNT), blank and vocabulary predictions
/// are separated, allowing the vocabulary predictor to function as a pure LM.
pub fn factorized_transducer_loss<W>(
    blank_logits: &[f64],      // [T] blank log-probs at each frame
    vocab_logits: &[Vec<f64>], // [U, V-1] vocabulary log-probs (excluding blank)
    targets: &[Label],
) -> TransducerLossResult
where
    W: Semiring + From<f64> + Into<f64>,
{
    let t_len = blank_logits.len();
    let u_len = targets.len() + 1;
    let vocab_size = vocab_logits.first().map_or(1, |v| v.len()) + 1;

    // Build lattice from factorized logits
    let mut lattice: TransducerLattice<W> = TransducerLattice::new(t_len, u_len, vocab_size);

    for t in 0..t_len {
        for u in 0..u_len {
            // Blank probability comes from blank predictor
            lattice.set(t, u, BLANK, blank_logits[t]);

            // Vocabulary probabilities come from vocab predictor
            // (shared across time frames in FNT)
            if u < vocab_logits.len() {
                for (v, &log_prob) in vocab_logits[u].iter().enumerate() {
                    lattice.set(t, u, (v + 1) as Label, log_prob);
                }
            }
        }
    }

    transducer_loss(&lattice, targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::MutableWfst;

    #[test]
    fn test_log_add() {
        assert!((log_add(0.0, 0.0) - 0.693).abs() < 0.01); // ln(2)
        assert!((log_add(f64::NEG_INFINITY, 0.0) - 0.0).abs() < 0.001);
        assert!((log_add(0.0, f64::NEG_INFINITY) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_transducer_loss_simple() {
        // Simple case: 2 frames, 1 target
        // Using proper log-probabilities (more negative to ensure valid distributions)
        let mut lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(2, 2, 3);

        // Set log-probs that represent valid distributions
        // At each position, these should be proper log-softmax outputs
        lattice.set(0, 0, BLANK, -1.5); // Blank probability
        lattice.set(0, 0, 1, -2.0); // Label 1 probability
        lattice.set(0, 0, 2, -3.0); // Label 2 probability
        lattice.set(1, 0, BLANK, -1.2);
        lattice.set(1, 0, 1, -1.8);
        lattice.set(1, 1, BLANK, -1.0);

        let targets = vec![1];
        let result = transducer_loss(&lattice, &targets);

        // Loss should be positive (negative log-prob) and finite
        assert!(
            result.loss > 0.0,
            "Loss should be positive, got {}",
            result.loss
        );
        assert!(result.loss.is_finite());
    }

    #[test]
    fn test_transducer_gradients() {
        let mut grads = TransducerGradients::new(2, 2, 3);

        grads.set(0, 0, 1, 0.5);
        assert!((grads.get(0, 0, 1) - 0.5).abs() < 1e-6);

        grads.add(0, 0, 1, 0.3);
        assert!((grads.get(0, 0, 1) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_transducer_loss_with_lm_log_sums_parallel_lm_paths() {
        let mut lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(1, 2, 3);
        lattice.set(0, 0, BLANK, -2.0);
        lattice.set(0, 0, 1, -0.1);
        lattice.set(0, 1, BLANK, -0.2);

        let mut lm: VectorWfst<Label, TropicalWeight> = VectorWfst::new();
        let s0 = lm.add_state();
        let s1 = lm.add_state();
        lm.set_start(s0);
        lm.set_final(s1, TropicalWeight::one());
        lm.add_arc(s0, Some(1), Some(1), s1, TropicalWeight::new(-0.2));
        lm.add_arc(s0, Some(1), Some(1), s1, TropicalWeight::new(-0.4));

        let targets = vec![1];
        let base = transducer_loss(&lattice, &targets);
        let fused = transducer_loss_with_lm(&lattice, &targets, &lm, 1.0);
        let expected_lm_score = log_add(-0.2, -0.4);

        assert!((fused.loss - (base.loss - expected_lm_score)).abs() < 1e-10);
    }

    #[test]
    fn test_lm_score_follows_chained_backoff_arcs() {
        let mut lm: VectorWfst<Label, TropicalWeight> = VectorWfst::new();
        let s0 = lm.add_state();
        let s1 = lm.add_state();
        let s2 = lm.add_state();
        let s3 = lm.add_state();
        lm.set_start(s0);
        lm.set_final(s3, TropicalWeight::one());
        lm.add_arc(s0, None, None, s1, TropicalWeight::new(-0.1));
        lm.add_arc(s1, None, None, s2, TropicalWeight::new(-0.2));
        lm.add_arc(s2, Some(1), Some(1), s3, TropicalWeight::new(-0.3));

        let score = compute_lm_score(&lm, &[1]);
        assert!((score - -0.6).abs() < 1e-10);
    }
}
