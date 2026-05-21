//! Transducer lattice construction and manipulation.

use super::{
    EncoderOutput, JointNetwork, Label, PredictorOutput, TransducerConfig, TransducerLattice, BLANK,
};
use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst};

/// Builder for transducer lattices from neural network outputs.
#[derive(Debug)]
pub struct TransducerLatticeBuilder {
    config: TransducerConfig,
}

impl TransducerLatticeBuilder {
    /// Create a new lattice builder with default config.
    pub fn new() -> Self {
        Self {
            config: TransducerConfig::default(),
        }
    }

    /// Create with custom config.
    pub fn with_config(config: TransducerConfig) -> Self {
        Self { config }
    }

    /// Build lattice from encoder and predictor outputs using joiner.
    pub fn build<W, J>(
        &self,
        encoder_out: &EncoderOutput,
        predictor_out: &PredictorOutput,
        joiner: &J,
    ) -> TransducerLattice<W>
    where
        W: Semiring + From<f64>,
        J: JointNetwork,
    {
        let t_len = encoder_out.num_frames;
        let u_len = predictor_out.num_positions;
        let vocab_size = joiner.vocab_size();

        let mut lattice = TransducerLattice::new(t_len, u_len, vocab_size);

        if self.config.use_batch_joiner {
            // Batch computation for efficiency
            self.build_batched(&mut lattice, encoder_out, predictor_out, joiner);
        } else {
            // Sequential computation
            self.build_sequential(&mut lattice, encoder_out, predictor_out, joiner);
        }

        lattice
    }

    fn build_sequential<W, J>(
        &self,
        lattice: &mut TransducerLattice<W>,
        encoder_out: &EncoderOutput,
        predictor_out: &PredictorOutput,
        joiner: &J,
    ) where
        W: Semiring,
        J: JointNetwork,
    {
        for t in 0..encoder_out.num_frames {
            let enc_frame = encoder_out.frame(t);
            for u in 0..predictor_out.num_positions {
                let pred_out = predictor_out.position(u);
                let log_probs = joiner.forward(enc_frame, pred_out);
                for (label, &log_prob) in log_probs.iter().enumerate() {
                    lattice.set(t, u, label as Label, log_prob as f64);
                }
            }
        }
    }

    fn build_batched<W, J>(
        &self,
        lattice: &mut TransducerLattice<W>,
        encoder_out: &EncoderOutput,
        predictor_out: &PredictorOutput,
        joiner: &J,
    ) where
        W: Semiring,
        J: JointNetwork,
    {
        // Collect frames for batched processing
        let batch_size = 64; // Process 64 positions at a time

        let mut enc_frames: Vec<&[f32]> = Vec::with_capacity(batch_size);
        let mut pred_outs: Vec<&[f32]> = Vec::with_capacity(batch_size);
        let mut positions: Vec<(usize, usize)> = Vec::with_capacity(batch_size);

        for t in 0..encoder_out.num_frames {
            for u in 0..predictor_out.num_positions {
                enc_frames.push(encoder_out.frame(t));
                pred_outs.push(predictor_out.position(u));
                positions.push((t, u));

                if enc_frames.len() >= batch_size {
                    let results = joiner.forward_batch(&enc_frames, &pred_outs);
                    for ((t, u), log_probs) in positions.iter().zip(results.iter()) {
                        for (label, &log_prob) in log_probs.iter().enumerate() {
                            lattice.set(*t, *u, label as Label, log_prob as f64);
                        }
                    }
                    enc_frames.clear();
                    pred_outs.clear();
                    positions.clear();
                }
            }
        }

        // Process remaining
        if !enc_frames.is_empty() {
            let results = joiner.forward_batch(&enc_frames, &pred_outs);
            for ((t, u), log_probs) in positions.iter().zip(results.iter()) {
                for (label, &log_prob) in log_probs.iter().enumerate() {
                    lattice.set(*t, *u, label as Label, log_prob as f64);
                }
            }
        }
    }
}

impl Default for TransducerLatticeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Dense FSA representation for neural network outputs.
///
/// This is an efficient representation for composition with sparse LMs
/// in the style of k2's DenseFsaVec.
#[derive(Debug, Clone)]
pub struct DenseFsa<W: Semiring> {
    /// Number of time frames.
    pub num_frames: usize,
    /// Vocabulary size.
    pub vocab_size: usize,
    /// Log-probabilities: [T, V] flattened.
    /// At each frame, we have V log-probs (including blank at index 0).
    pub scores: Vec<f64>,
    _phantom: std::marker::PhantomData<W>,
}

impl<W: Semiring> DenseFsa<W> {
    /// Create a new dense FSA.
    pub fn new(num_frames: usize, vocab_size: usize, scores: Vec<f64>) -> Self {
        debug_assert_eq!(scores.len(), num_frames * vocab_size);
        Self {
            num_frames,
            vocab_size,
            scores,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get scores at time frame `t`.
    #[inline]
    pub fn frame_scores(&self, t: usize) -> &[f64] {
        let start = t * self.vocab_size;
        &self.scores[start..start + self.vocab_size]
    }

    /// Get score for label at time `t`.
    #[inline]
    pub fn score(&self, t: usize, label: Label) -> f64 {
        self.scores[t * self.vocab_size + label as usize]
    }

    /// Convert from CTC-style posteriors.
    ///
    /// Takes frame-level log-posteriors and creates a dense FSA.
    pub fn from_posteriors(posteriors: &[Vec<f64>]) -> Self {
        let num_frames = posteriors.len();
        let vocab_size = posteriors.first().map_or(0, |v| v.len());
        let scores: Vec<f64> = posteriors.iter().flat_map(|v| v.iter().copied()).collect();
        Self::new(num_frames, vocab_size, scores)
    }
}

/// Compose dense FSA with sparse WFST (e.g., language model).
///
/// This is the core operation for neural transducer decoding with external LM.
/// The composition is done lazily with pruning.
pub fn compose_dense_sparse<W>(
    dense: &DenseFsa<W>,
    sparse: &VectorWfst<Label, W>,
    beam: f64,
) -> VectorWfst<Label, W>
where
    W: Semiring + From<f64> + Into<f64> + Clone,
{
    use std::collections::HashMap;

    let mut fst: VectorWfst<Label, W> = VectorWfst::new();

    // State = (time_frame, sparse_state)
    // We use a hash map to track state mappings
    let mut state_map: HashMap<(usize, StateId), StateId> = HashMap::new();

    let get_or_create_state = |map: &mut HashMap<(usize, StateId), StateId>,
                               t: usize,
                               s: StateId,
                               fst: &mut VectorWfst<Label, W>| {
        *map.entry((t, s)).or_insert_with(|| fst.add_state())
    };

    // Start state
    let sparse_start = sparse.start();
    let start_state = get_or_create_state(&mut state_map, 0, sparse_start, &mut fst);
    fst.set_start(start_state);

    // BFS/priority queue for composition
    let mut frontier: Vec<(usize, StateId, f64)> = vec![(0, sparse_start, 0.0)];
    let mut best_score: HashMap<(usize, StateId), f64> = HashMap::new();
    best_score.insert((0, sparse_start), 0.0);

    while let Some((t, sparse_state, score)) = frontier.pop() {
        if t >= dense.num_frames {
            // Reached end of acoustic frames
            let composed_state = *state_map.get(&(t, sparse_state)).expect("state must exist");

            // Check if sparse state is final
            if sparse.is_final(sparse_state) {
                let final_weight: f64 = sparse.final_weight(sparse_state).into();
                fst.set_final(composed_state, W::from(final_weight));
            }
            continue;
        }

        let from_state = get_or_create_state(&mut state_map, t, sparse_state, &mut fst);

        // Get acoustic scores at this frame
        let frame_scores = dense.frame_scores(t);

        // Iterate over sparse transitions
        for tr in sparse.transitions(sparse_state) {
            // Extract label from input (Option<Label>)
            let label = match tr.input {
                Some(l) => l,
                None => 0, // epsilon
            };
            let acoustic_score = if (label as usize) < frame_scores.len() {
                frame_scores[label as usize]
            } else {
                f64::NEG_INFINITY
            };

            if acoustic_score <= f64::NEG_INFINITY {
                continue;
            }

            let lm_score: f64 = tr.weight.clone().into();
            let combined_score = score + acoustic_score + lm_score;

            // Pruning check
            let best_at_next = best_score
                .get(&(t + 1, tr.to))
                .copied()
                .unwrap_or(f64::NEG_INFINITY);
            if combined_score < best_at_next - beam {
                continue;
            }

            // Update best score
            let entry = best_score
                .entry((t + 1, tr.to))
                .or_insert(f64::NEG_INFINITY);
            if combined_score > *entry {
                *entry = combined_score;
            }

            // Add transition
            let to_state = get_or_create_state(&mut state_map, t + 1, tr.to, &mut fst);
            fst.add_transition(WeightedTransition {
                from: from_state,
                input: Some(label),
                output: tr.output,
                to: to_state,
                weight: W::from(-(acoustic_score + lm_score)),
            });

            frontier.push((t + 1, tr.to, combined_score));
        }

        // Also handle epsilon transitions in sparse (for backoff)
        // These don't consume acoustic frames
        for tr in sparse.transitions(sparse_state) {
            if tr.input.is_none() && tr.output.is_none() {
                // Epsilon transition (backoff)
                let lm_score: f64 = tr.weight.clone().into();
                let combined_score = score + lm_score;

                let entry = best_score.entry((t, tr.to)).or_insert(f64::NEG_INFINITY);
                if combined_score > *entry {
                    *entry = combined_score;

                    let to_state = get_or_create_state(&mut state_map, t, tr.to, &mut fst);
                    fst.add_transition(WeightedTransition {
                        from: from_state,
                        input: None,
                        output: None,
                        to: to_state,
                        weight: W::from(-lm_score),
                    });

                    frontier.push((t, tr.to, combined_score));
                }
            }
        }
    }

    fst
}

/// Build a simple graph for target sequence (for training).
///
/// Creates a linear FSA accepting only the target sequence,
/// used as the numerator graph in transducer loss computation.
pub fn build_target_graph<W: Semiring + From<f64>>(targets: &[Label]) -> VectorWfst<Label, W> {
    let mut fst: VectorWfst<Label, W> = VectorWfst::new();

    // Create states: one for each target position plus final
    fst.add_states(targets.len() + 1);

    fst.set_start(0);
    fst.set_final(targets.len() as StateId, W::one());

    // Add transitions for each target
    for (i, &label) in targets.iter().enumerate() {
        fst.add_transition(WeightedTransition {
            from: i as StateId,
            input: Some(label),
            output: Some(label),
            to: (i + 1) as StateId,
            weight: W::one(),
        });
    }

    fst
}

/// Build the denominator graph (all possible sequences).
///
/// For transducer training, this is typically a simple loop
/// accepting any sequence of vocabulary symbols.
pub fn build_denominator_graph<W: Semiring + From<f64>>(vocab_size: usize) -> VectorWfst<Label, W> {
    let mut fst: VectorWfst<Label, W> = VectorWfst::new();

    // Single state with self-loops for all vocabulary items
    let state = fst.add_state();
    fst.set_start(state);
    fst.set_final(state, W::one());

    // Add self-loop for each vocabulary item (excluding blank)
    for label in 1..vocab_size as Label {
        fst.add_transition(WeightedTransition {
            from: state,
            input: Some(label),
            output: Some(label),
            to: state,
            weight: W::one(),
        });
    }

    fst
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_target_graph() {
        let targets = vec![1, 2, 3];
        let graph: VectorWfst<Label, TropicalWeight> = build_target_graph(&targets);

        assert_eq!(graph.num_states(), 4);
        assert_eq!(graph.start(), 0);
        assert!(graph.is_final(3));
    }

    #[test]
    fn test_denominator_graph() {
        let graph: VectorWfst<Label, TropicalWeight> = build_denominator_graph(10);

        assert_eq!(graph.num_states(), 1);
        assert_eq!(graph.start(), 0);
        assert!(graph.is_final(0));

        // Should have 9 self-loops (vocab 1-9, excluding blank)
        assert_eq!(graph.transitions(0).len(), 9);
    }

    #[test]
    fn test_transducer_lattice() {
        let lattice: TransducerLattice<TropicalWeight> = TransducerLattice::new(3, 2, 5);

        assert_eq!(lattice.num_frames, 3);
        assert_eq!(lattice.num_positions, 2);
        assert_eq!(lattice.vocab_size, 5);
    }
}
