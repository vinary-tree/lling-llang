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

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

use rustc_hash::FxHashMap;

use super::forward_score::forward_score;
use super::gradient::{backward, ArcIndex, GradientAccumulator, GradientWfst};
use crate::composition::{EpsilonFilter, FilterState, ProductStateId};
use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst};

/// Label type that can represent dense input-channel indices.
///
/// WFST convolution treats each input channel as a vocabulary item. Custom label
/// types can implement this trait to control how channel indices are encoded in
/// kernel and receptive-field graphs.
pub trait WfstConvLabel: Clone + Send + Sync + Eq + Hash {
    /// Convert a zero-based channel index into a WFST label.
    fn from_channel_index(index: usize) -> Self;
}

impl WfstConvLabel for u32 {
    #[inline]
    fn from_channel_index(index: usize) -> Self {
        index as u32
    }
}

impl WfstConvLabel for u64 {
    #[inline]
    fn from_channel_index(index: usize) -> Self {
        index as u64
    }
}

impl WfstConvLabel for usize {
    #[inline]
    fn from_channel_index(index: usize) -> Self {
        index
    }
}

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

impl<L: WfstConvLabel> WfstKernel<L> {
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

        // Add labeled transitions between consecutive states. Each position can
        // accept any vocabulary item/channel.
        for pos in 0..kernel_size {
            for label_idx in 0..vocab_size {
                let label = L::from_channel_index(label_idx);
                fst.add_arc(
                    states[pos],
                    Some(label.clone()),
                    Some(label),
                    states[pos + 1],
                    LogWeight::new(init_weight),
                );
            }
        }

        Self { fst, kernel_size }
    }
}

impl<L: Clone + Send + Sync> WfstKernel<L> {
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

impl<L: Clone + Send + Sync> ReceptiveField<L> {
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

impl<L: WfstConvLabel> WfstConvLayer<L> {
    /// Create a new WFST convolutional layer.
    pub fn new(config: WfstConvConfig) -> Self {
        let kernels = (0..config.output_channels)
            .map(|_| WfstKernel::new(config.input_channels, config.kernel_size, 0.0))
            .collect();

        Self { kernels, config }
    }
}

impl<L: Clone + Send + Sync> WfstConvLayer<L> {
    /// Create from existing kernels.
    pub fn from_kernels(kernels: Vec<WfstKernel<L>>, config: WfstConvConfig) -> Self {
        Self { kernels, config }
    }

    /// Compute the output length given input length.
    pub fn output_length(&self, input_length: usize) -> usize {
        if self.config.stride == 0 {
            return 0;
        }

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
pub struct WfstConvOutput<L: Clone> {
    /// Output features: [output_channels, output_length].
    pub features: Vec<Vec<f64>>,
    /// Gradient WFSTs for backward pass.
    pub gradient_wfsts: Vec<Vec<GradientWfst<L>>>,
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
pub fn wfst_conv_forward<L: WfstConvLabel>(
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
            let window = build_receptive_field_window(input, &layer.config, padding, in_start);
            let composed = compose_receptive_field(kernel, &window.rf);
            output[kernel_idx][out_pos] = compute_forward_score(&composed.fst);
        }
    }

    output
}

/// Apply WFST convolution, returning both features and per-position gradient WFSTs.
///
/// Use this instead of [`wfst_conv_forward`] when the caller needs to perform a
/// backward pass through the convolution. The returned [`WfstConvOutput`] carries
/// the receptive-field gradient WFSTs for each (kernel, output_position) pair.
pub fn wfst_conv_forward_with_gradients<L: WfstConvLabel>(
    layer: &WfstConvLayer<L>,
    input: &[Vec<f64>],
) -> WfstConvOutput<L> {
    let input_length = input.len();
    let output_length = layer.output_length(input_length);
    let num_kernels = layer.kernels.len();

    let mut features = vec![vec![0.0; output_length]; num_kernels];
    let mut gradient_wfsts: Vec<Vec<GradientWfst<L>>> = (0..num_kernels)
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
            let window = build_receptive_field_window(input, &layer.config, padding, in_start);
            let composed = compose_receptive_field(kernel, &window.rf);
            let grad_fst = GradientWfst::from_wfst(&composed.fst);
            let score = forward_score(&grad_fst);

            features[kernel_idx][out_pos] = score.value();
            gradient_wfsts[kernel_idx].push(grad_fst);
        }
    }

    WfstConvOutput {
        features,
        gradient_wfsts,
    }
}

/// Compute the forward score of an already composed convolution graph.
fn compute_forward_score<L: Clone + Send + Sync>(fst: &VectorWfst<L, LogWeight>) -> f64 {
    let grad_fst = GradientWfst::from_wfst(fst);
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
pub fn wfst_conv_backward<L: WfstConvLabel>(
    layer: &WfstConvLayer<L>,
    input: &[Vec<f64>],
    output_grad: &[Vec<f64>],
) -> (Vec<Vec<f64>>, Vec<GradientAccumulator>) {
    let input_length = input.len();
    let input_channels = layer.config.input_channels;
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
        let kernel = &layer.kernels[kernel_idx];

        for out_pos in 0..output_length {
            let in_start = out_pos * layer.config.stride;
            let out_grad = output_grad
                .get(kernel_idx)
                .and_then(|row| row.get(out_pos))
                .copied()
                .unwrap_or(0.0);

            let window = build_receptive_field_window(input, &layer.config, padding, in_start);
            let composed = compose_receptive_field(kernel, &window.rf);
            let grad_fst = GradientWfst::from_wfst(&composed.fst);
            let _ = forward_score(&grad_fst);
            let composed_grads = backward(&grad_fst);

            for arc_grad in &composed_grads.arc_gradients {
                let Some(origin) = composed.origins.get(&arc_grad.arc) else {
                    continue;
                };
                let scaled_gradient = out_grad * arc_grad.gradient;

                if let Some(kernel_arc) = origin.kernel {
                    kernel_grads[kernel_idx].add_gradient(kernel_arc, scaled_gradient);
                }

                if let Some(rf_arc) = origin.receptive_field {
                    if let Some(Some((actual_pos, channel))) = window.input_coords.get(&rf_arc) {
                        if *actual_pos < input_grad.len() && *channel < input_channels {
                            input_grad[*actual_pos][*channel] += scaled_gradient;
                        }
                    }
                }
            }
        }
    }

    (input_grad, kernel_grads)
}

/// Receptive field plus the map from each receptive-field arc to an input cell.
struct ReceptiveFieldWindow<L: Clone + Send + Sync> {
    rf: ReceptiveField<L>,
    input_coords: HashMap<ArcIndex, Option<(usize, usize)>>,
}

/// Origin information for a materialized composed arc.
#[derive(Clone, Copy, Debug)]
struct ComposedArcOrigin {
    kernel: Option<ArcIndex>,
    receptive_field: Option<ArcIndex>,
}

/// Materialized convolution composition plus arc-origin metadata.
struct ComposedConvolution<L: Clone + Send + Sync> {
    fst: VectorWfst<L, LogWeight>,
    origins: HashMap<ArcIndex, ComposedArcOrigin>,
}

/// Build a labeled receptive field for a sliding input window.
fn build_receptive_field_window<L: WfstConvLabel>(
    input: &[Vec<f64>],
    config: &WfstConvConfig,
    padding: usize,
    in_start: usize,
) -> ReceptiveFieldWindow<L> {
    let mut fst = VectorWfst::new();
    let mut states = Vec::with_capacity(config.kernel_size + 1);
    let mut input_coords = HashMap::new();

    for _ in 0..=config.kernel_size {
        states.push(fst.add_state());
    }

    fst.set_start(states[0]);
    fst.set_final(states[config.kernel_size], LogWeight::one());

    for k in 0..config.kernel_size {
        let in_pos = in_start + k;
        let actual_pos = if in_pos < padding || in_pos >= padding + input.len() {
            None
        } else {
            Some(in_pos - padding)
        };

        for channel in 0..config.input_channels {
            let label = L::from_channel_index(channel);
            let weight = actual_pos
                .and_then(|pos| input.get(pos))
                .and_then(|row| row.get(channel))
                .copied()
                .unwrap_or(0.0);
            let arc_idx = fst.transitions(states[k]).len();

            fst.add_arc(
                states[k],
                Some(label.clone()),
                Some(label),
                states[k + 1],
                LogWeight::new(weight),
            );

            input_coords.insert(
                ArcIndex::new(states[k], arc_idx),
                actual_pos.map(|pos| (pos, channel)),
            );
        }
    }

    ReceptiveFieldWindow {
        rf: ReceptiveField {
            fst,
            start_pos: in_start,
            size: config.kernel_size,
        },
        input_coords,
    }
}

/// Compose a kernel with a receptive field and retain arc origins.
fn compose_receptive_field<L: WfstConvLabel>(
    kernel: &WfstKernel<L>,
    rf: &ReceptiveField<L>,
) -> ComposedConvolution<L> {
    let mut result = VectorWfst::new();
    let mut origins = HashMap::new();
    let mut state_map: FxHashMap<ProductStateId, StateId> = FxHashMap::default();
    let mut queue = VecDeque::new();
    let filter = EpsilonFilter::default();

    let start_product = ProductStateId::new(kernel.fst.start(), rf.fst.start(), FilterState::None);
    let start_id = result.add_state();
    result.set_start(start_id);
    state_map.insert(start_product, start_id);
    queue.push_back(start_product);

    while let Some(product_state) = queue.pop_front() {
        let current_id = state_map[&product_state];

        if kernel.fst.is_final(product_state.s1) && rf.fst.is_final(product_state.s2) {
            let final_weight = kernel
                .fst
                .final_weight(product_state.s1)
                .times(&rf.fst.final_weight(product_state.s2));
            result.set_final(current_id, final_weight);
        }

        let (can_eps1, can_eps2, can_match) = filter.allowed_moves(product_state.filter);
        let kernel_transitions = kernel.fst.transitions(product_state.s1);
        let rf_transitions = rf.fst.transitions(product_state.s2);

        if can_eps1 {
            for (kernel_arc_idx, kernel_arc) in kernel_transitions.iter().enumerate() {
                if kernel_arc.to as usize >= kernel.fst.num_states() || kernel_arc.output.is_some()
                {
                    continue;
                }

                let target = ProductStateId::new(
                    kernel_arc.to,
                    product_state.s2,
                    filter.next_state(product_state.filter, true, false),
                );
                add_composed_arc(
                    ComposedArcBuilder {
                        result: &mut result,
                        state_map: &mut state_map,
                        queue: &mut queue,
                        origins: &mut origins,
                    },
                    ComposedArcSpec {
                        current_id,
                        target,
                        input: kernel_arc.input.clone(),
                        output: None,
                        weight: kernel_arc.weight,
                        origin: ComposedArcOrigin {
                            kernel: Some(ArcIndex::new(product_state.s1, kernel_arc_idx)),
                            receptive_field: None,
                        },
                    },
                );
            }
        }

        if can_eps2 {
            for (rf_arc_idx, rf_arc) in rf_transitions.iter().enumerate() {
                if rf_arc.to as usize >= rf.fst.num_states() || rf_arc.input.is_some() {
                    continue;
                }

                let target = ProductStateId::new(
                    product_state.s1,
                    rf_arc.to,
                    filter.next_state(product_state.filter, false, true),
                );
                add_composed_arc(
                    ComposedArcBuilder {
                        result: &mut result,
                        state_map: &mut state_map,
                        queue: &mut queue,
                        origins: &mut origins,
                    },
                    ComposedArcSpec {
                        current_id,
                        target,
                        input: None,
                        output: rf_arc.output.clone(),
                        weight: rf_arc.weight,
                        origin: ComposedArcOrigin {
                            kernel: None,
                            receptive_field: Some(ArcIndex::new(product_state.s2, rf_arc_idx)),
                        },
                    },
                );
            }
        }

        if can_match {
            for (kernel_arc_idx, kernel_arc) in kernel_transitions.iter().enumerate() {
                let Some(kernel_output) = kernel_arc.output.as_ref() else {
                    continue;
                };
                if kernel_arc.to as usize >= kernel.fst.num_states() {
                    continue;
                }

                for (rf_arc_idx, rf_arc) in rf_transitions.iter().enumerate() {
                    if rf_arc.to as usize >= rf.fst.num_states() {
                        continue;
                    }
                    if rf_arc.input.as_ref() != Some(kernel_output) {
                        continue;
                    }

                    let target = ProductStateId::new(
                        kernel_arc.to,
                        rf_arc.to,
                        filter.next_state(product_state.filter, false, false),
                    );
                    add_composed_arc(
                        ComposedArcBuilder {
                            result: &mut result,
                            state_map: &mut state_map,
                            queue: &mut queue,
                            origins: &mut origins,
                        },
                        ComposedArcSpec {
                            current_id,
                            target,
                            input: kernel_arc.input.clone(),
                            output: rf_arc.output.clone(),
                            weight: kernel_arc.weight.times(&rf_arc.weight),
                            origin: ComposedArcOrigin {
                                kernel: Some(ArcIndex::new(product_state.s1, kernel_arc_idx)),
                                receptive_field: Some(ArcIndex::new(product_state.s2, rf_arc_idx)),
                            },
                        },
                    );
                }
            }
        }
    }

    ComposedConvolution {
        fst: result,
        origins,
    }
}

struct ComposedArcBuilder<'a, L: Clone + Send + Sync> {
    result: &'a mut VectorWfst<L, LogWeight>,
    state_map: &'a mut FxHashMap<ProductStateId, StateId>,
    queue: &'a mut VecDeque<ProductStateId>,
    origins: &'a mut HashMap<ArcIndex, ComposedArcOrigin>,
}

struct ComposedArcSpec<L> {
    current_id: StateId,
    target: ProductStateId,
    input: Option<L>,
    output: Option<L>,
    weight: LogWeight,
    origin: ComposedArcOrigin,
}

fn add_composed_arc<L: Clone + Send + Sync>(
    builder: ComposedArcBuilder<'_, L>,
    spec: ComposedArcSpec<L>,
) {
    let target_id = if let Some(&id) = builder.state_map.get(&spec.target) {
        id
    } else {
        let new_id = builder.result.add_state();
        builder.state_map.insert(spec.target, new_id);
        builder.queue.push_back(spec.target);
        new_id
    };

    let arc_idx = builder.result.transitions(spec.current_id).len();
    builder.result.add_arc(
        spec.current_id,
        spec.input,
        spec.output,
        target_id,
        spec.weight,
    );
    builder
        .origins
        .insert(ArcIndex::new(spec.current_id, arc_idx), spec.origin);
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

impl<L: Clone + Send + Sync> WfstConvLayer<L> {
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
        assert_eq!(count_arcs(&kernel.fst), 30);
        assert_eq!(kernel.fst.transitions(0)[0].input, Some(0));
        assert_eq!(kernel.fst.transitions(0)[9].input, Some(9));
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
    fn test_wfst_conv_forward_respects_kernel_labels() {
        let mut kernel_fst = VectorWfst::<u32, LogWeight>::new();
        let s0 = kernel_fst.add_state();
        let s1 = kernel_fst.add_state();
        let s2 = kernel_fst.add_state();
        kernel_fst.set_start(s0);
        kernel_fst.set_final(s2, LogWeight::one());
        kernel_fst.add_arc(s0, Some(0), Some(0), s1, LogWeight::new(0.2));
        kernel_fst.add_arc(s1, Some(1), Some(1), s2, LogWeight::new(0.3));

        let layer = WfstConvLayer::from_kernels(
            vec![WfstKernel::from_wfst(kernel_fst, 2)],
            WfstConvConfig {
                input_channels: 2,
                output_channels: 1,
                kernel_size: 2,
                stride: 1,
                padding: PaddingMode::Valid,
            },
        );
        let input = vec![vec![1.0, 10.0], vec![20.0, 2.0]];

        let output = wfst_conv_forward(&layer, &input);

        assert_eq!(output.len(), 1);
        assert_eq!(output[0].len(), 1);
        assert!((output[0][0] - 3.5).abs() < 1e-9);
    }

    #[test]
    fn test_wfst_conv_forward_with_gradients_returns_composed_graphs() {
        let config = WfstConvConfig {
            input_channels: 2,
            output_channels: 1,
            kernel_size: 2,
            stride: 1,
            padding: PaddingMode::Valid,
        };
        let layer = WfstConvLayer::<u32>::new(config);
        let input = vec![vec![1.0, 2.0], vec![3.0, 4.0]];

        let output = wfst_conv_forward_with_gradients(&layer, &input);

        assert_eq!(output.features.len(), 1);
        assert_eq!(output.gradient_wfsts.len(), 1);
        assert_eq!(output.gradient_wfsts[0].len(), 1);
        assert!(output.gradient_wfsts[0][0].num_states() > 0);
    }

    #[test]
    fn test_wfst_conv_backward_maps_to_inputs_and_kernel_arcs() {
        let mut kernel_fst = VectorWfst::<u32, LogWeight>::new();
        let s0 = kernel_fst.add_state();
        let s1 = kernel_fst.add_state();
        let s2 = kernel_fst.add_state();
        kernel_fst.set_start(s0);
        kernel_fst.set_final(s2, LogWeight::one());
        kernel_fst.add_arc(s0, Some(0), Some(0), s1, LogWeight::new(0.2));
        kernel_fst.add_arc(s1, Some(1), Some(1), s2, LogWeight::new(0.3));

        let layer = WfstConvLayer::from_kernels(
            vec![WfstKernel::from_wfst(kernel_fst, 2)],
            WfstConvConfig {
                input_channels: 2,
                output_channels: 1,
                kernel_size: 2,
                stride: 1,
                padding: PaddingMode::Valid,
            },
        );
        let input = vec![vec![1.0, 10.0], vec![20.0, 2.0]];

        let (input_grad, kernel_grad) = wfst_conv_backward(&layer, &input, &[vec![2.0]]);

        assert!((input_grad[0][0] - 2.0).abs() < 1e-9);
        assert_eq!(input_grad[0][1], 0.0);
        assert_eq!(input_grad[1][0], 0.0);
        assert!((input_grad[1][1] - 2.0).abs() < 1e-9);
        assert!((kernel_grad[0].get_gradient(ArcIndex::new(s0, 0)) - 2.0).abs() < 1e-9);
        assert!((kernel_grad[0].get_gradient(ArcIndex::new(s1, 0)) - 2.0).abs() < 1e-9);
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

    #[test]
    fn test_zero_stride_has_empty_output() {
        let config = WfstConvConfig {
            input_channels: 2,
            output_channels: 2,
            kernel_size: 3,
            stride: 0,
            padding: PaddingMode::Valid,
        };

        let layer = WfstConvLayer::<u32>::new(config);
        let output = wfst_conv_forward(&layer, &[vec![1.0, 2.0], vec![3.0, 4.0]]);

        assert_eq!(layer.output_length(2), 0);
        assert_eq!(output, vec![Vec::<f64>::new(), Vec::<f64>::new()]);
    }
}
