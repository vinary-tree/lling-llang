//! Validation matrix for the 17-function `lling_*` C ABI builder surface.
//!
//! Exercises every builder-lifecycle function against its full argument-error
//! matrix: absent states, non-tropical weights (the builder-surface twin of
//! finding LLING-B2/F1), non-scalar labels, presence-flag abuse, build-time
//! preconditions and their restore behavior, post-build `Closed` semantics,
//! null in/out pointers, error-message thread locality, and the ABI/API
//! version pins.
//!
//! The status wire rule at the interop layer is raw `u32`: anywhere this file
//! touches a `vt.scalar-wfst.1` callback it compares against
//! `VtStatus::…::to_raw()`; the project-level `lling_*` functions return the
//! typed `LlingStatus` directly.
//!
//! Formal-model correspondence (invariant registry owned by the coordinator):
//! - `// INVARIANT-HOOK: LLING-BRIDGE-4` — NaN and -inf rejected at every
//!   weight-ingestion site (builder twin of the ABI-side F1 fix).
//! - `// INVARIANT-HOOK: LLING-BRIDGE-1` — the accepted tropical domain is
//!   exactly the finite reals plus +inf (the semiring zero).
#![cfg(feature = "ffi")]

use lling_llang::ffi::{
    lling_abi_version, lling_api_revision, lling_last_error_message, lling_resource_release,
    lling_wfst_builder_add_arc, lling_wfst_builder_add_state, lling_wfst_builder_build,
    lling_wfst_builder_clear_final, lling_wfst_builder_free, lling_wfst_builder_new,
    lling_wfst_builder_reserve_states, lling_wfst_builder_set_final, lling_wfst_builder_set_start,
    lling_wfst_compose, lling_wfst_free, lling_wfst_import, lling_wfst_resource, LlingStatus,
    LlingWfst, LlingWfstBuilder, LLING_ABI_VERSION, LLING_API_REVISION,
};
use std::ffi::CStr;
use std::ptr;
use vinary_tree_interop::{
    VtResource, VtStatus, VtWfstVTable, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
};

/// Copy this thread's last ABI error message into owned storage.
fn last_error() -> String {
    let pointer = lling_last_error_message();
    assert!(!pointer.is_null(), "last_error_message must never be null");
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

/// Allocate a fresh builder, panicking on failure.
fn new_builder() -> *mut LlingWfstBuilder {
    let mut builder = ptr::null_mut();
    assert_eq!(lling_wfst_builder_new(&mut builder), LlingStatus::Ok);
    assert!(!builder.is_null(), "builder_new must write a live pointer");
    builder
}

/// Add one state and return its compact identifier.
fn add_state(builder: *mut LlingWfstBuilder) -> u32 {
    let mut state = u32::MAX;
    assert_eq!(
        lling_wfst_builder_add_state(builder, &mut state),
        LlingStatus::Ok
    );
    state
}

/// Build the canonical two-state fixture: `0 -a:b/0.25-> 1`, final(1)=0.
fn two_state_builder() -> *mut LlingWfstBuilder {
    let builder = new_builder();
    let s0 = add_state(builder);
    let s1 = add_state(builder);
    assert_eq!(lling_wfst_builder_set_start(builder, s0), LlingStatus::Ok);
    assert_eq!(
        lling_wfst_builder_set_final(builder, s1, 0.0),
        LlingStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_add_arc(builder, s0, u64::from('a'), 1, u64::from('b'), 1, s1, 0.25),
        LlingStatus::Ok
    );
    builder
}

/// Consume a ready builder into an immutable WFST handle.
fn build(builder: *mut LlingWfstBuilder) -> *mut LlingWfst {
    let mut wfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_builder_build(builder, &mut wfst),
        LlingStatus::Ok
    );
    assert!(!wfst.is_null(), "build must write a live handle");
    wfst
}

/// Discover the scalar-WFST interface vtable of an exported resource.
///
/// # Safety
/// `resource` must be a live `vt.scalar-wfst.1` resource.
unsafe fn wfst_interface(resource: VtResource) -> *const VtWfstVTable {
    let mut interface = ptr::null();
    let raw = (*resource.vtable)
        .query_interface
        .expect("resource vtable must publish query_interface")(
        resource.context,
        &VT_WFST_INTERFACE_ID,
        VT_WFST_INTERFACE_VERSION,
        &mut interface,
    );
    assert_eq!(
        raw,
        VtStatus::Ok.to_raw(),
        "query_interface must accept the published interface id/version"
    );
    interface.cast::<VtWfstVTable>()
}

/// Read `(valid, is_final, final_weight)` for one state of a resource.
///
/// # Safety
/// `resource` must be a live `vt.scalar-wfst.1` resource.
unsafe fn state_info(resource: VtResource, state: u64) -> (u8, u8, f64) {
    let table = &*wfst_interface(resource);
    let mut valid = u8::MAX;
    let mut is_final = u8::MAX;
    let mut final_weight = f64::NAN;
    let raw = table.state_info.expect("state_info must be published")(
        resource.context,
        state,
        &mut valid,
        &mut is_final,
        &mut final_weight,
    );
    assert_eq!(raw, VtStatus::Ok.to_raw(), "state_info must succeed");
    (valid, is_final, final_weight)
}

#[test]
fn abi_version_and_api_revision_are_pinned() {
    assert_eq!(lling_abi_version(), 1);
    assert_eq!(lling_api_revision(), 1);
    assert_eq!(LLING_ABI_VERSION, 1);
    assert_eq!(LLING_API_REVISION, 1);
}

#[test]
fn status_discriminants_are_pinned() {
    // Exhaustive match: adding a variant without extending this pin (and
    // bindings/api.json) becomes a compile error here.
    fn pinned(status: LlingStatus) -> u32 {
        match status {
            LlingStatus::Ok => 0,
            LlingStatus::InvalidArgument => 1,
            LlingStatus::NullPointer => 2,
            LlingStatus::Panic => 3,
            LlingStatus::IncompatibleResource => 4,
            LlingStatus::ProviderError => 5,
            LlingStatus::LimitExceeded => 6,
            LlingStatus::Closed => 7,
        }
    }
    for status in [
        LlingStatus::Ok,
        LlingStatus::InvalidArgument,
        LlingStatus::NullPointer,
        LlingStatus::Panic,
        LlingStatus::IncompatibleResource,
        LlingStatus::ProviderError,
        LlingStatus::LimitExceeded,
        LlingStatus::Closed,
    ] {
        assert_eq!(pinned(status), status as u32);
    }
}

#[test]
fn builder_new_rejects_null_out_pointer() {
    assert_eq!(
        lling_wfst_builder_new(ptr::null_mut()),
        LlingStatus::NullPointer
    );
    assert!(last_error().contains("out_builder"));
}

#[test]
fn mutators_reject_absent_states() {
    let builder = new_builder();
    let s0 = add_state(builder);

    // No such state 7 in a one-state builder.
    assert_eq!(
        lling_wfst_builder_set_start(builder, 7),
        LlingStatus::InvalidArgument
    );
    assert!(last_error().contains("start state"));
    assert_eq!(
        lling_wfst_builder_set_final(builder, 7, 0.0),
        LlingStatus::InvalidArgument
    );
    assert_eq!(
        lling_wfst_builder_clear_final(builder, 7),
        LlingStatus::InvalidArgument
    );
    // Absent source and absent target are both rejected.
    assert_eq!(
        lling_wfst_builder_add_arc(builder, 7, u64::from('a'), 1, u64::from('a'), 1, s0, 0.0),
        LlingStatus::InvalidArgument
    );
    assert_eq!(
        lling_wfst_builder_add_arc(builder, s0, u64::from('a'), 1, u64::from('a'), 1, 7, 0.0),
        LlingStatus::InvalidArgument
    );

    // The failures above must not have poisoned the builder.
    assert_eq!(lling_wfst_builder_set_start(builder, s0), LlingStatus::Ok);
    unsafe { lling_wfst_builder_free(builder) };
}

// INVARIANT-HOOK: LLING-BRIDGE-4 — every builder weight-ingestion site rejects
// NaN and -inf as InvalidArgument (never a panic, never a poisoned graph):
// the builder-surface twin of the ABI-side F1 fix.
#[test]
fn weight_ingestion_rejects_nan_and_negative_infinity() {
    let builder = new_builder();
    let s0 = add_state(builder);
    let s1 = add_state(builder);
    assert_eq!(lling_wfst_builder_set_start(builder, s0), LlingStatus::Ok);

    for poison in [f64::NAN, f64::NEG_INFINITY] {
        assert_eq!(
            lling_wfst_builder_set_final(builder, s1, poison),
            LlingStatus::InvalidArgument,
            "set_final must reject {poison}"
        );
        assert!(last_error().contains("finite or +infinity"));
        assert_eq!(
            lling_wfst_builder_add_arc(
                builder,
                s0,
                u64::from('a'),
                1,
                u64::from('b'),
                1,
                s1,
                poison
            ),
            LlingStatus::InvalidArgument,
            "add_arc must reject {poison}"
        );
    }

    // The rejected weights left no trace: the builder still finishes cleanly.
    assert_eq!(
        lling_wfst_builder_set_final(builder, s1, 0.5),
        LlingStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_add_arc(builder, s0, u64::from('a'), 1, u64::from('b'), 1, s1, 1.0),
        LlingStatus::Ok
    );
    let wfst = build(builder);
    unsafe {
        lling_wfst_free(wfst);
        lling_wfst_builder_free(builder);
    }
}

// INVARIANT-HOOK: LLING-BRIDGE-1 — the accepted tropical domain is exactly
// {finite} ∪ {+inf}: +inf (the semiring zero) is representable at both
// builder ingestion sites and survives to the exported resource.
#[test]
fn positive_infinity_weight_is_accepted() {
    let builder = new_builder();
    let s0 = add_state(builder);
    let s1 = add_state(builder);
    assert_eq!(lling_wfst_builder_set_start(builder, s0), LlingStatus::Ok);
    assert_eq!(
        lling_wfst_builder_set_final(builder, s1, f64::INFINITY),
        LlingStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_add_arc(
            builder,
            s0,
            u64::from('a'),
            1,
            u64::from('b'),
            1,
            s1,
            f64::INFINITY
        ),
        LlingStatus::Ok
    );
    let wfst = build(builder);
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(wfst, &mut resource) },
        LlingStatus::Ok
    );
    let (valid, is_final, final_weight) = unsafe { state_info(resource, 1) };
    assert_eq!((valid, is_final), (1, 1));
    assert!(final_weight.is_infinite() && final_weight.is_sign_positive());
    lling_resource_release(resource);
    unsafe {
        lling_wfst_free(wfst);
        lling_wfst_builder_free(builder);
    }
}

#[test]
fn labels_must_be_unicode_scalars() {
    let builder = new_builder();
    let s0 = add_state(builder);
    assert_eq!(lling_wfst_builder_set_start(builder, s0), LlingStatus::Ok);

    let beyond_max = u64::from(u32::from(char::MAX)) + 1; // 0x110000
    let surrogate = 0xD800_u64;
    let beyond_u32 = u64::from(u32::MAX) + 1;
    for bad_label in [beyond_max, surrogate, beyond_u32] {
        assert_eq!(
            lling_wfst_builder_add_arc(builder, s0, bad_label, 1, u64::from('a'), 1, s0, 0.0),
            LlingStatus::InvalidArgument,
            "input label {bad_label:#x} must be rejected"
        );
        assert!(last_error().contains("input label"));
        assert_eq!(
            lling_wfst_builder_add_arc(builder, s0, u64::from('a'), 1, bad_label, 1, s0, 0.0),
            LlingStatus::InvalidArgument,
            "output label {bad_label:#x} must be rejected"
        );
        assert!(last_error().contains("output label"));
    }
    unsafe { lling_wfst_builder_free(builder) };
}

#[test]
fn presence_flags_must_be_zero_or_one() {
    let builder = new_builder();
    let s0 = add_state(builder);
    assert_eq!(lling_wfst_builder_set_start(builder, s0), LlingStatus::Ok);

    for bad_flag in [2_u8, 255_u8] {
        assert_eq!(
            lling_wfst_builder_add_arc(
                builder,
                s0,
                u64::from('a'),
                bad_flag,
                u64::from('a'),
                1,
                s0,
                0.0
            ),
            LlingStatus::InvalidArgument,
            "has_input={bad_flag} must be rejected"
        );
        assert!(last_error().contains("presence flag"));
        assert_eq!(
            lling_wfst_builder_add_arc(
                builder,
                s0,
                u64::from('a'),
                1,
                u64::from('a'),
                bad_flag,
                s0,
                0.0
            ),
            LlingStatus::InvalidArgument,
            "has_output={bad_flag} must be rejected"
        );
    }
    unsafe { lling_wfst_builder_free(builder) };
}

#[test]
fn epsilon_presence_ignores_label_payload() {
    // With a zero presence flag the label word is dead payload; even a value
    // no Unicode scalar could ever occupy is accepted and denotes epsilon.
    let builder = new_builder();
    let s0 = add_state(builder);
    assert_eq!(lling_wfst_builder_set_start(builder, s0), LlingStatus::Ok);
    assert_eq!(
        lling_wfst_builder_add_arc(builder, s0, u64::MAX, 0, u64::MAX, 0, s0, 0.0),
        LlingStatus::Ok
    );
    unsafe { lling_wfst_builder_free(builder) };
}

#[test]
fn build_without_start_reports_invalid_argument_and_restores_builder() {
    let builder = new_builder();
    let s0 = add_state(builder);
    let s1 = add_state(builder);
    assert_eq!(
        lling_wfst_builder_set_final(builder, s1, 0.0),
        LlingStatus::Ok
    );

    let mut wfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_builder_build(builder, &mut wfst),
        LlingStatus::InvalidArgument
    );
    assert!(wfst.is_null(), "failed build must not write a handle");
    assert!(last_error().contains("no start state"));

    // Pinned restore behavior: the failed build hands the graph back, so the
    // builder keeps accepting mutations and a corrected build succeeds.
    assert_eq!(
        lling_wfst_builder_add_arc(builder, s0, u64::from('a'), 1, u64::from('b'), 1, s1, 1.0),
        LlingStatus::Ok
    );
    assert_eq!(lling_wfst_builder_set_start(builder, s0), LlingStatus::Ok);
    let wfst = build(builder);
    unsafe {
        lling_wfst_free(wfst);
        lling_wfst_builder_free(builder);
    }
}

#[test]
fn build_with_null_out_pointer_preserves_builder() {
    // Regression: pointer validation must precede graph extraction; before
    // the fix this NullPointer failure silently consumed the builder and all
    // later calls answered Closed.
    let builder = two_state_builder();
    assert_eq!(
        lling_wfst_builder_build(builder, ptr::null_mut()),
        LlingStatus::NullPointer
    );
    let wfst = build(builder);
    unsafe {
        lling_wfst_free(wfst);
        lling_wfst_builder_free(builder);
    }
}

#[test]
fn operations_after_build_report_closed() {
    let builder = two_state_builder();
    let wfst = build(builder);

    assert_eq!(
        lling_wfst_builder_reserve_states(builder, 4),
        LlingStatus::Closed
    );
    let mut state = 0;
    assert_eq!(
        lling_wfst_builder_add_state(builder, &mut state),
        LlingStatus::Closed
    );
    assert_eq!(
        lling_wfst_builder_set_start(builder, 0),
        LlingStatus::Closed
    );
    assert_eq!(
        lling_wfst_builder_set_final(builder, 0, 0.0),
        LlingStatus::Closed
    );
    assert_eq!(
        lling_wfst_builder_clear_final(builder, 0),
        LlingStatus::Closed
    );
    assert_eq!(
        lling_wfst_builder_add_arc(builder, 0, u64::from('a'), 1, u64::from('a'), 1, 1, 0.0),
        LlingStatus::Closed
    );

    // Double build: the graph was consumed by the first build.
    let mut second = ptr::null_mut();
    assert_eq!(
        lling_wfst_builder_build(builder, &mut second),
        LlingStatus::Closed
    );
    assert!(second.is_null());
    assert!(last_error().contains("already been consumed"));

    // Pinned precedence: argument validation runs before the Closed check,
    // so a NaN weight still answers InvalidArgument on a consumed builder.
    assert_eq!(
        lling_wfst_builder_set_final(builder, 0, f64::NAN),
        LlingStatus::InvalidArgument
    );

    unsafe {
        lling_wfst_free(wfst);
        lling_wfst_builder_free(builder);
    }
}

#[test]
fn free_and_release_accept_null() {
    unsafe {
        lling_wfst_builder_free(ptr::null_mut());
        lling_wfst_free(ptr::null_mut());
    }
    lling_resource_release(VtResource::NULL);
    // A half-null resource (either word) is also a no-op, never a deref.
    lling_resource_release(VtResource {
        context: ptr::null_mut(),
        vtable: ptr::NonNull::<vinary_tree_interop::VtResourceVTable>::dangling().as_ptr(),
    });
}

#[test]
fn null_out_params_report_null_pointer() {
    let builder = new_builder();
    assert_eq!(
        lling_wfst_builder_add_state(builder, ptr::null_mut()),
        LlingStatus::NullPointer
    );
    assert!(last_error().contains("out_state"));
    unsafe { lling_wfst_builder_free(builder) };

    let builder = two_state_builder();
    let wfst = build(builder);
    let mut resource = VtResource::NULL;

    assert_eq!(
        unsafe { lling_wfst_resource(ptr::null(), &mut resource) },
        LlingStatus::NullPointer
    );
    assert_eq!(
        unsafe { lling_wfst_resource(wfst, ptr::null_mut()) },
        LlingStatus::NullPointer
    );

    assert_eq!(
        unsafe { lling_wfst_resource(wfst, &mut resource) },
        LlingStatus::Ok
    );
    assert_eq!(
        lling_wfst_import(resource, ptr::null_mut()),
        LlingStatus::NullPointer
    );
    assert_eq!(
        lling_wfst_compose(resource, resource, ptr::null_mut()),
        LlingStatus::NullPointer
    );

    lling_resource_release(resource);
    unsafe {
        lling_wfst_free(wfst);
        lling_wfst_builder_free(builder);
    }
}

#[test]
fn last_error_is_thread_local() {
    let builder = new_builder();
    // Trigger a distinctive error on this thread.
    assert_eq!(
        lling_wfst_builder_set_start(builder, 42),
        LlingStatus::InvalidArgument
    );
    let main_message = last_error();
    assert!(main_message.contains("start state"));

    let worker_messages = std::thread::spawn(|| {
        // A fresh thread starts from the pristine sentinel...
        let initial = last_error();
        // ...and its own failures write only its own slot.
        let mut builder = ptr::null_mut();
        assert_eq!(lling_wfst_builder_new(&mut builder), LlingStatus::Ok);
        let mut state = 0;
        assert_eq!(
            lling_wfst_builder_add_state(builder, &mut state),
            LlingStatus::Ok
        );
        assert_eq!(
            lling_wfst_builder_set_final(builder, state, f64::NAN),
            LlingStatus::InvalidArgument
        );
        let after = last_error();
        unsafe { lling_wfst_builder_free(builder) };
        (initial, after)
    })
    .join()
    .expect("worker thread must not panic");

    assert_eq!(worker_messages.0, "ok");
    assert!(worker_messages.1.contains("finite or +infinity"));
    // The worker's error never leaked into this thread's slot.
    assert_eq!(last_error(), main_message);
    unsafe { lling_wfst_builder_free(builder) };
}

#[test]
fn clear_final_resets_finality() {
    let builder = new_builder();
    let s0 = add_state(builder);
    assert_eq!(lling_wfst_builder_set_start(builder, s0), LlingStatus::Ok);
    assert_eq!(
        lling_wfst_builder_set_final(builder, s0, 2.5),
        LlingStatus::Ok
    );
    assert_eq!(lling_wfst_builder_clear_final(builder, s0), LlingStatus::Ok);
    let wfst = build(builder);
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(wfst, &mut resource) },
        LlingStatus::Ok
    );
    let (valid, is_final, final_weight) = unsafe { state_info(resource, 0) };
    assert_eq!((valid, is_final), (1, 0));
    assert!(final_weight.is_infinite() && final_weight.is_sign_positive());
    lling_resource_release(resource);
    unsafe {
        lling_wfst_free(wfst);
        lling_wfst_builder_free(builder);
    }
}
