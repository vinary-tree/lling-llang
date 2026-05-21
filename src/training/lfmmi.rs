//! Lattice-Free Maximum Mutual Information (LF-MMI) Training.
//!
//! LF-MMI is a sequence-discriminative training criterion that maximizes:
//!
//! ```text
//! L_MMI = log P(correct|x) - log Σ_y P(y|x)
//!       = log Σ_{π∈correct} w(π) - log Σ_{π∈all} w(π)
//! ```
//!
//! The "lattice-free" aspect means we don't need to generate lattices explicitly;
//! instead, we use a denominator graph (phone loop + LM) that covers all possible
//! transcriptions.

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst};

/// Configuration for LF-MMI training.
#[derive(Debug, Clone)]
pub struct LfMmiConfig {
    /// Leaky HMM coefficient (regularization).
    /// This adds a small probability of transitioning to any state,
    /// preventing the denominator from being too peaky.
    pub leaky_hmm_coefficient: f64,

    /// L2 regularization on network outputs.
    pub l2_regularize: f64,

    /// Cross-entropy regularization weight.
    /// Interpolates MMI with frame-level cross-entropy for stability.
    pub xent_regularize: f64,

    /// Whether to use chain topology (modified HMM).
    pub use_chain_topology: bool,

    /// Subsampling factor for output frames.
    pub subsampling_factor: usize,
}

impl Default for LfMmiConfig {
    fn default() -> Self {
        Self {
            leaky_hmm_coefficient: 0.1,
            l2_regularize: 0.0001,
            xent_regularize: 0.1,
            use_chain_topology: true,
            subsampling_factor: 3,
        }
    }
}

/// Result of LF-MMI loss computation.
#[derive(Debug, Clone)]
pub struct LfMmiResult {
    /// Total loss (numerator - denominator + regularization).
    pub loss: f64,
    /// Numerator log-probability (correct path).
    pub numerator_log_prob: f64,
    /// Denominator log-probability (all paths).
    pub denominator_log_prob: f64,
    /// Cross-entropy component (if xent_regularize > 0).
    pub xent_loss: f64,
    /// Gradients with respect to acoustic scores.
    pub gradients: LfMmiGradients,
}

/// Gradients from LF-MMI computation.
#[derive(Debug, Clone)]
pub struct LfMmiGradients {
    /// Number of frames.
    pub num_frames: usize,
    /// Number of output units (pdfs/phones).
    pub num_pdfs: usize,
    /// Gradient values [T, num_pdfs].
    pub data: Vec<f64>,
}

impl LfMmiGradients {
    /// Create new gradient container.
    pub fn new(num_frames: usize, num_pdfs: usize) -> Self {
        Self {
            num_frames,
            num_pdfs,
            data: vec![0.0; num_frames * num_pdfs],
        }
    }

    /// Get gradient at (t, pdf).
    #[inline]
    pub fn get(&self, t: usize, pdf: usize) -> f64 {
        self.data[t * self.num_pdfs + pdf]
    }

    /// Set gradient at (t, pdf).
    #[inline]
    pub fn set(&mut self, t: usize, pdf: usize, value: f64) {
        self.data[t * self.num_pdfs + pdf] = value;
    }

    /// Add to gradient at (t, pdf).
    #[inline]
    pub fn add(&mut self, t: usize, pdf: usize, value: f64) {
        self.data[t * self.num_pdfs + pdf] += value;
    }
}

/// Compute LF-MMI loss.
///
/// # Arguments
/// * `acoustic_scores` - Frame-level log-likelihoods [T, num_pdfs]
/// * `numerator_graph` - FST representing correct transcription
/// * `denominator_graph` - FST representing all possible transcriptions (phone loop + LM)
/// * `config` - Training configuration
///
/// # Returns
/// Loss value and gradients.
pub fn lfmmi_loss<W>(
    acoustic_scores: &[Vec<f64>],
    numerator_graph: &VectorWfst<u32, W>,
    denominator_graph: &VectorWfst<u32, W>,
    config: &LfMmiConfig,
) -> LfMmiResult
where
    W: Semiring + From<f64> + Into<f64> + Clone,
{
    let num_frames = acoustic_scores.len();
    let num_pdfs = acoustic_scores.first().map_or(0, |v| v.len());

    // Compute numerator: log P(correct|x)
    let (num_log_prob, num_posteriors) =
        compute_graph_score(acoustic_scores, numerator_graph, config);

    // Compute denominator: log Σ_y P(y|x)
    let (den_log_prob, den_posteriors) =
        compute_graph_score(acoustic_scores, denominator_graph, config);

    // MMI loss = -(numerator - denominator)
    let mmi_loss = -(num_log_prob - den_log_prob);

    // Compute gradients: grad = den_posterior - num_posterior
    let mut gradients = LfMmiGradients::new(num_frames, num_pdfs);
    for t in 0..num_frames {
        for pdf in 0..num_pdfs {
            let grad = den_posteriors.get(t, pdf) - num_posteriors.get(t, pdf);
            gradients.set(t, pdf, grad);
        }
    }

    // Cross-entropy regularization
    let xent_loss = if config.xent_regularize > 0.0 {
        compute_xent_loss(acoustic_scores, &num_posteriors, num_frames, num_pdfs)
    } else {
        0.0
    };

    // L2 regularization
    let l2_loss = if config.l2_regularize > 0.0 {
        compute_l2_loss(acoustic_scores, config.l2_regularize)
    } else {
        0.0
    };

    let total_loss = mmi_loss + config.xent_regularize * xent_loss + l2_loss;

    LfMmiResult {
        loss: total_loss,
        numerator_log_prob: num_log_prob,
        denominator_log_prob: den_log_prob,
        xent_loss,
        gradients,
    }
}

/// Compute graph score using forward-backward algorithm.
fn compute_graph_score<W>(
    acoustic_scores: &[Vec<f64>],
    graph: &VectorWfst<u32, W>,
    config: &LfMmiConfig,
) -> (f64, LfMmiGradients)
where
    W: Semiring + From<f64> + Into<f64> + Clone,
{
    let num_frames = acoustic_scores.len();
    let num_pdfs = acoustic_scores.first().map_or(0, |v| v.len());
    let num_states = graph.num_states();

    // Forward pass: α[t, s] = log P(reach state s at time t from start)
    let mut alpha = vec![vec![f64::NEG_INFINITY; num_states]; num_frames + 1];
    alpha[0][graph.start() as usize] = 0.0;

    // State-to-frame alignment: track which PDF each transition uses
    let mut frame_posteriors = LfMmiGradients::new(num_frames, num_pdfs);

    for t in 0..num_frames {
        for s in 0..num_states {
            if alpha[t][s] <= f64::NEG_INFINITY {
                continue;
            }

            let state = s as StateId;
            for tr in graph.transitions(state) {
                let pdf = tr.input.unwrap_or(0) as usize;
                if pdf >= num_pdfs {
                    continue;
                }

                // Acoustic score + transition weight
                let acoustic = acoustic_scores[t][pdf];
                let transition_weight: f64 = tr.weight.clone().into();
                let arc_score = acoustic - transition_weight; // Convert tropical to log

                // Add leaky HMM regularization
                let leaky_score = if config.leaky_hmm_coefficient > 0.0 {
                    log_add(arc_score, config.leaky_hmm_coefficient.ln())
                } else {
                    arc_score
                };

                let new_alpha = alpha[t][s] + leaky_score;
                let next_state = tr.to as usize;
                alpha[t + 1][next_state] = log_add(alpha[t + 1][next_state], new_alpha);
            }
        }
    }

    // Backward pass: β[t, s] = log P(reach final from state s at time t)
    let mut beta = vec![vec![f64::NEG_INFINITY; num_states]; num_frames + 1];
    for s in 0..num_states {
        let state = s as StateId;
        if graph.is_final(state) {
            let final_weight: f64 = graph.final_weight(state).into();
            beta[num_frames][s] = -final_weight; // Convert tropical to log
        }
    }

    for t in (0..num_frames).rev() {
        for s in 0..num_states {
            let state = s as StateId;
            for tr in graph.transitions(state) {
                let pdf = tr.input.unwrap_or(0) as usize;
                if pdf >= num_pdfs {
                    continue;
                }

                let next_state = tr.to as usize;
                if beta[t + 1][next_state] <= f64::NEG_INFINITY {
                    continue;
                }

                let acoustic = acoustic_scores[t][pdf];
                let transition_weight: f64 = tr.weight.clone().into();
                let arc_score = acoustic - transition_weight;

                let new_beta = arc_score + beta[t + 1][next_state];
                beta[t][s] = log_add(beta[t][s], new_beta);
            }
        }
    }

    // Total log-probability
    let total_log_prob = alpha[num_frames]
        .iter()
        .enumerate()
        .filter(|(s, _)| graph.is_final(*s as StateId))
        .map(|(s, &a)| {
            let final_weight: f64 = graph.final_weight(s as StateId).into();
            a - final_weight
        })
        .fold(f64::NEG_INFINITY, log_add);

    // Compute posteriors: P(arc at time t) = exp(α + arc_score + β - total)
    for t in 0..num_frames {
        for s in 0..num_states {
            if alpha[t][s] <= f64::NEG_INFINITY {
                continue;
            }

            let state = s as StateId;
            for tr in graph.transitions(state) {
                let pdf = tr.input.unwrap_or(0) as usize;
                if pdf >= num_pdfs {
                    continue;
                }

                let next_state = tr.to as usize;
                if beta[t + 1][next_state] <= f64::NEG_INFINITY {
                    continue;
                }

                let acoustic = acoustic_scores[t][pdf];
                let transition_weight: f64 = tr.weight.clone().into();
                let arc_score = acoustic - transition_weight;

                let posterior =
                    (alpha[t][s] + arc_score + beta[t + 1][next_state] - total_log_prob).exp();
                frame_posteriors.add(t, pdf, posterior);
            }
        }
    }

    (total_log_prob, frame_posteriors)
}

/// Compute cross-entropy loss component.
fn compute_xent_loss(
    acoustic_scores: &[Vec<f64>],
    posteriors: &LfMmiGradients,
    num_frames: usize,
    num_pdfs: usize,
) -> f64 {
    let mut loss = 0.0;
    for t in 0..num_frames {
        for pdf in 0..num_pdfs {
            let posterior = posteriors.get(t, pdf);
            if posterior > 1e-10 {
                // Cross-entropy: -Σ p(pdf) * log q(pdf)
                let log_prob = acoustic_scores[t][pdf];
                loss -= posterior * log_prob;
            }
        }
    }
    loss / num_frames as f64
}

/// Compute L2 regularization loss.
fn compute_l2_loss(acoustic_scores: &[Vec<f64>], l2_weight: f64) -> f64 {
    let mut loss = 0.0;
    for frame in acoustic_scores {
        for &score in frame {
            loss += score * score;
        }
    }
    0.5 * l2_weight * loss
}

/// Build numerator graph from alignment/transcription.
///
/// Creates an FST that accepts only the correct transcription,
/// with appropriate HMM topology.
pub fn build_numerator_graph<W>(
    transcript: &[u32],
    _pdf_to_phone: &[u32],
    hmm_topo: &HmmTopology,
) -> VectorWfst<u32, W>
where
    W: Semiring + From<f64>,
{
    let mut fst: VectorWfst<u32, W> = VectorWfst::new();

    // Build linear FST with HMM states
    let mut current_state = fst.add_state();
    fst.set_start(current_state);

    for &phone in transcript {
        // Add states for each HMM state of the phone
        let num_hmm_states = hmm_topo.num_states_for_phone(phone);

        for hmm_state in 0..num_hmm_states {
            let pdf = hmm_topo.pdf_for_state(phone, hmm_state);
            let next_state = fst.add_state();

            // Self-loop
            fst.add_transition(WeightedTransition {
                from: current_state,
                input: Some(pdf),
                output: Some(pdf),
                to: current_state,
                weight: W::from(hmm_topo.self_loop_prob(phone, hmm_state).ln()),
            });

            // Forward transition
            fst.add_transition(WeightedTransition {
                from: current_state,
                input: Some(pdf),
                output: Some(pdf),
                to: next_state,
                weight: W::from(hmm_topo.forward_prob(phone, hmm_state).ln()),
            });

            current_state = next_state;
        }
    }

    fst.set_final(current_state, W::one());
    fst
}

/// Build denominator graph (phone loop + optional LM).
///
/// This graph accepts any sequence of phones, representing all
/// possible transcriptions.
pub fn build_denominator_graph<W>(
    num_phones: usize,
    hmm_topo: &HmmTopology,
    phone_lm: Option<&VectorWfst<u32, W>>,
) -> VectorWfst<u32, W>
where
    W: Semiring + From<f64> + Clone,
{
    if let Some(lm) = phone_lm {
        // Use phone LM for better denominator estimation
        // Clone LM structure but add HMM expansions
        // (Simplified: just return the LM directly for now)
        return lm.clone();
    }

    let mut fst: VectorWfst<u32, W> = VectorWfst::new();

    // Simple phone loop (no LM)
    // Single state with transitions for all phones
    let state = fst.add_state();
    fst.set_start(state);
    fst.set_final(state, W::one());

    for phone in 0..num_phones as u32 {
        let num_hmm_states = hmm_topo.num_states_for_phone(phone);

        for hmm_state in 0..num_hmm_states {
            let pdf = hmm_topo.pdf_for_state(phone, hmm_state);

            // Add transition for this PDF
            fst.add_transition(WeightedTransition {
                from: state,
                input: Some(pdf),
                output: Some(pdf),
                to: state,
                weight: W::from(0.0), // Uniform prior
            });
        }
    }

    fst
}

/// HMM topology specification.
#[derive(Debug, Clone)]
pub struct HmmTopology {
    /// Number of HMM states per phone.
    pub states_per_phone: usize,
    /// Self-loop probability.
    pub self_loop_prob: f64,
    /// Forward transition probability.
    pub forward_prob: f64,
    /// Total number of phones.
    pub num_phones: usize,
}

impl Default for HmmTopology {
    fn default() -> Self {
        Self {
            states_per_phone: 3,
            self_loop_prob: 0.5,
            forward_prob: 0.5,
            num_phones: 0,
        }
    }
}

impl HmmTopology {
    /// Create a new HMM topology.
    pub fn new(num_phones: usize, states_per_phone: usize) -> Self {
        Self {
            states_per_phone,
            self_loop_prob: 0.5,
            forward_prob: 0.5,
            num_phones,
        }
    }

    /// Number of HMM states for a phone.
    pub fn num_states_for_phone(&self, _phone: u32) -> usize {
        self.states_per_phone
    }

    /// Get PDF ID for a phone's HMM state.
    pub fn pdf_for_state(&self, phone: u32, hmm_state: usize) -> u32 {
        phone * self.states_per_phone as u32 + hmm_state as u32
    }

    /// Self-loop probability for a state.
    pub fn self_loop_prob(&self, _phone: u32, _hmm_state: usize) -> f64 {
        self.self_loop_prob
    }

    /// Forward transition probability for a state.
    pub fn forward_prob(&self, _phone: u32, _hmm_state: usize) -> f64 {
        self.forward_prob
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_hmm_topology() {
        let topo = HmmTopology::new(40, 3);

        assert_eq!(topo.num_states_for_phone(0), 3);
        assert_eq!(topo.pdf_for_state(0, 0), 0);
        assert_eq!(topo.pdf_for_state(0, 1), 1);
        assert_eq!(topo.pdf_for_state(1, 0), 3);
    }

    #[test]
    fn test_denominator_graph() {
        let topo = HmmTopology::new(10, 3);
        let graph: VectorWfst<u32, TropicalWeight> = build_denominator_graph(10, &topo, None);

        assert_eq!(graph.num_states(), 1);
        assert!(graph.is_final(0));
        // Should have transitions for all PDFs
        assert_eq!(graph.transitions(0).len(), 30); // 10 phones * 3 states
    }

    #[test]
    fn test_lfmmi_gradients() {
        let mut grads = LfMmiGradients::new(10, 100);

        grads.set(0, 50, 0.5);
        assert!((grads.get(0, 50) - 0.5).abs() < 1e-10);

        grads.add(0, 50, 0.3);
        assert!((grads.get(0, 50) - 0.8).abs() < 1e-10);
    }
}
