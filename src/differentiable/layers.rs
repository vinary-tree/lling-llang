//! Convolutional WFST layers for neural network integration.
//!
//! This module provides WFST-based convolutional layers that can be used
//! as drop-in replacements for traditional convolutions in neural networks.
//!
//! ## Architecture
//!
//! A WFST convolutional layer applies kernel WFSTs to receptive fields:
//!
//! ```text
//! H'_{i,t} = logadd_{p∈K_i∘R_{H_{t:t+k}}} s(p)
//! ```
//!
//! Where:
//! - K_i is the i-th kernel WFST
//! - R_{H_{t:t+k}} is the receptive field as a linear graph
//! - s(p) is the path score
//!
//! ## Benefits
//!
//! - Far fewer parameters than a dense convolution (Hannun et al. (2020) report ≈38× on their setup)
//! - Better accuracy in many sequence modeling tasks
//! - Parameters scale with token vocabulary, not input channels
//!
//! ## References
//!
//! - Hannun et al., "Differentiable Weighted Finite-State Transducers" (ICML 2020, arXiv:2010.01003)

use super::forward_score::forward_score;
use super::gradient::{backward, GradientAccumulator, GradientWfst};
use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst};

/// A single WFST kernel for convolution.
///
/// Each kernel is a WFST that processes a receptive field and produces
/// a scalar output via forward score computation.
#[derive(Clone, Debug)]
pub struct WfstKernel<L: Clone + Send + Sync> {
    /// The kernel WFST.
    pub fst: VectorWfst<L, LogWeight>,
    /// Kernel size (receptive field width).
    pub kernel_size: usize,
}

impl<L: Clone + Send + Sync + Default + Eq + std::hash::Hash> WfstKernel<L> {
    /// Create a new WFST kernel with random initialization.
    ///
    /// # Arguments
    ///
    /// * `vocab_size` - Number of vocabulary items (input labels)
    /// * `kernel_size` - Receptive field width
    /// * `init_weight` - Initial weight for transitions
    pub fn new(vocab_size: usize, kernel_size: usize, init_weight: f64) -> Self {
        let mut fst = VectorWfst::new();

        // Create a linear chain of states for the kernel
        let mut states = Vec::with_capacity(kernel_size + 1);
        for _ in 0..=kernel_size {
            states.push(fst.add_state());
        }

        fst.set_start(states[0]);
        fst.set_final(states[kernel_size], LogWeight::one());

        // Add transitions between consecutive states
        // Each position can accept any vocabulary item
        for pos in 0..kernel_size {
            for _label_idx in 0..vocab_size {
                // We'll use a placeholder label - in practice this would be
                // the actual vocabulary labels
                fst.add_arc(
                    states[pos],
                    None, // Will be matched during composition
                    None,
                    states[pos + 1],
                    LogWeight::new(init_weight),
                );
            }
        }

        Self { fst, kernel_size }
    }

    /// Create a kernel from an existing WFST.
    pub fn from_wfst(fst: VectorWfst<L, LogWeight>, kernel_size: usize) -> Self {
        Self { fst, kernel_size }
    }
}

/// Receptive field as a linear graph.
///
/// Represents a window of hidden states as a weighted linear chain WFST.
#[derive(Clone, Debug)]
pub struct ReceptiveField<L: Clone + Send + Sync> {
    /// The linear graph WFST.
    pub fst: VectorWfst<L, LogWeight>,
    /// Start position in the input sequence.
    pub start_pos: usize,
    /// Window size.
    pub size: usize,
}

impl<L: Clone + Send + Sync + Default + Eq + std::hash::Hash> ReceptiveField<L> {
    /// Create a receptive field from hidden state values.
    ///
    /// # Arguments
    ///
    /// * `hidden_states` - Slice of (label, weight) pairs
    /// * `start_pos` - Starting position in the sequence
    pub fn from_hidden_states(hidden_states: &[(L, f64)], start_pos: usize) -> Self {
        let size = hidden_states.len();
        let mut fst = VectorWfst::new();

        // Create linear chain
        let mut states = Vec::with_capacity(size + 1);
        for _ in 0..=size {
            states.push(fst.add_state());
        }

        fst.set_start(states[0]);
        fst.set_final(states[size], LogWeight::one());

        // Add transitions with hidden state values as weights
        for (i, (label, weight)) in hidden_states.iter().enumerate() {
            fst.add_arc(
                states[i],
                Some(label.clone()),
                Some(label.clone()),
                states[i + 1],
                LogWeight::new(*weight),
            );
        }

        Self {
            fst,
            start_pos,
            size,
        }
    }

    /// Create a receptive field from weight values only.
    pub fn from_weights(weights: &[f64], start_pos: usize) -> Self
    where
        L: Default,
    {
        let size = weights.len();
        let mut fst = VectorWfst::new();

        let mut states = Vec::with_capacity(size + 1);
        for _ in 0..=size {
            states.push(fst.add_state());
        }

        fst.set_start(states[0]);
        fst.set_final(states[size], LogWeight::one());

        for (i, &weight) in weights.iter().enumerate() {
            fst.add_arc(states[i], None, None, states[i + 1], LogWeight::new(weight));
        }

        Self {
            fst,
            start_pos,
            size,
        }
    }
}

/// Configuration for WFST convolutional layer.
#[derive(Clone, Debug)]
pub struct WfstConvConfig {
    /// Number of input channels (vocabulary size).
    pub input_channels: usize,
    /// Number of output channels (number of kernels).
    pub output_channels: usize,
    /// Kernel size (receptive field width).
    pub kernel_size: usize,
    /// Stride for sliding the receptive field.
    pub stride: usize,
    /// Padding mode.
    pub padding: PaddingMode,
}

/// Padding mode for convolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaddingMode {
    /// No padding - output is shorter than input.
    Valid,
    /// Pad to maintain input length.
    Same,
    /// Custom padding amount.
    Custom(usize),
}

impl Default for WfstConvConfig {
    fn default() -> Self {
        Self {
            input_channels: 256,
            output_channels: 256,
            kernel_size: 3,
            stride: 1,
            padding: PaddingMode::Same,
        }
    }
}

/// WFST-based convolutional layer.
///
/// This layer applies multiple WFST kernels to sliding receptive fields
/// over an input sequence, producing output features via forward score
/// computation.
#[derive(Clone, Debug)]
pub struct WfstConvLayer<L: Clone + Send + Sync> {
    /// Kernel WFSTs, one per output channel.
    pub kernels: Vec<WfstKernel<L>>,
    /// Layer configuration.
    pub config: WfstConvConfig,
}

impl<L: Clone + Send + Sync + Default + Eq + std::hash::Hash> WfstConvLayer<L> {
    /// Create a new WFST convolutional layer.
    pub fn new(config: WfstConvConfig) -> Self {
        let kernels = (0..config.output_channels)
            .map(|_| WfstKernel::new(config.input_channels, config.kernel_size, 0.0))
            .collect();

        Self { kernels, config }
    }

    /// Create from existing kernels.
    pub fn from_kernels(kernels: Vec<WfstKernel<L>>, config: WfstConvConfig) -> Self {
        Self { kernels, config }
    }

    /// Compute the output length given input length.
    pub fn output_length(&self, input_length: usize) -> usize {
        let padding = match self.config.padding {
            PaddingMode::Valid => 0,
            PaddingMode::Same => self.config.kernel_size / 2,
            PaddingMode::Custom(p) => p,
        };

        let padded_length = input_length + 2 * padding;
        if padded_length < self.config.kernel_size {
            return 0;
        }

        (padded_length - self.config.kernel_size) / self.config.stride + 1
    }

    /// Get the number of parameters in this layer.
    pub fn num_parameters(&self) -> usize {
        self.kernels.iter().map(|k| count_arcs(&k.fst)).sum()
    }
}

/// Result of WFST convolution forward pass.
///
/// Returned by [`wfst_conv_forward_with_gradients`] for callers that need the
/// per-kernel, per-position gradient WFSTs in addition to the output features.
#[derive(Clone, Debug)]
pub struct WfstConvOutput {
    /// Output features: [output_channels, output_length].
    pub features: Vec<Vec<f64>>,
    /// Gradient WFSTs for backward pass.
    pub gradient_wfsts: Vec<Vec<GradientWfst<u32>>>,
}

/// Apply WFST convolution to input features.
///
/// # Arguments
///
/// * `layer` - The WFST convolutional layer
/// * `input` - Input features as [input_length, input_channels] weights
///
/// # Returns
///
/// Output features as [output_channels, output_length].
pub fn wfst_conv_forward<L: Clone + Send + Sync + Default + Eq + std::hash::Hash>(
    layer: &WfstConvLayer<L>,
    input: &[Vec<f64>],
) -> Vec<Vec<f64>> {
    let input_length = input.len();
    let output_length = layer.output_length(input_length);
    let num_kernels = layer.kernels.len();

    // Initialize output
    let mut output = vec![vec![0.0; output_length]; num_kernels];

    if output_length == 0 {
        return output;
    }

    // Compute padding
    let padding = match layer.config.padding {
        PaddingMode::Valid => 0,
        PaddingMode::Same => layer.config.kernel_size / 2,
        PaddingMode::Custom(p) => p,
    };

    // Apply each kernel at each position
    for kernel_idx in 0..num_kernels {
        let kernel = &layer.kernels[kernel_idx];

        for out_pos in 0..output_length {
            let in_start = out_pos * layer.config.stride;

            // Extract receptive field weights
            let mut rf_weights = Vec::with_capacity(layer.config.kernel_size);
            for k in 0..layer.config.kernel_size {
                let in_pos = in_start + k;
                if in_pos < padding || in_pos >= padding + input_length {
                    // Padding position - use zero weight
                    rf_weights.push(0.0);
                } else {
                    // Sum over input channels for this position
                    let actual_pos = in_pos - padding;
                    let weight: f64 = input[actual_pos].iter().sum();
                    rf_weights.push(weight);
                }
            }

            // Create receptive field graph and compose with kernel
            let rf: ReceptiveField<L> = ReceptiveField::from_weights(&rf_weights, in_start);

            // Compute forward score (log-sum-exp over paths)
            // For now, we'll use a simplified computation
            // In full implementation, this would compose rf.fst with kernel.fst
            let score = compute_receptive_field_score(&rf, kernel);
            output[kernel_idx][out_pos] = score;
        }
    }

    output
}

/// Apply WFST convolution, returning both features and per-position gradient WFSTs.
///
/// Use this instead of [`wfst_conv_forward`] when the caller needs to perform a
/// backward pass through the convolution. The returned [`WfstConvOutput`] carries
/// the receptive-field gradient WFSTs for each (kernel, output_position) pair.
pub fn wfst_conv_forward_with_gradients<L: Clone + Send + Sync + Default + Eq + std::hash::Hash>(
    layer: &WfstConvLayer<L>,
    input: &[Vec<f64>],
) -> WfstConvOutput {
    let input_length = input.len();
    let output_length = layer.output_length(input_length);
    let num_kernels = layer.kernels.len();

    let mut features = vec![vec![0.0; output_length]; num_kernels];
    let mut gradient_wfsts: Vec<Vec<GradientWfst<u32>>> = (0..num_kernels)
        .map(|_| Vec::with_capacity(output_length))
        .collect();

    if output_length == 0 {
        return WfstConvOutput {
            features,
            gradient_wfsts,
        };
    }

    let padding = match layer.config.padding {
        PaddingMode::Valid => 0,
        PaddingMode::Same => layer.config.kernel_size / 2,
        PaddingMode::Custom(p) => p,
    };

    for kernel_idx in 0..num_kernels {
        let kernel = &layer.kernels[kernel_idx];

        for out_pos in 0..output_length {
            let in_start = out_pos * layer.config.stride;

            let mut rf_weights = Vec::with_capacity(layer.config.kernel_size);
            for k in 0..layer.config.kernel_size {
                let in_pos = in_start + k;
                if in_pos < padding || in_pos >= padding + input_length {
                    rf_weights.push(0.0);
                } else {
                    let actual_pos = in_pos - padding;
                    let weight: f64 = input[actual_pos].iter().sum();
                    rf_weights.push(weight);
                }
            }

            let rf: ReceptiveField<L> = ReceptiveField::from_weights(&rf_weights, in_start);
            let grad_fst = GradientWfst::from_wfst(&rf.fst);
            let score = forward_score(&grad_fst);

            features[kernel_idx][out_pos] = score.value();
            // Retain the gradient WFST keyed by u32 for the backward pass.
            gradient_wfsts[kernel_idx].push(GradientWfst::from_wfst(&u32_view(&rf.fst, kernel)));
        }
    }

    WfstConvOutput {
        features,
        gradient_wfsts,
    }
}

/// Build a `u32`-labeled view of the receptive-field FST so it composes with
/// kernel FSTs uniformly. The kernel parameter is currently ignored; it is taken
/// so future kernel-aware composition can drop in without changing the signature.
fn u32_view<L: Clone + Send + Sync>(
    rf: &crate::wfst::VectorWfst<L, crate::semiring::LogWeight>,
    _kernel: &WfstKernel<L>,
) -> crate::wfst::VectorWfst<u32, crate::semiring::LogWeight> {
    use crate::wfst::{MutableWfst, StateId, Wfst};
    let mut out: crate::wfst::VectorWfst<u32, crate::semiring::LogWeight> =
        crate::wfst::VectorWfst::new();
    for _ in 0..rf.num_states() {
        out.add_state();
    }
    if rf.start() != crate::wfst::NO_STATE {
        out.set_start(rf.start());
    }
    for state in 0..rf.num_states() as StateId {
        if rf.is_final(state) {
            out.set_final(state, rf.final_weight(state));
        }
        for (idx, arc) in rf.transitions(state).iter().enumerate() {
            out.add_arc(
                arc.from,
                Some(idx as u32),
                Some(idx as u32),
                arc.to,
                arc.weight,
            );
        }
    }
    out
}

/// Compute forward score for a receptive field through a kernel.
fn compute_receptive_field_score<L: Clone + Send + Sync>(
    rf: &ReceptiveField<L>,
    _kernel: &WfstKernel<L>,
) -> f64 {
    // Simplified: just sum the receptive field weights
    // Full implementation would compose and compute forward score
    let grad_fst = GradientWfst::from_wfst(&rf.fst);
    let score = forward_score(&grad_fst);
    score.value()
}

/// Compute gradients for WFST convolution backward pass.
///
/// # Arguments
///
/// * `layer` - The WFST convolutional layer
/// * `input` - Original input features
/// * `output_grad` - Gradient of loss with respect to output
///
/// # Returns
///
/// Tuple of (input gradients, kernel gradients).
pub fn wfst_conv_backward<L: Clone + Send + Sync + Default + Eq + std::hash::Hash>(
    layer: &WfstConvLayer<L>,
    input: &[Vec<f64>],
    output_grad: &[Vec<f64>],
) -> (Vec<Vec<f64>>, Vec<GradientAccumulator>) {
    let input_length = input.len();
    let input_channels = if input.is_empty() { 0 } else { input[0].len() };
    let output_length = layer.output_length(input_length);
    let num_kernels = layer.kernels.len();

    // Initialize input gradients
    let mut input_grad = vec![vec![0.0; input_channels]; input_length];

    // Initialize kernel gradients
    let mut kernel_grads: Vec<GradientAccumulator> = layer
        .kernels
        .iter()
        .map(|k| GradientAccumulator::with_capacity(count_arcs(&k.fst)))
        .collect();

    if output_length == 0 {
        return (input_grad, kernel_grads);
    }

    // Compute padding
    let padding = match layer.config.padding {
        PaddingMode::Valid => 0,
        PaddingMode::Same => layer.config.kernel_size / 2,
        PaddingMode::Custom(p) => p,
    };

    // Backward pass through each kernel at each position
    for kernel_idx in 0..num_kernels {
        let _kernel = &layer.kernels[kernel_idx];

        for out_pos in 0..output_length {
            let in_start = out_pos * layer.config.stride;
            let out_grad = output_grad[kernel_idx][out_pos];

            // Extract receptive field weights
            let mut rf_weights = Vec::with_capacity(layer.config.kernel_size);
            for k in 0..layer.config.kernel_size {
                let in_pos = in_start + k;
                if in_pos < padding || in_pos >= padding + input_length {
                    rf_weights.push(0.0);
                } else {
                    let actual_pos = in_pos - padding;
                    let weight: f64 = input[actual_pos].iter().sum();
                    rf_weights.push(weight);
                }
            }

            // Create receptive field and compute backward pass
            let rf: ReceptiveField<L> = ReceptiveField::from_weights(&rf_weights, in_start);
            let grad_fst = GradientWfst::from_wfst(&rf.fst);
            let _ = forward_score(&grad_fst);
            let rf_grads = backward(&grad_fst);

            // Accumulate input gradients
            for arc_grad in &rf_grads.arc_gradients {
                let k = arc_grad.arc.from as usize;
                let in_pos = in_start + k;
                if in_pos >= padding && in_pos < padding + input_length {
                    let actual_pos = in_pos - padding;
                    // Distribute gradient across input channels
                    for c in 0..input_channels {
                        input_grad[actual_pos][c] +=
                            out_grad * arc_grad.gradient / input_channels as f64;
                    }
                }
            }

            // Accumulate kernel gradients
            // In full implementation, this would use gradients from composition
            kernel_grads[kernel_idx].merge(&rf_grads);
        }
    }

    (input_grad, kernel_grads)
}

/// Count total arcs in a WFST.
fn count_arcs<L: Clone + Send + Sync, W: Semiring>(fst: &VectorWfst<L, W>) -> usize {
    let mut count = 0;
    for s in 0..fst.num_states() as StateId {
        count += fst.transitions(s).len();
    }
    count
}

/// Statistics about a WFST convolutional layer.
#[derive(Clone, Debug, Default)]
pub struct WfstConvStats {
    /// Number of kernels.
    pub num_kernels: usize,
    /// Kernel size.
    pub kernel_size: usize,
    /// Total number of parameters.
    pub num_parameters: usize,
    /// Comparison: equivalent traditional conv parameters.
    pub equiv_traditional_params: usize,
    /// Parameter reduction ratio.
    pub reduction_ratio: f64,
}

impl<L: Clone + Send + Sync + Default + Eq + std::hash::Hash> WfstConvLayer<L> {
    /// Get statistics about this layer.
    pub fn stats(&self) -> WfstConvStats {
        let num_parameters = self.num_parameters();
        let equiv_traditional =
            self.config.input_channels * self.config.output_channels * self.config.kernel_size;

        WfstConvStats {
            num_kernels: self.kernels.len(),
            kernel_size: self.config.kernel_size,
            num_parameters,
            equiv_traditional_params: equiv_traditional,
            reduction_ratio: if num_parameters > 0 {
                equiv_traditional as f64 / num_parameters as f64
            } else {
                0.0
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wfst_kernel_creation() {
        let kernel = WfstKernel::<u32>::new(10, 3, 0.0);
        assert_eq!(kernel.kernel_size, 3);
        assert!(kernel.fst.num_states() > 0);
    }

    #[test]
    fn test_receptive_field_from_weights() {
        let weights = vec![1.0, 2.0, 3.0];
        let rf = ReceptiveField::<u32>::from_weights(&weights, 0);

        assert_eq!(rf.size, 3);
        assert_eq!(rf.start_pos, 0);
        assert_eq!(rf.fst.num_states(), 4); // 3 transitions + 1 = 4 states
    }

    #[test]
    fn test_wfst_conv_config_default() {
        let config = WfstConvConfig::default();
        assert_eq!(config.kernel_size, 3);
        assert_eq!(config.stride, 1);
    }

    #[test]
    fn test_wfst_conv_layer_creation() {
        let config = WfstConvConfig {
            input_channels: 10,
            output_channels: 5,
            kernel_size: 3,
            stride: 1,
            padding: PaddingMode::Valid,
        };

        let layer = WfstConvLayer::<u32>::new(config);
        assert_eq!(layer.kernels.len(), 5);
    }

    #[test]
    fn test_output_length_valid_padding() {
        let config = WfstConvConfig {
            input_channels: 10,
            output_channels: 5,
            kernel_size: 3,
            stride: 1,
            padding: PaddingMode::Valid,
        };

        let layer = WfstConvLayer::<u32>::new(config);
        assert_eq!(layer.output_length(10), 8); // 10 - 3 + 1 = 8
    }

    #[test]
    fn test_output_length_same_padding() {
        let config = WfstConvConfig {
            input_channels: 10,
            output_channels: 5,
            kernel_size: 3,
            stride: 1,
            padding: PaddingMode::Same,
        };

        let layer = WfstConvLayer::<u32>::new(config);
        // Same padding: input + 2*(k/2) - k + 1 = input for stride 1
        assert_eq!(layer.output_length(10), 10);
    }

    #[test]
    fn test_wfst_conv_forward() {
        let config = WfstConvConfig {
            input_channels: 2,
            output_channels: 2,
            kernel_size: 2,
            stride: 1,
            padding: PaddingMode::Valid,
        };

        let layer = WfstConvLayer::<u32>::new(config);
        let input = vec![vec![1.0, 0.5], vec![0.5, 1.0], vec![1.0, 0.5]];

        let output = wfst_conv_forward(&layer, &input);

        assert_eq!(output.len(), 2); // 2 output channels
        assert_eq!(output[0].len(), 2); // input_len - kernel_size + 1 = 2
    }

    #[test]
    fn test_wfst_conv_stats() {
        let config = WfstConvConfig {
            input_channels: 256,
            output_channels: 256,
            kernel_size: 3,
            stride: 1,
            padding: PaddingMode::Same,
        };

        let layer = WfstConvLayer::<u32>::new(config);
        let stats = layer.stats();

        assert_eq!(stats.num_kernels, 256);
        assert_eq!(stats.kernel_size, 3);
        // Traditional conv: 256 * 256 * 3 = 196608
        assert_eq!(stats.equiv_traditional_params, 196608);
    }

    #[test]
    fn test_padding_mode_custom() {
        let config = WfstConvConfig {
            input_channels: 10,
            output_channels: 5,
            kernel_size: 3,
            stride: 1,
            padding: PaddingMode::Custom(2),
        };

        let layer = WfstConvLayer::<u32>::new(config);
        // Input 10 + 2*2 padding = 14, output = 14 - 3 + 1 = 12
        assert_eq!(layer.output_length(10), 12);
    }

    #[test]
    fn test_stride_greater_than_one() {
        let config = WfstConvConfig {
            input_channels: 10,
            output_channels: 5,
            kernel_size: 3,
            stride: 2,
            padding: PaddingMode::Valid,
        };

        let layer = WfstConvLayer::<u32>::new(config);
        // Output = (10 - 3) / 2 + 1 = 4
        assert_eq!(layer.output_length(10), 4);
    }
}
