//! Safe consumer for host-defined immutable lattice values.
//!
//! Native Rust lattice implementations remain monomorphized through
//! `llattice::Lattice`. This module is the fallible dynamic path for values
//! owned by another runtime and exposed through `vt.lattice.val.1`.

use std::fmt;
use std::marker::PhantomData;
use std::mem::size_of;
use std::rc::Rc;
use std::sync::Arc;

use vinary_tree_interop::{
    lattice_flags, VtInterfaceId, VtLatticeVTable, VtResource, VtStatus, VT_LATTICE_INTERFACE_ID,
    VT_LATTICE_INTERFACE_VERSION, VT_RECOMMENDED_LATTICE_BATCH,
};

use crate::dynamic_abi::{
    copy_provider_bytes, decode_bool, decode_status, query_interface, status_ok, CallbackGate,
    DynamicAbiError, OwnedResource,
};

const MAX_LAW_SAMPLES: usize = 16;

/// Failure while validating or invoking a host-defined lattice value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DynamicLatticeError {
    /// One or both words of the supplied resource were null.
    NullResource,
    /// The resource base contract was incomplete or incompatible.
    IncompatibleResourceAbi,
    /// The resource did not publish `vt.lattice.val.1`.
    MissingLatticeInterface,
    /// The lattice capability was incomplete or internally inconsistent.
    IncompatibleInterface(&'static str),
    /// An optional operation required by the caller was not advertised.
    MissingCapability(&'static str),
    /// A provider returned a portable failure status.
    Provider {
        /// Operation that observed the failure.
        operation: &'static str,
        /// Portable status returned by the provider.
        status: VtStatus,
    },
    /// A provider returned output outside the published wire contract.
    InvalidProviderOutput {
        /// Operation that observed the malformed output.
        operation: &'static str,
        /// Stable diagnostic explaining the rejection.
        reason: &'static str,
    },
    /// Operands belong to different lattice domains.
    DomainMismatch,
    /// A thread-bound provider was invoked from a different thread.
    WrongThread,
    /// A non-reentrant provider was already executing a callback.
    ConcurrentCall,
    /// A provider-requested allocation exceeded the defensive byte limit.
    ResourceLimit,
    /// A representative conformance probe disproved a lattice law.
    LawViolation(&'static str),
    /// A caller supplied an invalid bounded-validation input.
    InvalidArgument(&'static str),
}

impl fmt::Display for DynamicLatticeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullResource => formatter.write_str("lattice resource is null"),
            Self::IncompatibleResourceAbi => {
                formatter.write_str("lattice resource uses an incompatible base ABI")
            }
            Self::MissingLatticeInterface => {
                formatter.write_str("resource has no dynamic-lattice interface")
            }
            Self::IncompatibleInterface(reason) => {
                write!(
                    formatter,
                    "incompatible dynamic-lattice interface: {reason}"
                )
            }
            Self::MissingCapability(name) => {
                write!(formatter, "dynamic lattice does not provide {name}")
            }
            Self::Provider { operation, status } => write!(
                formatter,
                "lattice provider returned {status:?} from {operation}"
            ),
            Self::InvalidProviderOutput { operation, reason } => write!(
                formatter,
                "lattice provider returned invalid {operation} output: {reason}"
            ),
            Self::DomainMismatch => {
                formatter.write_str("lattice operands belong to different domains")
            }
            Self::WrongThread => {
                formatter.write_str("thread-bound lattice provider was invoked from another thread")
            }
            Self::ConcurrentCall => {
                formatter.write_str("non-reentrant lattice provider already has an active callback")
            }
            Self::ResourceLimit => formatter
                .write_str("lattice provider requested a byte buffer above the defensive limit"),
            Self::LawViolation(law) => write!(formatter, "lattice provider violated {law}"),
            Self::InvalidArgument(reason) => {
                write!(formatter, "invalid lattice argument: {reason}")
            }
        }
    }
}

impl std::error::Error for DynamicLatticeError {}

impl From<DynamicAbiError> for DynamicLatticeError {
    fn from(error: DynamicAbiError) -> Self {
        match error {
            DynamicAbiError::NullResource => Self::NullResource,
            DynamicAbiError::IncompatibleResourceAbi => Self::IncompatibleResourceAbi,
            DynamicAbiError::Provider { operation, status } => Self::Provider { operation, status },
            DynamicAbiError::InvalidProviderOutput { operation, reason } => {
                Self::InvalidProviderOutput { operation, reason }
            }
            DynamicAbiError::WrongThread => Self::WrongThread,
            DynamicAbiError::ConcurrentCall => Self::ConcurrentCall,
            DynamicAbiError::ResourceLimit => Self::ResourceLimit,
        }
    }
}

mod access_sealed {
    pub trait Sealed {
        const REQUIRE_PARALLEL: bool;
    }
}

/// Type-level access mode for a dynamic lattice value.
pub trait LatticeAccess: access_sealed::Sealed + 'static {}

/// Same-thread access mode used for every newly imported lattice value.
pub struct LocalLatticeAccess(PhantomData<Rc<()>>);

impl access_sealed::Sealed for LocalLatticeAccess {
    const REQUIRE_PARALLEL: bool = false;
}
impl LatticeAccess for LocalLatticeAccess {}

/// Cross-thread access mode available only for parallel-reentrant providers.
pub struct ParallelLatticeAccess(());

impl access_sealed::Sealed for ParallelLatticeAccess {
    const REQUIRE_PARALLEL: bool = true;
}
impl LatticeAccess for ParallelLatticeAccess {}

struct DynamicLatticeInner {
    resource: OwnedResource,
    lattice: *const VtLatticeVTable,
    gate: CallbackGate,
}

// The raw pointers remain valid for the owned resource lifetime. Public
// cross-thread access exists only through `ParallelLatticeAccess`, after the
// provider's runtime claim has been validated.
unsafe impl Send for DynamicLatticeInner {}
unsafe impl Sync for DynamicLatticeInner {}

impl DynamicLatticeInner {
    fn table(&self) -> &VtLatticeVTable {
        // SAFETY: construction validates the table and the owned resource
        // keeps the provider's immutable vtable alive.
        unsafe { &*self.lattice }
    }

    fn raw(&self) -> VtResource {
        self.resource.raw()
    }
}

/// One owned, validated host-defined lattice value.
pub struct LatticeValue<M: LatticeAccess = LocalLatticeAccess> {
    inner: Arc<DynamicLatticeInner>,
    access: PhantomData<M>,
}

/// Same-thread dynamic lattice value.
pub type DynamicLatticeValue = LatticeValue<LocalLatticeAccess>;

/// Dynamic lattice value whose provider is parallel and reentrant.
pub type ParallelDynamicLatticeValue = LatticeValue<ParallelLatticeAccess>;

impl<M: LatticeAccess> Clone for LatticeValue<M> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            access: PhantomData,
        }
    }
}

impl<M: LatticeAccess> fmt::Debug for LatticeValue<M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LatticeValue")
            .field("domain_id", &self.domain_id())
            .field("flags", &self.flags())
            .finish_non_exhaustive()
    }
}

impl DynamicLatticeValue {
    /// Borrow a raw lattice resource and retain one independent lifetime.
    ///
    /// # Safety
    ///
    /// `resource` must remain live for this call and every base/capability
    /// callback must follow the contracts published by `vinary-tree-interop`.
    /// The returned value owns a new retain; the caller keeps ownership of the
    /// original resource.
    pub unsafe fn borrow_raw(resource: VtResource) -> Result<Self, DynamicLatticeError> {
        Self::from_resource(OwnedResource::retained(resource)?)
    }

    /// Promote this value to a cross-thread wrapper after validating the
    /// provider's parallel-reentrant claim.
    pub fn parallel(&self) -> Result<ParallelDynamicLatticeValue, DynamicLatticeError> {
        if self.flags() & lattice_flags::PARALLEL_REENTRANT == 0 {
            return Err(DynamicLatticeError::MissingCapability(
                "parallel reentrancy",
            ));
        }
        Ok(LatticeValue {
            inner: Arc::clone(&self.inner),
            access: PhantomData,
        })
    }
}

impl<M: LatticeAccess> LatticeValue<M> {
    fn from_resource(resource: OwnedResource) -> Result<Self, DynamicLatticeError> {
        let raw = resource.raw();
        // SAFETY: `OwnedResource` has already validated the live base vtable.
        let lattice = unsafe {
            query_interface::<VtLatticeVTable>(
                raw,
                &VT_LATTICE_INTERFACE_ID,
                VT_LATTICE_INTERFACE_VERSION,
                "lattice query_interface",
            )?
        }
        .ok_or(DynamicLatticeError::MissingLatticeInterface)?;
        // SAFETY: successful discovery returned a non-null pointer owned by
        // the retained resource.
        let table = unsafe { &*lattice };
        validate_lattice(table)?;
        if M::REQUIRE_PARALLEL && table.flags & lattice_flags::PARALLEL_REENTRANT == 0 {
            return Err(DynamicLatticeError::IncompatibleInterface(
                "operation result lost parallel reentrancy",
            ));
        }
        let gate = CallbackGate::from_flags(
            table.flags,
            lattice_flags::THREAD_BOUND,
            lattice_flags::PARALLEL_REENTRANT,
        );
        Ok(Self {
            inner: Arc::new(DynamicLatticeInner {
                resource,
                lattice,
                gate,
            }),
            access: PhantomData,
        })
    }

    /// Provider-defined identifier for both value representation and laws.
    pub fn domain_id(&self) -> VtInterfaceId {
        self.inner.table().domain_id
    }

    /// Validated callback and threading flags.
    pub fn flags(&self) -> u64 {
        self.inner.table().flags
    }

    /// Return whether canonical stable encoding is available.
    pub fn supports_stable_bytes(&self) -> bool {
        self.flags() & lattice_flags::STABLE_BYTES != 0 && self.inner.table().stable_bytes.is_some()
    }

    /// Return the least upper bound of two values in the same domain.
    pub fn join(&self, other: &Self) -> Result<Self, DynamicLatticeError> {
        self.binary_value("join", other, self.inner.table().join.unwrap())
    }

    /// Return the greatest lower bound of two values in the same domain.
    pub fn meet(&self, other: &Self) -> Result<Self, DynamicLatticeError> {
        self.binary_value("meet", other, self.inner.table().meet.unwrap())
    }

    /// Compare two values for exact semantic equality.
    pub fn equal(&self, other: &Self) -> Result<bool, DynamicLatticeError> {
        self.ensure_domain(other)?;
        self.inner.gate.invoke(|| {
            let mut output = u8::MAX;
            let raw_other = other.inner.raw();
            let raw = unsafe {
                (self.inner.table().equal.unwrap())(
                    self.inner.raw().context,
                    &raw_other,
                    &mut output,
                )
            };
            status_ok("equal", raw)?;
            Ok(decode_bool("equal", output)?)
        })
    }

    /// Return the canonical byte representation of this value.
    pub fn stable_bytes(&self) -> Result<Vec<u8>, DynamicLatticeError> {
        if !self.supports_stable_bytes() {
            return Err(DynamicLatticeError::MissingCapability("stable bytes"));
        }
        let callback = self.inner.table().stable_bytes.unwrap();
        copy_provider_bytes(
            &self.inner.gate,
            "stable_bytes",
            |bytes, capacity, written, required| unsafe {
                callback(self.inner.raw().context, bytes, capacity, written, required)
            },
        )
        .map_err(Into::into)
    }

    /// Return the provider's advisory UTF-8 diagnostic.
    pub fn diagnostic(&self) -> Result<String, DynamicLatticeError> {
        let callback = self
            .inner
            .table()
            .diagnostic
            .ok_or(DynamicLatticeError::MissingCapability("diagnostic"))?;
        let bytes = copy_provider_bytes(
            &self.inner.gate,
            "diagnostic",
            |buffer, capacity, written, required| unsafe {
                callback(
                    self.inner.raw().context,
                    buffer,
                    capacity,
                    written,
                    required,
                )
            },
        )?;
        String::from_utf8(bytes).map_err(|_| DynamicLatticeError::InvalidProviderOutput {
            operation: "diagnostic",
            reason: "diagnostic is not UTF-8",
        })
    }

    /// Fold joins using bounded native batches when advertised.
    pub fn join_many(&self, others: &[Self]) -> Result<Self, DynamicLatticeError> {
        self.fold_many(others, FoldOperation::Join)
    }

    /// Fold meets using bounded native batches when advertised.
    pub fn meet_many(&self, others: &[Self]) -> Result<Self, DynamicLatticeError> {
        self.fold_many(others, FoldOperation::Meet)
    }

    /// Probe associativity, commutativity, idempotence, and absorption over a
    /// bounded representative sample.
    ///
    /// Finite testing can disprove a false provider claim but cannot prove a
    /// universal law. Include boundary values and workload-representative
    /// values. The cap prevents an untrusted integration from requesting
    /// unbounded cubic work.
    pub fn validate_laws(samples: &[Self]) -> Result<(), DynamicLatticeError> {
        if samples.is_empty() {
            return Err(DynamicLatticeError::InvalidArgument(
                "law validation requires at least one sample",
            ));
        }
        if samples.len() > MAX_LAW_SAMPLES {
            return Err(DynamicLatticeError::InvalidArgument(
                "law-validation sample exceeds the bounded probe size",
            ));
        }
        let domain = samples[0].domain_id();
        if samples.iter().any(|sample| sample.domain_id() != domain) {
            return Err(DynamicLatticeError::DomainMismatch);
        }
        for value in samples {
            require_equal(&value.join(value)?, value, "join idempotence")?;
            require_equal(&value.meet(value)?, value, "meet idempotence")?;
        }
        for left in samples {
            for right in samples {
                require_equal(&left.join(right)?, &right.join(left)?, "join commutativity")?;
                require_equal(&left.meet(right)?, &right.meet(left)?, "meet commutativity")?;
                require_equal(&left.join(&left.meet(right)?)?, left, "join absorption")?;
                require_equal(&left.meet(&left.join(right)?)?, left, "meet absorption")?;
            }
        }
        for first in samples {
            for second in samples {
                for third in samples {
                    require_equal(
                        &first.join(second)?.join(third)?,
                        &first.join(&second.join(third)?)?,
                        "join associativity",
                    )?;
                    require_equal(
                        &first.meet(second)?.meet(third)?,
                        &first.meet(&second.meet(third)?)?,
                        "meet associativity",
                    )?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn raw_resource(&self) -> VtResource {
        self.inner.raw()
    }

    fn ensure_domain(&self, other: &Self) -> Result<(), DynamicLatticeError> {
        if self.domain_id() == other.domain_id() {
            Ok(())
        } else {
            Err(DynamicLatticeError::DomainMismatch)
        }
    }

    fn binary_value(
        &self,
        operation: &'static str,
        other: &Self,
        callback: unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const VtResource,
            *mut VtResource,
        ) -> u32,
    ) -> Result<Self, DynamicLatticeError> {
        self.ensure_domain(other)?;
        let raw_other = other.inner.raw();
        self.output_value(operation, |output| unsafe {
            callback(self.inner.raw().context, &raw_other, output)
        })
    }

    fn output_value(
        &self,
        operation: &'static str,
        callback: impl FnOnce(*mut VtResource) -> u32,
    ) -> Result<Self, DynamicLatticeError> {
        let raw = self.inner.gate.invoke(|| {
            let mut output = VtResource::NULL;
            let status = decode_status(operation, callback(&mut output))?;
            if status != VtStatus::Ok {
                if !output.is_null() {
                    return Err(DynamicLatticeError::InvalidProviderOutput {
                        operation,
                        reason: "failed operation wrote an output resource",
                    });
                }
                return Err(DynamicLatticeError::Provider { operation, status });
            }
            if output.is_null() {
                return Err(DynamicLatticeError::InvalidProviderOutput {
                    operation,
                    reason: "successful operation returned a null resource",
                });
            }
            Ok(output)
        })?;
        // SAFETY: success transfers one owned resource through `raw`.
        let resource = unsafe { OwnedResource::adopted(raw)? };
        let value = Self::from_resource(resource)?;
        if value.domain_id() != self.domain_id() {
            return Err(DynamicLatticeError::InvalidProviderOutput {
                operation,
                reason: "operation result changed the lattice domain",
            });
        }
        Ok(value)
    }

    fn fold_many(
        &self,
        others: &[Self],
        operation: FoldOperation,
    ) -> Result<Self, DynamicLatticeError> {
        for other in others {
            self.ensure_domain(other)?;
        }
        if others.is_empty() {
            return Ok(self.clone());
        }
        if !self.supports_batch(operation) {
            let mut accumulator = self.clone();
            for other in others {
                accumulator = match operation {
                    FoldOperation::Join => accumulator.join(other)?,
                    FoldOperation::Meet => accumulator.meet(other)?,
                };
            }
            return Ok(accumulator);
        }

        let mut accumulator = self.clone();
        for chunk in others.chunks(VT_RECOMMENDED_LATTICE_BATCH.max(1)) {
            if accumulator.supports_batch(operation) {
                let callback = accumulator.batch_callback(operation).unwrap();
                let resources: Vec<_> = chunk.iter().map(Self::raw_resource).collect();
                let context = accumulator.inner.raw().context;
                accumulator = accumulator.output_value(operation.name(), |output| unsafe {
                    callback(context, resources.as_ptr(), resources.len(), output)
                })?;
            } else {
                for other in chunk {
                    accumulator = match operation {
                        FoldOperation::Join => accumulator.join(other)?,
                        FoldOperation::Meet => accumulator.meet(other)?,
                    };
                }
            }
        }
        Ok(accumulator)
    }

    fn supports_batch(&self, operation: FoldOperation) -> bool {
        self.flags() & lattice_flags::BATCH != 0 && self.batch_callback(operation).is_some()
    }

    fn batch_callback(
        &self,
        operation: FoldOperation,
    ) -> Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const VtResource,
            usize,
            *mut VtResource,
        ) -> u32,
    > {
        match operation {
            FoldOperation::Join => self.inner.table().join_many,
            FoldOperation::Meet => self.inner.table().meet_many,
        }
    }
}

#[derive(Clone, Copy)]
enum FoldOperation {
    Join,
    Meet,
}

impl FoldOperation {
    const fn name(self) -> &'static str {
        match self {
            Self::Join => "join_many",
            Self::Meet => "meet_many",
        }
    }
}

fn require_equal<M: LatticeAccess>(
    left: &LatticeValue<M>,
    right: &LatticeValue<M>,
    law: &'static str,
) -> Result<(), DynamicLatticeError> {
    if left.equal(right)? {
        Ok(())
    } else {
        Err(DynamicLatticeError::LawViolation(law))
    }
}

fn validate_lattice(table: &VtLatticeVTable) -> Result<(), DynamicLatticeError> {
    if table.struct_size < size_of::<VtLatticeVTable>()
        || table.interface_version < VT_LATTICE_INTERFACE_VERSION
        || table.reserved != 0
        || table.join.is_none()
        || table.meet.is_none()
        || table.equal.is_none()
    {
        return Err(DynamicLatticeError::IncompatibleInterface("base"));
    }
    if table.flags & lattice_flags::THREAD_BOUND != 0
        && table.flags & lattice_flags::PARALLEL_REENTRANT != 0
    {
        return Err(DynamicLatticeError::IncompatibleInterface(
            "mutually exclusive threading flags",
        ));
    }
    if table.flags & lattice_flags::STABLE_BYTES != 0 && table.stable_bytes.is_none() {
        return Err(DynamicLatticeError::IncompatibleInterface("stable bytes"));
    }
    if table.flags & lattice_flags::BATCH != 0
        && (table.join_many.is_none() || table.meet_many.is_none())
    {
        return Err(DynamicLatticeError::IncompatibleInterface("batch"));
    }
    Ok(())
}
