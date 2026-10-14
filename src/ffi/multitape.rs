//! Immutable runtime-arity multi-tape graphs at the C boundary.

use super::{
    boundary, bounded_usize, decode_unit_domain, decode_weight_domain, export_native_wfst,
    graph_budget_from_v2, map_error, required_mut, set_error, AbiScalarLabel, AbiScalarWeight,
    ArcticWeight, BoolWeight, CountWeight, GraphBudget, LlingBudgetV2, LlingLlangStatus, LogWeight,
    OwnedWfstResource, ProbabilityWeight, SignedTropicalWeight, TropicalWeight, LLING_BUDGET_ARCS,
    LLING_BUDGET_BYTES, LLING_BUDGET_STATES, LLING_BUDGET_WORK,
};
use crate::multitape::{RuntimeMultiTapeError, RuntimeMultiTapeLimits, RuntimeMultiTapeWfst};
use crate::wfst::{StateId, NO_STATE};
use std::hash::Hash;
use vinary_tree_interop::{VtUnitDomain, VtWeightDomain};

const ALL_BUDGET_AXES: u64 =
    LLING_BUDGET_STATES | LLING_BUDGET_ARCS | LLING_BUDGET_BYTES | LLING_BUDGET_WORK;

fn finite_budget(
    ptr: *const LlingBudgetV2,
) -> Result<(GraphBudget, LlingBudgetV2), LlingLlangStatus> {
    let budget = graph_budget_from_v2(ptr)?;
    let raw = unsafe { *ptr };
    if raw.header.flags & ALL_BUDGET_AXES != ALL_BUDGET_AXES {
        set_error("multi-tape operations require finite state, arc, byte, and work limits");
        return Err(LlingLlangStatus::InvalidArgument);
    }
    Ok((budget, raw))
}

fn map_multitape_error(error: RuntimeMultiTapeError) -> LlingLlangStatus {
    set_error(error.to_string());
    match error {
        RuntimeMultiTapeError::StateIdOverflow
        | RuntimeMultiTapeError::TransitionCountOverflow
        | RuntimeMultiTapeError::WorkLimitExceeded
        | RuntimeMultiTapeError::StateLimitExceeded
        | RuntimeMultiTapeError::ArcLimitExceeded
        | RuntimeMultiTapeError::ByteLimitExceeded => LlingLlangStatus::LimitExceeded,
        _ => LlingLlangStatus::InvalidArgument,
    }
}

fn checked_slice_len<T>(length: usize, name: &'static str) -> Result<(), LlingLlangStatus> {
    if length > isize::MAX as usize / std::mem::size_of::<T>() {
        set_error(format!("{name} exceeds the native slice size limit"));
        return Err(LlingLlangStatus::LimitExceeded);
    }
    Ok(())
}

struct RawGraph<'a> {
    num_tapes: usize,
    num_states: usize,
    start: u64,
    final_flags: &'a [u8],
    final_weights: &'a [f64],
    arc_from: &'a [u64],
    arc_to: &'a [u64],
    arc_weights: &'a [f64],
    labels: &'a [u64],
    present: &'a [u8],
}

struct StateInfo {
    final_flag: u8,
    final_weight: f64,
    arc_count: usize,
}

struct ArcInfo {
    to: u64,
    weight: f64,
    labels: Vec<u64>,
    present: Vec<u8>,
}

fn charge_graph_copy<L, W>(
    mut budget: GraphBudget,
    states: usize,
    arcs: usize,
    tapes: usize,
) -> Result<(), LlingLlangStatus>
where
    W: crate::semiring::Semiring,
{
    let state_bytes = states
        .checked_mul(std::mem::size_of::<
            crate::multitape::RuntimeMultiTapeState<L, W>,
        >())
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    let arc_bytes = tapes
        .checked_mul(std::mem::size_of::<Option<L>>())
        .and_then(|labels| {
            labels.checked_add(std::mem::size_of::<
                crate::multitape::RuntimeMultiTapeTransition<L, W>,
            >())
        })
        .and_then(|arc| arc.checked_mul(arcs))
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    let bytes = state_bytes
        .checked_add(arc_bytes)
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    let work = arcs
        .checked_mul(tapes)
        .and_then(|units| units.checked_add(states))
        .and_then(|units| u64::try_from(units).ok())
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    budget
        .charge(
            u64::try_from(states).map_err(|_| LlingLlangStatus::LimitExceeded)?,
            u64::try_from(arcs).map_err(|_| LlingLlangStatus::LimitExceeded)?,
            bytes,
            work,
        )
        .map_err(map_error)
}

trait MultiTapeOps: Send + Sync {
    fn num_tapes(&self) -> usize;
    fn num_states(&self) -> usize;
    fn num_arcs(&self) -> usize;
    fn start(&self) -> Option<StateId>;
    fn state_info(&self, state: StateId) -> Result<StateInfo, LlingLlangStatus>;
    fn arc_info(&self, state: StateId, index: usize) -> Result<ArcInfo, LlingLlangStatus>;
    fn transduce(
        &self,
        labels: &[u64],
        present: &[u8],
        budget: GraphBudget,
        max_work: u64,
    ) -> Result<Option<f64>, LlingLlangStatus>;
    fn project(
        &self,
        tape: usize,
        budget: GraphBudget,
    ) -> Result<OwnedWfstResource, LlingLlangStatus>;
    fn project_tapes(
        &self,
        tapes: &[usize],
        budget: GraphBudget,
    ) -> Result<Box<dyn MultiTapeOps>, LlingLlangStatus>;
    fn max_delay(&self, max_work: u64) -> Result<Option<usize>, LlingLlangStatus>;
    fn synchronize(
        &self,
        max_delay: usize,
        allow_epsilon: bool,
        limits: RuntimeMultiTapeLimits,
    ) -> Result<Box<dyn MultiTapeOps>, LlingLlangStatus>;
}

struct TypedGraph<L, W>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    graph: RuntimeMultiTapeWfst<L, W>,
}

impl<L, W> MultiTapeOps for TypedGraph<L, W>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    fn num_tapes(&self) -> usize {
        self.graph.num_tapes()
    }

    fn num_states(&self) -> usize {
        self.graph.num_states()
    }

    fn num_arcs(&self) -> usize {
        self.graph.num_transitions()
    }

    fn start(&self) -> Option<StateId> {
        self.graph.start()
    }

    fn state_info(&self, state: StateId) -> Result<StateInfo, LlingLlangStatus> {
        let info = self.graph.state(state).map_err(map_multitape_error)?;
        let final_weight = info
            .final_weight
            .clone()
            .map(|weight| {
                weight.encode().ok_or_else(|| {
                    set_error("final weight is not exactly representable by the scalar ABI");
                    LlingLlangStatus::LimitExceeded
                })
            })
            .transpose()?;
        Ok(StateInfo {
            final_flag: u8::from(final_weight.is_some()),
            final_weight: final_weight.unwrap_or(0.0),
            arc_count: info.transitions.len(),
        })
    }

    fn arc_info(&self, state: StateId, index: usize) -> Result<ArcInfo, LlingLlangStatus> {
        let info = self.graph.state(state).map_err(map_multitape_error)?;
        let arc = info.transitions.get(index).ok_or_else(|| {
            set_error("arc index is outside the state's outgoing arc range");
            LlingLlangStatus::InvalidArgument
        })?;
        let weight = arc.weight.clone().encode().ok_or_else(|| {
            set_error("arc weight is not exactly representable by the scalar ABI");
            LlingLlangStatus::LimitExceeded
        })?;
        Ok(ArcInfo {
            to: u64::from(arc.to),
            weight,
            labels: arc
                .labels
                .iter()
                .map(|label| label.as_ref().map_or(0, AbiScalarLabel::encode))
                .collect(),
            present: arc
                .labels
                .iter()
                .map(|label| u8::from(label.is_some()))
                .collect(),
        })
    }

    fn transduce(
        &self,
        labels: &[u64],
        present: &[u8],
        mut budget: GraphBudget,
        max_work: u64,
    ) -> Result<Option<f64>, LlingLlangStatus> {
        let units = u64::try_from(labels.len()).map_err(|_| LlingLlangStatus::LimitExceeded)?;
        let bytes = labels
            .len()
            .checked_mul(std::mem::size_of::<Option<L>>())
            .and_then(|size| u64::try_from(size).ok())
            .ok_or(LlingLlangStatus::LimitExceeded)?;
        budget.charge(0, 0, bytes, units).map_err(map_error)?;
        let remaining_work = max_work.checked_sub(units).ok_or_else(|| {
            set_error("input decoding exhausts the multi-tape work bound");
            LlingLlangStatus::LimitExceeded
        })?;
        let mut input = Vec::with_capacity(labels.len() / self.graph.num_tapes());
        for (raw_labels, raw_present) in labels
            .chunks_exact(self.graph.num_tapes())
            .zip(present.chunks_exact(self.graph.num_tapes()))
        {
            let tuple = raw_labels
                .iter()
                .zip(raw_present)
                .map(|(&value, &present)| match present {
                    0 => Ok(None),
                    1 => L::decode(value).map(Some).ok_or_else(|| {
                        set_error("input label is outside the selected unit domain");
                        LlingLlangStatus::InvalidArgument
                    }),
                    _ => {
                        set_error("input label presence must be zero or one");
                        Err(LlingLlangStatus::InvalidArgument)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            input.push(tuple);
        }
        self.graph
            .transduce(&input, remaining_work)
            .map_err(map_multitape_error)?
            .map(|weight| {
                weight.encode().ok_or_else(|| {
                    set_error("path weight is not exactly representable by the scalar ABI");
                    LlingLlangStatus::LimitExceeded
                })
            })
            .transpose()
    }

    fn project(
        &self,
        tape: usize,
        budget: GraphBudget,
    ) -> Result<OwnedWfstResource, LlingLlangStatus> {
        charge_graph_copy::<L, W>(
            budget,
            self.graph.num_states(),
            self.graph.num_transitions(),
            1,
        )?;
        let graph = self.graph.project(tape).map_err(map_multitape_error)?;
        export_native_wfst(&graph).map_err(map_error)
    }

    fn project_tapes(
        &self,
        tapes: &[usize],
        budget: GraphBudget,
    ) -> Result<Box<dyn MultiTapeOps>, LlingLlangStatus> {
        charge_graph_copy::<L, W>(
            budget,
            self.graph.num_states(),
            self.graph.num_transitions(),
            tapes.len(),
        )?;
        let graph = self
            .graph
            .project_tapes(tapes)
            .map_err(map_multitape_error)?;
        Ok(Box::new(Self { graph }))
    }

    fn max_delay(&self, max_work: u64) -> Result<Option<usize>, LlingLlangStatus> {
        self.graph
            .compute_max_delay(max_work)
            .map_err(map_multitape_error)
    }

    fn synchronize(
        &self,
        max_delay: usize,
        allow_epsilon: bool,
        limits: RuntimeMultiTapeLimits,
    ) -> Result<Box<dyn MultiTapeOps>, LlingLlangStatus> {
        let graph = self
            .graph
            .synchronize(max_delay, allow_epsilon, limits)
            .map_err(map_multitape_error)?;
        Ok(Box::new(Self { graph }))
    }
}

/// Immutable, owned multi-tape graph with a runtime tape count.
pub struct LlingMultitape {
    inner: Box<dyn MultiTapeOps>,
}

fn make_typed<L, W>(
    raw: &RawGraph<'_>,
    mut budget: GraphBudget,
) -> Result<LlingMultitape, LlingLlangStatus>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    let state_bytes = raw
        .num_states
        .checked_mul(std::mem::size_of::<
            crate::multitape::RuntimeMultiTapeState<L, W>,
        >())
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    let arc_bytes = raw
        .num_tapes
        .checked_mul(std::mem::size_of::<Option<L>>())
        .and_then(|labels| {
            labels.checked_add(std::mem::size_of::<
                crate::multitape::RuntimeMultiTapeTransition<L, W>,
            >())
        })
        .and_then(|arc| arc.checked_mul(raw.arc_from.len()))
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    let bytes = state_bytes
        .checked_add(arc_bytes)
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    let work = raw
        .arc_from
        .len()
        .checked_mul(raw.num_tapes)
        .and_then(|labels| labels.checked_add(raw.num_states))
        .and_then(|units| u64::try_from(units).ok())
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    let states = u64::try_from(raw.num_states).map_err(|_| LlingLlangStatus::LimitExceeded)?;
    let arcs = u64::try_from(raw.arc_from.len()).map_err(|_| LlingLlangStatus::LimitExceeded)?;
    budget
        .charge(states, arcs, bytes, work)
        .map_err(super::map_error)?;
    let mut graph =
        RuntimeMultiTapeWfst::<L, W>::new(raw.num_tapes).map_err(map_multitape_error)?;
    for (index, (&flag, &raw_weight)) in raw.final_flags.iter().zip(raw.final_weights).enumerate() {
        let state = graph.add_state().map_err(map_multitape_error)?;
        if flag > 1 {
            set_error("final-state flag must be zero or one");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if flag == 1 {
            let weight = W::decode(raw_weight).ok_or_else(|| {
                set_error("final weight is outside the selected semiring domain");
                LlingLlangStatus::InvalidArgument
            })?;
            graph
                .set_final(state, weight)
                .map_err(map_multitape_error)?;
        }
        debug_assert_eq!(state as usize, index);
    }
    if raw.num_states == 0 {
        if raw.start != u64::from(NO_STATE) {
            set_error("an empty multi-tape graph has no start state");
            return Err(LlingLlangStatus::InvalidArgument);
        }
    } else {
        let start = StateId::try_from(raw.start).map_err(|_| {
            set_error("start state is outside the native state range");
            LlingLlangStatus::InvalidArgument
        })?;
        graph.set_start(start).map_err(map_multitape_error)?;
    }
    for index in 0..raw.arc_from.len() {
        let from = StateId::try_from(raw.arc_from[index]).map_err(|_| {
            set_error("arc source is outside the native state range");
            LlingLlangStatus::InvalidArgument
        })?;
        let to = StateId::try_from(raw.arc_to[index]).map_err(|_| {
            set_error("arc target is outside the native state range");
            LlingLlangStatus::InvalidArgument
        })?;
        let weight = W::decode(raw.arc_weights[index]).ok_or_else(|| {
            set_error("arc weight is outside the selected semiring domain");
            LlingLlangStatus::InvalidArgument
        })?;
        let begin = index * raw.num_tapes;
        let end = begin + raw.num_tapes;
        let labels = raw.labels[begin..end]
            .iter()
            .zip(&raw.present[begin..end])
            .map(|(&value, &present)| match present {
                0 => Ok(None),
                1 => L::decode(value).map(Some).ok_or_else(|| {
                    set_error("arc label is outside the selected unit domain");
                    LlingLlangStatus::InvalidArgument
                }),
                _ => {
                    set_error("arc label presence must be zero or one");
                    Err(LlingLlangStatus::InvalidArgument)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        graph
            .add_transition(from, to, labels, weight)
            .map_err(map_multitape_error)?;
    }
    Ok(LlingMultitape {
        inner: Box::new(TypedGraph { graph }),
    })
}

/// Capture a checked runtime-arity multi-tape graph from flat caller arrays.
///
/// Each arc has exactly `num_tapes` labels in row-major tape order. A presence
/// byte of zero means epsilon. The source arrays are borrowed only for this
/// call; the returned immutable graph owns the captured data.
///
/// # Safety
///
/// Every nonempty array must point to its declared readable length: final
/// arrays to `num_states`, arc arrays to `num_arcs`, and label/presence arrays
/// to `num_arcs * num_tapes`. `budget` must be readable and `out_graph` must
/// be writable and nonoverlapping with all inputs. Inputs remain live for the
/// duration of this call.
#[no_mangle]
pub unsafe extern "C" fn lling_multitape_new(
    unit_domain: u32,
    weight_domain: u32,
    num_tapes: u64,
    num_states: u64,
    start: u64,
    final_flags: *const u8,
    final_weights: *const f64,
    arc_from: *const u64,
    arc_to: *const u64,
    arc_weights: *const f64,
    labels: *const u64,
    present: *const u8,
    num_arcs: u64,
    budget: *const LlingBudgetV2,
    out_graph: *mut *mut LlingMultitape,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_graph, "out_graph")?;
        let num_tapes = bounded_usize(num_tapes, "num_tapes")?;
        let num_states = bounded_usize(num_states, "num_states")?;
        let num_arcs = bounded_usize(num_arcs, "num_arcs")?;
        if num_tapes == 0 || num_states >= NO_STATE as usize {
            set_error("tape count must be positive and state count must fit native IDs");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let label_count = num_tapes.checked_mul(num_arcs).ok_or_else(|| {
            set_error("multi-tape flat label count overflows native size");
            LlingLlangStatus::LimitExceeded
        })?;
        checked_slice_len::<u8>(num_states, "final_flags")?;
        checked_slice_len::<f64>(num_states, "final_weights")?;
        checked_slice_len::<u64>(num_arcs, "arc state arrays")?;
        checked_slice_len::<f64>(num_arcs, "arc_weights")?;
        checked_slice_len::<u64>(label_count, "labels")?;
        checked_slice_len::<u8>(label_count, "present")?;
        if (num_states != 0 && (final_flags.is_null() || final_weights.is_null()))
            || (num_arcs != 0 && (arc_from.is_null() || arc_to.is_null() || arc_weights.is_null()))
            || (label_count != 0 && (labels.is_null() || present.is_null()))
        {
            set_error("a required nonempty multi-tape input array is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let (budget, _) = finite_budget(budget)?;
        let unit = decode_unit_domain(unit_domain)?;
        let weight = decode_weight_domain(weight_domain)?;
        let raw = RawGraph {
            num_tapes,
            num_states,
            start,
            final_flags: if num_states == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(final_flags, num_states) }
            },
            final_weights: if num_states == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(final_weights, num_states) }
            },
            arc_from: if num_arcs == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(arc_from, num_arcs) }
            },
            arc_to: if num_arcs == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(arc_to, num_arcs) }
            },
            arc_weights: if num_arcs == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(arc_weights, num_arcs) }
            },
            labels: if label_count == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(labels, label_count) }
            },
            present: if label_count == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(present, label_count) }
            },
        };
        let graph = dispatch_scalar_domains!(unit, weight, make_typed, &raw, budget)?;
        *output = Box::into_raw(Box::new(graph));
        Ok(())
    })
}

/// Return the graph's tape and state counts, arc count, and optional start.
///
/// A missing start is reported as the native `NO_STATE` sentinel.
///
/// # Safety
///
/// `graph` must remain open for the call. Output pointers must be writable,
/// mutually nonoverlapping, and separate from the graph.
#[no_mangle]
pub unsafe extern "C" fn lling_multitape_info(
    graph: *const LlingMultitape,
    out_tapes: *mut usize,
    out_states: *mut usize,
    out_arcs: *mut usize,
    out_start: *mut u64,
) -> LlingLlangStatus {
    boundary(|| {
        let tapes = required_mut(out_tapes, "out_tapes")?;
        let states = required_mut(out_states, "out_states")?;
        let arcs = required_mut(out_arcs, "out_arcs")?;
        let start = required_mut(out_start, "out_start")?;
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let graph = unsafe { &*graph };
        *tapes = graph.inner.num_tapes();
        *states = graph.inner.num_states();
        *arcs = graph.inner.num_arcs();
        *start = graph.inner.start().map_or(u64::from(NO_STATE), u64::from);
        Ok(())
    })
}

/// Read one state's final status, weight, and outgoing arc count.
///
/// # Safety
///
/// `graph` must remain open; output pointers must be writable, mutually
/// nonoverlapping, and separate from the graph.
#[no_mangle]
pub unsafe extern "C" fn lling_multitape_state_info(
    graph: *const LlingMultitape,
    state: u64,
    out_final: *mut u8,
    out_weight: *mut f64,
    out_arc_count: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let final_flag = required_mut(out_final, "out_final")?;
        let weight = required_mut(out_weight, "out_weight")?;
        let arc_count = required_mut(out_arc_count, "out_arc_count")?;
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let state = StateId::try_from(state).map_err(|_| {
            set_error("state is outside the native state ID range");
            LlingLlangStatus::InvalidArgument
        })?;
        let info = unsafe { &*graph }.inner.state_info(state)?;
        *final_flag = info.final_flag;
        *weight = info.final_weight;
        *arc_count = info.arc_count;
        Ok(())
    })
}

/// Read one arc into caller-provided tape-sized output buffers.
///
/// # Safety
///
/// `graph` must remain open; `out_labels` and `out_present` must be writable
/// for `capacity` elements. All output slots and buffers must be disjoint.
#[no_mangle]
pub unsafe extern "C" fn lling_multitape_arc(
    graph: *const LlingMultitape,
    state: u64,
    arc_index: usize,
    out_to: *mut u64,
    out_weight: *mut f64,
    out_labels: *mut u64,
    out_present: *mut u8,
    capacity: usize,
) -> LlingLlangStatus {
    boundary(|| {
        let to = required_mut(out_to, "out_to")?;
        let weight = required_mut(out_weight, "out_weight")?;
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let graph = unsafe { &*graph };
        if capacity < graph.inner.num_tapes() {
            set_error("arc buffers are smaller than the graph's tape count");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        if out_labels.is_null() || out_present.is_null() {
            set_error("arc output buffer is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let state = StateId::try_from(state).map_err(|_| {
            set_error("state is outside the native state ID range");
            LlingLlangStatus::InvalidArgument
        })?;
        let arc = graph.inner.arc_info(state, arc_index)?;
        unsafe {
            std::ptr::copy_nonoverlapping(arc.labels.as_ptr(), out_labels, arc.labels.len());
            std::ptr::copy_nonoverlapping(arc.present.as_ptr(), out_present, arc.present.len());
        }
        *to = arc.to;
        *weight = arc.weight;
        Ok(())
    })
}

/// Return the first accepting path weight for a sequence of tape tuples.
///
/// Pure epsilon arcs may be traversed without consuming an input tuple. All
/// four budget axes must be finite; work includes input decoding and native
/// search. Outputs remain unchanged on error.
///
/// # Safety
///
/// `graph` must remain open. For nonzero `num_steps`, `labels` and `present`
/// must each contain `num_steps * graph.num_tapes` readable elements.
/// `budget` must be readable and result pointers must be writable, mutually
/// nonoverlapping, and separate from all inputs.
#[no_mangle]
pub unsafe extern "C" fn lling_multitape_transduce(
    graph: *const LlingMultitape,
    labels: *const u64,
    present: *const u8,
    num_steps: u64,
    budget: *const LlingBudgetV2,
    out_accepted: *mut u8,
    out_weight: *mut f64,
) -> LlingLlangStatus {
    boundary(|| {
        let accepted = required_mut(out_accepted, "out_accepted")?;
        let weight = required_mut(out_weight, "out_weight")?;
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let graph = unsafe { &*graph };
        let steps = bounded_usize(num_steps, "num_steps")?;
        let count = steps.checked_mul(graph.inner.num_tapes()).ok_or_else(|| {
            set_error("flat input label count overflows native size");
            LlingLlangStatus::LimitExceeded
        })?;
        checked_slice_len::<u64>(count, "labels")?;
        checked_slice_len::<u8>(count, "present")?;
        if count != 0 && (labels.is_null() || present.is_null()) {
            set_error("nonempty input label or presence array is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let (budget, raw_budget) = finite_budget(budget)?;
        let labels = if count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(labels, count) }
        };
        let present = if count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(present, count) }
        };
        let result = graph
            .inner
            .transduce(labels, present, budget, raw_budget.max_work)?;
        *accepted = u8::from(result.is_some());
        *weight = result.unwrap_or(0.0);
        Ok(())
    })
}

/// Project one zero-based tape to a scalar WFST under a finite budget.
///
/// # Safety
///
/// `graph` must remain open, `budget` readable, and `out_wfst` writable and
/// separate from both inputs.
#[no_mangle]
pub unsafe extern "C" fn lling_multitape_project(
    graph: *const LlingMultitape,
    tape: u64,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut super::LlingWfst,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_wfst, "out_wfst")?;
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let tape = bounded_usize(tape, "tape")?;
        let (budget, _) = finite_budget(budget)?;
        let resource = unsafe { &*graph }.inner.project(tape, budget)?;
        *output = Box::into_raw(Box::new(super::LlingWfst { resource }));
        Ok(())
    })
}

/// Project an ordered, nonempty selection of zero-based tapes.
///
/// Repeated tape indices are allowed. The result is a new immutable graph.
///
/// # Safety
///
/// `graph` must remain open. `tapes` must contain `num_tapes` readable values
/// when nonempty; `budget` must be readable and `out_graph` writable and
/// separate from all inputs.
#[no_mangle]
pub unsafe extern "C" fn lling_multitape_project_tapes(
    graph: *const LlingMultitape,
    tapes: *const u64,
    num_tapes: u64,
    budget: *const LlingBudgetV2,
    out_graph: *mut *mut LlingMultitape,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_graph, "out_graph")?;
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let count = bounded_usize(num_tapes, "num_tapes")?;
        checked_slice_len::<u64>(count, "tapes")?;
        if count != 0 && tapes.is_null() {
            set_error("nonempty tape index array is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let (budget, _) = finite_budget(budget)?;
        let tapes = if count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(tapes, count) }
        };
        let indices = tapes
            .iter()
            .map(|&tape| bounded_usize(tape, "tape"))
            .collect::<Result<Vec<_>, _>>()?;
        let inner = unsafe { &*graph }.inner.project_tapes(&indices, budget)?;
        *output = Box::into_raw(Box::new(LlingMultitape { inner }));
        Ok(())
    })
}

/// Analyze the exact maximum reachable pairwise tape delay.
///
/// `out_bounded` is zero for unbounded delay or an unrepresentable finite
/// result. `max_work` bounds pair-projection work.
///
/// # Safety
///
/// `graph` must remain open. Outputs must be writable, nonoverlapping, and
/// separate from the graph.
#[no_mangle]
pub unsafe extern "C" fn lling_multitape_max_delay(
    graph: *const LlingMultitape,
    max_work: u64,
    out_bounded: *mut u8,
    out_delay: *mut u64,
) -> LlingLlangStatus {
    boundary(|| {
        let bounded = required_mut(out_bounded, "out_bounded")?;
        let delay = required_mut(out_delay, "out_delay")?;
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let result = unsafe { &*graph }.inner.max_delay(max_work)?;
        let encoded = result.map(u64::try_from).transpose().map_err(|_| {
            set_error("delay exceeds the ABI u64 representation");
            LlingLlangStatus::LimitExceeded
        })?;
        *bounded = u8::from(encoded.is_some());
        *delay = encoded.unwrap_or(0);
        Ok(())
    })
}

/// Materialize the bounded-delay subgraph under four finite output limits.
///
/// # Safety
///
/// `graph` must remain open, `budget` readable, and `out_graph` writable and
/// separate from both inputs.
#[no_mangle]
pub unsafe extern "C" fn lling_multitape_synchronize(
    graph: *const LlingMultitape,
    max_delay: u64,
    allow_epsilon: u8,
    budget: *const LlingBudgetV2,
    out_graph: *mut *mut LlingMultitape,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_graph, "out_graph")?;
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        if allow_epsilon > 1 {
            set_error("allow_epsilon must be zero or one");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let max_delay = bounded_usize(max_delay, "max_delay")?;
        let (_, raw) = finite_budget(budget)?;
        let limits = RuntimeMultiTapeLimits {
            max_states: bounded_usize(raw.max_states, "max_states")?,
            max_arcs: bounded_usize(raw.max_arcs, "max_arcs")?,
            max_bytes: raw.max_bytes,
            max_work: raw.max_work,
        };
        let inner = unsafe { &*graph }
            .inner
            .synchronize(max_delay, allow_epsilon == 1, limits)?;
        *output = Box::into_raw(Box::new(LlingMultitape { inner }));
        Ok(())
    })
}

/// Release an immutable runtime-arity multi-tape graph.
///
/// # Safety
///
/// A non-null pointer must be an open graph returned by a multi-tape ABI
/// constructor, released exactly once without concurrent access.
#[no_mangle]
pub unsafe extern "C" fn lling_multitape_free(graph: *mut LlingMultitape) {
    if !graph.is_null() {
        drop(unsafe { Box::from_raw(graph) });
    }
}
