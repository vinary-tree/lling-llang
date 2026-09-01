//! Shared safety machinery for capability-negotiated dynamic providers.

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, ThreadId};

use vinary_tree_interop::{VtInterfaceId, VtResource, VtResourceVTable, VtStatus, VT_ABI_VERSION};

const MAX_PROVIDER_BYTES: usize = 16 * 1024 * 1024;
const MAX_BUFFER_ATTEMPTS: usize = 3;

#[derive(Debug)]
pub(crate) enum DynamicAbiError {
    NullResource,
    IncompatibleResourceAbi,
    Provider {
        operation: &'static str,
        status: VtStatus,
    },
    InvalidProviderOutput {
        operation: &'static str,
        reason: &'static str,
    },
    WrongThread,
    ConcurrentCall,
    ResourceLimit,
}

/// One validated, independently owned resource retain.
pub(crate) struct OwnedResource {
    raw: VtResource,
}

impl OwnedResource {
    /// Retain a borrowed resource after validating its base ABI.
    pub(crate) unsafe fn retained(resource: VtResource) -> Result<Self, DynamicAbiError> {
        let base = &*validate_base(resource)?;
        (base.retain.unwrap())(resource.context);
        Ok(Self { raw: resource })
    }

    /// Adopt an already-owned resource after validating its base ABI.
    pub(crate) unsafe fn adopted(resource: VtResource) -> Result<Self, DynamicAbiError> {
        validate_base(resource)?;
        Ok(Self { raw: resource })
    }

    pub(crate) fn raw(&self) -> VtResource {
        self.raw
    }
}

impl Drop for OwnedResource {
    fn drop(&mut self) {
        if self.raw.is_null() {
            return;
        }
        // SAFETY: both constructors validate the base vtable and establish
        // exactly one owned retain. The resource stays live through this call.
        unsafe { ((*self.raw.vtable).release.unwrap())(self.raw.context) };
    }
}

enum CallPolicy {
    Parallel,
    Serial,
    ThreadBound(ThreadId),
}

/// Runtime-checked callback admission shared by every dynamic capability.
pub(crate) struct CallbackGate {
    policy: CallPolicy,
    in_call: AtomicBool,
}

struct CallGuard<'a> {
    active: Option<&'a AtomicBool>,
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        if let Some(active) = self.active {
            active.store(false, Ordering::Release);
        }
    }
}

impl CallbackGate {
    pub(crate) fn from_flags(flags: u64, thread_bound: u64, parallel_reentrant: u64) -> Self {
        let policy = if flags & parallel_reentrant != 0 {
            CallPolicy::Parallel
        } else if flags & thread_bound != 0 {
            CallPolicy::ThreadBound(thread::current().id())
        } else {
            CallPolicy::Serial
        };
        Self {
            policy,
            in_call: AtomicBool::new(false),
        }
    }

    pub(crate) fn invoke<T, E>(&self, callback: impl FnOnce() -> Result<T, E>) -> Result<T, E>
    where
        E: From<DynamicAbiError>,
    {
        let _guard = self.enter().map_err(E::from)?;
        callback()
    }

    fn enter(&self) -> Result<CallGuard<'_>, DynamicAbiError> {
        match &self.policy {
            CallPolicy::Parallel => Ok(CallGuard { active: None }),
            CallPolicy::ThreadBound(owner) if *owner != thread::current().id() => {
                Err(DynamicAbiError::WrongThread)
            }
            CallPolicy::Serial | CallPolicy::ThreadBound(_) => self
                .in_call
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .map(|_| CallGuard {
                    active: Some(&self.in_call),
                })
                .map_err(|_| DynamicAbiError::ConcurrentCall),
        }
    }
}

pub(crate) unsafe fn validate_base(
    resource: VtResource,
) -> Result<*const VtResourceVTable, DynamicAbiError> {
    if resource.is_null() {
        return Err(DynamicAbiError::NullResource);
    }
    let base = &*resource.vtable;
    if base.struct_size < size_of::<VtResourceVTable>()
        || base.abi_version != VT_ABI_VERSION
        || base.reserved != 0
        || base.retain.is_none()
        || base.release.is_none()
        || base.query_interface.is_none()
    {
        return Err(DynamicAbiError::IncompatibleResourceAbi);
    }
    Ok(resource.vtable)
}

pub(crate) unsafe fn query_interface<T>(
    resource: VtResource,
    interface_id: &VtInterfaceId,
    minimum_version: u32,
    operation: &'static str,
) -> Result<Option<*const T>, DynamicAbiError> {
    let base = &*resource.vtable;
    let mut output: *const c_void = std::ptr::null();
    let raw = (base.query_interface.unwrap())(
        resource.context,
        interface_id,
        minimum_version,
        &mut output,
    );
    let status = decode_status(operation, raw)?;
    if status != VtStatus::Ok {
        if !output.is_null() {
            return Err(DynamicAbiError::InvalidProviderOutput {
                operation,
                reason: "failed query_interface call wrote an output pointer",
            });
        }
        return if status == VtStatus::Unsupported {
            Ok(None)
        } else {
            Err(DynamicAbiError::Provider { operation, status })
        };
    }
    if output.is_null() {
        return Err(DynamicAbiError::InvalidProviderOutput {
            operation,
            reason: "query_interface returned a null vtable",
        });
    }
    Ok(Some(output.cast()))
}

pub(crate) fn decode_status(
    operation: &'static str,
    raw: u32,
) -> Result<VtStatus, DynamicAbiError> {
    VtStatus::from_raw(raw).ok_or(DynamicAbiError::InvalidProviderOutput {
        operation,
        reason: "status is outside the published range",
    })
}

pub(crate) fn status_ok(operation: &'static str, raw: u32) -> Result<(), DynamicAbiError> {
    let status = decode_status(operation, raw)?;
    if status == VtStatus::Ok {
        Ok(())
    } else {
        Err(DynamicAbiError::Provider { operation, status })
    }
}

pub(crate) fn decode_bool(operation: &'static str, raw: u8) -> Result<bool, DynamicAbiError> {
    match raw {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(DynamicAbiError::InvalidProviderOutput {
            operation,
            reason: "boolean is not zero or one",
        }),
    }
}

pub(crate) fn copy_provider_bytes(
    gate: &CallbackGate,
    operation: &'static str,
    callback: impl Fn(*mut u8, usize, *mut usize, *mut usize) -> u32,
) -> Result<Vec<u8>, DynamicAbiError> {
    gate.invoke(|| {
        let mut required = 0;
        let mut written = usize::MAX;
        status_ok(
            operation,
            callback(std::ptr::null_mut(), 0, &mut written, &mut required),
        )?;
        if written != 0 {
            return Err(DynamicAbiError::InvalidProviderOutput {
                operation,
                reason: "size query reported bytes written",
            });
        }
        for _ in 0..MAX_BUFFER_ATTEMPTS {
            if required > MAX_PROVIDER_BYTES {
                return Err(DynamicAbiError::ResourceLimit);
            }
            let mut output = vec![0_u8; required];
            let mut next_written = usize::MAX;
            let mut next_required = usize::MAX;
            status_ok(
                operation,
                callback(
                    output.as_mut_ptr(),
                    output.len(),
                    &mut next_written,
                    &mut next_required,
                ),
            )?;
            if next_written > output.len() || next_written > next_required {
                return Err(DynamicAbiError::InvalidProviderOutput {
                    operation,
                    reason: "buffer counts exceed the supplied or required size",
                });
            }
            if next_required <= output.len() {
                if next_written != next_required {
                    return Err(DynamicAbiError::InvalidProviderOutput {
                        operation,
                        reason: "final buffer write was shorter than the required size",
                    });
                }
                output.truncate(next_required);
                return Ok(output);
            }
            required = next_required;
        }
        Err(DynamicAbiError::InvalidProviderOutput {
            operation,
            reason: "required byte count did not stabilize",
        })
    })
}
