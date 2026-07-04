//! Beam search decoding for Neural Transducers.
//!
//! This module implements efficient beam search decoding with support for:
//! - External language model shallow fusion
//! - WFST-based contextual biasing
//! - Streaming (frame-synchronous) decoding

use super::{
    AutoregressivePredictor, EncoderOutput, JointNetwork, Label, PredictorState, TransducerConfig,
    TransducerStats, BLANK,
};
use crate::semiring::Semiring;
use crate::wfst::{StateId, VectorWfst, Wfst};
use std::cmp::Ordering;
use std::collections::HashMap;

/// Decoding hypothesis.
#[derive(Debug, Clone)]
pub struct Hypothesis {
    /// Emitted label sequence (excluding blank).
    pub labels: Vec<Label>,
    /// Cumulative score (log-probability).
    pub score: f32,
    /// Predictor state for continuing this hypothesis.
    pub predictor_state: PredictorState,
    /// Predictor output corresponding to this hypothesis's emitted label history.
    predictor_out: Vec<f32>,
    /// LM state if using external LM.
    pub lm_state: Option<StateId>,
    /// Internal state for frame-level tracking.
    timestep: usize,
}

impl Hypothesis {
    /// Create initial hypothesis.
    pub fn initial(predictor_state: PredictorState) -> Self {
        Self {
            labels: Vec::new(),
            score: 0.0,
            predictor_state,
            predictor_out: Vec::new(),
            lm_state: None,
            timestep: 0,
        }
    }

    /// Create initial hypothesis with a precomputed predictor output.
    pub fn initial_with_predictor_output(
        predictor_state: PredictorState,
        predictor_out: Vec<f32>,
    ) -> Self {
        Self {
            labels: Vec::new(),
            score: 0.0,
            predictor_state,
            predictor_out,
            lm_state: None,
            timestep: 0,
        }
    }

    /// Create initial hypothesis with LM.
    pub fn initial_with_lm(predictor_state: PredictorState, lm_start: StateId) -> Self {
        Self {
            labels: Vec::new(),
            score: 0.0,
            predictor_state,
            predictor_out: Vec::new(),
            lm_state: Some(lm_start),
            timestep: 0,
        }
    }

    /// Create initial hypothesis with LM and a precomputed predictor output.
    pub fn initial_with_lm_output(
        predictor_state: PredictorState,
        predictor_out: Vec<f32>,
        lm_start: StateId,
    ) -> Self {
        Self {
            labels: Vec::new(),
            score: 0.0,
            predictor_state,
            predictor_out,
            lm_state: Some(lm_start),
            timestep: 0,
        }
    }

    /// Extend hypothesis with a new label.
    pub fn extend(
        &self,
        label: Label,
        score_delta: f32,
        new_predictor_state: PredictorState,
    ) -> Self {
        self.extend_with_predictor_output(
            label,
            score_delta,
            new_predictor_state,
            self.predictor_out.clone(),
        )
    }

    /// Extend hypothesis with a new label and predictor output.
    pub fn extend_with_predictor_output(
        &self,
        label: Label,
        score_delta: f32,
        new_predictor_state: PredictorState,
        new_predictor_out: Vec<f32>,
    ) -> Self {
        let mut new_labels = self.labels.clone();
        if label != BLANK {
            new_labels.push(label);
        }
        Self {
            labels: new_labels,
            score: self.score + score_delta,
            predictor_state: new_predictor_state,
            predictor_out: new_predictor_out,
            lm_state: self.lm_state,
            timestep: self.timestep + 1,
        }
    }

    /// Extend hypothesis with LM state update.
    pub fn extend_with_lm(
        &self,
        label: Label,
        score_delta: f32,
        new_predictor_state: PredictorState,
        new_lm_state: StateId,
    ) -> Self {
        self.extend_with_lm_output(
            label,
            score_delta,
            new_predictor_state,
            self.predictor_out.clone(),
            new_lm_state,
        )
    }

    /// Extend hypothesis with LM state and predictor output updates.
    pub fn extend_with_lm_output(
        &self,
        label: Label,
        score_delta: f32,
        new_predictor_state: PredictorState,
        new_predictor_out: Vec<f32>,
        new_lm_state: StateId,
    ) -> Self {
        let mut new_labels = self.labels.clone();
        if label != BLANK {
            new_labels.push(label);
        }
        Self {
            labels: new_labels,
            score: self.score + score_delta,
            predictor_state: new_predictor_state,
            predictor_out: new_predictor_out,
            lm_state: Some(new_lm_state),
            timestep: self.timestep + 1,
        }
    }
}

impl PartialEq for Hypothesis {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}

impl Eq for Hypothesis {}

impl PartialOrd for Hypothesis {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hypothesis {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse order for max-heap (higher score = higher priority)
        self.score
            .partial_cmp(&other.score)
            .unwrap_or(Ordering::Equal)
            .reverse()
    }
}

/// Beam search decoder for neural transducers.
#[derive(Debug)]
pub struct TransducerDecoder<P: AutoregressivePredictor, J: JointNetwork> {
    predictor: P,
    joiner: J,
    config: TransducerConfig,
}

impl<P: AutoregressivePredictor, J: JointNetwork> TransducerDecoder<P, J> {
    /// Create a new decoder.
    pub fn new(predictor: P, joiner: J, config: TransducerConfig) -> Self {
        Self {
            predictor,
            joiner,
            config,
        }
    }

    /// Decode encoder output using greedy search.
    pub fn greedy_decode(&self, encoder_out: &EncoderOutput) -> DecodingResult {
        let mut labels = Vec::new();
        let mut score = 0.0f32;
        let (mut predictor_state, mut predictor_out) =
            self.predictor.step(&self.predictor.initial_state(), 0); // BOS token

        for t in 0..encoder_out.num_frames {
            let enc_frame = encoder_out.frame(t);

            // Limit symbols per frame (for streaming)
            let mut symbols_this_frame = 0;

            loop {
                // Compute log-probs via joiner
                let log_probs = self.joiner.forward(enc_frame, &predictor_out);

                // Find best label
                let Some((best_label, best_prob)) = log_probs
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(Ordering::Equal))
                    .map(|(i, &p)| (i as Label, p))
                else {
                    break;
                };

                score += best_prob;

                if best_label == BLANK {
                    // Blank: move to next frame
                    break;
                }

                // Non-blank: emit label and update predictor
                labels.push(best_label);
                let (new_state, new_out) = self.predictor.step(&predictor_state, best_label);
                predictor_state = new_state;
                predictor_out = new_out;

                symbols_this_frame += 1;
                if symbols_this_frame >= self.config.max_symbols_per_frame {
                    break;
                }
            }
        }

        DecodingResult {
            labels,
            score,
            stats: TransducerStats::default(),
        }
    }

    /// Decode encoder output using beam search.
    pub fn beam_decode(&self, encoder_out: &EncoderOutput) -> Vec<DecodingResult> {
        let beam_width = self.config.beam_width;
        let (initial_state, initial_out) = self.predictor.step(&self.predictor.initial_state(), 0);
        let mut hypotheses: Vec<Hypothesis> = vec![Hypothesis::initial_with_predictor_output(
            initial_state,
            initial_out,
        )];

        for t in 0..encoder_out.num_frames {
            let enc_frame = encoder_out.frame(t);
            let mut new_hypotheses: Vec<Hypothesis> = Vec::new();

            for hyp in &hypotheses {
                // Compute log-probs via joiner
                let log_probs = self.joiner.forward(enc_frame, &hyp.predictor_out);

                // Consider all possible extensions
                for (label, &log_prob) in log_probs.iter().enumerate() {
                    let label = label as Label;

                    if label == BLANK {
                        // Blank: keep same hypothesis but advance time
                        let new_hyp = hyp.extend(BLANK, log_prob, hyp.predictor_state.clone());
                        new_hypotheses.push(new_hyp);
                    } else {
                        // Non-blank: extend with new label
                        let (new_state, new_out) = self.predictor.step(&hyp.predictor_state, label);
                        let new_hyp =
                            hyp.extend_with_predictor_output(label, log_prob, new_state, new_out);
                        new_hypotheses.push(new_hyp);
                    }
                }
            }

            // Prune to beam width
            new_hypotheses.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
            new_hypotheses.truncate(beam_width);

            // Merge hypotheses with same label sequence
            hypotheses = merge_hypotheses(new_hypotheses);
        }

        // Convert to results
        hypotheses
            .into_iter()
            .map(|hyp| DecodingResult {
                labels: hyp.labels,
                score: hyp.score,
                stats: TransducerStats::default(),
            })
            .collect()
    }

    /// Decode with external language model (shallow fusion).
    pub fn beam_decode_with_lm<W>(
        &self,
        encoder_out: &EncoderOutput,
        lm: &VectorWfst<Label, W>,
        lm_weight: f32,
    ) -> Vec<DecodingResult>
    where
        W: Semiring + Into<f32> + Clone,
    {
        let beam_width = self.config.beam_width;
        let lm_start = lm.start();
        let (initial_state, initial_out) = self.predictor.step(&self.predictor.initial_state(), 0);
        let mut hypotheses: Vec<Hypothesis> = vec![Hypothesis::initial_with_lm_output(
            initial_state,
            initial_out,
            lm_start,
        )];

        for t in 0..encoder_out.num_frames {
            let enc_frame = encoder_out.frame(t);
            let mut new_hypotheses: Vec<Hypothesis> = Vec::new();

            for hyp in &hypotheses {
                // Compute acoustic log-probs
                let log_probs = self.joiner.forward(enc_frame, &hyp.predictor_out);

                // Get LM state
                let Some(lm_state) = hyp.lm_state else {
                    continue;
                };

                // Blank transition (no LM update)
                let Some(&blank_prob) = log_probs.get(BLANK as usize) else {
                    continue;
                };
                let new_hyp = hyp.extend(BLANK, blank_prob, hyp.predictor_state.clone());
                new_hypotheses.push(new_hyp);

                // Non-blank transitions with LM scores
                for tr in lm.transitions(lm_state) {
                    let label = match tr.input {
                        Some(l) => l,
                        None => continue, // Skip epsilon transitions
                    };
                    if label == 0 || label as usize >= log_probs.len() {
                        continue;
                    }

                    let acoustic_prob = log_probs[label as usize];
                    let lm_prob: f32 = tr.weight.clone().into();
                    let combined_prob = acoustic_prob + lm_weight * lm_prob;

                    let (new_pred_state, new_pred_out) =
                        self.predictor.step(&hyp.predictor_state, label);
                    let new_hyp = hyp.extend_with_lm_output(
                        label,
                        combined_prob,
                        new_pred_state,
                        new_pred_out,
                        tr.to,
                    );
                    new_hypotheses.push(new_hyp);
                }
            }

            // Prune to beam width
            new_hypotheses.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
            new_hypotheses.truncate(beam_width);
            hypotheses = merge_hypotheses(new_hypotheses);
        }

        // Add final LM scores
        for hyp in &mut hypotheses {
            if let Some(lm_state) = hyp.lm_state {
                if lm.is_final(lm_state) {
                    let final_weight: f32 = lm.final_weight(lm_state).into();
                    hyp.score += lm_weight * final_weight;
                }
            }
        }

        hypotheses
            .into_iter()
            .map(|hyp| DecodingResult {
                labels: hyp.labels,
                score: hyp.score,
                stats: TransducerStats::default(),
            })
            .collect()
    }
}

/// Merge hypotheses with the same label sequence.
fn merge_hypotheses(hypotheses: Vec<Hypothesis>) -> Vec<Hypothesis> {
    let mut merged: HashMap<Vec<Label>, Hypothesis> = HashMap::new();

    for hyp in hypotheses {
        merged
            .entry(hyp.labels.clone())
            .and_modify(|existing| {
                // Keep hypothesis with better score
                if hyp.score > existing.score {
                    *existing = hyp.clone();
                }
            })
            .or_insert(hyp);
    }

    merged.into_values().collect()
}

/// Result of transducer decoding.
#[derive(Debug, Clone)]
pub struct DecodingResult {
    /// Decoded label sequence.
    pub labels: Vec<Label>,
    /// Log-probability score.
    pub score: f32,
    /// Decoding statistics.
    pub stats: TransducerStats,
}

/// Streaming decoder for real-time applications.
#[derive(Debug)]
pub struct StreamingTransducerDecoder<P: AutoregressivePredictor, J: JointNetwork> {
    predictor: P,
    joiner: J,
    config: TransducerConfig,
    /// Current hypotheses.
    hypotheses: Vec<Hypothesis>,
    /// Frames processed so far.
    frames_processed: usize,
    /// Finalized output (emitted labels).
    finalized: Vec<Label>,
}

impl<P: AutoregressivePredictor, J: JointNetwork> StreamingTransducerDecoder<P, J> {
    /// Create a new streaming decoder.
    pub fn new(predictor: P, joiner: J, config: TransducerConfig) -> Self {
        let (initial_state, initial_out) = predictor.step(&predictor.initial_state(), 0);
        let initial_hyp = Hypothesis::initial_with_predictor_output(initial_state, initial_out);
        Self {
            predictor,
            joiner,
            config,
            hypotheses: vec![initial_hyp],
            frames_processed: 0,
            finalized: Vec::new(),
        }
    }

    /// Process a single encoder frame.
    pub fn process_frame(&mut self, enc_frame: &[f32]) -> Vec<Label> {
        let mut new_labels = Vec::new();
        let beam_width = self.config.beam_width;
        let mut new_hypotheses: Vec<Hypothesis> = Vec::new();

        for hyp in &self.hypotheses {
            // Compute log-probs
            let log_probs = self.joiner.forward(enc_frame, &hyp.predictor_out);

            // Process emissions
            for (label, &log_prob) in log_probs.iter().enumerate() {
                let label = label as Label;

                if label == BLANK {
                    let new_hyp = hyp.extend(BLANK, log_prob, hyp.predictor_state.clone());
                    new_hypotheses.push(new_hyp);
                } else {
                    let (new_state, new_out) = self.predictor.step(&hyp.predictor_state, label);
                    let new_hyp =
                        hyp.extend_with_predictor_output(label, log_prob, new_state, new_out);
                    new_hypotheses.push(new_hyp);
                }
            }
        }

        // Prune and merge
        new_hypotheses.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
        new_hypotheses.truncate(beam_width);
        self.hypotheses = merge_hypotheses(new_hypotheses);

        // Check for stable prefix (all top hypotheses agree)
        if !self.hypotheses.is_empty() {
            let first_labels = &self.hypotheses[0].labels;
            let prefix_len = self
                .hypotheses
                .iter()
                .skip(1)
                .fold(first_labels.len(), |acc, h| {
                    common_prefix_len(first_labels, &h.labels).min(acc)
                });

            // Finalize stable prefix
            if prefix_len > self.finalized.len() {
                new_labels = first_labels[self.finalized.len()..prefix_len].to_vec();
                self.finalized.extend_from_slice(&new_labels);
            }
        }

        self.frames_processed += 1;
        new_labels
    }

    /// Get final result after all frames.
    pub fn finalize(&self) -> DecodingResult {
        if let Some(best) = self
            .hypotheses
            .iter()
            .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(Ordering::Equal))
        {
            DecodingResult {
                labels: best.labels.clone(),
                score: best.score,
                stats: TransducerStats {
                    num_frames: self.frames_processed,
                    ..Default::default()
                },
            }
        } else {
            DecodingResult {
                labels: self.finalized.clone(),
                score: 0.0,
                stats: TransducerStats::default(),
            }
        }
    }

    /// Reset decoder state for a new utterance.
    pub fn reset(&mut self) {
        let (initial_state, initial_out) = self.predictor.step(&self.predictor.initial_state(), 0);
        self.hypotheses = vec![Hypothesis::initial_with_predictor_output(
            initial_state,
            initial_out,
        )];
        self.frames_processed = 0;
        self.finalized.clear();
    }
}

/// Compute length of common prefix between two label sequences.
fn common_prefix_len(a: &[Label], b: &[Label]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

#[cfg(test)]
mod tests {
    use super::super::traits::PredictorOutput;
    use super::*;

    #[derive(Debug)]
    struct CountingPredictor;

    impl AutoregressivePredictor for CountingPredictor {
        fn output_dim(&self) -> usize {
            1
        }

        fn initial_state(&self) -> PredictorState {
            PredictorState::default()
        }

        fn step(&self, state: &PredictorState, _token: Label) -> (PredictorState, Vec<f32>) {
            let mut next = state.clone();
            next.num_tokens += 1;
            (next.clone(), vec![next.num_tokens as f32])
        }

        fn get_output<'a>(&self, predictor_out: &'a PredictorOutput, u: usize) -> &'a [f32] {
            predictor_out.position(u)
        }
    }

    #[derive(Debug)]
    struct HistorySensitiveJoiner;

    impl JointNetwork for HistorySensitiveJoiner {
        fn vocab_size(&self) -> usize {
            3
        }

        fn forward(&self, _encoder_frame: &[f32], predictor_output: &[f32]) -> Vec<f32> {
            match predictor_output.first().copied().unwrap_or_default() as usize {
                0 | 1 => vec![-10.0, 0.0, -10.0],
                2 => vec![0.0, -10.0, -10.0],
                _ => vec![-10.0, -10.0, 0.0],
            }
        }
    }

    #[test]
    fn test_hypothesis_ordering() {
        let h1 = Hypothesis::initial_with_predictor_output(PredictorState::default(), vec![])
            .extend(BLANK, -1.0, PredictorState::default());
        let h2 = Hypothesis::initial_with_predictor_output(PredictorState::default(), vec![])
            .extend(BLANK, -2.0, PredictorState::default());

        // Higher score should come first in max-heap
        assert!(h1 < h2); // -1.0 > -2.0, so h1 has priority
    }

    #[test]
    fn beam_decode_reuses_stored_predictor_output() {
        let decoder = TransducerDecoder::new(
            CountingPredictor,
            HistorySensitiveJoiner,
            TransducerConfig {
                beam_width: 1,
                ..Default::default()
            },
        );
        let encoder_out = EncoderOutput::new(vec![0.0, 0.0], 2, 1);

        let results = decoder.beam_decode(&encoder_out);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].labels, vec![1]);
    }

    #[test]
    fn test_common_prefix_len() {
        assert_eq!(common_prefix_len(&[1, 2, 3], &[1, 2, 4]), 2);
        assert_eq!(common_prefix_len(&[1, 2, 3], &[1, 2, 3]), 3);
        assert_eq!(common_prefix_len(&[1, 2, 3], &[4, 5, 6]), 0);
        assert_eq!(common_prefix_len(&[], &[1, 2, 3]), 0);
    }
}
