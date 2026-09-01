//! Stable project-owned C ABI for Unicode/tropical lling-llang WFSTs.

mod v2;
pub use v2::*;

use crate::bindings::{BindingError, OwnedWfstResource};
use crate::dynamic_semiring::{
    DynamicSemiringContext, DynamicSemiringError, DynamicSemiringWeight, NaturalOrder,
};
use crate::semiring::TropicalWeight;
use crate::wfst::{MutableWfst, VectorWfst, Wfst, NO_STATE};
use std::cell::RefCell;
use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use vinary_tree_interop::VtResource;

mod lattice;
pub use lattice::*;

/// Stable lling-llang C ABI version.
pub const LLING_ABI_VERSION: u32 = 1;
/// Additive project API revision.
pub const LLING_API_REVISION: u32 = 5;

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
/// Opaque same-thread host-defined semiring operation context.
pub struct LlingSemiring {
    context: DynamicSemiringContext,
}
/// Opaque owned value issued by one [`LlingSemiring`] context.
pub struct LlingSemiringWeight {
    weight: DynamicSemiringWeight,
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

fn map_semiring_error(error: DynamicSemiringError) -> LlingStatus {
    set_error(error.to_string());
    match error {
        DynamicSemiringError::NullResource => LlingStatus::NullPointer,
        DynamicSemiringError::IncompatibleResourceAbi
        | DynamicSemiringError::MissingSemiringInterface
        | DynamicSemiringError::IncompatibleInterface(_)
        | DynamicSemiringError::MissingCapability(_) => LlingStatus::IncompatibleResource,
        DynamicSemiringError::ContextMismatch | DynamicSemiringError::InvalidArgument(_) => {
            LlingStatus::InvalidArgument
        }
        DynamicSemiringError::ResourceLimit => LlingStatus::LimitExceeded,
        DynamicSemiringError::Provider { .. }
        | DynamicSemiringError::InvalidProviderOutput { .. }
        | DynamicSemiringError::WrongThread
        | DynamicSemiringError::ConcurrentCall
        | DynamicSemiringError::LawViolation(_) => LlingStatus::ProviderError,
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

unsafe fn copy_bytes_to_c(
    bytes: &[u8],
    out_bytes: *mut u8,
    capacity: usize,
    out_written: *mut usize,
    out_required: *mut usize,
) -> Result<(), LlingStatus> {
    let written = required_mut(out_written, "out_written")?;
    let required = required_mut(out_required, "out_required")?;
    if capacity != 0 && out_bytes.is_null() {
        set_error("out_bytes is null with nonzero capacity");
        return Err(LlingStatus::NullPointer);
    }
    *required = bytes.len();
    *written = capacity.min(bytes.len());
    if *written != 0 {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_bytes, *written);
    }
    Ok(())
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

fn dynamic_semiring(
    semiring: *const LlingSemiring,
) -> Result<&'static DynamicSemiringContext, LlingStatus> {
    if semiring.is_null() {
        set_error("semiring is null");
        Err(LlingStatus::NullPointer)
    } else {
        // SAFETY: the C caller promises this is a live opaque handle.
        Ok(unsafe { &(*semiring).context })
    }
}

fn dynamic_weight(
    weight: *const LlingSemiringWeight,
) -> Result<&'static DynamicSemiringWeight, LlingStatus> {
    if weight.is_null() {
        set_error("semiring weight is null");
        Err(LlingStatus::NullPointer)
    } else {
        // SAFETY: the C caller promises this is a live opaque handle.
        Ok(unsafe { &(*weight).weight })
    }
}

fn write_dynamic_weight(
    output: *mut *mut LlingSemiringWeight,
    create: impl FnOnce() -> Result<DynamicSemiringWeight, DynamicSemiringError>,
) -> Result<(), LlingStatus> {
    let output = required_mut(output, "out_weight")?;
    let weight = create().map_err(map_semiring_error)?;
    *output = Box::into_raw(Box::new(LlingSemiringWeight { weight }));
    Ok(())
}

fn write_optional_dynamic_weight(
    output: *mut *mut LlingSemiringWeight,
    out_defined: *mut u8,
    create: impl FnOnce() -> Result<Option<DynamicSemiringWeight>, DynamicSemiringError>,
) -> Result<(), LlingStatus> {
    let output = required_mut(output, "out_weight")?;
    let defined = required_mut(out_defined, "out_defined")?;
    match create().map_err(map_semiring_error)? {
        Some(weight) => {
            *output = Box::into_raw(Box::new(LlingSemiringWeight { weight }));
            *defined = 1;
        }
        None => {
            *output = std::ptr::null_mut();
            *defined = 0;
        }
    }
    Ok(())
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

/// Retain and validate a host-defined semiring operation context.
///
/// # Safety
/// `resource` must point to a live `VtResource` for this call. The returned
/// context owns an independent retain and is same-thread unless its provider
/// is explicitly promoted through the Rust API.
#[no_mangle]
pub unsafe extern "C" fn lling_semiring_open(
    resource: *const VtResource,
    out_semiring: *mut *mut LlingSemiring,
) -> LlingStatus {
    boundary(|| {
        let output = required_mut(out_semiring, "out_semiring")?;
        if resource.is_null() {
            set_error("resource is null");
            return Err(LlingStatus::NullPointer);
        }
        *output = std::ptr::null_mut();
        let context = DynamicSemiringContext::borrow_raw(*resource).map_err(map_semiring_error)?;
        *output = Box::into_raw(Box::new(LlingSemiring { context }));
        Ok(())
    })
}

/// Release an imported semiring context. Null is accepted.
///
/// # Safety
/// A non-null pointer must have been returned by [`lling_semiring_open`] and
/// must not already have been freed.
#[no_mangle]
pub unsafe extern "C" fn lling_semiring_free(semiring: *mut LlingSemiring) {
    if !semiring.is_null() {
        drop(Box::from_raw(semiring));
    }
}

/// Release one owned dynamic semiring weight. Null is accepted.
///
/// # Safety
/// A non-null pointer must have been returned by a `lling_semiring_*`
/// constructor or algebra operation and must not already have been freed.
#[no_mangle]
pub unsafe extern "C" fn lling_semiring_weight_free(weight: *mut LlingSemiringWeight) {
    if !weight.is_null() {
        drop(Box::from_raw(weight));
    }
}

/// Return the provider's declared algebraic-property bits.
#[no_mangle]
pub extern "C" fn lling_semiring_properties(
    semiring: *const LlingSemiring,
    out_properties: *mut u64,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let output = required_mut(out_properties, "out_properties")?;
        *output = context.declared_properties();
        Ok(())
    })
}

/// Construct the additive identity.
#[no_mangle]
pub extern "C" fn lling_semiring_zero(
    semiring: *const LlingSemiring,
    out_weight: *mut *mut LlingSemiringWeight,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        write_dynamic_weight(out_weight, || context.zero())
    })
}

/// Construct the multiplicative identity.
#[no_mangle]
pub extern "C" fn lling_semiring_one(
    semiring: *const LlingSemiring,
    out_weight: *mut *mut LlingSemiringWeight,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        write_dynamic_weight(out_weight, || context.one())
    })
}

/// Clone one owned weight through its provider.
#[no_mangle]
pub extern "C" fn lling_semiring_weight_clone(
    weight: *const LlingSemiringWeight,
    out_weight: *mut *mut LlingSemiringWeight,
) -> LlingStatus {
    boundary(|| {
        let weight = dynamic_weight(weight)?;
        write_dynamic_weight(out_weight, || weight.try_clone())
    })
}

/// Add two dynamic weights.
#[no_mangle]
pub extern "C" fn lling_semiring_plus(
    semiring: *const LlingSemiring,
    left: *const LlingSemiringWeight,
    right: *const LlingSemiringWeight,
    out_weight: *mut *mut LlingSemiringWeight,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let left = dynamic_weight(left)?;
        let right = dynamic_weight(right)?;
        write_dynamic_weight(out_weight, || context.plus(left, right))
    })
}

/// Multiply two dynamic weights.
#[no_mangle]
pub extern "C" fn lling_semiring_times(
    semiring: *const LlingSemiring,
    left: *const LlingSemiringWeight,
    right: *const LlingSemiringWeight,
    out_weight: *mut *mut LlingSemiringWeight,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let left = dynamic_weight(left)?;
        let right = dynamic_weight(right)?;
        write_dynamic_weight(out_weight, || context.times(left, right))
    })
}

/// Compare two dynamic weights for exact equality.
#[no_mangle]
pub extern "C" fn lling_semiring_equal(
    semiring: *const LlingSemiring,
    left: *const LlingSemiringWeight,
    right: *const LlingSemiringWeight,
    out_equal: *mut u8,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let left = dynamic_weight(left)?;
        let right = dynamic_weight(right)?;
        let output = required_mut(out_equal, "out_equal")?;
        *output = u8::from(context.equal(left, right).map_err(map_semiring_error)?);
        Ok(())
    })
}

/// Compare two dynamic weights using the provider's natural metric.
#[no_mangle]
pub extern "C" fn lling_semiring_approx_equal(
    semiring: *const LlingSemiring,
    left: *const LlingSemiringWeight,
    right: *const LlingSemiringWeight,
    epsilon: f64,
    out_equal: *mut u8,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let left = dynamic_weight(left)?;
        let right = dynamic_weight(right)?;
        let output = required_mut(out_equal, "out_equal")?;
        *output = u8::from(
            context
                .approx_equal(left, right, epsilon)
                .map_err(map_semiring_error)?,
        );
        Ok(())
    })
}

/// Compare two dynamic weights in natural order.
#[no_mangle]
pub extern "C" fn lling_semiring_natural_order(
    semiring: *const LlingSemiring,
    left: *const LlingSemiringWeight,
    right: *const LlingSemiringWeight,
    out_order: *mut i32,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let left = dynamic_weight(left)?;
        let right = dynamic_weight(right)?;
        let output = required_mut(out_order, "out_order")?;
        *output = match context
            .natural_order(left, right)
            .map_err(map_semiring_error)?
        {
            NaturalOrder::Better => -1,
            NaturalOrder::Equal => 0,
            NaturalOrder::Worse => 1,
            NaturalOrder::Incomparable => 2,
        };
        Ok(())
    })
}

/// Compute right division. `out_defined` is zero for an undefined quotient.
#[no_mangle]
pub extern "C" fn lling_semiring_divide(
    semiring: *const LlingSemiring,
    dividend: *const LlingSemiringWeight,
    divisor: *const LlingSemiringWeight,
    out_weight: *mut *mut LlingSemiringWeight,
    out_defined: *mut u8,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let dividend = dynamic_weight(dividend)?;
        let divisor = dynamic_weight(divisor)?;
        write_optional_dynamic_weight(out_weight, out_defined, || {
            context.divide(dividend, divisor)
        })
    })
}

/// Compute weak left division. `out_defined` is zero when undefined.
#[no_mangle]
pub extern "C" fn lling_semiring_left_divide(
    semiring: *const LlingSemiring,
    value: *const LlingSemiringWeight,
    divisor: *const LlingSemiringWeight,
    out_weight: *mut *mut LlingSemiringWeight,
    out_defined: *mut u8,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let value = dynamic_weight(value)?;
        let divisor = dynamic_weight(divisor)?;
        write_optional_dynamic_weight(out_weight, out_defined, || {
            context.left_divide(value, divisor)
        })
    })
}

/// Compute Kleene closure. `out_defined` is zero when closure diverges.
#[no_mangle]
pub extern "C" fn lling_semiring_star(
    semiring: *const LlingSemiring,
    value: *const LlingSemiringWeight,
    out_weight: *mut *mut LlingSemiringWeight,
    out_defined: *mut u8,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let value = dynamic_weight(value)?;
        write_optional_dynamic_weight(out_weight, out_defined, || context.star(value))
    })
}

/// Extract the numerical projection of one dynamic weight.
#[no_mangle]
pub extern "C" fn lling_semiring_numerical_value(
    semiring: *const LlingSemiring,
    value: *const LlingSemiringWeight,
    out_value: *mut f64,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let value = dynamic_weight(value)?;
        let output = required_mut(out_value, "out_value")?;
        *output = context.numerical_value(value).map_err(map_semiring_error)?;
        Ok(())
    })
}

/// Quantize one dynamic weight.
#[no_mangle]
pub extern "C" fn lling_semiring_quantize(
    semiring: *const LlingSemiring,
    value: *const LlingSemiringWeight,
    epsilon: f64,
    out_value: *mut i64,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let value = dynamic_weight(value)?;
        let output = required_mut(out_value, "out_value")?;
        *output = context
            .quantize(value, epsilon)
            .map_err(map_semiring_error)?;
        Ok(())
    })
}

/// Convert one dynamic weight to a sampling probability.
#[no_mangle]
pub extern "C" fn lling_semiring_to_probability(
    semiring: *const LlingSemiring,
    value: *const LlingSemiringWeight,
    out_value: *mut f64,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let value = dynamic_weight(value)?;
        let output = required_mut(out_value, "out_value")?;
        *output = context.to_probability(value).map_err(map_semiring_error)?;
        Ok(())
    })
}

/// Return the optional uniform closure bound.
#[no_mangle]
pub extern "C" fn lling_semiring_closure_bound(
    semiring: *const LlingSemiring,
    out_bound: *mut usize,
    out_known: *mut u8,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let bound = required_mut(out_bound, "out_bound")?;
        let known = required_mut(out_known, "out_known")?;
        match context.closure_bound().map_err(map_semiring_error)? {
            Some(value) => {
                *bound = value;
                *known = 1;
            }
            None => {
                *bound = 0;
                *known = 0;
            }
        }
        Ok(())
    })
}

/// Copy canonical value bytes into caller-owned storage.
///
/// # Safety
/// When `capacity` is nonzero, `out_bytes` must point to at least `capacity`
/// writable bytes. Every other non-null handle and output pointer must be live
/// for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_semiring_stable_bytes(
    semiring: *const LlingSemiring,
    value: *const LlingSemiringWeight,
    out_bytes: *mut u8,
    capacity: usize,
    out_written: *mut usize,
    out_required: *mut usize,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let value = dynamic_weight(value)?;
        let bytes = context.stable_bytes(value).map_err(map_semiring_error)?;
        // SAFETY: the C caller promises `capacity` writable bytes.
        unsafe { copy_bytes_to_c(&bytes, out_bytes, capacity, out_written, out_required) }
    })
}

/// Validate base axioms and declared laws over borrowed representative values.
///
/// # Safety
/// For nonzero `count`, `weights` must point to `count` live weight handles.
#[no_mangle]
pub unsafe extern "C" fn lling_semiring_validate_laws(
    semiring: *const LlingSemiring,
    weights: *const *const LlingSemiringWeight,
    count: usize,
    epsilon: f64,
) -> LlingStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        if count != 0 && weights.is_null() {
            set_error("weights is null with nonzero count");
            return Err(LlingStatus::NullPointer);
        }
        let pointers = if count == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(weights, count)
        };
        let mut owned = Vec::with_capacity(pointers.len());
        for pointer in pointers {
            owned.push(
                dynamic_weight(*pointer)?
                    .try_clone()
                    .map_err(map_semiring_error)?,
            );
        }
        context
            .validate_declared_laws(&owned, epsilon)
            .map_err(map_semiring_error)
    })
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
        // Validate the out-pointer BEFORE mutating the graph: adding the state
        // first meant a null `out_state` left an orphan state in the builder
        // while still returning NullPointer. Pointer validation must never
        // mutate caller state (mirrors the build/out_wfst discipline). Builder
        // validity is still checked first, preserving the builder -> out
        // precedence.
        let graph = graph(builder)?;
        let output = required_mut(out_state, "out_state")?;
        *output = graph.add_state();
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
        // Validate the out-pointer BEFORE materializing the import: assignment
        // evaluates its right operand first, so a null `out_wfst` would leak the
        // fully-built LlingWfst and its captured resource retain. Pointer
        // validation must never leak caller-visible resources (mirrors the
        // build/out_wfst discipline).
        let output = required_mut(out_wfst, "out_wfst")?;
        let graph = crate::bindings::import_tropical_wfst(resource).map_err(map_error)?;
        *output = Box::into_raw(Box::new(LlingWfst {
            resource: OwnedWfstResource::from_wfst(graph),
        }));
        Ok(())
    })
}

/// Pointer-form import for FFIs that cannot pass C aggregates by value.
///
/// # Safety
/// `resource` must be null or point to a readable `VtResource` for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_import_ref(
    resource: *const VtResource,
    out_wfst: *mut *mut LlingWfst,
) -> LlingStatus {
    if resource.is_null() {
        set_error("resource is null");
        return LlingStatus::NullPointer;
    }
    lling_wfst_import(unsafe { *resource }, out_wfst)
}

/// Lazily compose two captured scalar-WFST resources.
#[no_mangle]
pub extern "C" fn lling_wfst_compose(
    first: VtResource,
    second: VtResource,
    out_wfst: *mut *mut LlingWfst,
) -> LlingStatus {
    boundary(|| {
        // Validate the out-pointer BEFORE composing: assignment evaluates its
        // right operand first, so a null `out_wfst` would leak the composition
        // handle together with both captured snapshot retains it holds. Checking
        // the pointer first also avoids the two retains entirely on that path.
        let output = required_mut(out_wfst, "out_wfst")?;
        let resource = OwnedWfstResource::compose(first, second).map_err(map_error)?;
        *output = Box::into_raw(Box::new(LlingWfst { resource }));
        Ok(())
    })
}

/// Pointer-form composition for FFIs that cannot pass C aggregates by value.
///
/// # Safety
/// `first` and `second` must be null or point to readable `VtResource` values
/// for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_compose_refs(
    first: *const VtResource,
    second: *const VtResource,
    out_wfst: *mut *mut LlingWfst,
) -> LlingStatus {
    if first.is_null() {
        set_error("first resource is null");
        return LlingStatus::NullPointer;
    }
    if second.is_null() {
        set_error("second resource is null");
        return LlingStatus::NullPointer;
    }
    lling_wfst_compose(unsafe { *first }, unsafe { *second }, out_wfst)
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

fn checked_v2_pointer<T>(pointer: *const T, name: &'static str) -> Result<(), LlingStatus> {
    if pointer.is_null() {
        set_error(format!("{name} is null"));
        return Err(LlingStatus::NullPointer);
    }
    if (pointer as usize) % std::mem::align_of::<T>() != 0 {
        set_error(format!("{name} is misaligned"));
        return Err(LlingStatus::InvalidArgument);
    }
    Ok(())
}

fn required_v2_ref<'a, T>(pointer: *const T, name: &'static str) -> Result<&'a T, LlingStatus> {
    checked_v2_pointer(pointer, name)?;
    Ok(unsafe { &*pointer })
}

fn required_v2_mut<'a, T>(pointer: *mut T, name: &'static str) -> Result<&'a mut T, LlingStatus> {
    checked_v2_pointer(pointer.cast_const(), name)?;
    Ok(unsafe { &mut *pointer })
}

fn read_v2_struct<T: Copy>(
    pointer: *const T,
    name: &'static str,
    known_flags: u64,
) -> Result<T, LlingStatus> {
    checked_v2_pointer(pointer, name)?;
    let header = unsafe { pointer.cast::<LlingAbiV2Header>().read() };
    if !validate_abi_v2_header(&header, std::mem::size_of::<T>(), known_flags) {
        set_error(format!("{name} has an invalid ABI-v2 header"));
        return Err(LlingStatus::InvalidArgument);
    }
    Ok(unsafe { pointer.read() })
}

fn decode_v2_bool(raw: u8, name: &'static str) -> Result<bool, LlingStatus> {
    match raw {
        0 => Ok(false),
        1 => Ok(true),
        _ => {
            set_error(format!("{name} must be zero or one"));
            Err(LlingStatus::InvalidArgument)
        }
    }
}

/// Validate a typed ABI-v2 header and its additive known prefix.
#[no_mangle]
pub extern "C" fn lling_abi_v2_validate_header(
    header: *const LlingAbiV2Header,
    required_size: u32,
    known_flags: u64,
) -> LlingStatus {
    boundary(|| {
        let header = *required_v2_ref(header, "header")?;
        if !validate_abi_v2_header(&header, required_size as usize, known_flags) {
            set_error("header is not a canonical ABI-v2 prefix");
            return Err(LlingStatus::InvalidArgument);
        }
        Ok(())
    })
}

/// Validate a WFST descriptor and report whether typed evidence is admissible.
#[no_mangle]
pub extern "C" fn lling_abi_v2_validate_descriptor(
    descriptor: *const LlingWfstDescriptorV2,
    out_typed_evidence_allowed: *mut u8,
) -> LlingStatus {
    boundary(|| {
        let descriptor = read_v2_struct(
            descriptor,
            "descriptor",
            LLING_DESCRIPTOR_SIGNATURE_KNOWN
                | LLING_DESCRIPTOR_SNAPSHOT_PRESENT
                | LLING_DESCRIPTOR_CONTEXT_PRESENT,
        )?;
        let output = required_v2_mut(out_typed_evidence_allowed, "out_typed_evidence_allowed")?;
        if !validate_descriptor_v2(&descriptor) {
            set_error("descriptor fields and presence flags are not canonical");
            return Err(LlingStatus::InvalidArgument);
        }
        *output = u8::from(abi_v2_typed_evidence_allowed(&descriptor));
        Ok(())
    })
}

/// Validate a canonical ABI-v2 resource budget.
#[no_mangle]
pub extern "C" fn lling_abi_v2_validate_budget(budget: *const LlingBudgetV2) -> LlingStatus {
    boundary(|| {
        let budget = read_v2_struct(
            budget,
            "budget",
            LLING_BUDGET_STATES | LLING_BUDGET_ARCS | LLING_BUDGET_BYTES | LLING_BUDGET_WORK,
        )?;
        if !validate_budget_v2(&budget) {
            set_error("budget flags, limits, or reserved fields are not canonical");
            return Err(LlingStatus::InvalidArgument);
        }
        Ok(())
    })
}

/// Validate an outcome and report whether it is authoritative and exact.
#[no_mangle]
pub extern "C" fn lling_abi_v2_validate_outcome(
    outcome: *const LlingOutcomeV2,
    resource_present: u8,
    evidence_present: u8,
    out_authoritative_exact: *mut u8,
) -> LlingStatus {
    boundary(|| {
        let outcome = read_v2_struct(outcome, "outcome", 0)?;
        let resource_present = decode_v2_bool(resource_present, "resource_present")?;
        let evidence_present = decode_v2_bool(evidence_present, "evidence_present")?;
        let output = required_v2_mut(out_authoritative_exact, "out_authoritative_exact")?;
        if !validate_outcome_v2(&outcome, resource_present, evidence_present) {
            set_error("outcome axes or publication state are not canonical");
            return Err(LlingStatus::InvalidArgument);
        }
        *output = u8::from(abi_v2_authoritative_exact(&outcome, evidence_present));
        Ok(())
    })
}

/// Compare the replay-critical tape, algebra, snapshot, and context identities.
#[no_mangle]
pub extern "C" fn lling_abi_v2_identity_matches(
    expected: *const LlingWfstDescriptorV2,
    observed: *const LlingWfstDescriptorV2,
    out_matches: *mut u8,
) -> LlingStatus {
    boundary(|| {
        let known_flags = LLING_DESCRIPTOR_SIGNATURE_KNOWN
            | LLING_DESCRIPTOR_SNAPSHOT_PRESENT
            | LLING_DESCRIPTOR_CONTEXT_PRESENT;
        let expected = read_v2_struct(expected, "expected", known_flags)?;
        let observed = read_v2_struct(observed, "observed", known_flags)?;
        let output = required_v2_mut(out_matches, "out_matches")?;
        if !validate_descriptor_v2(&expected) || !validate_descriptor_v2(&observed) {
            set_error("identity comparison requires canonical descriptors");
            return Err(LlingStatus::InvalidArgument);
        }
        *output = u8::from(abi_v2_identity_matches(&expected, &observed));
        Ok(())
    })
}

/// Allocate a live cooperative-cancellation handle.
#[no_mangle]
pub extern "C" fn lling_cancellation_v2_new(
    out_cancellation: *mut *mut LlingCancellationV2,
) -> LlingStatus {
    boundary(|| {
        let output = required_v2_mut(out_cancellation, "out_cancellation")?;
        if !output.is_null() {
            set_error("out_cancellation must initially be null");
            return Err(LlingStatus::InvalidArgument);
        }
        let layout = std::alloc::Layout::new::<LlingCancellationV2>();
        let allocation = unsafe { std::alloc::alloc(layout) }.cast::<LlingCancellationV2>();
        if allocation.is_null() {
            set_error("unable to allocate cancellation handle");
            return Err(LlingStatus::LimitExceeded);
        }
        unsafe { allocation.write(LlingCancellationV2::new()) };
        *output = allocation;
        Ok(())
    })
}

/// Request cancellation; the first valid reason remains sticky.
#[no_mangle]
pub extern "C" fn lling_cancellation_v2_request(
    cancellation: *const LlingCancellationV2,
    reason: u32,
) -> LlingStatus {
    boundary(|| {
        let cancellation = required_v2_ref(cancellation, "cancellation")?;
        let reason = LlingCancellationReasonV2::from_raw(reason).ok_or_else(|| {
            set_error("cancellation reason is not a known wire discriminant");
            LlingStatus::InvalidArgument
        })?;
        cancellation.request(reason);
        Ok(())
    })
}

/// Read zero for a live handle or its first cancellation reason.
#[no_mangle]
pub extern "C" fn lling_cancellation_v2_reason(
    cancellation: *const LlingCancellationV2,
    out_reason: *mut u32,
) -> LlingStatus {
    boundary(|| {
        let cancellation = required_v2_ref(cancellation, "cancellation")?;
        let output = required_v2_mut(out_reason, "out_reason")?;
        *output = cancellation.reason();
        Ok(())
    })
}

/// Release a cancellation handle exactly once and null the caller's slot.
#[no_mangle]
pub extern "C" fn lling_cancellation_v2_free(
    cancellation: *mut *mut LlingCancellationV2,
) -> LlingStatus {
    boundary(|| {
        let slot = required_v2_mut(cancellation, "cancellation")?;
        if slot.is_null() {
            set_error("cancellation handle has already been released");
            return Err(LlingStatus::Closed);
        }
        checked_v2_pointer((*slot).cast_const(), "*cancellation")?;
        let owned = *slot;
        *slot = std::ptr::null_mut();
        unsafe { drop(Box::from_raw(owned)) };
        Ok(())
    })
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
