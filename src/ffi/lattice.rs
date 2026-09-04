//! C facade for the validated host-defined lattice consumer.

use crate::dynamic_lattice::{DynamicLatticeError, DynamicLatticeValue};
use vinary_tree_interop::{VtInterfaceId, VtResource};

use super::{boundary, copy_bytes_to_c, required_mut, set_error, LlingLlangStatus};

/// Opaque same-thread handle for one immutable host-defined lattice value.
pub struct LlingLatticeValue {
    value: DynamicLatticeValue,
}

fn map_lattice_error(error: DynamicLatticeError) -> LlingLlangStatus {
    set_error(error.to_string());
    match error {
        DynamicLatticeError::NullResource => LlingLlangStatus::NullPointer,
        DynamicLatticeError::IncompatibleResourceAbi
        | DynamicLatticeError::MissingLatticeInterface
        | DynamicLatticeError::IncompatibleInterface(_)
        | DynamicLatticeError::MissingCapability(_) => LlingLlangStatus::IncompatibleResource,
        DynamicLatticeError::DomainMismatch | DynamicLatticeError::InvalidArgument(_) => {
            LlingLlangStatus::InvalidArgument
        }
        DynamicLatticeError::ResourceLimit => LlingLlangStatus::LimitExceeded,
        DynamicLatticeError::Provider { .. }
        | DynamicLatticeError::InvalidProviderOutput { .. }
        | DynamicLatticeError::WrongThread
        | DynamicLatticeError::ConcurrentCall
        | DynamicLatticeError::LawViolation(_) => LlingLlangStatus::ProviderError,
    }
}

fn lattice_value(
    value: *const LlingLatticeValue,
) -> Result<&'static DynamicLatticeValue, LlingLlangStatus> {
    if value.is_null() {
        set_error("lattice value is null");
        Err(LlingLlangStatus::NullPointer)
    } else {
        // SAFETY: the C caller promises this is a live opaque handle.
        Ok(unsafe { &(*value).value })
    }
}

fn write_lattice_value(
    output: *mut *mut LlingLatticeValue,
    create: impl FnOnce() -> Result<DynamicLatticeValue, DynamicLatticeError>,
) -> Result<(), LlingLlangStatus> {
    let output = required_mut(output, "out_value")?;
    *output = std::ptr::null_mut();
    let value = create().map_err(map_lattice_error)?;
    *output = Box::into_raw(Box::new(LlingLatticeValue { value }));
    Ok(())
}

unsafe fn lattice_values(
    values: *const *const LlingLatticeValue,
    count: usize,
) -> Result<Vec<DynamicLatticeValue>, LlingLlangStatus> {
    if count != 0 && values.is_null() {
        set_error("lattice values are null with nonzero count");
        return Err(LlingLlangStatus::NullPointer);
    }
    let pointers = if count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(values, count)
    };
    pointers
        .iter()
        .map(|pointer| lattice_value(*pointer).cloned())
        .collect()
}

/// Retain and validate one `vt.lattice.val.1` resource.
///
/// The resulting handle is same-thread. Foreign runtimes must not move it to
/// a worker thread, even when the provider advertises parallel reentrancy.
///
/// # Safety
/// `resource` must point to a live resource for this call and `out_value` must
/// point to writable storage. On success the caller owns the returned handle.
#[no_mangle]
pub unsafe extern "C" fn lling_lattice_open(
    resource: *const VtResource,
    out_value: *mut *mut LlingLatticeValue,
) -> LlingLlangStatus {
    boundary(|| {
        if resource.is_null() {
            set_error("resource is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        write_lattice_value(out_value, || DynamicLatticeValue::borrow_raw(*resource))
    })
}

/// Release an imported or computed lattice value. Null is accepted.
///
/// # Safety
/// A non-null pointer must be one live handle returned by this module and must
/// not already have been freed.
#[no_mangle]
pub unsafe extern "C" fn lling_lattice_free(value: *mut LlingLatticeValue) {
    if !value.is_null() {
        drop(Box::from_raw(value));
    }
}

/// Copy the value's stable provider-defined domain identifier.
#[no_mangle]
pub extern "C" fn lling_lattice_domain_id(
    value: *const LlingLatticeValue,
    out_domain: *mut VtInterfaceId,
) -> LlingLlangStatus {
    boundary(|| {
        let value = lattice_value(value)?;
        *required_mut(out_domain, "out_domain")? = value.domain_id();
        Ok(())
    })
}

/// Copy the value's validated capability flags.
#[no_mangle]
pub extern "C" fn lling_lattice_flags(
    value: *const LlingLatticeValue,
    out_flags: *mut u64,
) -> LlingLlangStatus {
    boundary(|| {
        let value = lattice_value(value)?;
        *required_mut(out_flags, "out_flags")? = value.flags();
        Ok(())
    })
}

fn binary_lattice(
    left: *const LlingLatticeValue,
    right: *const LlingLatticeValue,
    output: *mut *mut LlingLatticeValue,
    join: bool,
) -> LlingLlangStatus {
    boundary(|| {
        let left = lattice_value(left)?;
        let right = lattice_value(right)?;
        write_lattice_value(output, || {
            if join {
                left.join(right)
            } else {
                left.meet(right)
            }
        })
    })
}

/// Return the least upper bound of two same-domain values.
#[no_mangle]
pub extern "C" fn lling_lattice_join(
    left: *const LlingLatticeValue,
    right: *const LlingLatticeValue,
    out_value: *mut *mut LlingLatticeValue,
) -> LlingLlangStatus {
    binary_lattice(left, right, out_value, true)
}

/// Return the greatest lower bound of two same-domain values.
#[no_mangle]
pub extern "C" fn lling_lattice_meet(
    left: *const LlingLatticeValue,
    right: *const LlingLatticeValue,
    out_value: *mut *mut LlingLatticeValue,
) -> LlingLlangStatus {
    binary_lattice(left, right, out_value, false)
}

/// Compare two same-domain values for exact semantic equality.
#[no_mangle]
pub extern "C" fn lling_lattice_equal(
    left: *const LlingLatticeValue,
    right: *const LlingLatticeValue,
    out_equal: *mut u8,
) -> LlingLlangStatus {
    boundary(|| {
        let left = lattice_value(left)?;
        let right = lattice_value(right)?;
        *required_mut(out_equal, "out_equal")? =
            u8::from(left.equal(right).map_err(map_lattice_error)?);
        Ok(())
    })
}

unsafe fn write_lattice_bytes(
    value: *const LlingLatticeValue,
    out_bytes: *mut u8,
    capacity: usize,
    out_written: *mut usize,
    out_required: *mut usize,
    diagnostic: bool,
) -> LlingLlangStatus {
    boundary(|| {
        let value = lattice_value(value)?;
        let bytes = if diagnostic {
            value.diagnostic().map_err(map_lattice_error)?.into_bytes()
        } else {
            value.stable_bytes().map_err(map_lattice_error)?
        };
        copy_bytes_to_c(&bytes, out_bytes, capacity, out_written, out_required)
    })
}

/// Copy canonical bytes into caller-owned storage.
///
/// # Safety
/// When `capacity` is nonzero, `out_bytes` must address at least `capacity`
/// writable bytes. The handle and count outputs must be live for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_lattice_stable_bytes(
    value: *const LlingLatticeValue,
    out_bytes: *mut u8,
    capacity: usize,
    out_written: *mut usize,
    out_required: *mut usize,
) -> LlingLlangStatus {
    write_lattice_bytes(value, out_bytes, capacity, out_written, out_required, false)
}

/// Copy the provider's advisory UTF-8 diagnostic into caller-owned storage.
///
/// # Safety
/// The pointer requirements are identical to [`lling_lattice_stable_bytes`].
#[no_mangle]
pub unsafe extern "C" fn lling_lattice_diagnostic(
    value: *const LlingLatticeValue,
    out_bytes: *mut u8,
    capacity: usize,
    out_written: *mut usize,
    out_required: *mut usize,
) -> LlingLlangStatus {
    write_lattice_bytes(value, out_bytes, capacity, out_written, out_required, true)
}

unsafe fn fold_lattice(
    receiver: *const LlingLatticeValue,
    others: *const *const LlingLatticeValue,
    count: usize,
    output: *mut *mut LlingLatticeValue,
    join: bool,
) -> LlingLlangStatus {
    boundary(|| {
        let receiver = lattice_value(receiver)?;
        let others = lattice_values(others, count)?;
        write_lattice_value(output, || {
            if join {
                receiver.join_many(&others)
            } else {
                receiver.meet_many(&others)
            }
        })
    })
}

/// Fold joins over a bounded sequence of same-domain value handles.
///
/// # Safety
/// For nonzero `count`, `others` must address `count` live handle pointers.
#[no_mangle]
pub unsafe extern "C" fn lling_lattice_join_many(
    receiver: *const LlingLatticeValue,
    others: *const *const LlingLatticeValue,
    count: usize,
    out_value: *mut *mut LlingLatticeValue,
) -> LlingLlangStatus {
    fold_lattice(receiver, others, count, out_value, true)
}

/// Fold meets over a bounded sequence of same-domain value handles.
///
/// # Safety
/// For nonzero `count`, `others` must address `count` live handle pointers.
#[no_mangle]
pub unsafe extern "C" fn lling_lattice_meet_many(
    receiver: *const LlingLatticeValue,
    others: *const *const LlingLatticeValue,
    count: usize,
    out_value: *mut *mut LlingLatticeValue,
) -> LlingLlangStatus {
    fold_lattice(receiver, others, count, out_value, false)
}

/// Probe the lattice laws over a bounded representative sample.
///
/// # Safety
/// For nonzero `count`, `values` must address `count` live handle pointers.
#[no_mangle]
pub unsafe extern "C" fn lling_lattice_validate_laws(
    values: *const *const LlingLatticeValue,
    count: usize,
) -> LlingLlangStatus {
    boundary(|| {
        let values = lattice_values(values, count)?;
        DynamicLatticeValue::validate_laws(&values).map_err(map_lattice_error)
    })
}
