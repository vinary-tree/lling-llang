//! Stable project-owned C ABI for Unicode/tropical lling-llang WFSTs.

use crate::bindings::{BindingError, OwnedWfstResource};
use crate::semiring::TropicalWeight;
use crate::wfst::{MutableWfst, VectorWfst, Wfst, NO_STATE};
use std::cell::RefCell;
use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use vinary_tree_interop::VtResource;

/// Stable lling-llang C ABI version.
pub const LLING_ABI_VERSION: u32 = 1;
/// Additive project API revision.
pub const LLING_API_REVISION: u32 = 1;

/// Status returned by lling-llang C functions.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LlingStatus {
    /// Operation completed successfully.
    Ok = 0,
    /// An argument was invalid.
    InvalidArgument = 1,
    /// A required pointer was null.
    NullPointer = 2,
    /// A Rust panic was caught at the ABI boundary.
    Panic = 3,
    /// A resource did not expose a compatible scalar-WFST interface.
    IncompatibleResource = 4,
    /// A foreign provider callback failed.
    ProviderError = 5,
    /// A label/state/count exceeded the native representation.
    LimitExceeded = 6,
    /// The builder was already consumed.
    Closed = 7,
}

/// Opaque mutable WFST builder.
pub struct LlingWfstBuilder {
    graph: Option<VectorWfst<char, TropicalWeight>>,
}
/// Opaque immutable scalar-WFST handle.
pub struct LlingWfst {
    resource: OwnedWfstResource,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::new("ok").expect("literal has no NUL"));
}

fn set_error(message: impl Into<String>) {
    let message = message.into().replace('\0', "\\0");
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = CString::new(message)
            .unwrap_or_else(|_| CString::new("invalid error message").unwrap());
    });
}

fn map_error(error: BindingError) -> LlingStatus {
    set_error(error.to_string());
    match error {
        BindingError::Provider(_) | BindingError::InvalidProviderOutput(_) => {
            LlingStatus::ProviderError
        }
        BindingError::RepresentationLimit => LlingStatus::LimitExceeded,
        BindingError::NullResource => LlingStatus::NullPointer,
        BindingError::IncompatibleResourceAbi
        | BindingError::MissingWfstInterface
        | BindingError::IncompatibleWfstInterface
        | BindingError::UnitDomainMismatch(_)
        | BindingError::WeightDomainMismatch(_) => LlingStatus::IncompatibleResource,
    }
}

fn boundary(operation: impl FnOnce() -> Result<(), LlingStatus>) -> LlingStatus {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => LlingStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            set_error("panic caught at lling-llang C boundary");
            LlingStatus::Panic
        }
    }
}

fn required_mut<'a, T>(pointer: *mut T, name: &'static str) -> Result<&'a mut T, LlingStatus> {
    if pointer.is_null() {
        set_error(format!("{name} is null"));
        Err(LlingStatus::NullPointer)
    } else {
        Ok(unsafe { &mut *pointer })
    }
}

fn graph(
    builder: *mut LlingWfstBuilder,
) -> Result<&'static mut VectorWfst<char, TropicalWeight>, LlingStatus> {
    required_mut(builder, "builder")?
        .graph
        .as_mut()
        .ok_or_else(|| {
            set_error("builder has already been consumed");
            LlingStatus::Closed
        })
}

/// Return the project C ABI version.
#[no_mangle]
pub extern "C" fn lling_abi_version() -> u32 {
    LLING_ABI_VERSION
}

/// Return the additive project API revision.
#[no_mangle]
pub extern "C" fn lling_api_revision() -> u32 {
    LLING_API_REVISION
}

/// Return this thread's last error message.
#[no_mangle]
pub extern "C" fn lling_last_error_message() -> *const c_char {
    LAST_ERROR.with(|slot| slot.borrow().as_ptr())
}

/// Allocate an empty Unicode/tropical WFST builder.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_new(out_builder: *mut *mut LlingWfstBuilder) -> LlingStatus {
    boundary(|| {
        let output = required_mut(out_builder, "out_builder")?;
        *output = Box::into_raw(Box::new(LlingWfstBuilder {
            graph: Some(VectorWfst::new()),
        }));
        Ok(())
    })
}

/// Free a builder. Null is accepted.
///
/// # Safety
/// A non-null pointer must have been returned by `lling_wfst_builder_new` and
/// must not already have been freed.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_builder_free(builder: *mut LlingWfstBuilder) {
    if !builder.is_null() {
        unsafe {
            drop(Box::from_raw(builder));
        }
    }
}

/// Reserve state capacity.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_reserve_states(
    builder: *mut LlingWfstBuilder,
    additional: usize,
) -> LlingStatus {
    boundary(|| {
        graph(builder)?.reserve_states(additional);
        Ok(())
    })
}

/// Add one state and return its compact state ID.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_add_state(
    builder: *mut LlingWfstBuilder,
    out_state: *mut u32,
) -> LlingStatus {
    boundary(|| {
        let state = graph(builder)?.add_state();
        *required_mut(out_state, "out_state")? = state;
        Ok(())
    })
}

/// Set the initial state.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_set_start(
    builder: *mut LlingWfstBuilder,
    state: u32,
) -> LlingStatus {
    boundary(|| {
        let graph = graph(builder)?;
        if !graph.try_set_start(state) {
            set_error("start state is not present in the builder");
            return Err(LlingStatus::InvalidArgument);
        }
        Ok(())
    })
}

/// Set a final state and its tropical weight.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_set_final(
    builder: *mut LlingWfstBuilder,
    state: u32,
    weight: f64,
) -> LlingStatus {
    boundary(|| {
        // Builder-surface twin of finding LLING-B2/F1: the tropical domain is
        // finite-or-+inf only, so -inf must be rejected exactly like NaN
        // (previously it slipped the is_nan check and panicked inside
        // TropicalWeight::new, surfacing as LLING_STATUS_PANIC).
        if !TropicalWeight::is_valid_raw(weight) {
            set_error("weight must be a finite or +infinity tropical value");
            return Err(LlingStatus::InvalidArgument);
        }
        let graph = graph(builder)?;
        let state = graph.state_mut(state).ok_or_else(|| {
            set_error("final state is not present in the builder");
            LlingStatus::InvalidArgument
        })?;
        state.is_final = true;
        state.final_weight = TropicalWeight::new(weight);
        Ok(())
    })
}

/// Clear a state's final status.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_clear_final(
    builder: *mut LlingWfstBuilder,
    state: u32,
) -> LlingStatus {
    boundary(|| {
        let state = graph(builder)?.state_mut(state).ok_or_else(|| {
            set_error("state is not present in the builder");
            LlingStatus::InvalidArgument
        })?;
        state.is_final = false;
        state.final_weight = TropicalWeight::new(f64::INFINITY);
        Ok(())
    })
}

fn decode_label(value: u64, present: u8, name: &'static str) -> Result<Option<char>, LlingStatus> {
    match present {
        0 => Ok(None),
        1 => u32::try_from(value)
            .ok()
            .and_then(char::from_u32)
            .map(Some)
            .ok_or_else(|| {
                set_error(format!("{name} is not a Unicode scalar"));
                LlingStatus::InvalidArgument
            }),
        _ => {
            set_error(format!("{name} presence flag must be zero or one"));
            Err(LlingStatus::InvalidArgument)
        }
    }
}

/// Add a Unicode/tropical arc. `has_input`/`has_output` zero denotes epsilon.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_add_arc(
    builder: *mut LlingWfstBuilder,
    from: u32,
    input_label: u64,
    has_input: u8,
    output_label: u64,
    has_output: u8,
    to: u32,
    weight: f64,
) -> LlingStatus {
    boundary(|| {
        // Builder-surface twin of finding LLING-B2/F1 (see set_final above).
        if !TropicalWeight::is_valid_raw(weight) {
            set_error("weight must be a finite or +infinity tropical value");
            return Err(LlingStatus::InvalidArgument);
        }
        let input = decode_label(input_label, has_input, "input label")?;
        let output = decode_label(output_label, has_output, "output label")?;
        let graph = graph(builder)?;
        if !graph.is_valid_state(from) || !graph.is_valid_state(to) {
            set_error("arc source or target state is not present in the builder");
            return Err(LlingStatus::InvalidArgument);
        }
        graph.add_arc(from, input, output, to, TropicalWeight::new(weight));
        Ok(())
    })
}

/// Consume a builder and freeze it into an immutable resource handle.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_build(
    builder: *mut LlingWfstBuilder,
    out_wfst: *mut *mut LlingWfst,
) -> LlingStatus {
    boundary(|| {
        let builder = required_mut(builder, "builder")?;
        // Validate the out-pointer BEFORE taking the graph: taking first meant
        // a NullPointer failure silently consumed the builder (the graph was
        // dropped and every later call answered Closed). Pointer validation
        // must never destroy caller state.
        let output = required_mut(out_wfst, "out_wfst")?;
        let graph = builder.graph.take().ok_or_else(|| {
            set_error("builder has already been consumed");
            LlingStatus::Closed
        })?;
        if graph.start() == NO_STATE {
            builder.graph = Some(graph);
            set_error("WFST has no start state");
            return Err(LlingStatus::InvalidArgument);
        }
        *output = Box::into_raw(Box::new(LlingWfst {
            resource: OwnedWfstResource::from_wfst(graph),
        }));
        Ok(())
    })
}

/// Free an immutable WFST handle. Null is accepted.
///
/// # Safety
/// A non-null pointer must have been returned by this API and must not already
/// have been freed.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_free(wfst: *mut LlingWfst) {
    if !wfst.is_null() {
        unsafe {
            drop(Box::from_raw(wfst));
        }
    }
}

/// Import any compatible scalar-WFST resource as an independently owned handle.
#[no_mangle]
pub extern "C" fn lling_wfst_import(
    resource: VtResource,
    out_wfst: *mut *mut LlingWfst,
) -> LlingStatus {
    boundary(|| {
        let graph = crate::bindings::import_tropical_wfst(resource).map_err(map_error)?;
        *required_mut(out_wfst, "out_wfst")? = Box::into_raw(Box::new(LlingWfst {
            resource: OwnedWfstResource::from_wfst(graph),
        }));
        Ok(())
    })
}

/// Lazily compose two captured scalar-WFST resources.
#[no_mangle]
pub extern "C" fn lling_wfst_compose(
    first: VtResource,
    second: VtResource,
    out_wfst: *mut *mut LlingWfst,
) -> LlingStatus {
    boundary(|| {
        let resource = OwnedWfstResource::compose(first, second).map_err(map_error)?;
        *required_mut(out_wfst, "out_wfst")? = Box::into_raw(Box::new(LlingWfst { resource }));
        Ok(())
    })
}

/// Return a new owned resource retain for a WFST handle.
///
/// # Safety
/// `wfst` must point to a live handle returned by this API and `out_resource`
/// must be writable when non-null.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_resource(
    wfst: *const LlingWfst,
    out_resource: *mut VtResource,
) -> LlingStatus {
    boundary(|| {
        if wfst.is_null() {
            set_error("wfst is null");
            return Err(LlingStatus::NullPointer);
        }
        let output = required_mut(out_resource, "out_resource")?;
        *output = unsafe { &*wfst }.resource.clone().into_raw();
        Ok(())
    })
}

/// Release an owned resource obtained from this or another Vinary Tree API.
#[no_mangle]
pub extern "C" fn lling_resource_release(resource: VtResource) {
    if resource.context.is_null() || resource.vtable.is_null() {
        return;
    }
    unsafe {
        if let Some(release) = (*resource.vtable).release {
            release(resource.context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;
    use vinary_tree_interop::{
        VtWfstArc, VtWfstVTable, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
    };

    #[test]
    fn c_builder_exports_batched_resource_arcs() {
        let mut builder = ptr::null_mut();
        assert_eq!(lling_wfst_builder_new(&mut builder), LlingStatus::Ok);
        let mut s0 = 0;
        let mut s1 = 0;
        assert_eq!(
            lling_wfst_builder_add_state(builder, &mut s0),
            LlingStatus::Ok
        );
        assert_eq!(
            lling_wfst_builder_add_state(builder, &mut s1),
            LlingStatus::Ok
        );
        assert_eq!(lling_wfst_builder_set_start(builder, s0), LlingStatus::Ok);
        assert_eq!(
            lling_wfst_builder_set_final(builder, s1, 0.0),
            LlingStatus::Ok
        );
        assert_eq!(
            lling_wfst_builder_add_arc(builder, s0, 'a' as u64, 1, 'b' as u64, 1, s1, 0.25),
            LlingStatus::Ok
        );
        let mut wfst = ptr::null_mut();
        assert_eq!(
            lling_wfst_builder_build(builder, &mut wfst),
            LlingStatus::Ok
        );
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(wfst, &mut resource) },
            LlingStatus::Ok
        );
        unsafe {
            lling_wfst_free(wfst);
            lling_wfst_builder_free(builder);
        }

        unsafe {
            let mut interface = ptr::null();
            assert_eq!(
                (*resource.vtable).query_interface.unwrap()(
                    resource.context,
                    &VT_WFST_INTERFACE_ID,
                    VT_WFST_INTERFACE_VERSION,
                    &mut interface
                ),
                vinary_tree_interop::VtStatus::Ok.to_raw()
            );
            let table = &*interface.cast::<VtWfstVTable>();
            let mut arc = VtWfstArc::default();
            let mut written = 0;
            let mut total = 0;
            assert_eq!(
                table.state_arcs.unwrap()(
                    resource.context,
                    0,
                    0,
                    &mut arc,
                    1,
                    &mut written,
                    &mut total
                ),
                vinary_tree_interop::VtStatus::Ok.to_raw()
            );
            assert_eq!((written, total, arc.output_label), (1, 1, 'b' as u64));
        }
        lling_resource_release(resource);
    }
}
