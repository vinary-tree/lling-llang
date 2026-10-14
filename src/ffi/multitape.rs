//! Immutable runtime-arity multi-tape graphs at the C boundary.

use super::{
    boundary, bounded_usize, decode_unit_domain, decode_weight_domain, graph_budget_from_v2,
    required_mut, set_error, AbiScalarLabel, AbiScalarWeight, ArcticWeight, BoolWeight,
    CountWeight, GraphBudget, LlingBudgetV2, LlingLlangStatus, LogWeight, ProbabilityWeight,
    SignedTropicalWeight, TropicalWeight, LLING_BUDGET_ARCS, LLING_BUDGET_BYTES,
    LLING_BUDGET_STATES, LLING_BUDGET_WORK,
};
use crate::multitape::{RuntimeMultiTapeError, RuntimeMultiTapeWfst};
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

trait MultiTapeOps: Send + Sync {
    fn num_tapes(&self) -> usize;
    fn num_states(&self) -> usize;
    fn num_arcs(&self) -> usize;
    fn start(&self) -> Option<StateId>;
    fn state_info(&self, state: StateId) -> Result<StateInfo, LlingLlangStatus>;
    fn arc_info(&self, state: StateId, index: usize) -> Result<ArcInfo, LlingLlangStatus>;
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
