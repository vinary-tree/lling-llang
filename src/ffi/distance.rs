//! Additive C ABI for bounded exact scalar graph-distance analysis.

use super::{
    boundary, bounded_usize, required_mut, set_error, LlingCancellationV2, LlingGraph,
    LlingLlangStatus,
};
use crate::bindings::{DistancePoll, GraphAnalysisError, GraphDistanceCursor, GraphDistances};
use std::mem::size_of;
use std::ptr;
use std::sync::Arc;

/// Versioned work limits for exact graph-distance analysis.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingGraphDistanceConfig {
    /// Exact C struct size in bytes (24 for version 1).
    pub struct_size: u32,
    /// Configuration version; exactly 1 for this layout.
    pub version: u32,
    /// Maximum graph-edge or graph-vertex transitions over the cursor lifetime.
    pub max_work: u64,
    /// Maximum transitions performed by one `next` call.
    pub work_per_call: u64,
}

/// Mutable, bounded graph-analysis handle retaining an independent graph lease.
pub struct LlingGraphDistanceCursor {
    cursor: Option<GraphDistanceCursor>,
}

/// Exact forward/backward result independent of the graph and cursor handles.
pub struct LlingGraphDistances {
    distances: GraphDistances,
    graph: Arc<crate::bindings::ScalarGraph>,
}

/// More graph-analysis work remains.
pub const LLING_DISTANCE_POLL_PENDING: u32 = 1;
/// Exact forward and backward distances are available for transfer.
pub const LLING_DISTANCE_POLL_COMPLETE: u32 = 2;
/// The caller cancelled before exact analysis completed.
pub const LLING_DISTANCE_POLL_CANCELLED: u32 = 3;

fn map_analysis_error(error: GraphAnalysisError) -> LlingLlangStatus {
    set_error(format!("graph distance analysis failed: {error:?}"));
    match error {
        GraphAnalysisError::NumericFailure | GraphAnalysisError::WorkLimit => {
            LlingLlangStatus::LimitExceeded
        }
        GraphAnalysisError::NonConvergent => LlingLlangStatus::NonConvergent,
        GraphAnalysisError::UnsupportedCycle | GraphAnalysisError::UnsupportedDomain => {
            LlingLlangStatus::Unsupported
        }
        GraphAnalysisError::NoAcceptingPath => LlingLlangStatus::InvalidArgument,
        GraphAnalysisError::InvalidGraph => LlingLlangStatus::ProviderError,
    }
}

/// Begin a resumable native semiring-distance analysis of a complete graph.
/// The graph handle may be freed immediately after this call.
///
/// # Safety
/// Inputs must be live and readable; output must point to a writable null slot.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_distance_open(
    graph: *const LlingGraph,
    config: *const LlingGraphDistanceConfig,
    out_cursor: *mut *mut LlingGraphDistanceCursor,
) -> LlingLlangStatus {
    boundary(|| {
        if graph.is_null() || config.is_null() {
            set_error("graph and config must be non-null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let output = required_mut(out_cursor, "out_cursor")?;
        if !output.is_null() {
            set_error("out_cursor must initially be null");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let config = unsafe { *config };
        if config.struct_size as usize != size_of::<LlingGraphDistanceConfig>()
            || config.version != 1
        {
            set_error("graph distance config has an unsupported size or version");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let max_work = bounded_usize(config.max_work, "max_work")?;
        let work_per_call = bounded_usize(config.work_per_call, "work_per_call")?;
        if max_work == 0 || work_per_call == 0 {
            set_error("graph distance work bounds must be positive");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let source = Arc::clone(&unsafe { &*graph }.graph);
        let cursor = GraphDistanceCursor::new(source, max_work, work_per_call)
            .map_err(map_analysis_error)?;
        *output = Box::into_raw(Box::new(LlingGraphDistanceCursor {
            cursor: Some(cursor),
        }));
        Ok(())
    })
}

/// Advance by at most `work_per_call` graph-edge or graph-vertex transitions.
/// Cancellation is checked between transitions, and terminal outcomes stick.
///
/// # Safety
/// Cursor and output must be live. Callers must synchronize mutable access.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_distance_next(
    cursor: *mut LlingGraphDistanceCursor,
    cancellation: *const LlingCancellationV2,
    out_poll: *mut u32,
) -> LlingLlangStatus {
    boundary(|| {
        let cursor = required_mut(cursor, "cursor")?;
        let output = required_mut(out_poll, "out_poll")?;
        let inner = cursor.cursor.as_mut().ok_or_else(|| {
            set_error("graph distance cursor was consumed by take");
            LlingLlangStatus::Closed
        })?;
        let result = inner
            .poll(|| !cancellation.is_null() && unsafe { (*cancellation).reason() } != 0)
            .map_err(map_analysis_error)?;
        *output = match result {
            DistancePoll::Pending => LLING_DISTANCE_POLL_PENDING,
            DistancePoll::Complete => LLING_DISTANCE_POLL_COMPLETE,
            DistancePoll::Cancelled => LLING_DISTANCE_POLL_CANCELLED,
        };
        Ok(())
    })
}

/// Transfer a result exactly once, and only after complete analysis.
///
/// # Safety
/// Cursor must be live and exclusively held; output a writable null slot.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_distance_take(
    cursor: *mut LlingGraphDistanceCursor,
    out_result: *mut *mut LlingGraphDistances,
) -> LlingLlangStatus {
    boundary(|| {
        let cursor = required_mut(cursor, "cursor")?;
        let output = required_mut(out_result, "out_result")?;
        if !output.is_null() {
            set_error("out_result must initially be null");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if !cursor
            .cursor
            .as_ref()
            .is_some_and(GraphDistanceCursor::is_complete)
        {
            set_error("graph distance analysis is not complete");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let graph = cursor
            .cursor
            .as_ref()
            .expect("checked complete")
            .graph_lease();
        let distances = cursor
            .cursor
            .take()
            .expect("checked complete")
            .into_distances()
            .map_err(map_analysis_error)?;
        *output = Box::into_raw(Box::new(LlingGraphDistances { distances, graph }));
        Ok(())
    })
}

/// Release an analysis cursor and its graph lease. Null is accepted.
///
/// # Safety
/// A non-null pointer must be an unfreed handle from `open`.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_distance_cursor_free(cursor: *mut LlingGraphDistanceCursor) {
    if !cursor.is_null() {
        unsafe { drop(Box::from_raw(cursor)) };
    }
}

/// Return the exact total semiring weight and state count.
///
/// # Safety
/// Result and outputs must be live/writable.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_distance_info(
    result: *const LlingGraphDistances,
    out_total_weight: *mut f64,
    out_state_count: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        if result.is_null() {
            set_error("graph distances result is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let weight = required_mut(out_total_weight, "out_total_weight")?;
        let count = required_mut(out_state_count, "out_state_count")?;
        let distances = &unsafe { &*result }.distances;
        *weight = distances.total;
        *count = distances.forward.len();
        Ok(())
    })
}

/// Copy one bounded page of forward and backward state distances.
///
/// # Safety
/// Result and output slots must be live. Both arrays must be writable for
/// `capacity` f64 values when capacity is nonzero.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_distance_page(
    result: *const LlingGraphDistances,
    offset: usize,
    out_forward: *mut f64,
    out_backward: *mut f64,
    capacity: usize,
    out_written: *mut usize,
    out_total: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        if result.is_null() {
            set_error("graph distances result is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let written = required_mut(out_written, "out_written")?;
        let total = required_mut(out_total, "out_total")?;
        let distances = &unsafe { &*result }.distances;
        if offset > distances.forward.len() || capacity > 256 {
            set_error("distance offset or page capacity is out of range");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if capacity != 0 && (out_forward.is_null() || out_backward.is_null()) {
            set_error("distance output arrays are null with nonzero capacity");
            return Err(LlingLlangStatus::NullPointer);
        }
        let count = (distances.forward.len() - offset).min(capacity);
        if count != 0 {
            unsafe {
                ptr::copy_nonoverlapping(
                    distances.forward.as_ptr().add(offset),
                    out_forward,
                    count,
                );
                ptr::copy_nonoverlapping(
                    distances.backward.as_ptr().add(offset),
                    out_backward,
                    count,
                );
            }
        }
        *written = count;
        *total = distances.forward.len();
        Ok(())
    })
}

/// Copy at most 256 posterior probabilities for one state's outgoing arcs.
/// The result's graph lease supplies the exact arc ordering and domains.
///
/// # Safety
/// The result, counts, and nonempty output page must be live/writable.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_posterior_arcs(
    result: *const LlingGraphDistances,
    local_id: usize,
    offset: usize,
    out_probabilities: *mut f64,
    capacity: usize,
    out_written: *mut usize,
    out_total: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        if result.is_null() {
            set_error("graph distances result is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let written = required_mut(out_written, "out_written")?;
        let total = required_mut(out_total, "out_total")?;
        let result = unsafe { &*result };
        let state = result.graph.states.get(local_id).ok_or_else(|| {
            set_error("local state ID is out of range");
            LlingLlangStatus::InvalidArgument
        })?;
        if offset > state.arcs.len() || capacity > 256 {
            set_error("posterior arc offset or page capacity is out of range");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if capacity != 0 && out_probabilities.is_null() {
            set_error("posterior output is null with nonzero capacity");
            return Err(LlingLlangStatus::NullPointer);
        }
        let count = (state.arcs.len() - offset).min(capacity);
        let mut values = Vec::with_capacity(count);
        for arc_index in offset..offset + count {
            values.push(
                result
                    .distances
                    .arc_posterior(&result.graph, local_id, arc_index)
                    .map_err(map_analysis_error)?,
            );
        }
        if count != 0 {
            unsafe { ptr::copy_nonoverlapping(values.as_ptr(), out_probabilities, count) };
        }
        *written = count;
        *total = state.arcs.len();
        Ok(())
    })
}

/// Return the posterior probability of stopping at one final state.
///
/// # Safety
/// The result and output must be live/writable.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_posterior_final(
    result: *const LlingGraphDistances,
    local_id: usize,
    out_probability: *mut f64,
) -> LlingLlangStatus {
    boundary(|| {
        if result.is_null() {
            set_error("graph distances result is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let output = required_mut(out_probability, "out_probability")?;
        let result = unsafe { &*result };
        let value = result
            .distances
            .final_posterior(&result.graph, local_id)
            .map_err(map_analysis_error)?;
        *output = value;
        Ok(())
    })
}

/// Release a complete distance result. Null is accepted.
///
/// # Safety
/// A non-null pointer must be an unfreed handle from `take`.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_distance_free(result: *mut LlingGraphDistances) {
    if !result.is_null() {
        unsafe { drop(Box::from_raw(result)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{OwnedWfstResource, ScalarWfstGraph};
    use crate::ffi::graph::{
        lling_graph_cursor_free, lling_graph_cursor_next, lling_graph_cursor_open,
        lling_graph_cursor_take, lling_graph_free, LlingGraphConfig, LLING_GRAPH_POLL_COMPLETE,
    };
    use vinary_tree_interop::{VtUnitDomain, VtWeightDomain, VtWfstArc};

    fn complete_graph(domain: VtWeightDomain, cycle: Option<f64>) -> *mut LlingGraph {
        let mut graph = ScalarWfstGraph::new(VtUnitDomain::Byte, domain);
        let start = graph.add_state().expect("start");
        let end = graph.add_state().expect("end");
        assert!(graph.set_start(start));
        assert!(graph.set_final(
            end,
            match domain {
                VtWeightDomain::ProbabilityF64 => 0.5,
                _ => 3.0,
            }
        ));
        assert!(graph.add_arc(
            start,
            VtWfstArc {
                target_state: u64::from(end),
                weight: match domain {
                    VtWeightDomain::ProbabilityF64 => 0.4,
                    _ => 2.0,
                },
                ..VtWfstArc::default()
            }
        ));
        if let Some(weight) = cycle {
            assert!(graph.add_arc(
                end,
                VtWfstArc {
                    target_state: u64::from(start),
                    weight,
                    ..VtWfstArc::default()
                }
            ));
        }
        let owner = OwnedWfstResource::from_scalar_wfst(graph);
        let config = LlingGraphConfig {
            struct_size: size_of::<LlingGraphConfig>() as u32,
            version: 1,
            max_states: 2,
            max_arcs: if cycle.is_some() { 2 } else { 1 },
            max_work: 8,
            work_per_call: 1,
        };
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_cursor_open(&owner.as_raw(), &config, &mut cursor) },
            LlingLlangStatus::Ok
        );
        drop(owner);
        let mut poll = 0;
        while poll != LLING_GRAPH_POLL_COMPLETE {
            assert_eq!(
                unsafe { lling_graph_cursor_next(cursor, ptr::null(), &mut poll) },
                LlingLlangStatus::Ok
            );
        }
        let mut result = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_cursor_take(cursor, &mut result) },
            LlingLlangStatus::Ok
        );
        unsafe { lling_graph_cursor_free(cursor) };
        result
    }

    fn config(max_work: u64) -> LlingGraphDistanceConfig {
        LlingGraphDistanceConfig {
            struct_size: size_of::<LlingGraphDistanceConfig>() as u32,
            version: 1,
            max_work,
            work_per_call: 1,
        }
    }

    #[test]
    fn exact_distance_result_survives_graph_and_cursor_closure() {
        let graph = complete_graph(VtWeightDomain::TropicalF64, None);
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_distance_open(graph, &config(100), &mut cursor) },
            LlingLlangStatus::Ok
        );
        unsafe { lling_graph_free(graph) };
        let mut poll = 0;
        let mut pending = 0;
        while poll != LLING_DISTANCE_POLL_COMPLETE {
            assert_eq!(
                unsafe { lling_graph_distance_next(cursor, ptr::null(), &mut poll) },
                LlingLlangStatus::Ok
            );
            pending += usize::from(poll == LLING_DISTANCE_POLL_PENDING);
        }
        assert!(pending > 8);
        let mut result = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_distance_take(cursor, &mut result) },
            LlingLlangStatus::Ok
        );
        unsafe { lling_graph_distance_cursor_free(cursor) };
        let mut total = 0.0;
        let mut states = 0;
        assert_eq!(
            unsafe { lling_graph_distance_info(result, &mut total, &mut states) },
            LlingLlangStatus::Ok
        );
        assert_eq!(total, 5.0);
        assert_eq!(states, 2);
        let mut forward = [f64::NAN; 2];
        let mut backward = [f64::NAN; 2];
        let mut written = 0;
        let mut reported = 0;
        assert_eq!(
            unsafe {
                lling_graph_distance_page(
                    result,
                    0,
                    forward.as_mut_ptr(),
                    backward.as_mut_ptr(),
                    2,
                    &mut written,
                    &mut reported,
                )
            },
            LlingLlangStatus::Ok
        );
        assert_eq!((written, reported), (2, 2));
        assert_eq!(forward, [0.0, 2.0]);
        assert_eq!(backward, [5.0, 3.0]);
        unsafe { lling_graph_distance_free(result) };
    }

    #[test]
    fn incomplete_divergent_and_unsupported_results_are_never_taken() {
        let graph = complete_graph(VtWeightDomain::TropicalF64, None);
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_distance_open(graph, &config(1), &mut cursor) },
            LlingLlangStatus::Ok
        );
        let mut result = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_distance_take(cursor, &mut result) },
            LlingLlangStatus::InvalidArgument
        );
        let mut poll = 0;
        assert_eq!(
            unsafe { lling_graph_distance_next(cursor, ptr::null(), &mut poll) },
            LlingLlangStatus::Ok
        );
        assert_eq!(
            unsafe { lling_graph_distance_next(cursor, ptr::null(), &mut poll) },
            LlingLlangStatus::LimitExceeded
        );
        assert_eq!(
            unsafe { lling_graph_distance_take(cursor, &mut result) },
            LlingLlangStatus::InvalidArgument
        );
        assert!(result.is_null());
        unsafe {
            lling_graph_distance_cursor_free(cursor);
            lling_graph_free(graph)
        };

        for (domain, cycle, expected) in [
            (
                VtWeightDomain::TropicalF64,
                -4.0,
                LlingLlangStatus::NonConvergent,
            ),
            (
                VtWeightDomain::ProbabilityF64,
                0.5,
                LlingLlangStatus::Unsupported,
            ),
        ] {
            let graph = complete_graph(domain, Some(cycle));
            let mut cursor = ptr::null_mut();
            assert_eq!(
                unsafe { lling_graph_distance_open(graph, &config(100), &mut cursor) },
                LlingLlangStatus::Ok
            );
            let mut poll = 0;
            let status = loop {
                let status = unsafe { lling_graph_distance_next(cursor, ptr::null(), &mut poll) };
                if status != LlingLlangStatus::Ok {
                    break status;
                }
                assert_eq!(poll, LLING_DISTANCE_POLL_PENDING);
            };
            assert_eq!(status, expected);
            assert_eq!(
                unsafe { lling_graph_distance_take(cursor, &mut result) },
                LlingLlangStatus::InvalidArgument
            );
            assert!(result.is_null());
            unsafe {
                lling_graph_distance_cursor_free(cursor);
                lling_graph_free(graph)
            };
        }
    }
}
