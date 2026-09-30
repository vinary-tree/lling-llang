//! Stable project-owned C ABI for scalar lling-llang WFSTs.

mod v2;
pub use v2::*;

use crate::bindings::{
    export_native_lazy_wfst, export_native_wfst, import_native_wfst_with_budget_and_stats,
    valid_scalar_label, valid_scalar_weight, wfst_domains, AbiScalarLabel, AbiScalarWeight,
    BindingError, GraphBudget, OwnedWfstResource, ScalarWfstGraph,
};
use crate::dynamic_semiring::{
    DynamicSemiringContext, DynamicSemiringError, DynamicSemiringWeight, NaturalOrder,
};
use crate::semiring::{
    ArcticWeight, BoolWeight, CountWeight, LogWeight, ProbabilityWeight, SignedTropicalWeight,
    TropicalWeight,
};
use crate::wfst::{
    rational::{ClosurePlusSource, ClosureSource, ConcatSource, UnionSource},
    unary::reverse,
    unary::ProjectSource,
    VectorWfst, NO_STATE,
};
use std::cell::RefCell;
use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use vinary_tree_interop::{VtResource, VtStatus, VtUnitDomain, VtWeightDomain, VtWfstArc};

mod lattice;
pub use lattice::*;

/// Stable lling-llang C ABI version.
pub const LLING_ABI_VERSION: u32 = 1;
/// Additive project API revision.
pub const LLING_LLANG_API_REVISION: u32 = 11;

/// Status returned by lling-llang C functions.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LlingLlangStatus {
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
    graph: Option<ScalarWfstGraph>,
    unit_domain: VtUnitDomain,
    weight_domain: VtWeightDomain,
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

fn map_error(error: BindingError) -> LlingLlangStatus {
    set_error(error.to_string());
    match error {
        BindingError::Provider(VtStatus::LimitExceeded) => LlingLlangStatus::LimitExceeded,
        BindingError::Provider(VtStatus::Closed) => LlingLlangStatus::Closed,
        BindingError::Provider(_) | BindingError::InvalidProviderOutput(_) => {
            LlingLlangStatus::ProviderError
        }
        BindingError::RepresentationLimit | BindingError::BudgetExceeded(_) => {
            LlingLlangStatus::LimitExceeded
        }
        BindingError::NullResource => LlingLlangStatus::NullPointer,
        BindingError::IncompatibleResourceAbi
        | BindingError::MissingWfstInterface
        | BindingError::IncompatibleWfstInterface
        | BindingError::UnitDomainMismatch(_)
        | BindingError::WeightDomainMismatch(_) => LlingLlangStatus::IncompatibleResource,
    }
}

fn map_semiring_error(error: DynamicSemiringError) -> LlingLlangStatus {
    set_error(error.to_string());
    match error {
        DynamicSemiringError::NullResource => LlingLlangStatus::NullPointer,
        DynamicSemiringError::IncompatibleResourceAbi
        | DynamicSemiringError::MissingSemiringInterface
        | DynamicSemiringError::IncompatibleInterface(_)
        | DynamicSemiringError::MissingCapability(_) => LlingLlangStatus::IncompatibleResource,
        DynamicSemiringError::ContextMismatch | DynamicSemiringError::InvalidArgument(_) => {
            LlingLlangStatus::InvalidArgument
        }
        DynamicSemiringError::ResourceLimit => LlingLlangStatus::LimitExceeded,
        DynamicSemiringError::Provider { .. }
        | DynamicSemiringError::InvalidProviderOutput { .. }
        | DynamicSemiringError::WrongThread
        | DynamicSemiringError::ConcurrentCall
        | DynamicSemiringError::LawViolation(_) => LlingLlangStatus::ProviderError,
    }
}

fn boundary(operation: impl FnOnce() -> Result<(), LlingLlangStatus>) -> LlingLlangStatus {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => LlingLlangStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            set_error("panic caught at lling-llang C boundary");
            LlingLlangStatus::Panic
        }
    }
}

fn required_mut<'a, T>(pointer: *mut T, name: &'static str) -> Result<&'a mut T, LlingLlangStatus> {
    if pointer.is_null() {
        set_error(format!("{name} is null"));
        Err(LlingLlangStatus::NullPointer)
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
) -> Result<(), LlingLlangStatus> {
    let written = required_mut(out_written, "out_written")?;
    let required = required_mut(out_required, "out_required")?;
    if capacity != 0 && out_bytes.is_null() {
        set_error("out_bytes is null with nonzero capacity");
        return Err(LlingLlangStatus::NullPointer);
    }
    *required = bytes.len();
    *written = capacity.min(bytes.len());
    if *written != 0 {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_bytes, *written);
    }
    Ok(())
}

fn graph(builder: *mut LlingWfstBuilder) -> Result<&'static mut ScalarWfstGraph, LlingLlangStatus> {
    required_mut(builder, "builder")?
        .graph
        .as_mut()
        .ok_or_else(|| {
            set_error("builder has already been consumed");
            LlingLlangStatus::Closed
        })
}

fn dynamic_semiring(
    semiring: *const LlingSemiring,
) -> Result<&'static DynamicSemiringContext, LlingLlangStatus> {
    if semiring.is_null() {
        set_error("semiring is null");
        Err(LlingLlangStatus::NullPointer)
    } else {
        // SAFETY: the C caller promises this is a live opaque handle.
        Ok(unsafe { &(*semiring).context })
    }
}

fn dynamic_weight(
    weight: *const LlingSemiringWeight,
) -> Result<&'static DynamicSemiringWeight, LlingLlangStatus> {
    if weight.is_null() {
        set_error("semiring weight is null");
        Err(LlingLlangStatus::NullPointer)
    } else {
        // SAFETY: the C caller promises this is a live opaque handle.
        Ok(unsafe { &(*weight).weight })
    }
}

fn write_dynamic_weight(
    output: *mut *mut LlingSemiringWeight,
    create: impl FnOnce() -> Result<DynamicSemiringWeight, DynamicSemiringError>,
) -> Result<(), LlingLlangStatus> {
    let output = required_mut(output, "out_weight")?;
    let weight = create().map_err(map_semiring_error)?;
    *output = Box::into_raw(Box::new(LlingSemiringWeight { weight }));
    Ok(())
}

fn write_optional_dynamic_weight(
    output: *mut *mut LlingSemiringWeight,
    out_defined: *mut u8,
    create: impl FnOnce() -> Result<Option<DynamicSemiringWeight>, DynamicSemiringError>,
) -> Result<(), LlingLlangStatus> {
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
pub extern "C" fn lling_llang_api_revision() -> u32 {
    LLING_LLANG_API_REVISION
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
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_semiring, "out_semiring")?;
        if resource.is_null() {
            set_error("resource is null");
            return Err(LlingLlangStatus::NullPointer);
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let value = dynamic_weight(value)?;
        let bytes = context.stable_bytes(value).map_err(map_semiring_error)?;
        // SAFETY: the C caller promises `capacity` writable bytes.
        unsafe { copy_bytes_to_c(&bytes, out_bytes, capacity, out_written, out_required) }
    })
}

/// Copy the provider's advisory UTF-8 diagnostic into caller-owned storage.
///
/// Passing a null `value` asks for a context-level diagnostic. The buffer
/// protocol matches [`lling_semiring_stable_bytes`].
///
/// # Safety
/// When `capacity` is nonzero, `out_bytes` must point to at least `capacity`
/// writable bytes. Every other non-null pointer must remain live for the call.
#[no_mangle]
pub unsafe extern "C" fn lling_semiring_diagnostic(
    semiring: *const LlingSemiring,
    value: *const LlingSemiringWeight,
    out_bytes: *mut u8,
    capacity: usize,
    out_written: *mut usize,
    out_required: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        let value = if value.is_null() {
            None
        } else {
            Some(dynamic_weight(value)?)
        };
        let bytes = context
            .diagnostic(value)
            .map(String::into_bytes)
            .map_err(map_semiring_error)?;
        // SAFETY: the C caller promises `capacity` writable bytes.
        unsafe { copy_bytes_to_c(&bytes, out_bytes, capacity, out_written, out_required) }
    })
}

unsafe fn semiring_weights(
    weights: *const *const LlingSemiringWeight,
    count: usize,
) -> Result<Vec<DynamicSemiringWeight>, LlingLlangStatus> {
    if count != 0 && weights.is_null() {
        set_error("weights is null with nonzero count");
        return Err(LlingLlangStatus::NullPointer);
    }
    let pointers = if count == 0 {
        &[][..]
    } else {
        // SAFETY: the caller promises `count` live handle pointers.
        unsafe { std::slice::from_raw_parts(weights, count) }
    };
    let mut owned = Vec::with_capacity(pointers.len());
    for pointer in pointers {
        owned.push(
            dynamic_weight(*pointer)?
                .try_clone()
                .map_err(map_semiring_error)?,
        );
    }
    Ok(owned)
}

/// Add a bounded array of weights, using the provider's batch capability when
/// advertised and the dynamic consumer's pairwise fallback otherwise.
///
/// # Safety
/// For nonzero `count`, `weights` must point to `count` live weight handles.
#[no_mangle]
pub unsafe extern "C" fn lling_semiring_plus_many(
    semiring: *const LlingSemiring,
    weights: *const *const LlingSemiringWeight,
    count: usize,
    out_weight: *mut *mut LlingSemiringWeight,
) -> LlingLlangStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        // SAFETY: upheld by this function's caller contract.
        let weights = unsafe { semiring_weights(weights, count)? };
        write_dynamic_weight(out_weight, || context.plus_many(&weights))
    })
}

/// Multiply a bounded array of weights, using the provider's batch capability
/// when advertised and the dynamic consumer's pairwise fallback otherwise.
///
/// # Safety
/// For nonzero `count`, `weights` must point to `count` live weight handles.
#[no_mangle]
pub unsafe extern "C" fn lling_semiring_times_many(
    semiring: *const LlingSemiring,
    weights: *const *const LlingSemiringWeight,
    count: usize,
    out_weight: *mut *mut LlingSemiringWeight,
) -> LlingLlangStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        // SAFETY: upheld by this function's caller contract.
        let weights = unsafe { semiring_weights(weights, count)? };
        write_dynamic_weight(out_weight, || context.times_many(&weights))
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
) -> LlingLlangStatus {
    boundary(|| {
        let context = dynamic_semiring(semiring)?;
        if count != 0 && weights.is_null() {
            set_error("weights is null with nonzero count");
            return Err(LlingLlangStatus::NullPointer);
        }
        // SAFETY: upheld by this function's caller contract.
        let owned = unsafe { semiring_weights(weights, count)? };
        context
            .validate_declared_laws(&owned, epsilon)
            .map_err(map_semiring_error)
    })
}

fn decode_unit_domain(raw: u32) -> Result<VtUnitDomain, LlingLlangStatus> {
    match raw {
        1 => Ok(VtUnitDomain::Byte),
        2 => Ok(VtUnitDomain::UnicodeScalar),
        3 => Ok(VtUnitDomain::U64),
        _ => {
            set_error("unit_domain is not a known VtUnitDomain value");
            Err(LlingLlangStatus::InvalidArgument)
        }
    }
}

fn decode_weight_domain(raw: u32) -> Result<VtWeightDomain, LlingLlangStatus> {
    match raw {
        1 => Ok(VtWeightDomain::TropicalF64),
        2 => Ok(VtWeightDomain::LogF64),
        3 => Ok(VtWeightDomain::ProbabilityF64),
        4 => Ok(VtWeightDomain::ArcticF64),
        5 => Ok(VtWeightDomain::SignedTropicalF64),
        6 => Ok(VtWeightDomain::CountF64),
        7 => Ok(VtWeightDomain::BooleanF64),
        _ => {
            set_error("weight_domain is not a known VtWeightDomain value");
            Err(LlingLlangStatus::InvalidArgument)
        }
    }
}

/// Allocate an empty Unicode/tropical WFST builder.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_new(
    out_builder: *mut *mut LlingWfstBuilder,
) -> LlingLlangStatus {
    lling_wfst_builder_new_for_domains(2, 1, out_builder)
}

/// Allocate an empty scalar WFST builder for any family ABI domain pair.
///
/// Raw `u32` discriminants are decoded before enum construction so malformed
/// foreign values are rejected without invoking Rust enum undefined behavior.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_new_for_domains(
    unit_domain: u32,
    weight_domain: u32,
    out_builder: *mut *mut LlingWfstBuilder,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_builder, "out_builder")?;
        let unit_domain = decode_unit_domain(unit_domain)?;
        let weight_domain = decode_weight_domain(weight_domain)?;
        *output = Box::into_raw(Box::new(LlingWfstBuilder {
            graph: Some(ScalarWfstGraph::new(unit_domain, weight_domain)),
            unit_domain,
            weight_domain,
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
    boundary(|| {
        // Validate the out-pointer BEFORE mutating the graph: adding the state
        // first meant a null `out_state` left an orphan state in the builder
        // while still returning NullPointer. Pointer validation must never
        // mutate caller state (mirrors the build/out_wfst discipline). Builder
        // validity is still checked first, preserving the builder -> out
        // precedence.
        let graph = graph(builder)?;
        let output = required_mut(out_state, "out_state")?;
        *output = graph.add_state().map_err(map_error)?;
        Ok(())
    })
}

/// Set the initial state.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_set_start(
    builder: *mut LlingWfstBuilder,
    state: u32,
) -> LlingLlangStatus {
    boundary(|| {
        let graph = graph(builder)?;
        if !graph.set_start(state) {
            set_error("start state is not present in the builder");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        Ok(())
    })
}

/// Set a final state and its domain-specific scalar weight.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_set_final(
    builder: *mut LlingWfstBuilder,
    state: u32,
    weight: f64,
) -> LlingLlangStatus {
    boundary(|| {
        let builder = required_mut(builder, "builder")?;
        if !valid_scalar_weight(builder.weight_domain, weight) {
            set_error("weight does not belong to the builder's semiring domain");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let graph = builder.graph.as_mut().ok_or_else(|| {
            set_error("builder has already been consumed");
            LlingLlangStatus::Closed
        })?;
        if !graph.set_final(state, weight) {
            set_error("final state is not present in the builder");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        Ok(())
    })
}

/// Clear a state's final status.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_clear_final(
    builder: *mut LlingWfstBuilder,
    state: u32,
) -> LlingLlangStatus {
    boundary(|| {
        if !graph(builder)?.clear_final(state) {
            set_error("state is not present in the builder");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        Ok(())
    })
}

fn validate_label(
    domain: VtUnitDomain,
    value: u64,
    present: u8,
    name: &'static str,
) -> Result<(), LlingLlangStatus> {
    match present {
        0 => Ok(()),
        1 => {
            if valid_scalar_label(domain, value) {
                Ok(())
            } else {
                set_error(format!(
                    "{name} does not belong to the builder's label domain"
                ));
                Err(LlingLlangStatus::InvalidArgument)
            }
        }
        _ => {
            set_error(format!("{name} presence flag must be zero or one"));
            Err(LlingLlangStatus::InvalidArgument)
        }
    }
}

/// Add a domain-validated scalar arc. Zero presence denotes epsilon.
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
) -> LlingLlangStatus {
    boundary(|| {
        let builder = required_mut(builder, "builder")?;
        if !valid_scalar_weight(builder.weight_domain, weight) {
            set_error("weight does not belong to the builder's semiring domain");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        validate_label(builder.unit_domain, input_label, has_input, "input label")?;
        validate_label(
            builder.unit_domain,
            output_label,
            has_output,
            "output label",
        )?;
        let graph = builder.graph.as_mut().ok_or_else(|| {
            set_error("builder has already been consumed");
            LlingLlangStatus::Closed
        })?;
        let arc = VtWfstArc {
            input_label,
            output_label,
            target_state: u64::from(to),
            weight,
            has_input,
            has_output,
            reserved: [0; 6],
        };
        if !graph.add_arc(from, arc) {
            set_error("arc source or target state is not present in the builder");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        Ok(())
    })
}

/// Consume a builder and freeze it into an immutable resource handle.
#[no_mangle]
pub extern "C" fn lling_wfst_builder_build(
    builder: *mut LlingWfstBuilder,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    boundary(|| {
        let builder = required_mut(builder, "builder")?;
        // Validate the out-pointer BEFORE taking the graph: taking first meant
        // a NullPointer failure silently consumed the builder (the graph was
        // dropped and every later call answered Closed). Pointer validation
        // must never destroy caller state.
        let output = required_mut(out_wfst, "out_wfst")?;
        let graph = builder.graph.take().ok_or_else(|| {
            set_error("builder has already been consumed");
            LlingLlangStatus::Closed
        })?;
        if graph.start().is_none() {
            builder.graph = Some(graph);
            set_error("WFST has no start state");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        *output = Box::into_raw(Box::new(LlingWfst {
            resource: OwnedWfstResource::from_scalar_wfst(graph),
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
) -> LlingLlangStatus {
    boundary(|| {
        // Validate the out-pointer BEFORE materializing the import: assignment
        // evaluates its right operand first, so a null `out_wfst` would leak the
        // fully-built LlingWfst and its captured resource retain. Pointer
        // validation must never leak caller-visible resources (mirrors the
        // build/out_wfst discipline).
        let output = required_mut(out_wfst, "out_wfst")?;
        let resource = OwnedWfstResource::import(resource).map_err(map_error)?;
        *output = Box::into_raw(Box::new(LlingWfst { resource }));
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
) -> LlingLlangStatus {
    if resource.is_null() {
        set_error("resource is null");
        return LlingLlangStatus::NullPointer;
    }
    lling_wfst_import(unsafe { *resource }, out_wfst)
}

/// Lazily compose two captured scalar-WFST resources.
#[no_mangle]
pub extern "C" fn lling_wfst_compose(
    first: VtResource,
    second: VtResource,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
    if first.is_null() {
        set_error("first resource is null");
        return LlingLlangStatus::NullPointer;
    }
    if second.is_null() {
        set_error("second resource is null");
        return LlingLlangStatus::NullPointer;
    }
    lling_wfst_compose(unsafe { *first }, unsafe { *second }, out_wfst)
}

#[derive(Clone, Copy)]
enum UnaryOperation {
    ProjectInput,
    ProjectOutput,
    Reverse,
}

fn unary_wfst_typed<L: AbiScalarLabel, W: AbiScalarWeight>(
    resource: VtResource,
    mut budget: GraphBudget,
    operation: UnaryOperation,
) -> Result<OwnedWfstResource, BindingError> {
    let (graph, stats): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(resource, &mut budget)?;
    match operation {
        UnaryOperation::ProjectInput => {
            budget.charge_output::<L, W>(stats.states, stats.arcs)?;
            export_native_lazy_wfst(ProjectSource::<L, W, _, true>::new(graph))
        }
        UnaryOperation::ProjectOutput => {
            budget.charge_output::<L, W>(stats.states, stats.arcs)?;
            export_native_lazy_wfst(ProjectSource::<L, W, _, false>::new(graph))
        }
        UnaryOperation::Reverse => {
            if stats.states >= u64::from(NO_STATE) {
                return Err(BindingError::RepresentationLimit);
            }
            let output_states = stats
                .states
                .checked_add(1)
                .ok_or(BindingError::BudgetExceeded("states"))?;
            let output_arcs = stats
                .arcs
                .checked_add(stats.finals)
                .ok_or(BindingError::BudgetExceeded("arcs"))?;
            budget.charge_output::<L, W>(output_states, output_arcs)?;
            export_native_wfst(&reverse(&graph))
        }
    }
}

// One exhaustive type dispatch is shared by all checked scalar operations.
// The leaf function contains the operation-specific semantics; labels and
// semirings never require duplicated algorithm bodies.
macro_rules! dispatch_scalar_weight {
    ($label:ty, $weight:expr, $function:ident, $($argument:expr),*) => {
        match $weight {
            VtWeightDomain::TropicalF64 => $function::<$label, TropicalWeight>($($argument),*),
            VtWeightDomain::LogF64 => $function::<$label, LogWeight>($($argument),*),
            VtWeightDomain::ProbabilityF64 => $function::<$label, ProbabilityWeight>($($argument),*),
            VtWeightDomain::ArcticF64 => $function::<$label, ArcticWeight>($($argument),*),
            VtWeightDomain::SignedTropicalF64 => $function::<$label, SignedTropicalWeight>($($argument),*),
            VtWeightDomain::CountF64 => $function::<$label, CountWeight>($($argument),*),
            VtWeightDomain::BooleanF64 => $function::<$label, BoolWeight>($($argument),*),
        }
    };
}

macro_rules! dispatch_scalar_domains {
    ($unit:expr, $weight:expr, $function:ident, $($argument:expr),*) => {
        match $unit {
            VtUnitDomain::Byte => dispatch_scalar_weight!(u8, $weight, $function, $($argument),*),
            VtUnitDomain::UnicodeScalar => dispatch_scalar_weight!(char, $weight, $function, $($argument),*),
            VtUnitDomain::U64 => dispatch_scalar_weight!(u64, $weight, $function, $($argument),*),
        }
    };
}

mod transforms;
use transforms::{materialize, materialize_ref, MaterializingTransform};
mod set_operations;

/// Intersect two verified weighted acceptors using native epsilon-filtered composition.
///
/// Both inputs must have equal scalar label and semiring domains, and every
/// reachable arc must have equal input and output labels (including epsilon).
/// All four budget flags are required. The result owns an eager snapshot;
/// `out_wfst` is unchanged on failure.
#[no_mangle]
pub extern "C" fn lling_wfst_acceptor_intersect(
    first: VtResource,
    second: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    set_operations::acceptor_intersect(first, second, budget, out_wfst)
}

/// Pointer-form weighted acceptor intersection.
///
/// # Safety
/// `first` and `second` must be null or point to readable `VtResource` values.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_acceptor_intersect_refs(
    first: *const VtResource,
    second: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    if first.is_null() {
        set_error("first resource is null");
        return LlingLlangStatus::NullPointer;
    }
    if second.is_null() {
        set_error("second resource is null");
        return LlingLlangStatus::NullPointer;
    }
    set_operations::acceptor_intersect(unsafe { *first }, unsafe { *second }, budget, out_wfst)
}

/// Materialize native weighted determinization of a scalar WFST.
#[no_mangle]
pub extern "C" fn lling_wfst_determinize(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    materialize(
        resource,
        budget,
        out_wfst,
        MaterializingTransform::Determinize,
    )
}

/// Materialize native weighted minimization of a deterministic scalar WFST.
#[no_mangle]
pub extern "C" fn lling_wfst_minimize(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    materialize(resource, budget, out_wfst, MaterializingTransform::Minimize)
}

/// Materialize native epsilon removal of a scalar WFST.
#[no_mangle]
pub extern "C" fn lling_wfst_remove_epsilon(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    materialize(
        resource,
        budget,
        out_wfst,
        MaterializingTransform::RemoveEpsilon,
    )
}

/// Materialize native connect/trim of a scalar WFST.
#[no_mangle]
pub extern "C" fn lling_wfst_connect(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    materialize(resource, budget, out_wfst, MaterializingTransform::Connect)
}

/// Pointer-form native weighted determinization.
///
/// # Safety
/// `resource` must be null or point to a readable `VtResource`.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_determinize_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    materialize_ref(
        resource,
        budget,
        out_wfst,
        MaterializingTransform::Determinize,
    )
}

/// Pointer-form native weighted minimization.
///
/// # Safety
/// `resource` must be null or point to a readable `VtResource`.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_minimize_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    materialize_ref(resource, budget, out_wfst, MaterializingTransform::Minimize)
}

/// Pointer-form native epsilon removal.
///
/// # Safety
/// `resource` must be null or point to a readable `VtResource`.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_remove_epsilon_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    materialize_ref(
        resource,
        budget,
        out_wfst,
        MaterializingTransform::RemoveEpsilon,
    )
}

/// Pointer-form native connect/trim.
///
/// # Safety
/// `resource` must be null or point to a readable `VtResource`.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_connect_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    materialize_ref(resource, budget, out_wfst, MaterializingTransform::Connect)
}

fn unary_wfst_dispatch(
    resource: VtResource,
    budget: GraphBudget,
    operation: UnaryOperation,
) -> Result<OwnedWfstResource, BindingError> {
    let (unit, weight) = wfst_domains(resource)?;
    dispatch_scalar_domains!(unit, weight, unary_wfst_typed, resource, budget, operation)
}

fn graph_budget_from_v2(budget: *const LlingBudgetV2) -> Result<GraphBudget, LlingLlangStatus> {
    let budget = read_v2_struct(
        budget,
        "budget",
        LLING_BUDGET_STATES | LLING_BUDGET_ARCS | LLING_BUDGET_BYTES | LLING_BUDGET_WORK,
    )?;
    if !validate_budget_v2(&budget) {
        set_error("budget flags, limits, or reserved fields are not canonical");
        return Err(LlingLlangStatus::InvalidArgument);
    }
    let flags = budget.header.flags;
    Ok(GraphBudget::new(
        (flags & LLING_BUDGET_STATES != 0).then_some(budget.max_states),
        (flags & LLING_BUDGET_ARCS != 0).then_some(budget.max_arcs),
        (flags & LLING_BUDGET_BYTES != 0).then_some(budget.max_bytes),
        (flags & LLING_BUDGET_WORK != 0).then_some(budget.max_work),
    ))
}

fn unary_wfst(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
    operation: UnaryOperation,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_wfst, "out_wfst")?;
        let budget = graph_budget_from_v2(budget)?;
        let resource = unary_wfst_dispatch(resource, budget, operation).map_err(map_error)?;
        *output = Box::into_raw(Box::new(LlingWfst { resource }));
        Ok(())
    })
}

/// Lazily project a borrowed scalar WFST onto its input labels.
///
/// The canonical ABI-v2 budget bounds the imported input plus the potential
/// complete output graph; bytes are logical graph payload, not process RSS or
/// allocations performed inside the foreign provider. The returned handle is
/// owned only on success, and `out_wfst` is otherwise untouched.
#[no_mangle]
pub extern "C" fn lling_wfst_project_input(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    unary_wfst(resource, budget, out_wfst, UnaryOperation::ProjectInput)
}

/// Lazily project a borrowed scalar WFST onto its output labels.
///
/// Budget and ownership rules are identical to [`lling_wfst_project_input`].
#[no_mangle]
pub extern "C" fn lling_wfst_project_output(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    unary_wfst(resource, budget, out_wfst, UnaryOperation::ProjectOutput)
}

/// Constructively reverse a borrowed scalar WFST.
///
/// Budget and ownership rules are identical to [`lling_wfst_project_input`].
#[no_mangle]
pub extern "C" fn lling_wfst_reverse(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    unary_wfst(resource, budget, out_wfst, UnaryOperation::Reverse)
}

fn unary_wfst_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
    operation: UnaryOperation,
) -> LlingLlangStatus {
    if resource.is_null() {
        set_error("resource is null");
        return LlingLlangStatus::NullPointer;
    }
    unary_wfst(unsafe { *resource }, budget, out_wfst, operation)
}

/// Pointer-form input projection for FFIs unable to pass C aggregates by value.
///
/// # Safety
/// `resource` must be null or point to a readable `VtResource` for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_project_input_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    unary_wfst_ref(resource, budget, out_wfst, UnaryOperation::ProjectInput)
}

/// Pointer-form output projection for FFIs unable to pass C aggregates by value.
///
/// # Safety
/// `resource` must be null or point to a readable `VtResource` for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_project_output_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    unary_wfst_ref(resource, budget, out_wfst, UnaryOperation::ProjectOutput)
}

/// Pointer-form reversal for FFIs unable to pass C aggregates by value.
///
/// # Safety
/// `resource` must be null or point to a readable `VtResource` for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_reverse_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    unary_wfst_ref(resource, budget, out_wfst, UnaryOperation::Reverse)
}

#[derive(Clone, Copy)]
enum RationalBinaryOperation {
    Union,
    Concat,
}

#[derive(Clone, Copy)]
enum RationalUnaryOperation {
    Closure,
    ClosurePlus,
}

fn rational_binary_typed<L: AbiScalarLabel, W: AbiScalarWeight>(
    first: VtResource,
    second: VtResource,
    mut budget: GraphBudget,
    operation: RationalBinaryOperation,
) -> Result<OwnedWfstResource, BindingError> {
    let (first, left): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(first, &mut budget)?;
    let (second, right): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(second, &mut budget)?;
    let input_states = left
        .states
        .checked_add(right.states)
        .ok_or(BindingError::BudgetExceeded("states"))?;
    let input_arcs = left
        .arcs
        .checked_add(right.arcs)
        .ok_or(BindingError::BudgetExceeded("arcs"))?;
    let (output_states, output_arcs) = match operation {
        RationalBinaryOperation::Union => (
            input_states
                .checked_add(1)
                .ok_or(BindingError::BudgetExceeded("states"))?,
            input_arcs
                .checked_add(2)
                .ok_or(BindingError::BudgetExceeded("arcs"))?,
        ),
        RationalBinaryOperation::Concat => (
            input_states,
            input_arcs
                .checked_add(left.finals)
                .ok_or(BindingError::BudgetExceeded("arcs"))?,
        ),
    };
    if output_states > u64::from(NO_STATE) {
        return Err(BindingError::RepresentationLimit);
    }
    budget.charge_output::<L, W>(output_states, output_arcs)?;
    match operation {
        RationalBinaryOperation::Union => {
            export_native_lazy_wfst(UnionSource::<L, W, _, _>::new(first, second))
        }
        RationalBinaryOperation::Concat => {
            export_native_lazy_wfst(ConcatSource::<L, W, _, _>::new(first, second))
        }
    }
}

fn rational_unary_typed<L: AbiScalarLabel, W: AbiScalarWeight>(
    resource: VtResource,
    mut budget: GraphBudget,
    operation: RationalUnaryOperation,
) -> Result<OwnedWfstResource, BindingError> {
    let (graph, stats): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(resource, &mut budget)?;
    let (output_states, output_arcs) = match operation {
        RationalUnaryOperation::Closure => (
            stats
                .states
                .checked_add(1)
                .ok_or(BindingError::BudgetExceeded("states"))?,
            stats
                .arcs
                .checked_add(stats.finals)
                .and_then(|arcs| arcs.checked_add(1))
                .ok_or(BindingError::BudgetExceeded("arcs"))?,
        ),
        RationalUnaryOperation::ClosurePlus => (
            stats.states,
            stats
                .arcs
                .checked_add(stats.finals)
                .ok_or(BindingError::BudgetExceeded("arcs"))?,
        ),
    };
    if output_states > u64::from(NO_STATE) {
        return Err(BindingError::RepresentationLimit);
    }
    budget.charge_output::<L, W>(output_states, output_arcs)?;
    match operation {
        RationalUnaryOperation::Closure => {
            export_native_lazy_wfst(ClosureSource::<L, W, _>::new(graph))
        }
        RationalUnaryOperation::ClosurePlus => {
            export_native_lazy_wfst(ClosurePlusSource::<L, W, _>::new(graph))
        }
    }
}

fn rational_binary_dispatch(
    first: VtResource,
    second: VtResource,
    budget: GraphBudget,
    operation: RationalBinaryOperation,
) -> Result<OwnedWfstResource, BindingError> {
    let (unit, weight) = wfst_domains(first)?;
    let (other_unit, other_weight) = wfst_domains(second)?;
    if other_unit != unit {
        return Err(BindingError::UnitDomainMismatch(other_unit));
    }
    if other_weight != weight {
        return Err(BindingError::WeightDomainMismatch(other_weight));
    }
    dispatch_scalar_domains!(
        unit,
        weight,
        rational_binary_typed,
        first,
        second,
        budget,
        operation
    )
}

fn rational_unary_dispatch(
    resource: VtResource,
    budget: GraphBudget,
    operation: RationalUnaryOperation,
) -> Result<OwnedWfstResource, BindingError> {
    let (unit, weight) = wfst_domains(resource)?;
    dispatch_scalar_domains!(
        unit,
        weight,
        rational_unary_typed,
        resource,
        budget,
        operation
    )
}

fn rational_binary_wfst(
    first: VtResource,
    second: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
    operation: RationalBinaryOperation,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_wfst, "out_wfst")?;
        let budget = graph_budget_from_v2(budget)?;
        let resource =
            rational_binary_dispatch(first, second, budget, operation).map_err(map_error)?;
        *output = Box::into_raw(Box::new(LlingWfst { resource }));
        Ok(())
    })
}

fn rational_unary_wfst(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
    operation: RationalUnaryOperation,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_wfst, "out_wfst")?;
        let budget = graph_budget_from_v2(budget)?;
        let resource = rational_unary_dispatch(resource, budget, operation).map_err(map_error)?;
        *output = Box::into_raw(Box::new(LlingWfst { resource }));
        Ok(())
    })
}

/// Lazily accept either borrowed scalar WFST, with a shared cumulative budget.
#[no_mangle]
pub extern "C" fn lling_wfst_union(
    first: VtResource,
    second: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    rational_binary_wfst(
        first,
        second,
        budget,
        out_wfst,
        RationalBinaryOperation::Union,
    )
}

/// Lazily accept the first borrowed WFST followed by the second.
#[no_mangle]
pub extern "C" fn lling_wfst_concat(
    first: VtResource,
    second: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    rational_binary_wfst(
        first,
        second,
        budget,
        out_wfst,
        RationalBinaryOperation::Concat,
    )
}

/// Lazily accept zero or more repetitions of a borrowed scalar WFST.
#[no_mangle]
pub extern "C" fn lling_wfst_closure(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    rational_unary_wfst(resource, budget, out_wfst, RationalUnaryOperation::Closure)
}

/// Lazily accept one or more repetitions of a borrowed scalar WFST.
#[no_mangle]
pub extern "C" fn lling_wfst_closure_plus(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    rational_unary_wfst(
        resource,
        budget,
        out_wfst,
        RationalUnaryOperation::ClosurePlus,
    )
}

fn rational_binary_wfst_refs(
    first: *const VtResource,
    second: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
    operation: RationalBinaryOperation,
) -> LlingLlangStatus {
    if first.is_null() {
        set_error("first resource is null");
        return LlingLlangStatus::NullPointer;
    }
    if second.is_null() {
        set_error("second resource is null");
        return LlingLlangStatus::NullPointer;
    }
    rational_binary_wfst(
        unsafe { *first },
        unsafe { *second },
        budget,
        out_wfst,
        operation,
    )
}

fn rational_unary_wfst_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
    operation: RationalUnaryOperation,
) -> LlingLlangStatus {
    if resource.is_null() {
        set_error("resource is null");
        return LlingLlangStatus::NullPointer;
    }
    rational_unary_wfst(unsafe { *resource }, budget, out_wfst, operation)
}

/// Pointer-form union for FFIs unable to pass C aggregates by value.
///
/// # Safety
/// Each resource pointer must be null or readable for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_union_refs(
    first: *const VtResource,
    second: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    rational_binary_wfst_refs(
        first,
        second,
        budget,
        out_wfst,
        RationalBinaryOperation::Union,
    )
}

/// Pointer-form concatenation for FFIs unable to pass C aggregates by value.
///
/// # Safety
/// Each resource pointer must be null or readable for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_concat_refs(
    first: *const VtResource,
    second: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    rational_binary_wfst_refs(
        first,
        second,
        budget,
        out_wfst,
        RationalBinaryOperation::Concat,
    )
}

/// Pointer-form closure for FFIs unable to pass C aggregates by value.
///
/// # Safety
/// `resource` must be null or readable for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_closure_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    rational_unary_wfst_ref(resource, budget, out_wfst, RationalUnaryOperation::Closure)
}

/// Pointer-form Kleene plus for FFIs unable to pass C aggregates by value.
///
/// # Safety
/// `resource` must be null or readable for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn lling_wfst_closure_plus_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    rational_unary_wfst_ref(
        resource,
        budget,
        out_wfst,
        RationalUnaryOperation::ClosurePlus,
    )
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
) -> LlingLlangStatus {
    boundary(|| {
        if wfst.is_null() {
            set_error("wfst is null");
            return Err(LlingLlangStatus::NullPointer);
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

fn checked_v2_pointer<T>(pointer: *const T, name: &'static str) -> Result<(), LlingLlangStatus> {
    if pointer.is_null() {
        set_error(format!("{name} is null"));
        return Err(LlingLlangStatus::NullPointer);
    }
    if (pointer as usize) % std::mem::align_of::<T>() != 0 {
        set_error(format!("{name} is misaligned"));
        return Err(LlingLlangStatus::InvalidArgument);
    }
    Ok(())
}

fn required_v2_ref<'a, T>(
    pointer: *const T,
    name: &'static str,
) -> Result<&'a T, LlingLlangStatus> {
    checked_v2_pointer(pointer, name)?;
    Ok(unsafe { &*pointer })
}

fn required_v2_mut<'a, T>(
    pointer: *mut T,
    name: &'static str,
) -> Result<&'a mut T, LlingLlangStatus> {
    checked_v2_pointer(pointer.cast_const(), name)?;
    Ok(unsafe { &mut *pointer })
}

fn read_v2_struct<T: Copy>(
    pointer: *const T,
    name: &'static str,
    known_flags: u64,
) -> Result<T, LlingLlangStatus> {
    checked_v2_pointer(pointer, name)?;
    let header = unsafe { pointer.cast::<LlingAbiV2Header>().read() };
    if !validate_abi_v2_header(&header, std::mem::size_of::<T>(), known_flags) {
        set_error(format!("{name} has an invalid ABI-v2 header"));
        return Err(LlingLlangStatus::InvalidArgument);
    }
    Ok(unsafe { pointer.read() })
}

fn decode_v2_bool(raw: u8, name: &'static str) -> Result<bool, LlingLlangStatus> {
    match raw {
        0 => Ok(false),
        1 => Ok(true),
        _ => {
            set_error(format!("{name} must be zero or one"));
            Err(LlingLlangStatus::InvalidArgument)
        }
    }
}

/// Validate a typed ABI-v2 header and its additive known prefix.
#[no_mangle]
pub extern "C" fn lling_abi_v2_validate_header(
    header: *const LlingAbiV2Header,
    required_size: u32,
    known_flags: u64,
) -> LlingLlangStatus {
    boundary(|| {
        let header = *required_v2_ref(header, "header")?;
        if !validate_abi_v2_header(&header, required_size as usize, known_flags) {
            set_error("header is not a canonical ABI-v2 prefix");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        Ok(())
    })
}

/// Validate a WFST descriptor and report whether typed evidence is admissible.
#[no_mangle]
pub extern "C" fn lling_abi_v2_validate_descriptor(
    descriptor: *const LlingWfstDescriptorV2,
    out_typed_evidence_allowed: *mut u8,
) -> LlingLlangStatus {
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
            return Err(LlingLlangStatus::InvalidArgument);
        }
        *output = u8::from(abi_v2_typed_evidence_allowed(&descriptor));
        Ok(())
    })
}

/// Validate a canonical ABI-v2 resource budget.
#[no_mangle]
pub extern "C" fn lling_abi_v2_validate_budget(budget: *const LlingBudgetV2) -> LlingLlangStatus {
    boundary(|| {
        let budget = read_v2_struct(
            budget,
            "budget",
            LLING_BUDGET_STATES | LLING_BUDGET_ARCS | LLING_BUDGET_BYTES | LLING_BUDGET_WORK,
        )?;
        if !validate_budget_v2(&budget) {
            set_error("budget flags, limits, or reserved fields are not canonical");
            return Err(LlingLlangStatus::InvalidArgument);
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
) -> LlingLlangStatus {
    boundary(|| {
        let outcome = read_v2_struct(outcome, "outcome", 0)?;
        let resource_present = decode_v2_bool(resource_present, "resource_present")?;
        let evidence_present = decode_v2_bool(evidence_present, "evidence_present")?;
        let output = required_v2_mut(out_authoritative_exact, "out_authoritative_exact")?;
        if !validate_outcome_v2(&outcome, resource_present, evidence_present) {
            set_error("outcome axes or publication state are not canonical");
            return Err(LlingLlangStatus::InvalidArgument);
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
) -> LlingLlangStatus {
    boundary(|| {
        let known_flags = LLING_DESCRIPTOR_SIGNATURE_KNOWN
            | LLING_DESCRIPTOR_SNAPSHOT_PRESENT
            | LLING_DESCRIPTOR_CONTEXT_PRESENT;
        let expected = read_v2_struct(expected, "expected", known_flags)?;
        let observed = read_v2_struct(observed, "observed", known_flags)?;
        let output = required_v2_mut(out_matches, "out_matches")?;
        if !validate_descriptor_v2(&expected) || !validate_descriptor_v2(&observed) {
            set_error("identity comparison requires canonical descriptors");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        *output = u8::from(abi_v2_identity_matches(&expected, &observed));
        Ok(())
    })
}

/// Allocate a live cooperative-cancellation handle.
#[no_mangle]
pub extern "C" fn lling_cancellation_v2_new(
    out_cancellation: *mut *mut LlingCancellationV2,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_v2_mut(out_cancellation, "out_cancellation")?;
        if !output.is_null() {
            set_error("out_cancellation must initially be null");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let layout = std::alloc::Layout::new::<LlingCancellationV2>();
        let allocation = unsafe { std::alloc::alloc(layout) }.cast::<LlingCancellationV2>();
        if allocation.is_null() {
            set_error("unable to allocate cancellation handle");
            return Err(LlingLlangStatus::LimitExceeded);
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
) -> LlingLlangStatus {
    boundary(|| {
        let cancellation = required_v2_ref(cancellation, "cancellation")?;
        let reason = LlingCancellationReasonV2::from_raw(reason).ok_or_else(|| {
            set_error("cancellation reason is not a known wire discriminant");
            LlingLlangStatus::InvalidArgument
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
) -> LlingLlangStatus {
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
) -> LlingLlangStatus {
    boundary(|| {
        let slot = required_v2_mut(cancellation, "cancellation")?;
        if slot.is_null() {
            set_error("cancellation handle has already been released");
            return Err(LlingLlangStatus::Closed);
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
    use crate::bindings::import_native_wfst;
    use crate::bindings::ScalarStateData;
    use crate::semiring::Semiring;
    use crate::wfst::{MutableWfst, WeightedTransition, Wfst, WfstState};
    #[test]
    fn provider_limit_and_closed_statuses_keep_their_direct_abi_meaning() {
        assert_eq!(
            map_error(BindingError::Provider(VtStatus::LimitExceeded)),
            LlingLlangStatus::LimitExceeded
        );
        assert_eq!(
            map_error(BindingError::Provider(VtStatus::Closed)),
            LlingLlangStatus::Closed
        );
        assert_eq!(
            map_error(BindingError::Provider(VtStatus::Ok)),
            LlingLlangStatus::ProviderError
        );
        assert_eq!(
            map_error(BindingError::Provider(VtStatus::IoError)),
            LlingLlangStatus::ProviderError
        );
    }
    use std::ptr;
    use vinary_tree_interop::{
        VtWfstArc, VtWfstVTable, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
    };

    fn canonical_budget() -> LlingBudgetV2 {
        LlingBudgetV2 {
            header: LlingAbiV2Header {
                struct_size: std::mem::size_of::<LlingBudgetV2>() as u32,
                abi_version: LLING_ABI_V2,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn set_budget_axis(budget: &mut LlingBudgetV2, flag: u64, value: u64) {
        match flag {
            LLING_BUDGET_STATES => budget.max_states = value,
            LLING_BUDGET_ARCS => budget.max_arcs = value,
            LLING_BUDGET_BYTES => budget.max_bytes = value,
            LLING_BUDGET_WORK => budget.max_work = value,
            _ => unreachable!(),
        }
    }

    fn unary_result<L: AbiScalarLabel, W: AbiScalarWeight>(
        handle: *mut LlingWfst,
    ) -> VectorWfst<L, W> {
        let mut raw = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(handle, &mut raw) },
            LlingLlangStatus::Ok
        );
        unsafe { lling_wfst_free(handle) };
        let graph = import_native_wfst(raw).unwrap();
        lling_resource_release(raw);
        graph
    }

    fn assert_same_graph<L: AbiScalarLabel, W: AbiScalarWeight>(
        actual: &VectorWfst<L, W>,
        expected: &VectorWfst<L, W>,
    ) {
        assert_eq!(actual.num_states(), expected.num_states());
        assert_eq!(actual.start(), expected.start());
        for state in 0..actual.num_states() as u32 {
            assert_eq!(actual.is_final(state), expected.is_final(state));
            assert_eq!(
                actual.final_weight(state).encode(),
                expected.final_weight(state).encode()
            );
            let actual_arcs = actual.transitions(state);
            let expected_arcs = expected.transitions(state);
            assert_eq!(actual_arcs.len(), expected_arcs.len());
            for (actual, expected) in actual_arcs.iter().zip(expected_arcs) {
                assert_eq!((actual.from, actual.to), (expected.from, expected.to));
                assert_eq!(
                    actual.input.as_ref().map(L::encode),
                    expected.input.as_ref().map(L::encode)
                );
                assert_eq!(
                    actual.output.as_ref().map(L::encode),
                    expected.output.as_ref().map(L::encode)
                );
                assert_eq!(actual.weight.encode(), expected.weight.encode());
            }
        }
    }

    fn unary_matrix_case<L: AbiScalarLabel, W: AbiScalarWeight>() {
        let mut graph = VectorWfst::<L, W>::new();
        let first = graph.add_state();
        let second = graph.add_state();
        graph.set_start(first);
        graph.set_final(second, W::one());
        graph
            .try_add_transition(WeightedTransition::new(
                first,
                Some(L::decode(97).unwrap()),
                Some(L::decode(98).unwrap()),
                second,
                W::one(),
            ))
            .unwrap();
        let resource = export_native_wfst(&graph).unwrap();
        let budget = canonical_budget();
        let mut handle = ptr::null_mut();
        assert_eq!(
            lling_wfst_project_input(resource.as_raw(), &budget, &mut handle),
            LlingLlangStatus::Ok,
        );
        let input: VectorWfst<L, W> = unary_result(handle);
        assert_eq!(input.num_states(), 2);
        assert_eq!(input.start(), 0);
        assert!(input.is_final(1));
        assert_eq!(input.transitions(0).len(), 1);
        let arc = &input.transitions(0)[0];
        assert_eq!(arc.input.as_ref().map(L::encode), Some(97));
        assert_eq!(arc.output.as_ref().map(L::encode), Some(97));
        assert_eq!(arc.weight.encode(), W::one().encode());

        handle = ptr::null_mut();
        assert_eq!(
            unsafe { lling_wfst_project_output_ref(&resource.as_raw(), &budget, &mut handle) },
            LlingLlangStatus::Ok,
        );
        let output: VectorWfst<L, W> = unary_result(handle);
        let arc = &output.transitions(0)[0];
        assert_eq!(arc.input.as_ref().map(L::encode), Some(98));
        assert_eq!(arc.output.as_ref().map(L::encode), Some(98));
        assert_eq!(arc.weight.encode(), W::one().encode());

        handle = ptr::null_mut();
        assert_eq!(
            unsafe { lling_wfst_reverse_ref(&resource.as_raw(), &budget, &mut handle) },
            LlingLlangStatus::Ok,
        );
        let reversed: VectorWfst<L, W> = unary_result(handle);
        assert_eq!(reversed.num_states(), 3);
        assert_eq!(reversed.start(), 0);
        assert_eq!(reversed.transitions(0).len(), 1);
        // Import renumbers reachable states in traversal order. The reversed
        // path must nevertheless remain start -> former final -> former start.
        assert_eq!(reversed.transitions(0)[0].to, 1);
        assert_eq!(reversed.transitions(1).len(), 1);
        assert_eq!(reversed.transitions(1)[0].to, 2);
        assert!(reversed.is_final(2));
        let native_reversed = reverse(&graph);
        let native_resource = export_native_wfst(&native_reversed).unwrap();
        let native_normalized: VectorWfst<L, W> =
            import_native_wfst(native_resource.as_raw()).unwrap();
        assert_same_graph(&reversed, &native_normalized);
    }

    #[test]
    fn unary_c_dispatch_covers_all_scalar_domains_and_semirings() {
        macro_rules! all_weights {
            ($label:ty) => {
                unary_matrix_case::<$label, TropicalWeight>();
                unary_matrix_case::<$label, LogWeight>();
                unary_matrix_case::<$label, ProbabilityWeight>();
                unary_matrix_case::<$label, ArcticWeight>();
                unary_matrix_case::<$label, SignedTropicalWeight>();
                unary_matrix_case::<$label, CountWeight>();
                unary_matrix_case::<$label, BoolWeight>();
            };
        }
        all_weights!(u8);
        all_weights!(char);
        all_weights!(u64);
    }

    #[test]
    fn unary_c_budget_is_canonical_cumulative_and_failure_atomic() {
        let mut graph = VectorWfst::<char, TropicalWeight>::new();
        let first = graph.add_state();
        let second = graph.add_state();
        graph.set_start(first);
        graph.set_final(second, TropicalWeight::one());
        graph
            .try_add_transition(WeightedTransition::new(
                first,
                Some('a'),
                Some('b'),
                second,
                TropicalWeight::one(),
            ))
            .unwrap();
        let resource = export_native_wfst(&graph).unwrap();
        let sentinel = ptr::dangling_mut::<LlingWfst>();
        let mut output = sentinel;
        let mut invalid = canonical_budget();
        invalid.header.flags = LLING_BUDGET_STATES;
        assert_eq!(
            lling_wfst_project_input(resource.as_raw(), &invalid, &mut output),
            LlingLlangStatus::InvalidArgument
        );
        assert_eq!(output, sentinel);

        let state_bytes = (std::mem::size_of::<WfstState<char, TropicalWeight>>()
            + std::mem::size_of::<ScalarStateData>()) as u64;
        let arc_bytes = (std::mem::size_of::<VtWfstArc>()
            + std::mem::size_of::<WeightedTransition<char, TropicalWeight>>())
            as u64;
        let limits = [
            (LLING_BUDGET_STATES, 3, 4),
            (LLING_BUDGET_ARCS, 1, 2),
            (
                LLING_BUDGET_BYTES,
                4 * state_bytes + 2 * arc_bytes - 1,
                4 * state_bytes + 2 * arc_bytes,
            ),
            (LLING_BUDGET_WORK, 5, 6),
        ];
        for (flag, below, exact) in limits {
            let mut budget = canonical_budget();
            budget.header.flags = flag;
            match flag {
                LLING_BUDGET_STATES => budget.max_states = below,
                LLING_BUDGET_ARCS => budget.max_arcs = below,
                LLING_BUDGET_BYTES => budget.max_bytes = below,
                LLING_BUDGET_WORK => budget.max_work = below,
                _ => unreachable!(),
            }
            assert_eq!(
                lling_wfst_project_input(resource.as_raw(), &budget, &mut output),
                LlingLlangStatus::LimitExceeded
            );
            assert_eq!(output, sentinel);
            match flag {
                LLING_BUDGET_STATES => budget.max_states = exact,
                LLING_BUDGET_ARCS => budget.max_arcs = exact,
                LLING_BUDGET_BYTES => budget.max_bytes = exact,
                LLING_BUDGET_WORK => budget.max_work = exact,
                _ => unreachable!(),
            }
            assert_eq!(
                lling_wfst_project_input(resource.as_raw(), &budget, &mut output),
                LlingLlangStatus::Ok
            );
            let _: VectorWfst<char, TropicalWeight> = unary_result(output);
            output = sentinel;
        }

        assert_eq!(
            unsafe { lling_wfst_project_input_ref(ptr::null(), &canonical_budget(), &mut output) },
            LlingLlangStatus::NullPointer
        );
        assert_eq!(output, sentinel);
        assert_eq!(
            lling_wfst_reverse(resource.as_raw(), &canonical_budget(), ptr::null_mut()),
            LlingLlangStatus::NullPointer
        );
    }

    fn rational_matrix_case<L: AbiScalarLabel, W: AbiScalarWeight>() {
        let mut graph = VectorWfst::<L, W>::new();
        let first = graph.add_state();
        let final_state = graph.add_state();
        graph.set_start(first);
        graph.set_final(final_state, W::one());
        graph
            .try_add_transition(WeightedTransition::new(
                first,
                Some(L::decode(97).unwrap()),
                Some(L::decode(98).unwrap()),
                final_state,
                W::one(),
            ))
            .unwrap();
        let resource = export_native_wfst(&graph).unwrap();
        let budget = canonical_budget();

        let mut handle = ptr::null_mut();
        assert_eq!(
            lling_wfst_union(resource.as_raw(), resource.as_raw(), &budget, &mut handle),
            LlingLlangStatus::Ok,
        );
        let actual: VectorWfst<L, W> = unary_result(handle);
        let native =
            export_native_lazy_wfst(UnionSource::<L, W, _, _>::new(graph.clone(), graph.clone()))
                .unwrap();
        let expected: VectorWfst<L, W> = import_native_wfst(native.as_raw()).unwrap();
        assert_same_graph(&actual, &expected);
        assert_eq!(actual.num_states(), 5);

        handle = ptr::null_mut();
        assert_eq!(
            unsafe {
                lling_wfst_concat_refs(&resource.as_raw(), &resource.as_raw(), &budget, &mut handle)
            },
            LlingLlangStatus::Ok,
        );
        let actual: VectorWfst<L, W> = unary_result(handle);
        let native = export_native_lazy_wfst(ConcatSource::<L, W, _, _>::new(
            graph.clone(),
            graph.clone(),
        ))
        .unwrap();
        let expected: VectorWfst<L, W> = import_native_wfst(native.as_raw()).unwrap();
        assert_same_graph(&actual, &expected);
        assert_eq!(actual.num_states(), 4);

        handle = ptr::null_mut();
        assert_eq!(
            lling_wfst_closure(resource.as_raw(), &budget, &mut handle),
            LlingLlangStatus::Ok,
        );
        let actual: VectorWfst<L, W> = unary_result(handle);
        let native = export_native_lazy_wfst(ClosureSource::<L, W, _>::new(graph.clone())).unwrap();
        let expected: VectorWfst<L, W> = import_native_wfst(native.as_raw()).unwrap();
        assert_same_graph(&actual, &expected);
        assert!(actual.is_final(actual.start()));

        handle = ptr::null_mut();
        assert_eq!(
            unsafe { lling_wfst_closure_plus_ref(&resource.as_raw(), &budget, &mut handle) },
            LlingLlangStatus::Ok,
        );
        let actual: VectorWfst<L, W> = unary_result(handle);
        let native = export_native_lazy_wfst(ClosurePlusSource::<L, W, _>::new(graph)).unwrap();
        let expected: VectorWfst<L, W> = import_native_wfst(native.as_raw()).unwrap();
        assert_same_graph(&actual, &expected);
        assert_eq!(actual.num_states(), 2);
        assert!(!actual.is_final(actual.start()));
        assert!(actual.transitions(1).iter().any(|arc| {
            arc.input.is_none() && arc.output.is_none() && arc.to == actual.start()
        }));
    }

    #[test]
    fn rational_c_dispatch_matches_native_sources_across_every_scalar_domain() {
        macro_rules! all_weights {
            ($label:ty) => {
                rational_matrix_case::<$label, TropicalWeight>();
                rational_matrix_case::<$label, LogWeight>();
                rational_matrix_case::<$label, ProbabilityWeight>();
                rational_matrix_case::<$label, ArcticWeight>();
                rational_matrix_case::<$label, SignedTropicalWeight>();
                rational_matrix_case::<$label, CountWeight>();
                rational_matrix_case::<$label, BoolWeight>();
            };
        }
        all_weights!(u8);
        all_weights!(char);
        all_weights!(u64);
    }

    #[test]
    fn rational_c_budget_domain_and_failure_atomicity_are_exact() {
        let mut graph = VectorWfst::<char, TropicalWeight>::new();
        let start = graph.add_state();
        let final_state = graph.add_state();
        graph.set_start(start);
        graph.set_final(final_state, TropicalWeight::one());
        graph
            .try_add_transition(WeightedTransition::new(
                start,
                Some('a'),
                Some('b'),
                final_state,
                TropicalWeight::one(),
            ))
            .unwrap();
        let resource = export_native_wfst(&graph).unwrap();
        let sentinel = ptr::dangling_mut::<LlingWfst>();
        let mut output = sentinel;

        let state_bytes = (std::mem::size_of::<WfstState<char, TropicalWeight>>()
            + std::mem::size_of::<ScalarStateData>()) as u64;
        let arc_bytes = (std::mem::size_of::<VtWfstArc>()
            + std::mem::size_of::<WeightedTransition<char, TropicalWeight>>())
            as u64;
        for (flag, below, exact) in [
            (LLING_BUDGET_STATES, 8, 9),
            (LLING_BUDGET_ARCS, 5, 6),
            (
                LLING_BUDGET_BYTES,
                9 * state_bytes + 6 * arc_bytes - 1,
                9 * state_bytes + 6 * arc_bytes,
            ),
            (LLING_BUDGET_WORK, 14, 15),
        ] {
            let mut budget = canonical_budget();
            budget.header.flags = flag;
            set_budget_axis(&mut budget, flag, below);
            assert_eq!(
                lling_wfst_union(resource.as_raw(), resource.as_raw(), &budget, &mut output),
                LlingLlangStatus::LimitExceeded
            );
            assert_eq!(output, sentinel);
            set_budget_axis(&mut budget, flag, exact);
            assert_eq!(
                lling_wfst_union(resource.as_raw(), resource.as_raw(), &budget, &mut output),
                LlingLlangStatus::Ok
            );
            let _: VectorWfst<char, TropicalWeight> = unary_result(output);
            output = sentinel;
        }

        let mut bad_budget = canonical_budget();
        bad_budget.header.flags = LLING_BUDGET_STATES;
        assert_eq!(
            lling_wfst_concat(
                resource.as_raw(),
                resource.as_raw(),
                &bad_budget,
                &mut output
            ),
            LlingLlangStatus::InvalidArgument
        );
        assert_eq!(output, sentinel);
        let mut byte_graph = VectorWfst::<u8, TropicalWeight>::new();
        let byte_start = byte_graph.add_state();
        byte_graph.set_start(byte_start);
        byte_graph.set_final(byte_start, TropicalWeight::one());
        let byte_resource = export_native_wfst(&byte_graph).unwrap();
        assert_eq!(
            lling_wfst_union(
                resource.as_raw(),
                byte_resource.as_raw(),
                &canonical_budget(),
                &mut output
            ),
            LlingLlangStatus::IncompatibleResource
        );
        assert_eq!(output, sentinel);
        let mut bool_graph = VectorWfst::<char, BoolWeight>::new();
        let bool_start = bool_graph.add_state();
        bool_graph.set_start(bool_start);
        bool_graph.set_final(bool_start, BoolWeight::one());
        let bool_resource = export_native_wfst(&bool_graph).unwrap();
        assert_eq!(
            lling_wfst_concat(
                resource.as_raw(),
                bool_resource.as_raw(),
                &canonical_budget(),
                &mut output
            ),
            LlingLlangStatus::IncompatibleResource
        );
        assert_eq!(output, sentinel);
        assert_eq!(
            unsafe {
                lling_wfst_union_refs(
                    ptr::null(),
                    &resource.as_raw(),
                    &canonical_budget(),
                    &mut output,
                )
            },
            LlingLlangStatus::NullPointer
        );
        assert_eq!(output, sentinel);
        assert_eq!(
            lling_wfst_closure(resource.as_raw(), &canonical_budget(), ptr::null_mut()),
            LlingLlangStatus::NullPointer
        );

        // Kleene plus must accept epsilon when its operand already does.
        let mut epsilon = VectorWfst::<char, TropicalWeight>::new();
        let only = epsilon.add_state();
        epsilon.set_start(only);
        epsilon.set_final(only, TropicalWeight::one());
        let epsilon_resource = export_native_wfst(&epsilon).unwrap();
        assert_eq!(
            lling_wfst_closure_plus(epsilon_resource.as_raw(), &canonical_budget(), &mut output),
            LlingLlangStatus::Ok
        );
        let result: VectorWfst<char, TropicalWeight> = unary_result(output);
        assert!(result.is_final(result.start()));
        assert!(result
            .transitions(result.start())
            .iter()
            .any(|arc| arc.to == result.start()));
    }

    #[test]
    fn c_builder_exports_batched_resource_arcs() {
        let mut builder = ptr::null_mut();
        assert_eq!(lling_wfst_builder_new(&mut builder), LlingLlangStatus::Ok);
        let mut s0 = 0;
        let mut s1 = 0;
        assert_eq!(
            lling_wfst_builder_add_state(builder, &mut s0),
            LlingLlangStatus::Ok
        );
        assert_eq!(
            lling_wfst_builder_add_state(builder, &mut s1),
            LlingLlangStatus::Ok
        );
        assert_eq!(
            lling_wfst_builder_set_start(builder, s0),
            LlingLlangStatus::Ok
        );
        assert_eq!(
            lling_wfst_builder_set_final(builder, s1, 0.0),
            LlingLlangStatus::Ok
        );
        assert_eq!(
            lling_wfst_builder_add_arc(builder, s0, 'a' as u64, 1, 'b' as u64, 1, s1, 0.25),
            LlingLlangStatus::Ok
        );
        let mut wfst = ptr::null_mut();
        assert_eq!(
            lling_wfst_builder_build(builder, &mut wfst),
            LlingLlangStatus::Ok
        );
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(wfst, &mut resource) },
            LlingLlangStatus::Ok
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
