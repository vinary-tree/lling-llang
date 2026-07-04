//! RNN-T loss computation via WFST (k2-style).
//!
//! This module implements differentiable transducer loss using the WFST framework,
//! following the approach of k2-fsa for efficient forward-backward computation.

use std::collections::{HashMap, VecDeque};

use super::{Label, TransducerLattice, TransducerLatticeError, BLANK};
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
    ///
    /// # Panics
    /// Panics if `(t, u, label)` is outside the `[T, U+1, V]` gradient shape.
    /// Use [`TransducerGradients::try_get`] for a checked variant.
    #[inline]
    pub fn get(&self, t: usize, u: usize, label: Label) -> f64 {
        self.try_get(t, u, label)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to get the gradient at (t, u, label).
    #[inline]
    pub fn try_get(&self, t: usize, u: usize, label: Label) -> Result<f64, TransducerLatticeError> {
        let idx = self.try_index(t, u, label)?;
        self.data
            .get(idx)
            .copied()
            .ok_or_else(|| self.length_mismatch())
    }

    /// Set gradient at (t, u, label).
    ///
    /// # Panics
    /// Panics if `(t, u, label)` is outside the `[T, U+1, V]` gradient shape.
    /// Use [`TransducerGradients::try_set`] for a checked variant.
    #[inline]
    pub fn set(&mut self, t: usize, u: usize, label: Label, value: f64) {
        self.try_set(t, u, label, value)
            .unwrap_or_else(|err| panic!("{err}"));
    }

    /// Try to set the gradient at (t, u, label).
    #[inline]
    pub fn try_set(
        &mut self,
        t: usize,
        u: usize,
        label: Label,
        value: f64,
    ) -> Result<(), TransducerLatticeError> {
        let idx = self.try_index(t, u, label)?;
        match self.data.get_mut(idx) {
            Some(slot) => {
                *slot = value;
                Ok(())
            }
            None => Err(self.length_mismatch()),
        }
    }

    /// Add to gradient at (t, u, label).
    ///
    /// # Panics
    /// Panics if `(t, u, label)` is outside the `[T, U+1, V]` gradient shape.
    /// Use [`TransducerGradients::try_add`] for a checked variant.
    #[inline]
    pub fn add(&mut self, t: usize, u: usize, label: Label, value: f64) {
        self.try_add(t, u, label, value)
            .unwrap_or_else(|err| panic!("{err}"));
    }

    /// Try to add to the gradient at (t, u, label).
    #[inline]
    pub fn try_add(
        &mut self,
        t: usize,
        u: usize,
        label: Label,
        value: f64,
    ) -> Result<(), TransducerLatticeError> {
        let idx = self.try_index(t, u, label)?;
        match self.data.get_mut(idx) {
            Some(slot) => {
                *slot += value;
                Ok(())
            }
            None => Err(self.length_mismatch()),
        }
    }

    /// Bounds-check `(t, u, label)` against the `[T, U+1, V]` shape and return
    /// the flat index. Mirrors [`TransducerLattice::try_index`] so a stray
    /// index can never silently read or corrupt a different `(t, u, label)`
    /// cell (findings share `TransducerLatticeError` since the tensor shape and
    /// bounds are identical).
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
        if (label as usize) >= self.vocab_size {
            return Err(TransducerLatticeError::LabelOutOfBounds {
                label,
                vocab_size: self.vocab_size,
            });
        }

        t.checked_mul(self.num_positions)
            .and_then(|base| base.checked_add(u))
            .and_then(|position| {
                position
                    .checked_mul(self.vocab_size)
                    .and_then(|base| base.checked_add(label as usize))
            })
            .ok_or(TransducerLatticeError::ShapeSizeOverflow {
                num_frames: self.num_frames,
                num_positions: self.num_positions,
                vocab_size: self.vocab_size,
            })
    }

    /// The gradient buffer length disagrees with the declared shape.
    #[inline]
    fn length_mismatch(&self) -> TransducerLatticeError {
        TransducerLatticeError::LogProbLengthMismatch {
            expected: self
                .num_frames
                .saturating_mul(self.num_positions)
                .saturating_mul(self.vocab_size),
            actual: self.data.len(),
        }
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
///
/// # Panics
/// Panics if `BLANK` or any target label is outside `lattice.vocab_size`. Use
/// [`try_transducer_loss`] for a checked variant that reports the offending
/// label instead of panicking.
pub fn transducer_loss<W>(lattice: &TransducerLattice<W>, targets: &[Label]) -> TransducerLossResult
where
    W: Semiring + From<f64> + Into<f64>,
{
    try_transducer_loss(lattice, targets).unwrap_or_else(|err| panic!("{err}"))
}

/// Fallible [`transducer_loss`]: validates that blank and every target label
/// lie inside the lattice vocabulary before running the forward/backward
/// passes, so an out-of-vocabulary target (e.g. a tokenizer/vocab mismatch)
/// returns [`TransducerLatticeError::LabelOutOfBounds`] instead of panicking.
pub fn try_transducer_loss<W>(
    lattice: &TransducerLattice<W>,
    targets: &[Label],
) -> Result<TransducerLossResult, TransducerLatticeError>
where
    W: Semiring + From<f64> + Into<f64>,
{
    // Validate every label the passes will index (blank plus each target)
    // against the vocabulary up front. After this, no internal `lattice.get`
    // or `gradients.set` can be reached with an out-of-range label.
    if (BLANK as usize) >= lattice.vocab_size {
        return Err(TransducerLatticeError::LabelOutOfBounds {
            label: BLANK,
            vocab_size: lattice.vocab_size,
        });
    }
    for &label in targets {
        if (label as usize) >= lattice.vocab_size {
            return Err(TransducerLatticeError::LabelOutOfBounds {
                label,
                vocab_size: lattice.vocab_size,
            });
        }
    }

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

    Ok(TransducerLossResult {
        loss,
        gradients,
        forward_scores,
        backward_scores,
    })
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
///
/// # Panics
/// Panics if any target label is outside `lattice.vocab_size`; use
/// [`try_transducer_loss_with_lm`] for a checked variant.
pub fn transducer_loss_with_lm<W>(
    lattice: &TransducerLattice<W>,
    targets: &[Label],
    lm: &VectorWfst<Label, W>,
    lm_weight: f64,
) -> TransducerLossResult
where
    W: Semiring + From<f64> + Into<f64> + Clone,
{
    try_transducer_loss_with_lm(lattice, targets, lm, lm_weight)
        .unwrap_or_else(|err| panic!("{err}"))
}

/// Fallible [`transducer_loss_with_lm`]: forwards the target/vocab validation
/// of [`try_transducer_loss`] before applying the LM shallow-fusion term.
pub fn try_transducer_loss_with_lm<W>(
    lattice: &TransducerLattice<W>,
    targets: &[Label],
    lm: &VectorWfst<Label, W>,
    lm_weight: f64,
) -> Result<TransducerLossResult, TransducerLatticeError>
where
    W: Semiring + From<f64> + Into<f64> + Clone,
{
    let mut result = try_transducer_loss(lattice, targets)?;

    let lm_score = compute_lm_score(lm, targets);
    result.loss -= lm_weight * lm_score;

    Ok(result)
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
///
/// # Panics
/// Panics if any utterance has an out-of-vocabulary target; use
/// [`try_transducer_loss_batch`] for a checked variant.
pub fn transducer_loss_batch<W>(
    lattices: &[TransducerLattice<W>],
    targets_batch: &[Vec<Label>],
) -> Vec<TransducerLossResult>
where
    W: Semiring + From<f64> + Into<f64>,
{
    try_transducer_loss_batch(lattices, targets_batch).unwrap_or_else(|err| panic!("{err}"))
}

/// Fallible [`transducer_loss_batch`]: returns the first
/// [`TransducerLatticeError`] instead of panicking when an utterance has an
/// out-of-vocabulary target.
pub fn try_transducer_loss_batch<W>(
    lattices: &[TransducerLattice<W>],
    targets_batch: &[Vec<Label>],
) -> Result<Vec<TransducerLossResult>, TransducerLatticeError>
where
    W: Semiring + From<f64> + Into<f64>,
{
    lattices
        .iter()
        .zip(targets_batch.iter())
        .map(|(lattice, targets)| try_transducer_loss(lattice, targets))
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
    try_factorized_transducer_loss::<W>(blank_logits, vocab_logits, targets)
        .unwrap_or_else(|err| panic!("{err}"))
}

/// Fallible [`factorized_transducer_loss`]: validates that `vocab_logits` is
/// rectangular (every row shares the first row's `V-1` length) and that all
/// targets are in-vocabulary, so a jagged predictor output or out-of-vocab
/// target returns a [`TransducerLatticeError`] instead of panicking.
pub fn try_factorized_transducer_loss<W>(
    blank_logits: &[f64],
    vocab_logits: &[Vec<f64>],
    targets: &[Label],
) -> Result<TransducerLossResult, TransducerLatticeError>
where
    W: Semiring + From<f64> + Into<f64>,
{
    let t_len = blank_logits.len();
    let u_len = targets.len() + 1;
    // Preserve the original vocabulary-size derivation (first row width + 1,
    // defaulting to 2 when there are no vocab rows) so valid inputs are
    // unchanged; `vocab_row` is the required per-row width for the check below.
    let vocab_size = vocab_logits.first().map_or(1, |v| v.len()) + 1;
    let vocab_row = vocab_size - 1;

    // Reject jagged vocab_logits: a row longer than the first would index
    // label (v + 1) >= vocab_size and panic in the unchecked path.
    for row in vocab_logits {
        if row.len() != vocab_row {
            return Err(TransducerLatticeError::LogProbLengthMismatch {
                expected: vocab_row,
                actual: row.len(),
            });
        }
    }

    // Build the lattice with checked construction and writes; target
    // validation happens inside try_transducer_loss below.
    let mut lattice: TransducerLattice<W> = TransducerLattice::try_new(t_len, u_len, vocab_size)?;

    for t in 0..t_len {
        for u in 0..u_len {
            // Blank probability comes from blank predictor.
            lattice.try_set(t, u, BLANK, blank_logits[t])?;

            // Vocabulary probabilities come from vocab predictor
            // (shared across time frames in FNT).
            if u < vocab_logits.len() {
                for (v, &log_prob) in vocab_logits[u].iter().enumerate() {
                    lattice.try_set(t, u, (v + 1) as Label, log_prob)?;
                }
            }
        }
    }

    try_transducer_loss(&lattice, targets)
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

    #[test]
    fn test_try_transducer_loss_rejects_out_of_vocab_target() {
        // vocab_size = 3 → valid labels are 0, 1, 2; target 5 is out of range.
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(2, 2, 3);
        let err = try_transducer_loss(&lattice, &[5]).unwrap_err();
        assert!(matches!(
            err,
            TransducerLatticeError::LabelOutOfBounds {
                label: 5,
                vocab_size: 3
            }
        ));
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn test_transducer_loss_panics_on_out_of_vocab_target() {
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(2, 2, 3);
        let _ = transducer_loss(&lattice, &[5]);
    }

    #[test]
    fn test_try_transducer_loss_accepts_in_vocab_target() {
        let mut lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(2, 2, 3);
        lattice.set(0, 0, BLANK, -1.5);
        lattice.set(0, 0, 1, -2.0);
        lattice.set(1, 0, BLANK, -1.2);
        lattice.set(1, 1, BLANK, -1.0);
        let result = try_transducer_loss(&lattice, &[1]).expect("in-vocab target");
        assert!(result.loss.is_finite());
    }

    #[test]
    fn test_transducer_gradients_try_reject_out_of_range() {
        let mut grads = TransducerGradients::new(2, 2, 3);
        // Valid round-trip still works.
        grads.try_set(1, 1, 2, 0.5).expect("valid index");
        assert!((grads.try_get(1, 1, 2).expect("valid index") - 0.5).abs() < 1e-9);
        // Out-of-range frame / position / label are each rejected.
        assert!(matches!(
            grads.try_get(9, 0, 0).unwrap_err(),
            TransducerLatticeError::FrameOutOfBounds { .. }
        ));
        assert!(matches!(
            grads.try_set(0, 9, 0, 1.0).unwrap_err(),
            TransducerLatticeError::PositionOutOfBounds { .. }
        ));
        assert!(matches!(
            grads.try_add(0, 0, 7, 1.0).unwrap_err(),
            TransducerLatticeError::LabelOutOfBounds { .. }
        ));
    }

    #[test]
    fn test_transducer_gradients_reject_silent_corruption() {
        // num_positions = 2, vocab_size = 3 → flat len 12. The old unchecked
        // code mapped (t=0, u=2, label=0) to flat idx 6 — an in-range but WRONG
        // cell, namely (t=1, u=0, label=0) — and silently overwrote it.
        let mut grads = TransducerGradients::new(2, 2, 3);
        assert!(matches!(
            grads.try_set(0, 2, 0, 42.0).unwrap_err(),
            TransducerLatticeError::PositionOutOfBounds { .. }
        ));
        // Nothing was written: the (t=1, u=0, label=0) cell is still zero.
        assert_eq!(grads.get(1, 0, 0), 0.0);
    }

    #[test]
    fn test_try_factorized_rejects_jagged_vocab_logits() {
        let blank_logits = vec![-1.0, -1.0];
        // Row 0 has width 2, row 1 has width 3 → jagged.
        let vocab_logits = vec![vec![-1.0, -2.0], vec![-1.0, -2.0, -3.0]];
        let err =
            try_factorized_transducer_loss::<TropicalWeight>(&blank_logits, &vocab_logits, &[1])
                .unwrap_err();
        assert!(matches!(
            err,
            TransducerLatticeError::LogProbLengthMismatch {
                expected: 2,
                actual: 3
            }
        ));
    }

    #[test]
    fn test_try_factorized_accepts_rectangular_vocab_logits() {
        let blank_logits = vec![-1.0, -1.0];
        let vocab_logits = vec![vec![-1.0, -2.0], vec![-1.5, -2.5]];
        let result =
            try_factorized_transducer_loss::<TropicalWeight>(&blank_logits, &vocab_logits, &[1])
                .expect("rectangular vocab_logits");
        assert!(result.loss.is_finite());
    }

    #[test]
    fn test_try_transducer_loss_batch_reports_out_of_vocab() {
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(2, 2, 3);
        let lattices = vec![lattice];
        let targets_batch = vec![vec![5]];
        assert!(matches!(
            try_transducer_loss_batch(&lattices, &targets_batch).unwrap_err(),
            TransducerLatticeError::LabelOutOfBounds { .. }
        ));
    }
}
