//! Weakly Supervised Transducer (WST) Training.
//!
//! WST introduces flexible training graphs that handle transcript errors
//! using bypass arcs in the WFST framework. This allows training from:
//!
//! - Noisy/imperfect transcripts
//! - Crowd-sourced annotations
//! - OCR errors
//! - Machine-generated labels
//!
//! ## Key Innovations
//!
//! WST adds two types of bypass arcs to standard transducer graphs:
//! - **Token bypass arcs**: Skip unreliable tokens
//! - **Blank bypass arcs**: Handle timing uncertainties
//!
//! ```text
//! Standard Graph:        WST Graph:
//!     ─[a]─[b]─[c]─         ─[a]─[b]─[c]─
//!                               │     │
//!                               └──ε──┘ (bypass)
//! ```
//!
//! ## References
//!
//! - [WST: Weakly Supervised Transducer for ASR (arXiv 2511.04035)](https://arxiv.org/abs/2511.04035)

use crate::semiring::{LogWeight, Semiring};
use crate::transducer::{Label, BLANK};
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst};

/// Configuration for Weakly Supervised Training.
#[derive(Debug, Clone)]
pub struct WstConfig {
    /// Cost for using token bypass arcs.
    /// Higher values discourage skipping tokens.
    pub token_bypass_weight: f64,

    /// Cost for using blank bypass arcs.
    pub blank_bypass_weight: f64,

    /// Confidence threshold below which to add bypass arcs.
    /// Tokens with confidence below this get bypass alternatives.
    pub confidence_threshold: f64,

    /// Maximum number of consecutive tokens that can be bypassed.
    pub max_bypass_span: usize,

    /// Whether to allow deletion of any token (not just low-confidence).
    pub allow_universal_bypass: bool,

    /// Weight for universal bypass (if enabled).
    pub universal_bypass_weight: f64,
}

impl Default for WstConfig {
    fn default() -> Self {
        Self {
            token_bypass_weight: 2.0,
            blank_bypass_weight: 0.5,
            confidence_threshold: 0.5,
            max_bypass_span: 3,
            allow_universal_bypass: false,
            universal_bypass_weight: 5.0,
        }
    }
}

/// Token with associated confidence score.
#[derive(Debug, Clone)]
pub struct ConfidentToken {
    /// Token label.
    pub label: Label,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f64,
    /// Optional alternative tokens with confidences.
    pub alternatives: Vec<(Label, f64)>,
}

impl ConfidentToken {
    /// Create a new confident token.
    pub fn new(label: Label, confidence: f64) -> Self {
        Self {
            label,
            confidence,
            alternatives: Vec::new(),
        }
    }

    /// Create with alternatives.
    pub fn with_alternatives(
        label: Label,
        confidence: f64,
        alternatives: Vec<(Label, f64)>,
    ) -> Self {
        Self {
            label,
            confidence,
            alternatives,
        }
    }
}

/// Build WST training graph with bypass arcs.
///
/// Creates a WFST that accepts the target sequence but also allows
/// bypassing low-confidence tokens.
///
/// # Arguments
/// * `targets` - Target tokens with confidence scores
/// * `config` - WST configuration
///
/// # Returns
/// A WFST representing the flexible training graph.
pub fn build_wst_graph<W>(targets: &[ConfidentToken], config: &WstConfig) -> VectorWfst<Label, W>
where
    W: Semiring + From<f64>,
{
    let mut fst: VectorWfst<Label, W> = VectorWfst::new();

    // Create states: one for each token position plus final
    let num_states = targets.len() + 1;
    fst.add_states(num_states);

    fst.set_start(0);
    fst.set_final(targets.len() as StateId, W::one());

    // Add transitions for each token
    for (i, token) in targets.iter().enumerate() {
        let from_state = i as StateId;
        let to_state = (i + 1) as StateId;

        // Main transition: emit the target token
        fst.add_transition(WeightedTransition {
            from: from_state,
            input: Some(token.label),
            output: Some(token.label),
            to: to_state,
            weight: W::one(),
        });

        // Add alternative tokens (if any)
        for (alt_label, alt_conf) in &token.alternatives {
            // Weight based on confidence difference
            let weight = (token.confidence - alt_conf).max(0.0);
            fst.add_transition(WeightedTransition {
                from: from_state,
                input: Some(*alt_label),
                output: Some(*alt_label),
                to: to_state,
                weight: W::from(weight),
            });
        }

        // Token bypass arc (skip this token)
        if token.confidence < config.confidence_threshold || config.allow_universal_bypass {
            let bypass_weight = if token.confidence < config.confidence_threshold {
                config.token_bypass_weight * (1.0 - token.confidence)
            } else {
                config.universal_bypass_weight
            };

            // Epsilon transition to skip token
            fst.add_transition(WeightedTransition {
                from: from_state,
                input: None, // Epsilon
                output: None,
                to: to_state,
                weight: W::from(bypass_weight),
            });
        }

        // Multi-token bypass arcs (skip multiple tokens)
        if config.max_bypass_span > 1 {
            for span in 2..=config.max_bypass_span.min(targets.len() - i) {
                let skip_to = (i + span) as StateId;

                // Check if any token in the span has low confidence
                let min_confidence: f64 = targets[i..i + span]
                    .iter()
                    .map(|t| t.confidence)
                    .fold(f64::INFINITY, f64::min);

                if min_confidence < config.confidence_threshold {
                    let bypass_weight =
                        config.token_bypass_weight * span as f64 * (1.0 - min_confidence);
                    fst.add_transition(WeightedTransition {
                        from: from_state,
                        input: None,
                        output: None,
                        to: skip_to,
                        weight: W::from(bypass_weight),
                    });
                }
            }
        }
    }

    // Add blank bypass arcs at each position
    for i in 0..=targets.len() {
        let state = i as StateId;

        // Self-loop for blank (timing flexibility)
        fst.add_transition(WeightedTransition {
            from: state,
            input: Some(BLANK),
            output: Some(BLANK),
            to: state,
            weight: W::from(config.blank_bypass_weight),
        });
    }

    fst
}

/// Build WST graph from labels with uniform confidence.
///
/// Convenience function when confidence scores are not available.
pub fn build_wst_graph_uniform<W>(
    targets: &[Label],
    default_confidence: f64,
    config: &WstConfig,
) -> VectorWfst<Label, W>
where
    W: Semiring + From<f64>,
{
    let confident_targets: Vec<ConfidentToken> = targets
        .iter()
        .map(|&label| ConfidentToken::new(label, default_confidence))
        .collect();

    build_wst_graph(&confident_targets, config)
}

/// Build WST graph with insertion allowance.
///
/// This variant allows inserting extra tokens (for handling transcripts
/// that may be missing words).
pub fn build_wst_graph_with_insertions<W>(
    targets: &[ConfidentToken],
    vocab_size: usize,
    insertion_weight: f64,
    config: &WstConfig,
) -> VectorWfst<Label, W>
where
    W: Semiring + From<f64>,
{
    let mut fst: VectorWfst<Label, W> = VectorWfst::new();

    // Create states with extra "insertion" states
    let num_base_states = targets.len() + 1;
    fst.add_states(num_base_states * 2);

    fst.set_start(0);
    fst.set_final(targets.len() as StateId, W::one());

    // Main transitions
    for (i, token) in targets.iter().enumerate() {
        let from_state = i as StateId;
        let to_state = (i + 1) as StateId;
        let insert_state = (num_base_states + i) as StateId;

        // Main transition
        fst.add_transition(WeightedTransition {
            from: from_state,
            input: Some(token.label),
            output: Some(token.label),
            to: to_state,
            weight: W::one(),
        });

        // Bypass transition
        if token.confidence < config.confidence_threshold {
            let bypass_weight = config.token_bypass_weight * (1.0 - token.confidence);
            fst.add_transition(WeightedTransition {
                from: from_state,
                input: None,
                output: None,
                to: to_state,
                weight: W::from(bypass_weight),
            });
        }

        // Transition to insertion state
        fst.add_transition(WeightedTransition {
            from: from_state,
            input: None,
            output: None,
            to: insert_state,
            weight: W::from(insertion_weight),
        });

        // From insertion state, can emit any token and return
        for label in 1..vocab_size as Label {
            fst.add_transition(WeightedTransition {
                from: insert_state,
                input: Some(label),
                output: Some(label),
                to: from_state,
                weight: W::one(),
            });
        }
    }

    // Blank self-loops
    for i in 0..num_base_states {
        let state = i as StateId;
        fst.add_transition(WeightedTransition {
            from: state,
            input: Some(BLANK),
            output: Some(BLANK),
            to: state,
            weight: W::from(config.blank_bypass_weight),
        });
    }

    fst
}

/// Result of WST loss computation.
#[derive(Debug, Clone)]
pub struct WstLossResult {
    /// Total loss.
    pub loss: f64,
    /// Path through WST graph (for debugging).
    pub alignment: Vec<WstAlignmentStep>,
    /// Fraction of tokens that used bypass.
    pub bypass_ratio: f64,
}

/// Single step in WST alignment.
#[derive(Debug, Clone)]
pub struct WstAlignmentStep {
    /// Position in target sequence.
    pub target_pos: usize,
    /// Emitted label (0 for bypass/blank).
    pub label: Label,
    /// Whether this was a bypass.
    pub is_bypass: bool,
    /// Arc weight used.
    pub weight: f64,
}

/// Compute WST loss using forward-backward algorithm.
///
/// This computes the log-probability of all paths through the WST graph,
/// weighted by the acoustic model scores.
pub fn wst_loss<W>(acoustic_scores: &[Vec<f64>], wst_graph: &VectorWfst<Label, W>) -> WstLossResult
where
    W: Semiring + From<f64> + Into<f64> + Clone,
{
    let num_frames = acoustic_scores.len();
    let num_states = wst_graph.num_states();

    // Forward pass (log-sum-exp for total probability)
    let mut alpha = vec![vec![f64::NEG_INFINITY; num_states]; num_frames + 1];
    alpha[0][wst_graph.start() as usize] = 0.0;

    for t in 0..num_frames {
        for s in 0..num_states {
            if alpha[t][s] <= f64::NEG_INFINITY {
                continue;
            }

            let state = s as StateId;
            for tr in wst_graph.transitions(state) {
                let label = tr.input.unwrap_or(0) as usize;
                let acoustic = if label < acoustic_scores[t].len() {
                    acoustic_scores[t][label]
                } else {
                    f64::NEG_INFINITY
                };

                if acoustic <= f64::NEG_INFINITY {
                    continue;
                }

                let graph_weight: f64 = tr.weight.clone().into();
                let arc_score = acoustic - graph_weight;

                let next_state = tr.to as usize;
                alpha[t + 1][next_state] =
                    log_add(alpha[t + 1][next_state], alpha[t][s] + arc_score);
            }

            // Handle epsilon transitions (don't consume frame)
            for tr in wst_graph.transitions(state) {
                if tr.input.is_none() && tr.output.is_none() {
                    let graph_weight: f64 = tr.weight.clone().into();
                    let next_state = tr.to as usize;
                    alpha[t][next_state] =
                        log_add(alpha[t][next_state], alpha[t][s] - graph_weight);
                }
            }
        }
    }

    // Compute total log-probability
    let mut total_log_prob = f64::NEG_INFINITY;
    for s in 0..num_states {
        let state = s as StateId;
        if wst_graph.is_final(state) {
            let final_weight: f64 = wst_graph.final_weight(state).into();
            total_log_prob = log_add(total_log_prob, alpha[num_frames][s] - final_weight);
        }
    }

    // Viterbi pass (max/min for best path)
    let (alignment, _viterbi_score) = viterbi_alignment(acoustic_scores, wst_graph);

    // Compute bypass ratio from alignment
    let num_bypasses = alignment.iter().filter(|s| s.is_bypass).count();
    let bypass_ratio = if alignment.is_empty() {
        0.0
    } else {
        num_bypasses as f64 / alignment.len() as f64
    };

    WstLossResult {
        loss: -total_log_prob,
        alignment,
        bypass_ratio,
    }
}

/// Compute Viterbi (best path) alignment through WST graph.
///
/// Uses the tropical semiring (min-plus) to find the single best path,
/// useful for debugging and computing alignment statistics.
fn viterbi_alignment<W>(
    acoustic_scores: &[Vec<f64>],
    wst_graph: &VectorWfst<Label, W>,
) -> (Vec<WstAlignmentStep>, f64)
where
    W: Semiring + Into<f64> + Clone,
{
    let num_frames = acoustic_scores.len();
    let num_states = wst_graph.num_states();

    if num_frames == 0 || num_states == 0 {
        return (Vec::new(), f64::INFINITY);
    }

    // Viterbi forward pass with backpointers
    // delta[t][s] = best score to reach state s at time t
    let mut delta = vec![vec![f64::INFINITY; num_states]; num_frames + 1];
    // backpointer[t][s] = (prev_time, prev_state, arc_index, is_bypass)
    let mut backpointer: Vec<Vec<Option<(usize, usize, Label, bool, f64)>>> =
        vec![vec![None; num_states]; num_frames + 1];

    delta[0][wst_graph.start() as usize] = 0.0;

    // Process time steps
    for t in 0..num_frames {
        // First, process epsilon transitions at time t (don't consume frame)
        // Iterate until no more improvements (handles chains of epsilon transitions)
        let mut changed = true;
        while changed {
            changed = false;
            for s in 0..num_states {
                if delta[t][s] >= f64::INFINITY {
                    continue;
                }

                let state = s as StateId;
                for tr in wst_graph.transitions(state) {
                    // Only epsilon transitions
                    if tr.input.is_some() || tr.output.is_some() {
                        continue;
                    }

                    let graph_weight: f64 = tr.weight.clone().into();
                    let new_score = delta[t][s] + graph_weight;
                    let next_state = tr.to as usize;

                    if new_score < delta[t][next_state] {
                        delta[t][next_state] = new_score;
                        backpointer[t][next_state] = Some((t, s, 0, true, graph_weight));
                        changed = true;
                    }
                }
            }
        }

        // Then, process label transitions (consume a frame)
        for s in 0..num_states {
            if delta[t][s] >= f64::INFINITY {
                continue;
            }

            let state = s as StateId;
            for tr in wst_graph.transitions(state) {
                // Skip epsilon transitions
                if tr.input.is_none() && tr.output.is_none() {
                    continue;
                }

                let label = tr.input.unwrap_or(0);
                let label_idx = label as usize;
                let acoustic = if label_idx < acoustic_scores[t].len() {
                    acoustic_scores[t][label_idx]
                } else {
                    f64::NEG_INFINITY
                };

                if acoustic <= f64::NEG_INFINITY {
                    continue;
                }

                let graph_weight: f64 = tr.weight.clone().into();
                // For Viterbi (tropical), we minimize cost = graph_weight - acoustic_score
                // Higher acoustic score = lower cost = better
                let arc_cost = graph_weight - acoustic;
                let new_score = delta[t][s] + arc_cost;
                let next_state = tr.to as usize;

                if new_score < delta[t + 1][next_state] {
                    delta[t + 1][next_state] = new_score;
                    backpointer[t + 1][next_state] = Some((t, s, label, false, graph_weight));
                }
            }
        }
    }

    // Handle epsilon transitions at final time step
    let mut changed = true;
    while changed {
        changed = false;
        for s in 0..num_states {
            if delta[num_frames][s] >= f64::INFINITY {
                continue;
            }

            let state = s as StateId;
            for tr in wst_graph.transitions(state) {
                if tr.input.is_some() || tr.output.is_some() {
                    continue;
                }

                let graph_weight: f64 = tr.weight.clone().into();
                let new_score = delta[num_frames][s] + graph_weight;
                let next_state = tr.to as usize;

                if new_score < delta[num_frames][next_state] {
                    delta[num_frames][next_state] = new_score;
                    backpointer[num_frames][next_state] =
                        Some((num_frames, s, 0, true, graph_weight));
                    changed = true;
                }
            }
        }
    }

    // Find best final state
    let mut best_score = f64::INFINITY;
    let mut best_final_state = None;

    for s in 0..num_states {
        let state = s as StateId;
        if wst_graph.is_final(state) {
            let final_weight: f64 = wst_graph.final_weight(state).into();
            let total = delta[num_frames][s] + final_weight;
            if total < best_score {
                best_score = total;
                best_final_state = Some(s);
            }
        }
    }

    // Backtrack to reconstruct path
    let mut alignment = Vec::new();

    if let Some(mut state) = best_final_state {
        let mut time = num_frames;
        let mut target_pos = 0;

        while let Some((prev_time, prev_state, label, is_bypass, weight)) = backpointer[time][state]
        {
            // Update target position estimate based on state index
            // In the WST graph, states roughly correspond to target positions
            target_pos = prev_state;

            alignment.push(WstAlignmentStep {
                target_pos,
                label,
                is_bypass,
                weight,
            });

            time = prev_time;
            state = prev_state;
        }

        alignment.reverse();
    }

    (alignment, best_score)
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

/// Estimate token confidences from ASR n-best output.
///
/// Uses the probability mass across n-best hypotheses to estimate
/// how reliable each token position is.
pub fn estimate_confidences_from_nbest(
    nbest: &[(Vec<Label>, f64)],
    reference: &[Label],
) -> Vec<f64> {
    if nbest.is_empty() || reference.is_empty() {
        return vec![0.5; reference.len()];
    }

    let mut confidences = vec![0.0; reference.len()];

    // Normalize n-best probabilities
    let total_prob: f64 = nbest.iter().map(|(_, p)| p.exp()).sum();

    for (hyp, log_prob) in nbest {
        let prob = log_prob.exp() / total_prob;

        // Align hypothesis to reference and accumulate confidence
        let alignment = align_sequences(reference, hyp);
        for (ref_pos, _hyp_pos, matched) in alignment {
            if matched {
                confidences[ref_pos] += prob;
            }
        }
    }

    confidences
}

/// Simple sequence alignment (edit distance based).
fn align_sequences(ref_seq: &[Label], hyp_seq: &[Label]) -> Vec<(usize, usize, bool)> {
    // Simple alignment: match positions where labels agree
    let mut alignment = Vec::new();

    let mut j = 0;
    for (i, &ref_label) in ref_seq.iter().enumerate() {
        if j < hyp_seq.len() && hyp_seq[j] == ref_label {
            alignment.push((i, j, true));
            j += 1;
        } else {
            alignment.push((i, j, false));
        }
    }

    alignment
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_build_wst_graph() {
        let targets = vec![
            ConfidentToken::new(1, 0.9),
            ConfidentToken::new(2, 0.3), // Low confidence
            ConfidentToken::new(3, 0.8),
        ];

        let config = WstConfig::default();
        let graph: VectorWfst<Label, TropicalWeight> = build_wst_graph(&targets, &config);

        // Should have 4 states (3 tokens + final)
        assert_eq!(graph.num_states(), 4);
        assert!(graph.is_final(3));

        // State 1 (low confidence token) should have bypass arc
        let state1_transitions = graph.transitions(1);
        assert!(state1_transitions.iter().any(|t| t.input.is_none())); // Epsilon bypass
    }

    #[test]
    fn test_wst_config() {
        let config = WstConfig {
            token_bypass_weight: 1.0,
            confidence_threshold: 0.7,
            ..Default::default()
        };

        let targets = vec![
            ConfidentToken::new(1, 0.5), // Below threshold
            ConfidentToken::new(2, 0.9), // Above threshold
        ];

        let graph: VectorWfst<Label, TropicalWeight> = build_wst_graph(&targets, &config);

        // First token should have bypass
        let state0_eps = graph
            .transitions(0)
            .iter()
            .filter(|t| t.input.is_none())
            .count();
        assert!(state0_eps > 0);
    }

    #[test]
    fn test_confident_token() {
        let token = ConfidentToken::with_alternatives(1, 0.8, vec![(2, 0.1), (3, 0.05)]);

        assert_eq!(token.label, 1);
        assert_eq!(token.confidence, 0.8);
        assert_eq!(token.alternatives.len(), 2);
    }

    #[test]
    fn test_wst_loss_with_alignment() {
        use crate::semiring::LogWeight;

        // Create tokens: high confidence, low confidence, high confidence
        let targets = vec![
            ConfidentToken::new(1, 0.95), // High confidence
            ConfidentToken::new(2, 0.3),  // Low confidence - should get bypass
            ConfidentToken::new(3, 0.9),  // High confidence
        ];

        let config = WstConfig {
            confidence_threshold: 0.5,
            token_bypass_weight: 2.0,
            ..Default::default()
        };

        let graph: VectorWfst<Label, LogWeight> = build_wst_graph(&targets, &config);

        // Create acoustic scores favoring the correct labels
        // 3 frames, vocab size 4 (labels 0-3)
        let acoustic_scores = vec![
            vec![-0.1, -1.0, -2.0, -3.0], // Frame 0: label 1 has best score
            vec![-3.0, -2.0, -0.1, -1.0], // Frame 1: label 2 has best score
            vec![-2.0, -3.0, -1.0, -0.1], // Frame 2: label 3 has best score
        ];

        let result = wst_loss(&acoustic_scores, &graph);

        // Should have an alignment
        assert!(
            !result.alignment.is_empty(),
            "Alignment should not be empty"
        );

        // With high confidence tokens and matching acoustics, bypass_ratio should be low
        // (bypass arcs only exist for low-confidence token at position 1)
        assert!(
            result.bypass_ratio < 1.0,
            "Bypass ratio should be less than 1.0"
        );

        // The loss should be finite
        assert!(result.loss.is_finite(), "Loss should be finite");
    }

    #[test]
    fn test_wst_loss_all_bypass() {
        use crate::semiring::LogWeight;

        // All low confidence tokens
        let targets = vec![ConfidentToken::new(1, 0.1), ConfidentToken::new(2, 0.1)];

        let config = WstConfig {
            confidence_threshold: 0.5,
            token_bypass_weight: 0.1, // Very cheap bypass
            ..Default::default()
        };

        let graph: VectorWfst<Label, LogWeight> = build_wst_graph(&targets, &config);

        // Create acoustic scores that make the correct labels expensive
        let acoustic_scores = vec![
            vec![-10.0, -10.0, -10.0], // All labels expensive
            vec![-10.0, -10.0, -10.0],
        ];

        let result = wst_loss(&acoustic_scores, &graph);

        // With cheap bypass and expensive acoustics, Viterbi may prefer bypasses
        // The alignment should exist
        assert!(result.loss.is_finite(), "Loss should be finite");
    }
}
