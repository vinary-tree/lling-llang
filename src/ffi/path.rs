//! Additive C ABI for bounded traversal of snapshot-captured scalar WFSTs.

use super::{boundary, map_error, required_mut, set_error, LlingCancellationV2, LlingLlangStatus};
use crate::bindings::{PathPoll, ScalarPath, ScalarPathConfig, ScalarPathCursor};
use std::mem::size_of;
use std::ptr;
use vinary_tree_interop::{VtResource, VtWfstArc};

/// Versioned traversal bounds. Every bound is explicit; no hidden unlimited walk.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingPathConfig {
    /// Exact C struct size in bytes (56 for version 1).
    pub struct_size: u32,
    /// Configuration version; exactly 1 for this layout.
    pub version: u32,
    /// Maximum number of distinct reachable states expanded.
    pub max_states: u64,
    /// Maximum aggregate number of arcs cached from those states.
    pub max_arcs: u64,
    /// Maximum traversal decisions across the cursor lifetime.
    pub max_work: u64,
    /// Maximum traversal decisions in one `next` call.
    pub work_per_call: u64,
    /// Maximum number of arcs in one emitted path; zero permits empty paths.
    pub max_depth: u64,
    /// Maximum number of paths yielded before reporting truncation.
    pub max_paths: u64,
}

/// One source-state/arc pair of an owned accepting path.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingPathStep {
    /// State that owns the following arc.
    pub from_state: u64,
    /// Original domain-preserving scalar arc.
    pub arc: VtWfstArc,
}

/// Opaque, snapshot-owning native traversal cursor.
pub struct LlingPathCursor {
    cursor: ScalarPathCursor,
}

/// Opaque owned path; valid independently of the cursor after `next` returns.
pub struct LlingPath {
    path: ScalarPath,
}

/// One path was yielded.
pub const LLING_PATH_POLL_PATH: u32 = 1;
/// More work remains; poll again without blocking the caller indefinitely.
pub const LLING_PATH_POLL_PENDING: u32 = 2;
/// Every accepting path within the declared depth was exhausted.
pub const LLING_PATH_POLL_EXHAUSTED: u32 = 3;
/// A path or depth bound curtailed enumeration; the result is incomplete.
pub const LLING_PATH_POLL_TRUNCATED: u32 = 4;
/// The caller's cancellation request terminated the cursor.
pub const LLING_PATH_POLL_CANCELLED: u32 = 5;

fn bounded_usize(value: u64, name: &'static str) -> Result<usize, LlingLlangStatus> {
    usize::try_from(value).map_err(|_| {
        set_error(format!(
            "{name} exceeds this platform's usize representation"
        ));
        LlingLlangStatus::LimitExceeded
    })
}

fn decode_config(config: LlingPathConfig) -> Result<ScalarPathConfig, LlingLlangStatus> {
    if config.struct_size as usize != size_of::<LlingPathConfig>() || config.version != 1 {
        set_error("path config has an unsupported size or version");
        return Err(LlingLlangStatus::InvalidArgument);
    }
    ScalarPathConfig {
        max_states: bounded_usize(config.max_states, "max_states")?,
        max_arcs: bounded_usize(config.max_arcs, "max_arcs")?,
        max_work: bounded_usize(config.max_work, "max_work")?,
        work_per_call: bounded_usize(config.work_per_call, "work_per_call")?,
        max_depth: bounded_usize(config.max_depth, "max_depth")?,
        max_paths: bounded_usize(config.max_paths, "max_paths")?,
    }
    .validate()
    .map_err(map_error)
}

/// Capture a scalar WFST snapshot and create a bounded depth-first cursor.
///
/// Unlike a graph import, opening this cursor never enumerates all states or
/// asks for a known state count. `resource` may be released after return.
///
/// # Safety
/// `resource` and `config` must be readable when non-null. `out_cursor` must
/// point to a writable null pointer. The resource must remain live for this
/// call; the cursor owns an independent snapshot afterward.
#[no_mangle]
pub unsafe extern "C" fn lling_path_cursor_open(
    resource: *const VtResource,
    config: *const LlingPathConfig,
    out_cursor: *mut *mut LlingPathCursor,
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
        let cursor = unsafe { ScalarPathCursor::capture(*resource, config) }.map_err(map_error)?;
        *output = Box::into_raw(Box::new(LlingPathCursor { cursor }));
        Ok(())
    })
}

/// Advance by at most the cursor's `work_per_call` bound.
///
/// `cancellation` is borrowed only during this call; null means no cancellation.
/// A `PATH` poll transfers one owned `LlingPath` to `out_path`. All other polls
/// leave that slot null. A `PENDING` result is not completion and is safe to
/// poll again. `TRUNCATED` is not an exact exhaustive result.
/// Exhaustion, truncation, cancellation, and failures are sticky on later
/// calls; a terminal cursor never reports a different completion category.
///
/// # Safety
/// All non-null handles and output pointers must be live for this call. One
/// cursor must not be concurrently advanced or freed by another thread.
#[no_mangle]
pub unsafe extern "C" fn lling_path_cursor_next(
    cursor: *mut LlingPathCursor,
    cancellation: *const LlingCancellationV2,
    out_poll: *mut u32,
    out_path: *mut *mut LlingPath,
) -> LlingLlangStatus {
    boundary(|| {
        let cursor = required_mut(cursor, "cursor")?;
        let poll_output = required_mut(out_poll, "out_poll")?;
        let path_output = required_mut(out_path, "out_path")?;
        if !path_output.is_null() {
            set_error("out_path must initially be null");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let result = cursor
            .cursor
            .poll(|| !cancellation.is_null() && unsafe { (*cancellation).reason() } != 0)
            .map_err(map_error)?;
        match result {
            PathPoll::Path(path) => {
                *path_output = Box::into_raw(Box::new(LlingPath { path }));
                *poll_output = LLING_PATH_POLL_PATH;
            }
            PathPoll::Pending => *poll_output = LLING_PATH_POLL_PENDING,
            PathPoll::Exhausted => *poll_output = LLING_PATH_POLL_EXHAUSTED,
            PathPoll::Truncated => *poll_output = LLING_PATH_POLL_TRUNCATED,
            PathPoll::Cancelled => *poll_output = LLING_PATH_POLL_CANCELLED,
        }
        Ok(())
    })
}

/// Release a cursor and its captured snapshot. Null is accepted.
///
/// # Safety
/// A non-null pointer must have been returned by `lling_path_cursor_open` and
/// not previously freed.
#[no_mangle]
pub unsafe extern "C" fn lling_path_cursor_free(cursor: *mut LlingPathCursor) {
    if !cursor.is_null() {
        unsafe { drop(Box::from_raw(cursor)) };
    }
}

/// Return the final state, complete path weight, and number of steps.
///
/// # Safety
/// `path` must be live. Every output pointer must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_path_info(
    path: *const LlingPath,
    out_final_state: *mut u64,
    out_weight: *mut f64,
    out_step_count: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        if path.is_null() {
            set_error("path is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let final_state = required_mut(out_final_state, "out_final_state")?;
        let weight = required_mut(out_weight, "out_weight")?;
        let step_count = required_mut(out_step_count, "out_step_count")?;
        let path = unsafe { &(*path).path };
        *final_state = path.final_state;
        *weight = path.weight;
        *step_count = path.steps.len();
        Ok(())
    })
}

/// Copy a bounded page of path steps in the original transition order.
///
/// # Safety
/// `path` must be live, outputs writable, and `out_steps` writable for
/// `capacity` elements when capacity is nonzero. No cursor call is required.
#[no_mangle]
pub unsafe extern "C" fn lling_path_steps(
    path: *const LlingPath,
    offset: usize,
    out_steps: *mut LlingPathStep,
    capacity: usize,
    out_written: *mut usize,
    out_total: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        if path.is_null() {
            set_error("path is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let written = required_mut(out_written, "out_written")?;
        let total = required_mut(out_total, "out_total")?;
        let steps = &unsafe { &*path }.path.steps;
        if offset > steps.len() {
            set_error("path step offset exceeds step count");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if capacity != 0 && out_steps.is_null() {
            set_error("out_steps is null with nonzero capacity");
            return Err(LlingLlangStatus::NullPointer);
        }
        *total = steps.len();
        *written = (steps.len() - offset).min(capacity);
        for (index, step) in steps[offset..offset + *written].iter().enumerate() {
            unsafe {
                ptr::write(
                    out_steps.add(index),
                    LlingPathStep {
                        from_state: step.from,
                        arc: step.arc,
                    },
                );
            }
        }
        Ok(())
    })
}

/// Release one owned path. Null is accepted.
///
/// # Safety
/// A non-null pointer must have been returned by `lling_path_cursor_next` and
/// not previously freed.
#[no_mangle]
pub unsafe extern "C" fn lling_path_free(path: *mut LlingPath) {
    if !path.is_null() {
        unsafe { drop(Box::from_raw(path)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{
        OwnedWfstResource, ScalarWfstGraph, ScalarWfstProvider, ScalarWfstState,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use vinary_tree_interop::{VtStatus, VtUnitDomain, VtWeightDomain};

    fn fixture() -> OwnedWfstResource {
        let mut graph = ScalarWfstGraph::new(VtUnitDomain::Byte, VtWeightDomain::TropicalF64);
        let root = graph.add_state().expect("root state fits u32");
        let terminal = graph.add_state().expect("terminal state fits u32");
        assert!(graph.set_start(root));
        assert!(graph.set_final(root, 0.0));
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

    fn config() -> LlingPathConfig {
        LlingPathConfig {
            struct_size: size_of::<LlingPathConfig>() as u32,
            version: 1,
            max_states: 2,
            max_arcs: 1,
            max_work: 100,
            work_per_call: 1,
            max_depth: 1,
            max_paths: 3,
        }
    }

    unsafe fn open(resource: VtResource, config: &LlingPathConfig) -> *mut LlingPathCursor {
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { lling_path_cursor_open(&resource, config, &mut cursor) },
            LlingLlangStatus::Ok
        );
        assert!(!cursor.is_null());
        cursor
    }

    unsafe fn next_path(cursor: *mut LlingPathCursor) -> (u32, *mut LlingPath) {
        for _ in 0..32 {
            let mut poll = 0;
            let mut path = ptr::null_mut();
            assert_eq!(
                unsafe { lling_path_cursor_next(cursor, ptr::null(), &mut poll, &mut path) },
                LlingLlangStatus::Ok
            );
            if poll != LLING_PATH_POLL_PENDING {
                return (poll, path);
            }
        }
        panic!("bounded path cursor did not make progress");
    }

    #[test]
    fn traversal_owns_snapshot_and_path_outlives_cursor() {
        let owner = fixture();
        let cursor = unsafe { open(owner.as_raw(), &config()) };
        drop(owner);

        let (first_poll, empty_path) = unsafe { next_path(cursor) };
        assert_eq!(first_poll, LLING_PATH_POLL_PATH);
        let mut final_state = u64::MAX;
        let mut weight = f64::NAN;
        let mut count = usize::MAX;
        assert_eq!(
            unsafe { lling_path_info(empty_path, &mut final_state, &mut weight, &mut count) },
            LlingLlangStatus::Ok
        );
        assert_eq!((final_state, weight, count), (0, 0.0, 0));
        unsafe { lling_path_free(empty_path) };

        let (second_poll, path) = unsafe { next_path(cursor) };
        assert_eq!(second_poll, LLING_PATH_POLL_PATH);
        unsafe { lling_path_cursor_free(cursor) };
        assert_eq!(
            unsafe { lling_path_info(path, &mut final_state, &mut weight, &mut count) },
            LlingLlangStatus::Ok
        );
        assert_eq!((final_state, weight, count), (1, 5.0, 1));
        let mut steps = [LlingPathStep {
            from_state: 0,
            arc: VtWfstArc::default(),
        }];
        let mut written = 0;
        let mut total = 0;
        assert_eq!(
            unsafe { lling_path_steps(path, 0, steps.as_mut_ptr(), 1, &mut written, &mut total) },
            LlingLlangStatus::Ok
        );
        assert_eq!((written, total), (1, 1));
        assert_eq!(steps[0].from_state, 0);
        assert_eq!(steps[0].arc.input_label, u64::from(b'a'));
        unsafe { lling_path_free(path) };
    }

    #[test]
    fn depth_and_state_bounds_are_not_silent_completion() {
        let owner = fixture();
        let mut shallow = config();
        shallow.max_depth = 0;
        let cursor = unsafe { open(owner.as_raw(), &shallow) };
        let (poll, path) = unsafe { next_path(cursor) };
        assert_eq!(poll, LLING_PATH_POLL_PATH);
        unsafe { lling_path_free(path) };
        let (poll, path) = unsafe { next_path(cursor) };
        assert_eq!(poll, LLING_PATH_POLL_TRUNCATED);
        assert!(path.is_null());
        let (repeated, path) = unsafe { next_path(cursor) };
        assert_eq!(repeated, LLING_PATH_POLL_TRUNCATED);
        assert!(path.is_null());
        unsafe { lling_path_cursor_free(cursor) };

        let mut limited = config();
        limited.max_states = 1;
        let cursor = unsafe { open(owner.as_raw(), &limited) };
        let (_, path) = unsafe { next_path(cursor) };
        unsafe { lling_path_free(path) };
        let mut poll = 0;
        let mut path = ptr::null_mut();
        loop {
            let status =
                unsafe { lling_path_cursor_next(cursor, ptr::null(), &mut poll, &mut path) };
            if status != LlingLlangStatus::Ok {
                assert_eq!(status, LlingLlangStatus::LimitExceeded);
                break;
            }
            assert_eq!(poll, LLING_PATH_POLL_PENDING);
        }
        assert_eq!(
            unsafe { lling_path_cursor_next(cursor, ptr::null(), &mut poll, &mut path) },
            LlingLlangStatus::LimitExceeded
        );
        unsafe { lling_path_cursor_free(cursor) };
    }

    #[test]
    fn cancellation_is_borrowed_per_call_and_output_is_unchanged_on_failure() {
        let owner = fixture();
        let cursor = unsafe { open(owner.as_raw(), &config()) };
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
        let mut path = ptr::null_mut();
        assert_eq!(
            unsafe { lling_path_cursor_next(cursor, cancellation, &mut poll, &mut path) },
            LlingLlangStatus::Ok
        );
        assert_eq!(poll, LLING_PATH_POLL_CANCELLED);
        assert!(path.is_null());
        assert_eq!(unsafe { next_path(cursor) }.0, LLING_PATH_POLL_CANCELLED);
        assert_eq!(
            super::super::lling_cancellation_v2_free(&mut cancellation),
            LlingLlangStatus::Ok
        );
        unsafe { lling_path_cursor_free(cursor) };

        let mut invalid = config();
        invalid.version = 2;
        let mut unchanged = ptr::null_mut();
        assert_eq!(
            unsafe { lling_path_cursor_open(&owner.as_raw(), &invalid, &mut unchanged) },
            LlingLlangStatus::InvalidArgument
        );
        assert!(unchanged.is_null());
    }

    struct UnknownCountProvider {
        state_calls: Arc<AtomicUsize>,
        count_calls: Arc<AtomicUsize>,
    }

    impl ScalarWfstProvider for UnknownCountProvider {
        fn unit_domain(&self) -> VtUnitDomain {
            VtUnitDomain::Byte
        }

        fn start(&self) -> Result<u64, VtStatus> {
            Ok(0)
        }

        fn num_states(&self) -> Result<Option<usize>, VtStatus> {
            self.count_calls.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }

        fn state(&self, state: u64) -> Result<ScalarWfstState, VtStatus> {
            self.state_calls.fetch_add(1, Ordering::Relaxed);
            Ok(match state {
                0 => ScalarWfstState {
                    valid: true,
                    is_final: true,
                    final_weight: 0.0,
                    arcs: vec![VtWfstArc {
                        input_label: u64::from(b'x'),
                        output_label: u64::from(b'y'),
                        target_state: 1,
                        weight: 1.0,
                        has_input: 1,
                        has_output: 1,
                        reserved: [0; 6],
                    }],
                },
                1 => ScalarWfstState {
                    valid: true,
                    is_final: true,
                    final_weight: 0.0,
                    arcs: Vec::new(),
                },
                _ => ScalarWfstState {
                    valid: false,
                    is_final: false,
                    final_weight: f64::INFINITY,
                    arcs: Vec::new(),
                },
            })
        }
    }

    #[test]
    fn unknown_count_provider_expands_only_demanded_states() {
        let state_calls = Arc::new(AtomicUsize::new(0));
        let count_calls = Arc::new(AtomicUsize::new(0));
        let owner = OwnedWfstResource::from_provider(Arc::new(UnknownCountProvider {
            state_calls: Arc::clone(&state_calls),
            count_calls: Arc::clone(&count_calls),
        }));
        let cursor = unsafe { open(owner.as_raw(), &config()) };
        drop(owner);
        assert_eq!(state_calls.load(Ordering::Relaxed), 0);
        assert_eq!(count_calls.load(Ordering::Relaxed), 0);

        let (_, first) = unsafe { next_path(cursor) };
        unsafe { lling_path_free(first) };
        assert_eq!(state_calls.load(Ordering::Relaxed), 1);
        let (_, second) = unsafe { next_path(cursor) };
        unsafe { lling_path_free(second) };
        assert_eq!(state_calls.load(Ordering::Relaxed), 2);
        assert_eq!(count_calls.load(Ordering::Relaxed), 0);
        unsafe { lling_path_cursor_free(cursor) };
    }

    #[test]
    fn exact_work_budget_still_reports_exhaustion() {
        let owner = fixture();
        let mut exact = config();
        exact.max_work = 6;
        let cursor = unsafe { open(owner.as_raw(), &exact) };
        for _ in 0..2 {
            let (poll, path) = unsafe { next_path(cursor) };
            assert_eq!(poll, LLING_PATH_POLL_PATH);
            unsafe { lling_path_free(path) };
        }
        let (poll, path) = unsafe { next_path(cursor) };
        assert_eq!(poll, LLING_PATH_POLL_EXHAUSTED);
        assert!(path.is_null());
        assert_eq!(unsafe { next_path(cursor) }.0, LLING_PATH_POLL_EXHAUSTED);
        unsafe { lling_path_cursor_free(cursor) };
    }

    #[test]
    fn finite_path_weight_overflow_is_an_explicit_failure() {
        let mut graph = ScalarWfstGraph::new(VtUnitDomain::Byte, VtWeightDomain::TropicalF64);
        let root = graph.add_state().expect("root state fits u32");
        let terminal = graph.add_state().expect("terminal state fits u32");
        assert!(graph.set_start(root));
        assert!(graph.set_final(terminal, f64::MAX));
        assert!(graph.add_arc(
            root,
            VtWfstArc {
                target_state: u64::from(terminal),
                weight: f64::MAX,
                ..VtWfstArc::default()
            },
        ));
        let owner = OwnedWfstResource::from_scalar_wfst(graph);
        let cursor = unsafe { open(owner.as_raw(), &config()) };
        let mut poll = 0;
        let mut path = ptr::null_mut();
        loop {
            let status =
                unsafe { lling_path_cursor_next(cursor, ptr::null(), &mut poll, &mut path) };
            if status != LlingLlangStatus::Ok {
                assert_eq!(status, LlingLlangStatus::LimitExceeded);
                assert!(path.is_null());
                break;
            }
            assert_eq!(poll, LLING_PATH_POLL_PENDING);
        }
        unsafe { lling_path_cursor_free(cursor) };
    }
}
