//! Additive C ABI for bounded, deterministic best-first path enumeration.

use super::{
    boundary, bounded_usize, distance::map_analysis_error, required_mut, set_error,
    LlingCancellationV2, LlingGraph, LlingLlangStatus, LlingPath,
};
use crate::bindings::{RankedPathConfig, RankedPathCursor, RankedPoll};
use std::mem::size_of;
use std::sync::Arc;

/// Versioned work, depth, path-count, and frontier limits.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingRankedPathConfig {
    /// Exact C struct size in bytes (48 for version 1).
    pub struct_size: u32,
    /// Configuration version; exactly 1 for this layout.
    pub version: u32,
    /// Maximum native graph/suffix/search work over the cursor lifetime.
    pub max_work: u64,
    /// Maximum work units performed in one `next` call.
    pub work_per_call: u64,
    /// Maximum arcs per emitted path; zero permits an empty path.
    pub max_depth: u64,
    /// Maximum accepting paths yielded before explicit truncation.
    pub max_paths: u64,
    /// Maximum number of queued partial or complete paths.
    pub max_frontier: u64,
}

/// Opaque mutable best-first cursor retaining one complete graph lease.
pub struct LlingRankedPathCursor {
    cursor: RankedPathCursor,
}

/// A ranked accepting path was transferred to the caller.
pub const LLING_RANKED_POLL_PATH: u32 = 1;
/// Work remains; poll again.
pub const LLING_RANKED_POLL_PENDING: u32 = 2;
/// All accepting paths were exhausted exactly.
pub const LLING_RANKED_POLL_EXHAUSTED: u32 = 3;
/// A depth or path-count limit cut off further accepting paths.
pub const LLING_RANKED_POLL_TRUNCATED: u32 = 4;
/// The caller cancelled before exact exhaustion.
pub const LLING_RANKED_POLL_CANCELLED: u32 = 5;

/// Open a ranked cursor over one complete graph. The graph handle may close
/// after this call; the cursor retains its own immutable graph lease.
///
/// # Safety
/// Graph/config must be live/readable. Output must be a writable null slot.
#[no_mangle]
pub unsafe extern "C" fn lling_ranked_path_cursor_open(
    graph: *const LlingGraph,
    config: *const LlingRankedPathConfig,
    out_cursor: *mut *mut LlingRankedPathCursor,
) -> LlingLlangStatus {
    boundary(|| {
        if graph.is_null() || config.is_null() {
            set_error("graph and ranked path config must be non-null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let output = required_mut(out_cursor, "out_cursor")?;
        if !output.is_null() {
            set_error("out_cursor must initially be null");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let config = unsafe { *config };
        if config.struct_size as usize != size_of::<LlingRankedPathConfig>() || config.version != 1
        {
            set_error("ranked path config has an unsupported size or version");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let config = RankedPathConfig {
            max_work: bounded_usize(config.max_work, "max_work")?,
            work_per_call: bounded_usize(config.work_per_call, "work_per_call")?,
            max_depth: bounded_usize(config.max_depth, "max_depth")?,
            max_paths: bounded_usize(config.max_paths, "max_paths")?,
            max_frontier: bounded_usize(config.max_frontier, "max_frontier")?,
        };
        if config.max_work == 0 || config.work_per_call == 0 || config.max_frontier == 0 {
            set_error("ranked path work and frontier bounds must be positive");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let source = Arc::clone(&unsafe { &*graph }.graph);
        let cursor = RankedPathCursor::new(source, config).map_err(map_analysis_error)?;
        *output = Box::into_raw(Box::new(LlingRankedPathCursor { cursor }));
        Ok(())
    })
}

/// Perform one bounded best-first scheduling slice. An emitted path uses the
/// existing `LlingPath` accessors and remains valid after cursor closure.
///
/// # Safety
/// Cursor and output slots must be live. `out_path` must initially be null.
/// Mutable cursor access must be synchronized by the caller.
#[no_mangle]
pub unsafe extern "C" fn lling_ranked_path_cursor_next(
    cursor: *mut LlingRankedPathCursor,
    cancellation: *const LlingCancellationV2,
    out_poll: *mut u32,
    out_path: *mut *mut LlingPath,
) -> LlingLlangStatus {
    boundary(|| {
        let cursor = required_mut(cursor, "cursor")?;
        let poll = required_mut(out_poll, "out_poll")?;
        let path = required_mut(out_path, "out_path")?;
        if !path.is_null() {
            set_error("out_path must initially be null");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let result = cursor
            .cursor
            .poll(|| !cancellation.is_null() && unsafe { (*cancellation).reason() } != 0)
            .map_err(map_analysis_error)?;
        *poll = match result {
            RankedPoll::Path(value) => {
                *path = Box::into_raw(Box::new(LlingPath { path: value }));
                LLING_RANKED_POLL_PATH
            }
            RankedPoll::Pending => LLING_RANKED_POLL_PENDING,
            RankedPoll::Exhausted => LLING_RANKED_POLL_EXHAUSTED,
            RankedPoll::Truncated => LLING_RANKED_POLL_TRUNCATED,
            RankedPoll::Cancelled => LLING_RANKED_POLL_CANCELLED,
        };
        Ok(())
    })
}

/// Release a ranked cursor and its graph lease. Null is accepted.
///
/// # Safety
/// Non-null pointer must be an unfreed handle from `open`.
#[no_mangle]
pub unsafe extern "C" fn lling_ranked_path_cursor_free(cursor: *mut LlingRankedPathCursor) {
    if !cursor.is_null() {
        unsafe { drop(Box::from_raw(cursor)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{ScalarGraph, ScalarGraphState};
    use crate::ffi::{lling_graph_free, lling_path_free, lling_path_info};
    use std::collections::HashMap;
    use std::ptr;
    use vinary_tree_interop::{VtUnitDomain, VtWeightDomain, VtWfstArc};

    #[test]
    fn ranked_path_and_graph_handles_have_independent_lifetimes() {
        let source = Box::into_raw(Box::new(LlingGraph {
            graph: Arc::new(ScalarGraph {
                unit_domain: VtUnitDomain::Byte,
                weight_domain: VtWeightDomain::TropicalF64,
                states: vec![
                    ScalarGraphState {
                        raw_id: 10,
                        is_final: false,
                        final_weight: f64::INFINITY,
                        arcs: vec![VtWfstArc {
                            target_state: 20,
                            weight: 2.0,
                            ..VtWfstArc::default()
                        }],
                    },
                    ScalarGraphState {
                        raw_id: 20,
                        is_final: true,
                        final_weight: 3.0,
                        arcs: vec![],
                    },
                ],
                local_ids: HashMap::from([(10, 0), (20, 1)]),
            }),
        }));
        let config = LlingRankedPathConfig {
            struct_size: size_of::<LlingRankedPathConfig>() as u32,
            version: 1,
            max_work: 100,
            work_per_call: 1,
            max_depth: 1,
            max_paths: 2,
            max_frontier: 4,
        };
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { lling_ranked_path_cursor_open(source, &config, &mut cursor) },
            LlingLlangStatus::Ok
        );
        unsafe { lling_graph_free(source) };
        let mut poll = 0;
        let mut path = ptr::null_mut();
        loop {
            assert_eq!(
                unsafe { lling_ranked_path_cursor_next(cursor, ptr::null(), &mut poll, &mut path) },
                LlingLlangStatus::Ok
            );
            if poll != LLING_RANKED_POLL_PENDING {
                break;
            }
        }
        assert_eq!(poll, LLING_RANKED_POLL_PATH);
        unsafe { lling_ranked_path_cursor_free(cursor) };
        let mut final_state = 0;
        let mut weight = 0.0;
        let mut count = 0;
        assert_eq!(
            unsafe { lling_path_info(path, &mut final_state, &mut weight, &mut count) },
            LlingLlangStatus::Ok
        );
        assert_eq!((final_state, weight, count), (20, 5.0, 1));
        unsafe { lling_path_free(path) };
    }
}
