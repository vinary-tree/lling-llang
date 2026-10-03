//! Additive C ABI for bounded, resumable scalar-WFST graph capture.

use super::{
    boundary, bounded_usize, map_error, required_mut, set_error, LlingCancellationV2,
    LlingLlangStatus,
};
use crate::bindings::{CapturedGraphCursor, GraphPoll, ScalarGraph, ScalarGraphConfig};
use std::mem::size_of;
use std::ptr;
use vinary_tree_interop::{VtResource, VtWfstArc};

/// Versioned state, arc, and provider-work bounds for one graph capture.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingGraphConfig {
    /// Exact C struct size in bytes (40 for version 1).
    pub struct_size: u32,
    /// Configuration version; exactly 1 for this layout.
    pub version: u32,
    /// Maximum number of distinct reachable states, including the start.
    pub max_states: u64,
    /// Maximum number of reachable arcs across all captured states.
    pub max_arcs: u64,
    /// Maximum number of provider callbacks across the cursor lifetime.
    pub max_work: u64,
    /// Maximum number of provider callbacks in one `next` call.
    pub work_per_call: u64,
}

/// One arc whose target has been assigned a deterministic local state ID.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingGraphArc {
    /// Breadth-first local ID of the arc's target state.
    pub target_local: u64,
    /// Original domain-preserving arc, including its provider target ID.
    pub arc: VtWfstArc,
}

/// Opaque, snapshot-owning resumable capture cursor.
pub struct LlingGraphCursor {
    cursor: Option<CapturedGraphCursor>,
}

/// Opaque complete reachable graph, independent of its provider snapshot.
pub struct LlingGraph {
    graph: ScalarGraph,
}

/// More provider work remains; poll again.
pub const LLING_GRAPH_POLL_PENDING: u32 = 1;
/// All reachable states and arcs were captured exactly.
pub const LLING_GRAPH_POLL_COMPLETE: u32 = 2;
/// The caller cancelled before exact capture completed.
pub const LLING_GRAPH_POLL_CANCELLED: u32 = 3;

fn decode_config(config: LlingGraphConfig) -> Result<ScalarGraphConfig, LlingLlangStatus> {
    if config.struct_size as usize != size_of::<LlingGraphConfig>() || config.version != 1 {
        set_error("graph config has an unsupported size or version");
        return Err(LlingLlangStatus::InvalidArgument);
    }
    ScalarGraphConfig {
        max_states: bounded_usize(config.max_states, "max_states")?,
        max_arcs: bounded_usize(config.max_arcs, "max_arcs")?,
        max_work: bounded_usize(config.max_work, "max_work")?,
        work_per_call: bounded_usize(config.work_per_call, "work_per_call")?,
    }
    .validate()
    .map_err(map_error)
}

/// Capture one immutable scalar WFST snapshot without asking for its state
/// count or expanding any state. The source may be released after return.
///
/// # Safety
/// Inputs must be live and readable. `out_cursor` must point to a writable
/// null pointer. The resource must remain live for this call only.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_cursor_open(
    resource: *const VtResource,
    config: *const LlingGraphConfig,
    out_cursor: *mut *mut LlingGraphCursor,
) -> LlingLlangStatus {
    boundary(|| {
        if resource.is_null() || config.is_null() {
            set_error("resource and config must be non-null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let output = required_mut(out_cursor, "out_cursor")?;
        if !output.is_null() {
            set_error("out_cursor must initially be null");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let config = decode_config(unsafe { *config })?;
        let cursor =
            unsafe { CapturedGraphCursor::capture(*resource, config) }.map_err(map_error)?;
        *output = Box::into_raw(Box::new(LlingGraphCursor {
            cursor: Some(cursor),
        }));
        Ok(())
    })
}

/// Advance by no more than `work_per_call` validated provider callbacks.
/// Cancellation is borrowed for this call and checked between callbacks, not
/// during a callback. Terminal outcomes remain sticky on later calls.
///
/// # Safety
/// Non-null handles and output pointers must be live and writable. A cursor
/// must not be concurrently advanced, taken, or freed by another thread.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_cursor_next(
    cursor: *mut LlingGraphCursor,
    cancellation: *const LlingCancellationV2,
    out_poll: *mut u32,
) -> LlingLlangStatus {
    boundary(|| {
        let cursor = required_mut(cursor, "cursor")?;
        let output = required_mut(out_poll, "out_poll")?;
        let inner = cursor.cursor.as_mut().ok_or_else(|| {
            set_error("graph cursor was consumed by take");
            LlingLlangStatus::Closed
        })?;
        let result = inner
            .poll(|| !cancellation.is_null() && unsafe { (*cancellation).reason() } != 0)
            .map_err(map_error)?;
        *output = match result {
            GraphPoll::Pending => LLING_GRAPH_POLL_PENDING,
            GraphPoll::Complete => LLING_GRAPH_POLL_COMPLETE,
            GraphPoll::Cancelled => LLING_GRAPH_POLL_CANCELLED,
        };
        Ok(())
    })
}

/// Transfer a completed graph out of its cursor exactly once. Incomplete,
/// cancelled, and failed cursors cannot yield a graph.
///
/// # Safety
/// The cursor must be live and exclusively owned. `out_graph` must be a
/// writable null pointer. The cursor remains valid but consumed afterward.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_cursor_take(
    cursor: *mut LlingGraphCursor,
    out_graph: *mut *mut LlingGraph,
) -> LlingLlangStatus {
    boundary(|| {
        let cursor = required_mut(cursor, "cursor")?;
        let output = required_mut(out_graph, "out_graph")?;
        if !output.is_null() {
            set_error("out_graph must initially be null");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if !cursor
            .cursor
            .as_ref()
            .is_some_and(CapturedGraphCursor::is_complete)
        {
            set_error("graph capture is not complete");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let graph = cursor
            .cursor
            .take()
            .expect("checked complete")
            .into_graph()
            .map_err(map_error)?;
        *output = Box::into_raw(Box::new(LlingGraph { graph }));
        Ok(())
    })
}

/// Release a cursor and any snapshot still owned by it. Null is accepted.
///
/// # Safety
/// A non-null pointer must come from `lling_graph_cursor_open` and not have
/// been freed previously.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_cursor_free(cursor: *mut LlingGraphCursor) {
    if !cursor.is_null() {
        unsafe { drop(Box::from_raw(cursor)) };
    }
}

/// Return domains, exact counts, and the provider's original start-state ID.
///
/// # Safety
/// The graph must be live. Every output pointer must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_info(
    graph: *const LlingGraph,
    out_unit_domain: *mut u32,
    out_weight_domain: *mut u32,
    out_start_raw: *mut u64,
    out_state_count: *mut usize,
    out_arc_count: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let unit_domain = required_mut(out_unit_domain, "out_unit_domain")?;
        let weight_domain = required_mut(out_weight_domain, "out_weight_domain")?;
        let start_raw = required_mut(out_start_raw, "out_start_raw")?;
        let state_count = required_mut(out_state_count, "out_state_count")?;
        let arc_count = required_mut(out_arc_count, "out_arc_count")?;
        let graph = &unsafe { &*graph }.graph;
        *unit_domain = graph.unit_domain as u32;
        *weight_domain = graph.weight_domain as u32;
        *start_raw = graph.states[graph.start()].raw_id;
        *state_count = graph.states.len();
        *arc_count = graph.arc_count();
        Ok(())
    })
}

/// Return one completed local state and its provider ID.
///
/// # Safety
/// The graph must be live and outputs writable.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_state(
    graph: *const LlingGraph,
    local_id: usize,
    out_raw_id: *mut u64,
    out_is_final: *mut u8,
    out_final_weight: *mut f64,
    out_arc_count: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let raw_id = required_mut(out_raw_id, "out_raw_id")?;
        let is_final = required_mut(out_is_final, "out_is_final")?;
        let final_weight = required_mut(out_final_weight, "out_final_weight")?;
        let arc_count = required_mut(out_arc_count, "out_arc_count")?;
        let state = unsafe { &(*graph).graph }
            .states
            .get(local_id)
            .ok_or_else(|| {
                set_error("local state ID is out of range");
                LlingLlangStatus::InvalidArgument
            })?;
        *raw_id = state.raw_id;
        *is_final = u8::from(state.is_final);
        *final_weight = state.final_weight;
        *arc_count = state.arcs.len();
        Ok(())
    })
}

/// Copy a bounded page of arcs for one local state. Each record contains the
/// original provider target and its deterministic compact local target ID.
///
/// # Safety
/// The graph must be live; outputs and the requested nonempty page must be
/// writable for the specified capacities.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_arcs(
    graph: *const LlingGraph,
    local_id: usize,
    offset: usize,
    out_arcs: *mut LlingGraphArc,
    capacity: usize,
    out_written: *mut usize,
    out_total: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        if graph.is_null() {
            set_error("graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let written = required_mut(out_written, "out_written")?;
        let total = required_mut(out_total, "out_total")?;
        let graph = &unsafe { &*graph }.graph;
        let state = graph.states.get(local_id).ok_or_else(|| {
            set_error("local state ID is out of range");
            LlingLlangStatus::InvalidArgument
        })?;
        if offset > state.arcs.len() {
            set_error("arc offset exceeds state arc count");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if capacity != 0 && out_arcs.is_null() {
            set_error("out_arcs is null with nonzero capacity");
            return Err(LlingLlangStatus::NullPointer);
        }
        let count = (state.arcs.len() - offset).min(capacity);
        for (index, arc) in state.arcs[offset..offset + count].iter().enumerate() {
            let target = graph.local_ids.get(&arc.target_state).ok_or_else(|| {
                set_error("complete graph lacks an arc target");
                LlingLlangStatus::ProviderError
            })?;
            let target_local = u64::try_from(*target).map_err(|_| {
                set_error("local state ID exceeds u64");
                LlingLlangStatus::LimitExceeded
            })?;
            unsafe {
                ptr::write(
                    out_arcs.add(index),
                    LlingGraphArc {
                        target_local,
                        arc: *arc,
                    },
                )
            };
        }
        *written = count;
        *total = state.arcs.len();
        Ok(())
    })
}

/// Release one complete graph. Null is accepted.
///
/// # Safety
/// A non-null pointer must come from `lling_graph_cursor_take` and not have
/// been freed previously.
#[no_mangle]
pub unsafe extern "C" fn lling_graph_free(graph: *mut LlingGraph) {
    if !graph.is_null() {
        unsafe { drop(Box::from_raw(graph)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{OwnedWfstResource, ScalarWfstGraph};
    use vinary_tree_interop::{VtUnitDomain, VtWeightDomain};

    fn fixture() -> OwnedWfstResource {
        let mut graph = ScalarWfstGraph::new(VtUnitDomain::Byte, VtWeightDomain::TropicalF64);
        let root = graph.add_state().expect("root");
        let terminal = graph.add_state().expect("terminal");
        assert!(graph.set_start(root));
        assert!(graph.set_final(terminal, 3.0));
        assert!(graph.add_arc(
            root,
            VtWfstArc {
                input_label: u64::from(b'a'),
                output_label: u64::from(b'A'),
                target_state: u64::from(terminal),
                weight: 2.0,
                has_input: 1,
                has_output: 1,
                reserved: [0; 6],
            },
        ));
        OwnedWfstResource::from_scalar_wfst(graph)
    }

    fn config() -> LlingGraphConfig {
        LlingGraphConfig {
            struct_size: size_of::<LlingGraphConfig>() as u32,
            version: 1,
            max_states: 2,
            max_arcs: 1,
            max_work: 4,
            work_per_call: 1,
        }
    }

    #[test]
    fn captured_graph_is_exact_paged_and_outlives_source_and_cursor() {
        let owner = fixture();
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_cursor_open(&owner.as_raw(), &config(), &mut cursor) },
            LlingLlangStatus::Ok
        );
        drop(owner);

        let mut graph = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_cursor_take(cursor, &mut graph) },
            LlingLlangStatus::InvalidArgument
        );
        assert!(graph.is_null());
        let mut pending = 0;
        loop {
            let mut poll = 0;
            assert_eq!(
                unsafe { lling_graph_cursor_next(cursor, ptr::null(), &mut poll) },
                LlingLlangStatus::Ok
            );
            if poll == LLING_GRAPH_POLL_COMPLETE {
                break;
            }
            assert_eq!(poll, LLING_GRAPH_POLL_PENDING);
            pending += 1;
        }
        assert_eq!(pending, 3);
        assert_eq!(
            unsafe { lling_graph_cursor_take(cursor, &mut graph) },
            LlingLlangStatus::Ok
        );
        unsafe { lling_graph_cursor_free(cursor) };

        let mut unit = 0;
        let mut weight_domain = 0;
        let mut start = u64::MAX;
        let mut states = 0;
        let mut arcs = 0;
        assert_eq!(
            unsafe {
                lling_graph_info(
                    graph,
                    &mut unit,
                    &mut weight_domain,
                    &mut start,
                    &mut states,
                    &mut arcs,
                )
            },
            LlingLlangStatus::Ok
        );
        assert_eq!((unit, weight_domain, start, states, arcs), (1, 1, 0, 2, 1));

        let mut raw_id = u64::MAX;
        let mut finality = 0;
        let mut final_weight = f64::NAN;
        let mut arc_count = usize::MAX;
        assert_eq!(
            unsafe {
                lling_graph_state(
                    graph,
                    1,
                    &mut raw_id,
                    &mut finality,
                    &mut final_weight,
                    &mut arc_count,
                )
            },
            LlingLlangStatus::Ok
        );
        assert_eq!((raw_id, finality, final_weight, arc_count), (1, 1, 3.0, 0));

        let mut page = [LlingGraphArc {
            target_local: u64::MAX,
            arc: VtWfstArc::default(),
        }];
        let mut written = 0;
        let mut total = 0;
        assert_eq!(
            unsafe {
                lling_graph_arcs(graph, 0, 0, page.as_mut_ptr(), 1, &mut written, &mut total)
            },
            LlingLlangStatus::Ok
        );
        assert_eq!((written, total, page[0].target_local), (1, 1, 1));
        assert_eq!(page[0].arc.target_state, 1);
        assert_eq!(page[0].arc.weight, 2.0);
        unsafe { lling_graph_free(graph) };
    }

    #[test]
    fn cancellation_and_too_small_arc_bound_never_return_a_graph() {
        let owner = fixture();
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_cursor_open(&owner.as_raw(), &config(), &mut cursor) },
            LlingLlangStatus::Ok
        );
        let mut cancellation = ptr::null_mut();
        assert_eq!(
            super::super::lling_cancellation_v2_new(&mut cancellation),
            LlingLlangStatus::Ok
        );
        assert_eq!(
            super::super::lling_cancellation_v2_request(cancellation, 1),
            LlingLlangStatus::Ok
        );
        let mut poll = 0;
        assert_eq!(
            unsafe { lling_graph_cursor_next(cursor, cancellation, &mut poll) },
            LlingLlangStatus::Ok
        );
        assert_eq!(poll, LLING_GRAPH_POLL_CANCELLED);
        let mut graph = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_cursor_take(cursor, &mut graph) },
            LlingLlangStatus::InvalidArgument
        );
        assert!(graph.is_null());
        unsafe { lling_graph_cursor_free(cursor) };
        assert_eq!(
            super::super::lling_cancellation_v2_free(&mut cancellation),
            LlingLlangStatus::Ok
        );

        let mut limited = config();
        limited.max_arcs = 0;
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { lling_graph_cursor_open(&owner.as_raw(), &limited, &mut cursor) },
            LlingLlangStatus::Ok
        );
        assert_eq!(
            unsafe { lling_graph_cursor_next(cursor, ptr::null(), &mut poll) },
            LlingLlangStatus::Ok
        );
        assert_eq!(
            unsafe { lling_graph_cursor_next(cursor, ptr::null(), &mut poll) },
            LlingLlangStatus::LimitExceeded
        );
        assert_eq!(
            unsafe { lling_graph_cursor_take(cursor, &mut graph) },
            LlingLlangStatus::InvalidArgument
        );
        assert!(graph.is_null());
        unsafe { lling_graph_cursor_free(cursor) };
    }
}
