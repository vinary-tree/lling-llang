//! Additive C ABI for bounded, seeded accepting-path sampling.

use super::{
    boundary, bounded_usize, distance::map_analysis_error, required_mut, set_error,
    LlingCancellationV2, LlingGraph, LlingLlangStatus, LlingPath,
};
use crate::bindings::{SamplePathConfig, SamplePathCursor, SamplePoll, SampleStrategy};
use std::mem::size_of;
use std::sync::Arc;

/// Versioned exact-distribution and bounded-work sampling configuration.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingSamplePathConfig {
    /// Exact structure size (56 bytes for version 1).
    pub struct_size: u32,
    /// Exactly 1 for this structure.
    pub version: u32,
    /// Whole-cursor native work budget, including backward analysis.
    pub max_work: u64,
    /// Maximum native work units in one poll.
    pub work_per_call: u64,
    /// Maximum arcs in one sampled path.
    pub max_depth: u64,
    /// Maximum paths emitted by the cursor.
    pub max_samples: u64,
    /// `LLING_SAMPLE_UNIFORM` or `LLING_SAMPLE_PROPORTIONAL`.
    pub strategy: u32,
    /// Must be zero.
    pub reserved: u32,
    /// Stable SplitMix64 seed.
    pub seed: u64,
}

/// Opaque mutable cursor retaining one complete graph lease.
pub struct LlingSamplePathCursor {
    cursor: SamplePathCursor,
}

/// Draw uniformly among finite accepting paths (scalar weights ignored).
pub const LLING_SAMPLE_UNIFORM: u32 = 1;
/// Draw in proportion to probability, log, or count path mass.
pub const LLING_SAMPLE_PROPORTIONAL: u32 = 2;
/// One owned path was transferred to the caller.
pub const LLING_SAMPLE_POLL_PATH: u32 = 1;
/// More bounded work remains.
pub const LLING_SAMPLE_POLL_PENDING: u32 = 2;
/// The complete graph has no accepting path.
pub const LLING_SAMPLE_POLL_EXHAUSTED: u32 = 3;
/// The sample count or path depth bound was reached.
pub const LLING_SAMPLE_POLL_TRUNCATED: u32 = 4;
/// Cancellation occurred before the next draw completed.
pub const LLING_SAMPLE_POLL_CANCELLED: u32 = 5;

/// Create a seeded stream over an immutable complete graph. Graph closure
/// does not invalidate this cursor.
///
/// # Safety
/// Inputs must be live/readable and output a writable null slot.
#[no_mangle]
pub unsafe extern "C" fn lling_sample_path_cursor_open(
    graph: *const LlingGraph,
    config: *const LlingSamplePathConfig,
    out_cursor: *mut *mut LlingSamplePathCursor,
) -> LlingLlangStatus {
    boundary(|| {
        if graph.is_null() || config.is_null() {
            set_error("graph and sample config must be non-null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let output = required_mut(out_cursor, "out_cursor")?;
        if !output.is_null() {
            set_error("out_cursor must initially be null");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let config = unsafe { *config };
        if config.struct_size as usize != size_of::<LlingSamplePathConfig>()
            || config.version != 1
            || config.reserved != 0
        {
            set_error("sample config has unsupported size, version, or reserved bits");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let strategy = match config.strategy {
            LLING_SAMPLE_UNIFORM => SampleStrategy::Uniform,
            LLING_SAMPLE_PROPORTIONAL => SampleStrategy::Proportional,
            _ => {
                set_error("sample strategy is invalid");
                return Err(LlingLlangStatus::InvalidArgument);
            }
        };
        let config = SamplePathConfig {
            max_work: bounded_usize(config.max_work, "max_work")?,
            work_per_call: bounded_usize(config.work_per_call, "work_per_call")?,
            max_depth: bounded_usize(config.max_depth, "max_depth")?,
            max_samples: bounded_usize(config.max_samples, "max_samples")?,
            strategy,
            seed: config.seed,
        };
        if config.max_work == 0 || config.work_per_call == 0 || config.max_samples == 0 {
            set_error("sample work and count bounds must be positive");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let source = Arc::clone(&unsafe { &*graph }.graph);
        let cursor = SamplePathCursor::new(source, config).map_err(map_analysis_error)?;
        *output = Box::into_raw(Box::new(LlingSamplePathCursor { cursor }));
        Ok(())
    })
}

/// Perform at most `work_per_call` native decisions. Output path ownership
/// transfers to the caller only for `LLING_SAMPLE_POLL_PATH`.
///
/// # Safety
/// Cursor and outputs must be live; out_path must initially be null. Mutable
/// cursor access must be synchronized by the caller.
#[no_mangle]
pub unsafe extern "C" fn lling_sample_path_cursor_next(
    cursor: *mut LlingSamplePathCursor,
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
            SamplePoll::Path(value) => {
                *path = Box::into_raw(Box::new(LlingPath { path: value }));
                LLING_SAMPLE_POLL_PATH
            }
            SamplePoll::Pending => LLING_SAMPLE_POLL_PENDING,
            SamplePoll::Exhausted => LLING_SAMPLE_POLL_EXHAUSTED,
            SamplePoll::Truncated => LLING_SAMPLE_POLL_TRUNCATED,
            SamplePoll::Cancelled => LLING_SAMPLE_POLL_CANCELLED,
        };
        Ok(())
    })
}

/// Release the cursor and its graph lease. Null is accepted.
///
/// # Safety
/// Non-null pointer must be an unfreed handle from `open`.
#[no_mangle]
pub unsafe extern "C" fn lling_sample_path_cursor_free(cursor: *mut LlingSamplePathCursor) {
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
    fn sampled_path_and_graph_handles_have_independent_lifetimes() {
        let source = Box::into_raw(Box::new(LlingGraph {
            graph: Arc::new(ScalarGraph {
                unit_domain: VtUnitDomain::Byte,
                weight_domain: VtWeightDomain::ProbabilityF64,
                states: vec![
                    ScalarGraphState {
                        raw_id: 10,
                        is_final: false,
                        final_weight: 0.0,
                        arcs: vec![VtWfstArc {
                            target_state: 20,
                            weight: 0.5,
                            ..VtWfstArc::default()
                        }],
                    },
                    ScalarGraphState {
                        raw_id: 20,
                        is_final: true,
                        final_weight: 0.5,
                        arcs: vec![],
                    },
                ],
                local_ids: HashMap::from([(10, 0), (20, 1)]),
            }),
        }));
        let config = LlingSamplePathConfig {
            struct_size: size_of::<LlingSamplePathConfig>() as u32,
            version: 1,
            max_work: 100,
            work_per_call: 1,
            max_depth: 1,
            max_samples: 1,
            strategy: LLING_SAMPLE_PROPORTIONAL,
            reserved: 0,
            seed: 42,
        };
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { lling_sample_path_cursor_open(source, &config, &mut cursor) },
            LlingLlangStatus::Ok
        );
        unsafe { lling_graph_free(source) };
        let mut poll = 0;
        let mut path = ptr::null_mut();
        loop {
            assert_eq!(
                unsafe { lling_sample_path_cursor_next(cursor, ptr::null(), &mut poll, &mut path) },
                LlingLlangStatus::Ok
            );
            if poll != LLING_SAMPLE_POLL_PENDING {
                break;
            }
        }
        assert_eq!(poll, LLING_SAMPLE_POLL_PATH);
        let mut final_state = 0;
        let mut weight = 0.0;
        let mut count = 0;
        assert_eq!(
            unsafe { lling_path_info(path, &mut final_state, &mut weight, &mut count) },
            LlingLlangStatus::Ok
        );
        assert_eq!((final_state, weight, count), (20, 0.25, 1));
        unsafe { lling_path_free(path) };
        path = ptr::null_mut();
        assert_eq!(
            unsafe { lling_sample_path_cursor_next(cursor, ptr::null(), &mut poll, &mut path) },
            LlingLlangStatus::Ok
        );
        assert_eq!(poll, LLING_SAMPLE_POLL_TRUNCATED);
        assert!(path.is_null());
        unsafe { lling_sample_path_cursor_free(cursor) };
    }
}
