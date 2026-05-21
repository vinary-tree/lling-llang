//! Pruned Composition with Gradients.
//!
//! This module implements k2-style pruned composition for memory-efficient
//! training of neural transducers with large vocabularies and language models.
//!
//! ## Key Concepts
//!
//! Pruned composition combines:
//! 1. Dense FSA from neural network (T × V matrix)
//! 2. Sparse FSA (language model or grammar)
//!
//! During composition, paths are pruned based on beam width, and gradients
//! only flow through surviving paths.
//!
//! ## Benefits
//!
//! - Memory efficiency: O(beam_width) instead of O(V × T)
//! - Enables training with full LM on GPU
//! - Critical for production-scale systems

use crate::semiring::Semiring;
use crate::transducer::{DenseFsa, Label};
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst};
use std::collections::HashMap;

/// Configuration for pruned composition.
#[derive(Debug, Clone)]
pub struct PrunedCompositionConfig {
    /// Beam width (log-prob difference from best).
    pub beam: f64,

    /// Maximum number of states to keep.
    pub max_states: usize,

    /// Maximum number of arcs per frame.
    pub max_arcs: usize,

    /// Output beam (additional pruning on output).
    pub output_beam: Option<f64>,

    /// Whether to track gradients.
    pub compute_gradients: bool,

    /// Minimum arc posterior for gradient computation.
    pub min_arc_posterior: f64,
}

impl Default for PrunedCompositionConfig {
    fn default() -> Self {
        Self {
            beam: 10.0,
            max_states: 10000,
            max_arcs: 50000,
            output_beam: None,
            compute_gradients: true,
            min_arc_posterior: 1e-10,
        }
    }
}

/// Result of pruned composition.
#[derive(Debug)]
pub struct PrunedComposition<W: Semiring> {
    /// Resulting WFST after pruning.
    pub wfst: VectorWfst<Label, W>,

    /// Mapping from composed state to (time, sparse_state).
    pub state_map: HashMap<StateId, (usize, StateId)>,

    /// Forward scores at each state.
    pub forward_scores: Vec<f64>,

    /// Backward scores at each state (computed lazily).
    pub backward_scores: Option<Vec<f64>>,

    /// Arc information for gradient computation.
    pub arc_info: Vec<ArcInfo>,

    /// Statistics about pruning.
    pub stats: PruningStats,
}

/// Information about a composed arc (for gradient computation).
#[derive(Debug, Clone)]
pub struct ArcInfo {
    /// Source state in composed WFST.
    pub from_state: StateId,
    /// Target state in composed WFST.
    pub to_state: StateId,
    /// Time frame.
    pub time: usize,
    /// Label (vocabulary index).
    pub label: Label,
    /// Acoustic score from dense FSA.
    pub acoustic_score: f64,
    /// LM/grammar score from sparse FSA.
    pub lm_score: f64,
    /// Combined arc score.
    pub arc_score: f64,
}

/// Statistics from pruned composition.
#[derive(Debug, Clone, Default)]
pub struct PruningStats {
    /// Number of states before pruning.
    pub states_before: usize,
    /// Number of states after pruning.
    pub states_after: usize,
    /// Number of arcs before pruning.
    pub arcs_before: usize,
    /// Number of arcs after pruning.
    pub arcs_after: usize,
    /// Average beam utilization.
    pub avg_beam_utilization: f64,
}

/// Perform pruned composition of dense and sparse FSAs.
///
/// # Arguments
/// * `dense` - Dense FSA from neural network (T × V scores)
/// * `sparse` - Sparse FSA (language model or grammar)
/// * `config` - Pruning configuration
///
/// # Returns
/// Pruned composition result with gradient information.
pub fn pruned_compose<W>(
    dense: &DenseFsa<W>,
    sparse: &VectorWfst<Label, W>,
    config: &PrunedCompositionConfig,
) -> PrunedComposition<W>
where
    W: Semiring + From<f64> + Into<f64> + Clone,
{
    let num_frames = dense.num_frames;
    let mut fst: VectorWfst<Label, W> = VectorWfst::new();
    let mut state_map: HashMap<(usize, StateId), StateId> = HashMap::new();
    let mut reverse_map: HashMap<StateId, (usize, StateId)> = HashMap::new();
    let mut arc_info: Vec<ArcInfo> = Vec::new();
    let mut forward_scores: Vec<f64> = Vec::new();
    let mut stats = PruningStats::default();

    // Helper to get or create composed state
    let get_or_create_state = |map: &mut HashMap<(usize, StateId), StateId>,
                               rev_map: &mut HashMap<StateId, (usize, StateId)>,
                               scores: &mut Vec<f64>,
                               fst: &mut VectorWfst<Label, W>,
                               t: usize,
                               s: StateId|
     -> StateId {
        *map.entry((t, s)).or_insert_with(|| {
            let id = fst.add_state();
            scores.push(f64::NEG_INFINITY);
            rev_map.insert(id, (t, s));
            id
        })
    };

    // Initialize with start state
    let sparse_start = sparse.start();
    let start_state = get_or_create_state(
        &mut state_map,
        &mut reverse_map,
        &mut forward_scores,
        &mut fst,
        0,
        sparse_start,
    );
    forward_scores[start_state as usize] = 0.0;
    fst.set_start(start_state);

    // Best score at each frame for beam pruning
    let mut best_scores: Vec<f64> = vec![f64::NEG_INFINITY; num_frames + 1];
    best_scores[0] = 0.0;

    // Process frame by frame
    for t in 0..num_frames {
        let frame_scores = dense.frame_scores(t);
        let beam_threshold = best_scores[t] - config.beam;

        // Collect active states at time t
        let active_states: Vec<(StateId, StateId, f64)> = state_map
            .iter()
            .filter(|((time, _), _)| *time == t)
            .map(|((_, sparse_s), &composed_s)| {
                let score = forward_scores[composed_s as usize];
                (*sparse_s, composed_s, score)
            })
            .filter(|(_, _, score)| *score >= beam_threshold)
            .collect();

        stats.states_before += active_states.len();

        // Process each active state
        for (sparse_state, composed_from, from_score) in active_states {
            // Iterate over sparse transitions
            for tr in sparse.transitions(sparse_state) {
                let label = match tr.input {
                    Some(l) => l,
                    None => continue, // Skip epsilon transitions here
                };

                // Get acoustic score
                let acoustic_score = if (label as usize) < frame_scores.len() {
                    frame_scores[label as usize] as f64
                } else {
                    continue; // Invalid label
                };

                if acoustic_score <= f64::NEG_INFINITY {
                    continue;
                }

                let lm_score: f64 = tr.weight.clone().into();
                let arc_score = acoustic_score - lm_score; // Convert to log-prob

                let new_score = from_score + arc_score;

                // Beam pruning
                if new_score < best_scores[t + 1] - config.beam {
                    continue;
                }

                // Update best score
                if new_score > best_scores[t + 1] {
                    best_scores[t + 1] = new_score;
                }

                // Create/get target state
                let composed_to = get_or_create_state(
                    &mut state_map,
                    &mut reverse_map,
                    &mut forward_scores,
                    &mut fst,
                    t + 1,
                    tr.to,
                );

                // Update forward score (log-add for multiple paths)
                let old_score = forward_scores[composed_to as usize];
                forward_scores[composed_to as usize] = log_add(old_score, new_score);

                // Add arc
                fst.add_transition(WeightedTransition {
                    from: composed_from,
                    input: Some(label),
                    output: tr.output,
                    to: composed_to,
                    weight: W::from(-arc_score), // Convert back to weight
                });

                stats.arcs_before += 1;

                // Track arc info for gradients
                if config.compute_gradients {
                    arc_info.push(ArcInfo {
                        from_state: composed_from,
                        to_state: composed_to,
                        time: t,
                        label,
                        acoustic_score,
                        lm_score,
                        arc_score,
                    });
                }
            }
        }

        stats.states_after = state_map.len();
        stats.arcs_after = arc_info.len();
    }

    // Set final states
    for (&(t, sparse_s), &composed_s) in &state_map {
        if t == num_frames && sparse.is_final(sparse_s) {
            let final_weight: f64 = sparse.final_weight(sparse_s).into();
            fst.set_final(composed_s, W::from(final_weight));
        }
    }

    // Compute stats
    if stats.states_before > 0 {
        stats.avg_beam_utilization = stats.states_after as f64 / stats.states_before as f64;
    }

    PrunedComposition {
        wfst: fst,
        state_map: reverse_map,
        forward_scores,
        backward_scores: None,
        arc_info,
        stats,
    }
}

impl<W: Semiring + From<f64> + Into<f64> + Clone> PrunedComposition<W> {
    /// Compute forward score (log-sum-exp over all paths).
    pub fn forward_score(&self) -> f64 {
        let mut total = f64::NEG_INFINITY;

        for state in 0..self.wfst.num_states() {
            let state_id = state as StateId;
            if self.wfst.is_final(state_id) {
                let final_weight: f64 = self.wfst.final_weight(state_id).into();
                let state_score = self.forward_scores[state];
                total = log_add(total, state_score - final_weight);
            }
        }

        total
    }

    /// Compute backward scores (for gradient computation).
    pub fn compute_backward(&mut self) {
        let num_states = self.wfst.num_states();
        let mut backward = vec![f64::NEG_INFINITY; num_states];

        // Initialize final states
        for state in 0..num_states {
            let state_id = state as StateId;
            if self.wfst.is_final(state_id) {
                let final_weight: f64 = self.wfst.final_weight(state_id).into();
                backward[state] = -final_weight;
            }
        }

        // Process in reverse topological order
        // (assuming states are ordered by time)
        for state in (0..num_states).rev() {
            let state_id = state as StateId;

            for tr in self.wfst.transitions(state_id) {
                let next_state = tr.to as usize;
                if backward[next_state] > f64::NEG_INFINITY {
                    let weight: f64 = tr.weight.clone().into();
                    let new_backward = -weight + backward[next_state];
                    backward[state] = log_add(backward[state], new_backward);
                }
            }
        }

        self.backward_scores = Some(backward);
    }

    /// Compute gradients with respect to dense FSA scores.
    ///
    /// Returns gradients as [T, V] matrix.
    pub fn backward(&mut self, output_grad: f64) -> DenseGradient {
        // Ensure backward scores are computed
        if self.backward_scores.is_none() {
            self.compute_backward();
        }

        let backward = self.backward_scores.as_ref().expect("backward computed");
        let total_log_prob = self.forward_score();

        // Determine dimensions from arc_info
        let num_frames = self.arc_info.iter().map(|a| a.time + 1).max().unwrap_or(0);
        let vocab_size = self
            .arc_info
            .iter()
            .map(|a| a.label as usize + 1)
            .max()
            .unwrap_or(0);

        let mut gradients = DenseGradient::new(num_frames, vocab_size);

        // Compute gradient for each arc
        for arc in &self.arc_info {
            let from_score = self.forward_scores[arc.from_state as usize];
            let to_backward = backward[arc.to_state as usize];

            // Arc posterior: exp(α + arc_score + β - total)
            let arc_posterior = (from_score + arc.arc_score + to_backward - total_log_prob).exp();

            // Gradient w.r.t. acoustic score
            // For softmax output: grad = output_grad * posterior
            gradients.add(arc.time, arc.label as usize, output_grad * arc_posterior);
        }

        gradients
    }
}

/// Dense gradient representation.
#[derive(Debug, Clone)]
pub struct DenseGradient {
    /// Number of frames.
    pub num_frames: usize,
    /// Vocabulary size.
    pub vocab_size: usize,
    /// Gradient data [T, V].
    pub data: Vec<f64>,
}

impl DenseGradient {
    /// Create new gradient container.
    pub fn new(num_frames: usize, vocab_size: usize) -> Self {
        Self {
            num_frames,
            vocab_size,
            data: vec![0.0; num_frames * vocab_size],
        }
    }

    /// Get gradient at (t, v).
    #[inline]
    pub fn get(&self, t: usize, v: usize) -> f64 {
        self.data[t * self.vocab_size + v]
    }

    /// Set gradient at (t, v).
    #[inline]
    pub fn set(&mut self, t: usize, v: usize, value: f64) {
        self.data[t * self.vocab_size + v] = value;
    }

    /// Add to gradient at (t, v).
    #[inline]
    pub fn add(&mut self, t: usize, v: usize, value: f64) {
        self.data[t * self.vocab_size + v] += value;
    }
}

/// Log-add operation.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pruned_composition_config() {
        let config = PrunedCompositionConfig::default();
        assert_eq!(config.beam, 10.0);
        assert_eq!(config.max_states, 10000);
    }

    #[test]
    fn test_dense_gradient() {
        let mut grad = DenseGradient::new(10, 100);

        grad.set(0, 50, 0.5);
        assert!((grad.get(0, 50) - 0.5).abs() < 1e-10);

        grad.add(0, 50, 0.3);
        assert!((grad.get(0, 50) - 0.8).abs() < 1e-10);
    }

    #[test]
    fn test_pruning_stats() {
        let stats = PruningStats {
            states_before: 1000,
            states_after: 100,
            arcs_before: 5000,
            arcs_after: 500,
            avg_beam_utilization: 0.1,
        };

        assert_eq!(stats.states_after, 100);
    }
}
