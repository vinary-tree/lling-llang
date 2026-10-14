//! Bounded native training entry points for contiguous host matrices.

use super::{
    boundary, graph_budget_from_v2, map_error, required_mut, set_error, LlingBudgetV2,
    LlingCancellationV2, LlingLlangStatus, LlingWfst,
};
use crate::algorithms::{remove_epsilon, EpsilonRemovalConfig};
use crate::bindings::{export_native_wfst, import_native_wfst_with_budget_and_stats, GraphBudget};
use crate::ctc::{
    try_compact_ctc, try_correct_ctc, try_minimal_ctc, try_selfless_compact_ctc,
    try_selfless_correct_ctc,
};
use crate::differentiable::{backward, forward_score, GradientWfst};
use crate::semiring::LogWeight;
use crate::training::{lfmmi_loss, LfMmiConfig};
use crate::wfst::{MutableWfst, VectorWfst, Wfst};
use std::collections::VecDeque;
use std::mem::size_of;
use vinary_tree_interop::VtResource;

const MAX_MATRIX_CELLS: usize = 1_048_576;
const MAX_STATE_FRAME_CELLS: usize = 2_097_152;

/// Construct one of the Rust CTC topology families over `UInt64` labels and
/// log-semiring costs. Variants: 0 correct, 1 compact, 2 minimal, 3 selfless
/// correct, 4 selfless compact. Label zero is blank.
///
/// # Safety
/// All pointers must be valid, and the returned WFST must be freed or adopted
/// through `lling_wfst_resource` and `lling_wfst_free`.
#[no_mangle]
pub unsafe extern "C" fn lling_ctc_topology(
    variant: u32,
    vocab_size: usize,
    budget: *const LlingBudgetV2,
    cancellation: *const LlingCancellationV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_wfst, "out_wfst")?;
        if vocab_size == 0 || vocab_size > 4_096 {
            return Err(invalid("CTC vocabulary must be in 1:4096"));
        }
        let (states, arcs) = match variant {
            0 => (vocab_size, vocab_size.checked_mul(vocab_size)),
            1 => (
                vocab_size,
                vocab_size.checked_mul(3).and_then(|n| n.checked_sub(2)),
            ),
            2 => (1, Some(vocab_size)),
            3 => (
                vocab_size,
                vocab_size
                    .checked_mul(vocab_size)
                    .and_then(|n| n.checked_sub(vocab_size - 1)),
            ),
            4 => (
                vocab_size,
                vocab_size.checked_mul(2).and_then(|n| n.checked_sub(1)),
            ),
            _ => return Err(invalid("unknown CTC topology variant")),
        };
        let arcs = arcs.ok_or_else(|| invalid("CTC arc count overflows"))?;
        if arcs > 131_072 {
            set_error("CTC topology exceeds its arc limit");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        let mut graph_budget = graph_budget_from_v2(budget)?;
        graph_budget.cap(4_096, 131_072, 64 * 1024 * 1024, 2_097_152);
        graph_budget
            .charge_output::<u64, LogWeight>(states as u64, arcs as u64)
            .map_err(map_error)?;
        cancelled(cancellation)?;
        let topology = match variant {
            0 => try_correct_ctc::<LogWeight>(vocab_size),
            1 => try_compact_ctc::<LogWeight>(vocab_size),
            2 => try_minimal_ctc::<LogWeight>(vocab_size),
            3 => try_selfless_correct_ctc::<LogWeight>(vocab_size),
            _ => try_selfless_compact_ctc::<LogWeight>(vocab_size),
        }
        .map_err(|error| {
            set_error(error.to_string());
            LlingLlangStatus::InvalidArgument
        })?;
        let source = topology.fst();
        let mut graph = VectorWfst::<u64, LogWeight>::new();
        for _ in 0..source.num_states() {
            graph.add_state();
        }
        graph.set_start(source.start());
        for state in 0..source.num_states() {
            let state = state as u32;
            if source.is_final(state) {
                graph.set_final(state, source.final_weight(state));
            }
            for arc in source.transitions(state) {
                graph.add_arc(
                    state,
                    arc.input.map(u64::from),
                    arc.output.map(u64::from),
                    arc.to,
                    arc.weight,
                );
            }
        }
        cancelled(cancellation)?;
        let resource = export_native_wfst(&graph).map_err(map_error)?;
        *output = Box::into_raw(Box::new(LlingWfst { resource }));
        Ok(())
    })
}

/// Posterior derivative of the graph's negative-log path sum with respect to
/// one arc cost, identified by its state and zero-based outgoing arc index.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LlingArcGradient {
    /// Source state identifier.
    pub from: u32,
    /// Zero-based index in the source state's outgoing arcs.
    pub arc_index: u32,
    /// Posterior derivative with respect to the arc's negative-log cost.
    pub gradient: f64,
}

/// Immutable paged result of an exact acyclic log-semiring forward/backward.
pub struct LlingGradientCursor {
    arcs: Vec<LlingArcGradient>,
}

fn require_acyclic(graph: &VectorWfst<u64, LogWeight>) -> Result<(), LlingLlangStatus> {
    let states = graph.num_states();
    let mut indegree = vec![0usize; states];
    for from in 0..states {
        for arc in graph.transitions(from as u32) {
            indegree[arc.to as usize] += 1;
        }
    }
    let mut ready: VecDeque<usize> = indegree
        .iter()
        .enumerate()
        .filter_map(|(state, count)| (*count == 0).then_some(state))
        .collect();
    let mut visited = 0;
    while let Some(from) = ready.pop_front() {
        visited += 1;
        for arc in graph.transitions(from as u32) {
            let next = &mut indegree[arc.to as usize];
            *next -= 1;
            if *next == 0 {
                ready.push_back(arc.to as usize);
            }
        }
    }
    if visited != states {
        set_error("exact log gradients require an acyclic graph");
        return Err(LlingLlangStatus::Unsupported);
    }
    Ok(())
}

/// Capture a bounded log-semiring u64-label graph and compute the negative-log
/// path sum and arc posteriors. The graph must be acyclic. Results are immutable
/// and can be fetched in caller-sized pages; close with `lling_log_gradients_free`.
///
/// # Safety
/// All pointers must be valid for this call. The resource must remain live
/// throughout it, and the resulting cursor must be freed exactly once.
#[no_mangle]
pub unsafe extern "C" fn lling_log_gradients_open(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    cancellation: *const LlingCancellationV2,
    out_cursor: *mut *mut LlingGradientCursor,
    out_score: *mut f64,
    out_arc_count: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        if resource.is_null() {
            set_error("log-gradient resource is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let cursor = required_mut(out_cursor, "out_cursor")?;
        let score = required_mut(out_score, "out_score")?;
        let arc_count = required_mut(out_arc_count, "out_arc_count")?;
        cancelled(cancellation)?;
        let mut graph_budget = graph_budget_from_v2(budget)?;
        graph_budget.cap(4_096, 131_072, 64 * 1024 * 1024, 2_097_152);
        let (graph, stats) = import_native_wfst_with_budget_and_stats::<u64, LogWeight>(
            unsafe { *resource },
            &mut graph_budget,
        )
        .map_err(map_error)?;
        cancelled(cancellation)?;
        require_acyclic(&graph)?;
        let differentiable = GradientWfst::from_wfst(&graph);
        let result = forward_score(&differentiable);
        if !result.value().is_finite() {
            set_error("log-gradient graph has no finite accepting path");
            return Err(LlingLlangStatus::NonConvergent);
        }
        cancelled(cancellation)?;
        let gradients = backward(&differentiable);
        let mut arcs = Vec::with_capacity(gradients.arc_gradients.len());
        for item in gradients.arc_gradients {
            if !item.gradient.is_finite() || item.arc.arc_idx > u32::MAX as usize {
                set_error("log-gradient result is non-finite or exceeds the arc index range");
                return Err(LlingLlangStatus::NonConvergent);
            }
            arcs.push(LlingArcGradient {
                from: item.arc.from,
                arc_index: item.arc.arc_idx as u32,
                gradient: item.gradient,
            });
        }
        if arcs.len() != stats.arcs as usize {
            set_error("log-gradient arc count differs from imported graph");
            return Err(LlingLlangStatus::ProviderError);
        }
        cancelled(cancellation)?;
        *score = result.value();
        *arc_count = arcs.len();
        *cursor = Box::into_raw(Box::new(LlingGradientCursor { arcs }));
        Ok(())
    })
}

/// Copy a page of arc gradients without retaining or mutating caller storage.
///
/// # Safety
/// `cursor` must be live and `out_arcs` must hold `capacity` elements.
#[no_mangle]
pub unsafe extern "C" fn lling_log_gradients_page(
    cursor: *const LlingGradientCursor,
    start: usize,
    out_arcs: *mut LlingArcGradient,
    capacity: usize,
    out_written: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        if cursor.is_null() || (capacity != 0 && out_arcs.is_null()) {
            set_error("log-gradient cursor or nonempty output page is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let written = required_mut(out_written, "out_written")?;
        let arcs = &unsafe { &*cursor }.arcs;
        if start > arcs.len() {
            return Err(invalid("log-gradient page start exceeds arc count"));
        }
        let count = capacity.min(arcs.len() - start);
        if count != 0 {
            unsafe { std::ptr::copy_nonoverlapping(arcs.as_ptr().add(start), out_arcs, count) };
        }
        *written = count;
        Ok(())
    })
}

/// Release an owned gradient result; null is harmless.
///
/// # Safety
/// A non-null cursor must be an owned live pointer returned by open.
#[no_mangle]
pub unsafe extern "C" fn lling_log_gradients_free(cursor: *mut LlingGradientCursor) {
    if !cursor.is_null() {
        drop(unsafe { Box::from_raw(cursor) });
    }
}

/// Versioned regularization and topology controls for LF-MMI training.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LlingLfMmiConfig {
    /// Exact byte size of this revision's structure.
    pub struct_size: u32,
    /// Must be one.
    pub version: u32,
    /// Additive leaky HMM coefficient.
    pub leaky_hmm_coefficient: f64,
    /// Nonnegative L2 coefficient.
    pub l2_regularize: f64,
    /// Nonnegative cross-entropy coefficient.
    pub xent_regularize: f64,
    /// Whether to request chain topology.
    pub use_chain_topology: u8,
    /// Must be zero.
    pub reserved: [u8; 7],
    /// Positive frame subsampling factor.
    pub subsampling_factor: u64,
}

/// Owned scalar result; gradients are written to the caller's matrix.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LlingLfMmiSummary {
    /// Complete regularized loss.
    pub loss: f64,
    /// Numerator graph log probability.
    pub numerator_log_prob: f64,
    /// Denominator graph log probability.
    pub denominator_log_prob: f64,
    /// Cross-entropy component before its configured scale.
    pub xent_loss: f64,
}

fn invalid(message: &'static str) -> LlingLlangStatus {
    set_error(message);
    LlingLlangStatus::InvalidArgument
}

fn cancelled(token: *const LlingCancellationV2) -> Result<(), LlingLlangStatus> {
    if !token.is_null() && unsafe { (*token).reason() } != 0 {
        set_error("LF-MMI training was cancelled");
        Err(LlingLlangStatus::Closed)
    } else {
        Ok(())
    }
}

fn checked_config(raw: LlingLfMmiConfig) -> Result<LfMmiConfig, LlingLlangStatus> {
    if raw.struct_size as usize != size_of::<LlingLfMmiConfig>()
        || raw.version != 1
        || raw.use_chain_topology > 1
        || raw.reserved != [0; 7]
        || raw.subsampling_factor == 0
    {
        return Err(invalid("LF-MMI config has invalid size, version, or flags"));
    }
    if raw.use_chain_topology != 1 || raw.subsampling_factor != 3 {
        set_error("LF-MMI topology and frame subsampling must be applied to the graph and matrix before this call");
        return Err(LlingLlangStatus::Unsupported);
    }
    if [
        raw.leaky_hmm_coefficient,
        raw.l2_regularize,
        raw.xent_regularize,
    ]
    .iter()
    .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(invalid(
            "LF-MMI coefficients must be finite and nonnegative",
        ));
    }
    Ok(LfMmiConfig {
        leaky_hmm_coefficient: raw.leaky_hmm_coefficient,
        l2_regularize: raw.l2_regularize,
        xent_regularize: raw.xent_regularize,
        use_chain_topology: raw.use_chain_topology != 0,
        subsampling_factor: usize::try_from(raw.subsampling_factor)
            .map_err(|_| invalid("LF-MMI subsampling factor exceeds usize"))?,
    })
}

fn checked_layout(
    frames: usize,
    pdfs: usize,
    frame_stride: usize,
    pdf_stride: usize,
) -> Result<usize, LlingLlangStatus> {
    let cells = frames
        .checked_mul(pdfs)
        .ok_or_else(|| invalid("LF-MMI matrix dimensions overflow"))?;
    if frames == 0 || pdfs == 0 || cells > MAX_MATRIX_CELLS {
        set_error("LF-MMI matrix is empty or exceeds its cell limit");
        return Err(LlingLlangStatus::LimitExceeded);
    }
    if !((frame_stride == pdfs && pdf_stride == 1) || (frame_stride == 1 && pdf_stride == frames)) {
        return Err(invalid(
            "LF-MMI matrix must be contiguous row or column major",
        ));
    }
    if cells > isize::MAX as usize / size_of::<f64>() {
        set_error("LF-MMI matrix exceeds pointer offset limit");
        return Err(LlingLlangStatus::LimitExceeded);
    }
    Ok(cells)
}

fn checked_graph(
    source: &VectorWfst<u64, LogWeight>,
    pdfs: usize,
    budget: &mut GraphBudget,
) -> Result<VectorWfst<u32, LogWeight>, LlingLlangStatus> {
    let mut graph = VectorWfst::new();
    let mut has_epsilon = false;
    for _ in 0..source.num_states() {
        graph.add_state();
    }
    graph.set_start(source.start());
    for state in 0..source.num_states() {
        let state = state as u32;
        if source.is_final(state) {
            graph.set_final(state, source.final_weight(state));
        }
        for arc in source.transitions(state) {
            if arc.input.is_none() {
                if arc.output.is_some() {
                    return Err(invalid(
                        "LF-MMI cannot normalize an input-epsilon arc with output",
                    ));
                }
                has_epsilon = true;
            }
            if arc
                .input
                .is_some_and(|input| input >= pdfs as u64 || input > u32::MAX as u64)
            {
                return Err(invalid(
                    "LF-MMI input label exceeds the acoustic PDF domain",
                ));
            }
            let output = arc
                .output
                .map(u32::try_from)
                .transpose()
                .map_err(|_| invalid("LF-MMI output label exceeds u32"))?;
            graph.add_arc(
                state,
                arc.input.map(|input| input as u32),
                output,
                arc.to,
                arc.weight,
            );
        }
    }
    if has_epsilon {
        let expanded_arcs = source
            .num_states()
            .checked_mul(source.total_transitions())
            .ok_or_else(|| invalid("LF-MMI epsilon expansion overflows"))?;
        if expanded_arcs > 131_072 {
            set_error("LF-MMI epsilon expansion exceeds its arc ceiling");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        budget
            .charge_output::<u64, LogWeight>(source.num_states() as u64, expanded_arcs as u64)
            .map_err(map_error)?;
        remove_epsilon(&mut graph, EpsilonRemovalConfig::default()).map_err(|error| {
            set_error(format!("LF-MMI epsilon normalization failed: {error}"));
            LlingLlangStatus::Unsupported
        })?;
        if graph.total_transitions() > 131_072
            || (0..graph.num_states()).any(|state| {
                graph
                    .transitions(state as u32)
                    .iter()
                    .any(|arc| arc.input.is_none())
            })
        {
            set_error("LF-MMI epsilon normalization did not yield a bounded frame graph");
            return Err(LlingLlangStatus::LimitExceeded);
        }
    }
    Ok(graph)
}

/// Compute native LF-MMI loss and gradient from two snapshot-consistent log
/// semiring u64-label WFST resources and a contiguous f64 acoustic matrix.
///
/// Both input and output accept C row-major `(pdfs, 1)` or Julia column-major
/// `(1, frames)` strides. The caller owns both buffers. Graph import is bounded
/// cumulatively by `budget` and hard training ceilings. Cancellation is checked
/// before and after imports and computation; the native objective itself is
/// finite and bounded but cannot be interrupted mid-call. Outputs are written
/// only after a complete finite result.
///
/// # Safety
/// Resources must be live for this call; all pointers must address readable or
/// writable storage of the specified size and must not alias mutable outputs.
#[no_mangle]
pub unsafe extern "C" fn lling_lfmmi_loss_ref(
    numerator: *const VtResource,
    denominator: *const VtResource,
    budget: *const LlingBudgetV2,
    cancellation: *const LlingCancellationV2,
    scores: *const f64,
    frames: usize,
    pdfs: usize,
    score_frame_stride: usize,
    score_pdf_stride: usize,
    config: *const LlingLfMmiConfig,
    out_summary: *mut LlingLfMmiSummary,
    out_gradients: *mut f64,
    gradient_capacity: usize,
    gradient_frame_stride: usize,
    gradient_pdf_stride: usize,
) -> LlingLlangStatus {
    boundary(|| {
        if numerator.is_null()
            || denominator.is_null()
            || scores.is_null()
            || config.is_null()
            || out_gradients.is_null()
        {
            set_error("LF-MMI requires non-null resources, config, and matrix buffers");
            return Err(LlingLlangStatus::NullPointer);
        }
        let summary = required_mut(out_summary, "out_summary")?;
        let cells = checked_layout(frames, pdfs, score_frame_stride, score_pdf_stride)?;
        checked_layout(frames, pdfs, gradient_frame_stride, gradient_pdf_stride)?;
        if gradient_capacity < cells {
            return Err(invalid("LF-MMI gradient buffer is too small"));
        }
        let config = checked_config(unsafe { *config })?;
        cancelled(cancellation)?;
        let mut graph_budget = graph_budget_from_v2(budget)?;
        graph_budget.cap(4_096, 131_072, 64 * 1024 * 1024, 2_097_152);
        let (numerator, _) = import_native_wfst_with_budget_and_stats::<u64, LogWeight>(
            unsafe { *numerator },
            &mut graph_budget,
        )
        .map_err(map_error)?;
        cancelled(cancellation)?;
        let (denominator, _) = import_native_wfst_with_budget_and_stats::<u64, LogWeight>(
            unsafe { *denominator },
            &mut graph_budget,
        )
        .map_err(map_error)?;
        let state_cells = frames
            .checked_mul(numerator.num_states().max(denominator.num_states()))
            .ok_or_else(|| invalid("LF-MMI state/frame product overflows"))?;
        if state_cells > MAX_STATE_FRAME_CELLS {
            set_error("LF-MMI state/frame product exceeds its cell limit");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        let numerator = checked_graph(&numerator, pdfs, &mut graph_budget)?;
        cancelled(cancellation)?;
        let denominator = checked_graph(&denominator, pdfs, &mut graph_budget)?;
        cancelled(cancellation)?;
        let mut acoustic = Vec::with_capacity(frames);
        for frame in 0..frames {
            cancelled(cancellation)?;
            let mut row = Vec::with_capacity(pdfs);
            for pdf in 0..pdfs {
                let value =
                    unsafe { *scores.add(frame * score_frame_stride + pdf * score_pdf_stride) };
                if value.is_nan() || value == f64::INFINITY {
                    return Err(invalid(
                        "LF-MMI acoustic scores must be finite or -infinity",
                    ));
                }
                if value == f64::NEG_INFINITY
                    && (config.xent_regularize > 0.0 || config.l2_regularize > 0.0)
                {
                    return Err(invalid("LF-MMI regularized acoustic scores must be finite"));
                }
                row.push(value);
            }
            acoustic.push(row);
        }
        let result = lfmmi_loss(&acoustic, &numerator, &denominator, &config);
        cancelled(cancellation)?;
        if [
            result.loss,
            result.numerator_log_prob,
            result.denominator_log_prob,
            result.xent_loss,
        ]
        .iter()
        .chain(result.gradients.data.iter())
        .any(|value| !value.is_finite())
        {
            set_error("LF-MMI produced a non-finite loss or gradient");
            return Err(LlingLlangStatus::NonConvergent);
        }
        for frame in 0..frames {
            for pdf in 0..pdfs {
                unsafe {
                    *out_gradients.add(frame * gradient_frame_stride + pdf * gradient_pdf_stride) =
                        result.gradients.get(frame, pdf);
                }
            }
        }
        *summary = LlingLfMmiSummary {
            loss: result.loss,
            numerator_log_prob: result.numerator_log_prob,
            denominator_log_prob: result.denominator_log_prob,
            xent_loss: result.xent_loss,
        };
        Ok(())
    })
}
