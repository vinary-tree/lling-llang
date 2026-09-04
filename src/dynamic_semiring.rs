//! Safe, lock-free consumer for host-defined semiring operation contexts.
//!
//! Native Rust semirings continue to use the monomorphized [`crate::semiring::Semiring`]
//! path. This module exists for values whose ownership belongs to another
//! runtime and therefore cannot honestly satisfy Rust's `Copy` bound. Each
//! weight owns one compact provider token and one share of the retained
//! operation context that issued it.

use std::cmp::Ordering;
use std::ffi::c_void;
use std::fmt;
use std::marker::PhantomData;
use std::mem::size_of;
use std::rc::Rc;
use std::sync::Arc;

use vinary_tree_interop::{
    semiring_flags, semiring_order, semiring_properties, VtInterfaceId, VtResource,
    VtSemiringDivisionVTable, VtSemiringNumericVTable, VtSemiringPropertiesVTable,
    VtSemiringStarVTable, VtSemiringVTable, VtSemiringValue, VtStatus,
    VT_RECOMMENDED_SEMIRING_BATCH, VT_SEMIRING_DIVISION_INTERFACE_ID,
    VT_SEMIRING_DIVISION_INTERFACE_VERSION, VT_SEMIRING_INTERFACE_ID,
    VT_SEMIRING_INTERFACE_VERSION, VT_SEMIRING_NUMERIC_INTERFACE_ID,
    VT_SEMIRING_NUMERIC_INTERFACE_VERSION, VT_SEMIRING_PROPERTIES_INTERFACE_ID,
    VT_SEMIRING_PROPERTIES_INTERFACE_VERSION, VT_SEMIRING_STAR_INTERFACE_ID,
    VT_SEMIRING_STAR_INTERFACE_VERSION,
};

use crate::dynamic_abi::{
    copy_provider_bytes, decode_bool, decode_status, query_interface, status_ok, CallbackGate,
    DynamicAbiError, OwnedResource,
};

const MAX_LAW_SAMPLES: usize = 16;
const MAX_CLOSURE_PROBE: usize = 4096;

/// Failure while validating or invoking a host-defined semiring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DynamicSemiringError {
    /// One or both words of the supplied resource were null.
    NullResource,
    /// The base resource contract was incomplete or incompatible.
    IncompatibleResourceAbi,
    /// The resource did not publish the required base semiring interface.
    MissingSemiringInterface,
    /// A discovered semiring capability had an incompatible prefix.
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
        /// Stable diagnostic explaining the rejected value.
        reason: &'static str,
    },
    /// Two operands came from different retained operation contexts.
    ContextMismatch,
    /// A thread-bound provider was invoked from a different thread.
    WrongThread,
    /// A non-reentrant provider was already executing a callback.
    ConcurrentCall,
    /// A caller-supplied numerical argument was outside its operation's domain.
    InvalidArgument(&'static str),
    /// A provider-requested allocation exceeded the defensive byte limit.
    ResourceLimit,
    /// A representative conformance probe disproved a provider-declared law.
    LawViolation(&'static str),
}

impl fmt::Display for DynamicSemiringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullResource => formatter.write_str("semiring resource is null"),
            Self::IncompatibleResourceAbi => {
                formatter.write_str("semiring resource uses an incompatible base ABI")
            }
            Self::MissingSemiringInterface => {
                formatter.write_str("resource has no dynamic-semiring interface")
            }
            Self::IncompatibleInterface(name) => {
                write!(formatter, "incompatible dynamic-semiring {name} interface")
            }
            Self::MissingCapability(name) => {
                write!(formatter, "dynamic semiring does not provide {name}")
            }
            Self::Provider { operation, status } => {
                write!(
                    formatter,
                    "semiring provider returned {status:?} from {operation}"
                )
            }
            Self::InvalidProviderOutput { operation, reason } => {
                write!(
                    formatter,
                    "semiring provider returned invalid {operation} output: {reason}"
                )
            }
            Self::ContextMismatch => {
                formatter.write_str("semiring operands belong to different operation contexts")
            }
            Self::WrongThread => formatter
                .write_str("thread-bound semiring provider was invoked from a different thread"),
            Self::ConcurrentCall => formatter
                .write_str("non-reentrant semiring provider already has an active callback"),
            Self::InvalidArgument(reason) => {
                write!(formatter, "invalid semiring argument: {reason}")
            }
            Self::ResourceLimit => formatter
                .write_str("semiring provider requested a byte buffer above the defensive limit"),
            Self::LawViolation(law) => {
                write!(
                    formatter,
                    "semiring provider violated its declared {law} law"
                )
            }
        }
    }
}

impl std::error::Error for DynamicSemiringError {}

impl From<DynamicAbiError> for DynamicSemiringError {
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

/// Validated result from a dynamic semiring's natural-order operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NaturalOrder {
    /// The left operand is better than the right operand.
    Better,
    /// The operands are equal in natural order.
    Equal,
    /// The left operand is worse than the right operand.
    Worse,
    /// The semiring does not order this operand pair.
    Incomparable,
}

impl NaturalOrder {
    /// Convert a comparable result to Rust's conventional ordering.
    pub const fn as_ordering(self) -> Option<Ordering> {
        match self {
            Self::Better => Some(Ordering::Less),
            Self::Equal => Some(Ordering::Equal),
            Self::Worse => Some(Ordering::Greater),
            Self::Incomparable => None,
        }
    }
}

mod access_sealed {
    pub trait Sealed {}
}

/// Type-level access mode for a dynamic operation context.
pub trait SemiringAccess: access_sealed::Sealed + 'static {}

/// Same-thread access mode used for every newly imported context.
///
/// The private `Rc` marker deliberately makes contexts and weights in this
/// mode neither `Send` nor `Sync`. This is the only sound default because the
/// provider's threading flags are runtime data.
pub struct LocalSemiringAccess(PhantomData<Rc<()>>);

impl access_sealed::Sealed for LocalSemiringAccess {}
impl SemiringAccess for LocalSemiringAccess {}

/// Cross-thread access mode available only after validating the provider's
/// `PARALLEL_REENTRANT` capability.
pub struct ParallelSemiringAccess(());

impl access_sealed::Sealed for ParallelSemiringAccess {}
impl SemiringAccess for ParallelSemiringAccess {}

struct DynamicSemiringInner {
    resource: OwnedResource,
    semiring: *const VtSemiringVTable,
    division: Option<*const VtSemiringDivisionVTable>,
    star: Option<*const VtSemiringStarVTable>,
    numeric: Option<*const VtSemiringNumericVTable>,
    properties: Option<*const VtSemiringPropertiesVTable>,
    gate: CallbackGate,
}

// Raw ABI pointers remain valid for the retained resource lifetime. They are
// reachable cross-thread only through `ParallelSemiringAccess`, which is
// constructed after validating PARALLEL_REENTRANT.
unsafe impl Send for DynamicSemiringInner {}
unsafe impl Sync for DynamicSemiringInner {}

impl DynamicSemiringInner {
    fn table(&self) -> &VtSemiringVTable {
        // SAFETY: capture validates the pointer, and the retained resource
        // keeps the provider-owned immutable vtable alive.
        unsafe { &*self.semiring }
    }

    fn context(&self) -> *mut c_void {
        self.resource.raw().context
    }

    fn invoke<T>(
        &self,
        callback: impl FnOnce() -> Result<T, DynamicSemiringError>,
    ) -> Result<T, DynamicSemiringError> {
        self.gate.invoke(callback)
    }

    fn output_value(
        &self,
        operation: &'static str,
        callback: impl FnOnce(*mut VtSemiringValue) -> u32,
    ) -> Result<VtSemiringValue, DynamicSemiringError> {
        self.invoke(|| {
            let mut output = VtSemiringValue {
                word0: u64::MAX,
                word1: u64::MAX,
            };
            status_ok(operation, callback(&mut output))?;
            Ok(output)
        })
    }

    fn release(&self, token: &mut VtSemiringValue) -> Result<(), DynamicSemiringError> {
        self.invoke(|| {
            Ok(status_ok(
                "release_values",
                // SAFETY: the token is owned by this exact context and remains
                // live until the callback reports successful consumption.
                unsafe { (self.table().release_values.unwrap())(self.context(), token, 1) },
            )?)
        })
    }
}

/// A validated dynamic semiring operation context.
///
/// Use [`DynamicSemiringContext`] when importing a raw provider. Convert it to
/// [`ParallelDynamicSemiringContext`] only when [`Self::parallel`] succeeds.
pub struct SemiringContext<M: SemiringAccess = LocalSemiringAccess> {
    inner: Arc<DynamicSemiringInner>,
    access: PhantomData<M>,
}

/// Same-thread dynamic semiring context.
pub type DynamicSemiringContext = SemiringContext<LocalSemiringAccess>;

/// Dynamic semiring context whose provider was validated as parallel and
/// reentrant.
pub type ParallelDynamicSemiringContext = SemiringContext<ParallelSemiringAccess>;

impl<M: SemiringAccess> Clone for SemiringContext<M> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            access: PhantomData,
        }
    }
}

impl<M: SemiringAccess> fmt::Debug for SemiringContext<M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemiringContext")
            .field("domain_id", &self.domain_id())
            .field("flags", &self.flags())
            .finish_non_exhaustive()
    }
}

impl DynamicSemiringContext {
    /// Borrow a raw resource and retain one independent operation-context
    /// lifetime.
    ///
    /// # Safety
    ///
    /// `resource` must name a live ABI resource for the complete duration of
    /// this call. Its vtable and callbacks must follow the contracts published
    /// by `vinary-tree-interop`. The returned wrapper owns a fresh retain; the
    /// caller remains responsible for its original resource ownership.
    pub unsafe fn borrow_raw(resource: VtResource) -> Result<Self, DynamicSemiringError> {
        let owned = OwnedResource::retained(resource)?;

        let semiring = query_interface::<VtSemiringVTable>(
            resource,
            &VT_SEMIRING_INTERFACE_ID,
            VT_SEMIRING_INTERFACE_VERSION,
            "base",
        )?
        .ok_or(DynamicSemiringError::MissingSemiringInterface)?;
        validate_semiring(&*semiring)?;

        let flags = (*semiring).flags;
        if flags & semiring_flags::THREAD_BOUND != 0
            && flags & semiring_flags::PARALLEL_REENTRANT != 0
        {
            return Err(DynamicSemiringError::IncompatibleInterface(
                "base threading flags",
            ));
        }

        let division = query_interface::<VtSemiringDivisionVTable>(
            resource,
            &VT_SEMIRING_DIVISION_INTERFACE_ID,
            VT_SEMIRING_DIVISION_INTERFACE_VERSION,
            "division",
        )?;
        if let Some(table) = division {
            validate_prefix(
                &*table,
                (*table).struct_size,
                (*table).interface_version,
                (*table).reserved,
                size_of::<VtSemiringDivisionVTable>(),
                VT_SEMIRING_DIVISION_INTERFACE_VERSION,
                "division",
            )?;
        }

        let star = query_interface::<VtSemiringStarVTable>(
            resource,
            &VT_SEMIRING_STAR_INTERFACE_ID,
            VT_SEMIRING_STAR_INTERFACE_VERSION,
            "star",
        )?;
        if let Some(table) = star {
            validate_prefix(
                &*table,
                (*table).struct_size,
                (*table).interface_version,
                (*table).reserved,
                size_of::<VtSemiringStarVTable>(),
                VT_SEMIRING_STAR_INTERFACE_VERSION,
                "star",
            )?;
        }

        let numeric = query_interface::<VtSemiringNumericVTable>(
            resource,
            &VT_SEMIRING_NUMERIC_INTERFACE_ID,
            VT_SEMIRING_NUMERIC_INTERFACE_VERSION,
            "numeric",
        )?;
        if let Some(table) = numeric {
            validate_prefix(
                &*table,
                (*table).struct_size,
                (*table).interface_version,
                (*table).reserved,
                size_of::<VtSemiringNumericVTable>(),
                VT_SEMIRING_NUMERIC_INTERFACE_VERSION,
                "numeric",
            )?;
        }

        let properties = query_interface::<VtSemiringPropertiesVTable>(
            resource,
            &VT_SEMIRING_PROPERTIES_INTERFACE_ID,
            VT_SEMIRING_PROPERTIES_INTERFACE_VERSION,
            "properties",
        )?;
        if let Some(table) = properties {
            validate_prefix(
                &*table,
                (*table).struct_size,
                (*table).interface_version,
                (*table).reserved,
                size_of::<VtSemiringPropertiesVTable>(),
                VT_SEMIRING_PROPERTIES_INTERFACE_VERSION,
                "properties",
            )?;
        }

        let gate = CallbackGate::from_flags(
            flags,
            semiring_flags::THREAD_BOUND,
            semiring_flags::PARALLEL_REENTRANT,
        );

        Ok(Self {
            inner: Arc::new(DynamicSemiringInner {
                resource: owned,
                semiring,
                division,
                star,
                numeric,
                properties,
                gate,
            }),
            access: PhantomData,
        })
    }

    /// Obtain a cross-thread wrapper when the provider advertises and honors
    /// parallel reentrancy.
    pub fn parallel(&self) -> Result<ParallelDynamicSemiringContext, DynamicSemiringError> {
        if self.flags() & semiring_flags::PARALLEL_REENTRANT == 0 {
            return Err(DynamicSemiringError::MissingCapability(
                "parallel reentrancy",
            ));
        }
        Ok(SemiringContext {
            inner: Arc::clone(&self.inner),
            access: PhantomData,
        })
    }
}

impl<M: SemiringAccess> SemiringContext<M> {
    /// Provider-defined semantic domain identifier.
    pub fn domain_id(&self) -> VtInterfaceId {
        self.inner.table().domain_id
    }

    /// Validated callback and threading flags.
    pub fn flags(&self) -> u64 {
        self.inner.table().flags
    }

    /// Return whether the optional right-division operation is present.
    pub fn supports_division(&self) -> bool {
        self.inner
            .division
            .is_some_and(|table| unsafe { (*table).divide.is_some() })
    }

    /// Return whether the optional weak left-division operation is present.
    pub fn supports_left_division(&self) -> bool {
        self.inner
            .division
            .is_some_and(|table| unsafe { (*table).left_divide.is_some() })
    }

    /// Return whether Kleene closure is present.
    pub fn supports_star(&self) -> bool {
        self.inner
            .star
            .is_some_and(|table| unsafe { (*table).star.is_some() })
    }

    /// Return the provider's declared algebraic-law bits.
    pub fn declared_properties(&self) -> u64 {
        self.inner
            .properties
            .map(|table| unsafe { (*table).properties })
            .unwrap_or(0)
    }

    /// Return whether the provider declares every supplied property bit.
    pub fn declares_property(&self, properties: u64) -> bool {
        self.declared_properties() & properties == properties
    }

    /// Construct the additive identity.
    pub fn zero(&self) -> Result<SemiringWeight<M>, DynamicSemiringError> {
        let token = self.inner.output_value("zero", |output| unsafe {
            (self.inner.table().zero.unwrap())(self.inner.context(), output)
        })?;
        Ok(self.own(token))
    }

    /// Construct the multiplicative identity.
    pub fn one(&self) -> Result<SemiringWeight<M>, DynamicSemiringError> {
        let token = self.inner.output_value("one", |output| unsafe {
            (self.inner.table().one.unwrap())(self.inner.context(), output)
        })?;
        Ok(self.own(token))
    }

    /// Add two values from this exact operation context.
    pub fn plus(
        &self,
        left: &SemiringWeight<M>,
        right: &SemiringWeight<M>,
    ) -> Result<SemiringWeight<M>, DynamicSemiringError> {
        let (left, right) = self.binary_tokens(left, right)?;
        let token = self.inner.output_value("plus", |output| unsafe {
            (self.inner.table().plus.unwrap())(self.inner.context(), left, right, output)
        })?;
        Ok(self.own(token))
    }

    /// Multiply two values from this exact operation context.
    pub fn times(
        &self,
        left: &SemiringWeight<M>,
        right: &SemiringWeight<M>,
    ) -> Result<SemiringWeight<M>, DynamicSemiringError> {
        let (left, right) = self.binary_tokens(left, right)?;
        let token = self.inner.output_value("times", |output| unsafe {
            (self.inner.table().times.unwrap())(self.inner.context(), left, right, output)
        })?;
        Ok(self.own(token))
    }

    /// Compare two values for exact semantic equality.
    pub fn equal(
        &self,
        left: &SemiringWeight<M>,
        right: &SemiringWeight<M>,
    ) -> Result<bool, DynamicSemiringError> {
        let (left, right) = self.binary_tokens(left, right)?;
        self.inner.invoke(|| {
            let mut output = u8::MAX;
            status_ok("equal", unsafe {
                (self.inner.table().equal.unwrap())(self.inner.context(), left, right, &mut output)
            })?;
            Ok(decode_bool("equal", output)?)
        })
    }

    /// Compare two values using the provider's natural metric.
    pub fn approx_equal(
        &self,
        left: &SemiringWeight<M>,
        right: &SemiringWeight<M>,
        epsilon: f64,
    ) -> Result<bool, DynamicSemiringError> {
        if !epsilon.is_finite() || epsilon < 0.0 {
            return Err(DynamicSemiringError::InvalidArgument(
                "epsilon must be finite and nonnegative",
            ));
        }
        let (left, right) = self.binary_tokens(left, right)?;
        self.inner.invoke(|| {
            let mut output = u8::MAX;
            status_ok("approx_equal", unsafe {
                (self.inner.table().approx_equal.unwrap())(
                    self.inner.context(),
                    left,
                    right,
                    epsilon,
                    &mut output,
                )
            })?;
            Ok(decode_bool("approx_equal", output)?)
        })
    }

    /// Compare two values in the semiring's natural order.
    pub fn natural_order(
        &self,
        left: &SemiringWeight<M>,
        right: &SemiringWeight<M>,
    ) -> Result<NaturalOrder, DynamicSemiringError> {
        let (left, right) = self.binary_tokens(left, right)?;
        self.inner.invoke(|| {
            let mut output = i32::MIN;
            status_ok("natural_order", unsafe {
                (self.inner.table().natural_order.unwrap())(
                    self.inner.context(),
                    left,
                    right,
                    &mut output,
                )
            })?;
            match output {
                semiring_order::BETTER => Ok(NaturalOrder::Better),
                semiring_order::EQUAL => Ok(NaturalOrder::Equal),
                semiring_order::WORSE => Ok(NaturalOrder::Worse),
                semiring_order::INCOMPARABLE
                    if self.declares_property(semiring_properties::TOTALLY_ORDERED) =>
                {
                    Err(DynamicSemiringError::InvalidProviderOutput {
                        operation: "natural_order",
                        reason: "totally ordered provider returned incomparable",
                    })
                }
                semiring_order::INCOMPARABLE => Ok(NaturalOrder::Incomparable),
                _ => Err(DynamicSemiringError::InvalidProviderOutput {
                    operation: "natural_order",
                    reason: "unrecognized order value",
                }),
            }
        })
    }

    /// Copy the canonical byte representation of one value.
    pub fn stable_bytes(&self, value: &SemiringWeight<M>) -> Result<Vec<u8>, DynamicSemiringError> {
        let callback = self
            .inner
            .table()
            .stable_bytes
            .filter(|_| self.flags() & semiring_flags::STABLE_BYTES != 0)
            .ok_or(DynamicSemiringError::MissingCapability("stable bytes"))?;
        let value = self.token(value)?;
        self.copy_bytes(
            "stable_bytes",
            |bytes, capacity, written, required| unsafe {
                callback(
                    self.inner.context(),
                    value,
                    bytes,
                    capacity,
                    written,
                    required,
                )
            },
        )
    }

    /// Copy the provider's advisory UTF-8 diagnostic.
    pub fn diagnostic(
        &self,
        value: Option<&SemiringWeight<M>>,
    ) -> Result<String, DynamicSemiringError> {
        let callback = self
            .inner
            .table()
            .diagnostic
            .ok_or(DynamicSemiringError::MissingCapability("diagnostic"))?;
        let value = value.map(|value| self.token(value)).transpose()?;
        let value = value.map_or(std::ptr::null(), |value| value as *const _);
        let bytes = self.copy_bytes("diagnostic", |bytes, capacity, written, required| unsafe {
            callback(
                self.inner.context(),
                value,
                bytes,
                capacity,
                written,
                required,
            )
        })?;
        String::from_utf8(bytes).map_err(|_| DynamicSemiringError::InvalidProviderOutput {
            operation: "diagnostic",
            reason: "diagnostic is not UTF-8",
        })
    }

    /// Add a slice using bounded provider folds when advertised, otherwise a
    /// pairwise left fold.
    pub fn plus_many(
        &self,
        values: &[SemiringWeight<M>],
    ) -> Result<SemiringWeight<M>, DynamicSemiringError> {
        self.fold_many(values, FoldOperation::Plus)
    }

    /// Multiply a slice using bounded provider folds when advertised,
    /// otherwise a pairwise left fold.
    pub fn times_many(
        &self,
        values: &[SemiringWeight<M>],
    ) -> Result<SemiringWeight<M>, DynamicSemiringError> {
        self.fold_many(values, FoldOperation::Times)
    }

    /// Compute right division, returning `None` when the quotient is undefined.
    pub fn divide(
        &self,
        dividend: &SemiringWeight<M>,
        divisor: &SemiringWeight<M>,
    ) -> Result<Option<SemiringWeight<M>>, DynamicSemiringError> {
        let callback = self
            .inner
            .division
            .and_then(|table| unsafe { (*table).divide })
            .ok_or(DynamicSemiringError::MissingCapability("division"))?;
        let (dividend, divisor) = self.binary_tokens(dividend, divisor)?;
        self.optional_output("divide", |output| unsafe {
            callback(self.inner.context(), dividend, divisor, output)
        })
    }

    /// Compute weak left division, returning `None` when undefined.
    pub fn left_divide(
        &self,
        value: &SemiringWeight<M>,
        divisor: &SemiringWeight<M>,
    ) -> Result<Option<SemiringWeight<M>>, DynamicSemiringError> {
        let callback = self
            .inner
            .division
            .and_then(|table| unsafe { (*table).left_divide })
            .ok_or(DynamicSemiringError::MissingCapability("left division"))?;
        let (value, divisor) = self.binary_tokens(value, divisor)?;
        self.optional_output("left_divide", |output| unsafe {
            callback(self.inner.context(), value, divisor, output)
        })
    }

    /// Compute Kleene closure, returning `None` when closure diverges.
    pub fn star(
        &self,
        value: &SemiringWeight<M>,
    ) -> Result<Option<SemiringWeight<M>>, DynamicSemiringError> {
        let callback = self
            .inner
            .star
            .and_then(|table| unsafe { (*table).star })
            .ok_or(DynamicSemiringError::MissingCapability("Kleene star"))?;
        let value = self.token(value)?;
        self.optional_output("star", |output| unsafe {
            callback(self.inner.context(), value, output)
        })
    }

    /// Extract the provider's numerical interpretation.
    pub fn numerical_value(&self, value: &SemiringWeight<M>) -> Result<f64, DynamicSemiringError> {
        let callback = self
            .inner
            .numeric
            .and_then(|table| unsafe { (*table).numerical_value })
            .ok_or(DynamicSemiringError::MissingCapability(
                "numerical projection",
            ))?;
        let value = self.token(value)?;
        self.inner.invoke(|| {
            let mut output = f64::NAN;
            status_ok("numerical_value", unsafe {
                callback(self.inner.context(), value, &mut output)
            })?;
            if output.is_nan() {
                return Err(DynamicSemiringError::InvalidProviderOutput {
                    operation: "numerical_value",
                    reason: "projection is NaN",
                });
            }
            Ok(output)
        })
    }

    /// Quantize one weight at a positive finite precision.
    pub fn quantize(
        &self,
        value: &SemiringWeight<M>,
        epsilon: f64,
    ) -> Result<i64, DynamicSemiringError> {
        if !epsilon.is_finite() || epsilon <= 0.0 {
            return Err(DynamicSemiringError::InvalidArgument(
                "quantization epsilon must be finite and positive",
            ));
        }
        let callback = self
            .inner
            .numeric
            .and_then(|table| unsafe { (*table).quantize })
            .ok_or(DynamicSemiringError::MissingCapability("quantization"))?;
        let value = self.token(value)?;
        self.inner.invoke(|| {
            let mut output = 0;
            status_ok("quantize", unsafe {
                callback(self.inner.context(), value, epsilon, &mut output)
            })?;
            Ok(output)
        })
    }

    /// Convert one weight to a finite nonnegative sampling weight.
    pub fn to_probability(&self, value: &SemiringWeight<M>) -> Result<f64, DynamicSemiringError> {
        let callback = self
            .inner
            .numeric
            .and_then(|table| unsafe { (*table).to_probability })
            .ok_or(DynamicSemiringError::MissingCapability(
                "probability projection",
            ))?;
        let value = self.token(value)?;
        self.inner.invoke(|| {
            let mut output = f64::NAN;
            status_ok("to_probability", unsafe {
                callback(self.inner.context(), value, &mut output)
            })?;
            if !output.is_finite() || output < 0.0 {
                return Err(DynamicSemiringError::InvalidProviderOutput {
                    operation: "to_probability",
                    reason: "probability must be finite and nonnegative",
                });
            }
            Ok(output)
        })
    }

    /// Read the optional uniform closure bound.
    pub fn closure_bound(&self) -> Result<Option<usize>, DynamicSemiringError> {
        let callback = self
            .inner
            .properties
            .and_then(|table| unsafe { (*table).closure_bound })
            .ok_or(DynamicSemiringError::MissingCapability("closure bound"))?;
        self.inner.invoke(|| {
            let mut bound = 0;
            let mut known = u8::MAX;
            status_ok("closure_bound", unsafe {
                callback(self.inner.context(), &mut bound, &mut known)
            })?;
            match known {
                0 => Ok(None),
                1 => Ok(Some(bound)),
                _ => Err(DynamicSemiringError::InvalidProviderOutput {
                    operation: "closure_bound",
                    reason: "known flag is not zero or one",
                }),
            }
        })
    }

    /// Probe the base semiring axioms and every advertised property over a
    /// caller-chosen representative sample.
    ///
    /// Finite testing cannot prove a universal algebraic law. It can prevent a
    /// false provider claim from silently selecting an unsound specialized
    /// algorithm. Include identities, boundary values, infinities where lawful,
    /// and values typical of the intended workload. The sample is capped so an
    /// untrusted integration cannot turn validation into unbounded cubic work.
    pub fn validate_declared_laws(
        &self,
        samples: &[SemiringWeight<M>],
        epsilon: f64,
    ) -> Result<(), DynamicSemiringError> {
        if samples.is_empty() {
            return Err(DynamicSemiringError::InvalidArgument(
                "law validation requires at least one sample",
            ));
        }
        if samples.len() > MAX_LAW_SAMPLES {
            return Err(DynamicSemiringError::InvalidArgument(
                "law-validation sample exceeds the bounded probe size",
            ));
        }
        if !epsilon.is_finite() || epsilon < 0.0 {
            return Err(DynamicSemiringError::InvalidArgument(
                "law-validation epsilon must be finite and nonnegative",
            ));
        }
        for sample in samples {
            self.token(sample)?;
        }

        let zero = self.zero()?;
        let one = self.one()?;
        let properties = self.declared_properties();

        for a in samples {
            let plus_zero = self.plus(a, &zero)?;
            self.require_law_eq(&plus_zero, a, epsilon, "additive identity")?;
            let times_one = self.times(a, &one)?;
            self.require_law_eq(&times_one, a, epsilon, "right multiplicative identity")?;
            let one_times = self.times(&one, a)?;
            self.require_law_eq(&one_times, a, epsilon, "left multiplicative identity")?;
            let times_zero = self.times(a, &zero)?;
            self.require_law_eq(&times_zero, &zero, epsilon, "right zero annihilation")?;
            let zero_times = self.times(&zero, a)?;
            self.require_law_eq(&zero_times, &zero, epsilon, "left zero annihilation")?;

            if properties & semiring_properties::IDEMPOTENT_PLUS != 0 {
                let doubled = self.plus(a, a)?;
                self.require_law_eq(&doubled, a, epsilon, "additive idempotence")?;
            }
            if properties & semiring_properties::HASHABLE != 0 {
                let cloned = a.try_clone()?;
                if !self.equal(a, &cloned)?
                    || self.stable_bytes(a)? != self.stable_bytes(&cloned)?
                {
                    return Err(DynamicSemiringError::LawViolation(
                        "hash/equality coherence",
                    ));
                }
            }
            if properties & semiring_properties::NONNEGATIVE != 0 {
                let projection = self.numerical_value(a)?;
                if projection.is_nan() || projection < 0.0 {
                    return Err(DynamicSemiringError::LawViolation("nonnegativity"));
                }
            }
            if properties & semiring_properties::TOTALLY_ORDERED != 0
                && self.natural_order(a, a)? != NaturalOrder::Equal
            {
                return Err(DynamicSemiringError::LawViolation(
                    "total-order reflexivity",
                ));
            }
        }

        for a in samples {
            for b in samples {
                let ab_plus = self.plus(a, b)?;
                let ba_plus = self.plus(b, a)?;
                self.require_law_eq(&ab_plus, &ba_plus, epsilon, "additive commutativity")?;
                if properties & semiring_properties::COMMUTATIVE_TIMES != 0 {
                    let ab_times = self.times(a, b)?;
                    let ba_times = self.times(b, a)?;
                    self.require_law_eq(
                        &ab_times,
                        &ba_times,
                        epsilon,
                        "multiplicative commutativity",
                    )?;
                }
                if properties & semiring_properties::ZERO_SUM_FREE != 0
                    && self.approx_equal(&ab_plus, &zero, epsilon)?
                    && !(self.approx_equal(a, &zero, epsilon)?
                        && self.approx_equal(b, &zero, epsilon)?)
                {
                    return Err(DynamicSemiringError::LawViolation("zero-sum freedom"));
                }
                if properties & semiring_properties::TOTALLY_ORDERED != 0 {
                    let forward = self.natural_order(a, b)?;
                    let reverse = self.natural_order(b, a)?;
                    let coherent = matches!(
                        (forward, reverse),
                        (NaturalOrder::Better, NaturalOrder::Worse)
                            | (NaturalOrder::Worse, NaturalOrder::Better)
                            | (NaturalOrder::Equal, NaturalOrder::Equal)
                    );
                    if !coherent {
                        return Err(DynamicSemiringError::LawViolation(
                            "total-order antisymmetry",
                        ));
                    }
                }
            }
        }

        for a in samples {
            for b in samples {
                for c in samples {
                    let ab = self.plus(a, b)?;
                    let left_plus = self.plus(&ab, c)?;
                    let bc = self.plus(b, c)?;
                    let right_plus = self.plus(a, &bc)?;
                    self.require_law_eq(
                        &left_plus,
                        &right_plus,
                        epsilon,
                        "additive associativity",
                    )?;

                    let ab = self.times(a, b)?;
                    let left_times = self.times(&ab, c)?;
                    let bc = self.times(b, c)?;
                    let right_times = self.times(a, &bc)?;
                    self.require_law_eq(
                        &left_times,
                        &right_times,
                        epsilon,
                        "multiplicative associativity",
                    )?;

                    let b_plus_c = self.plus(b, c)?;
                    let left_distributive = self.times(a, &b_plus_c)?;
                    let ab = self.times(a, b)?;
                    let ac = self.times(a, c)?;
                    let expanded = self.plus(&ab, &ac)?;
                    self.require_law_eq(
                        &left_distributive,
                        &expanded,
                        epsilon,
                        "left distributivity",
                    )?;

                    let a_plus_b = self.plus(a, b)?;
                    let right_distributive = self.times(&a_plus_b, c)?;
                    let ac = self.times(a, c)?;
                    let bc = self.times(b, c)?;
                    let expanded = self.plus(&ac, &bc)?;
                    self.require_law_eq(
                        &right_distributive,
                        &expanded,
                        epsilon,
                        "right distributivity",
                    )?;
                }
            }
        }

        if properties & semiring_properties::K_CLOSED != 0 {
            if let Some(bound) = self.closure_bound()? {
                if bound > MAX_CLOSURE_PROBE {
                    return Err(DynamicSemiringError::ResourceLimit);
                }
                for value in samples {
                    let mut sum = one.try_clone()?;
                    let mut power = one.try_clone()?;
                    for exponent in 1..=bound.saturating_add(1) {
                        power = self.times(&power, value)?;
                        let next = self.plus(&sum, &power)?;
                        if exponent == bound.saturating_add(1) {
                            self.require_law_eq(&next, &sum, epsilon, "bounded Kleene closure")?;
                        } else {
                            sum = next;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn own(&self, token: VtSemiringValue) -> SemiringWeight<M> {
        SemiringWeight {
            inner: Arc::clone(&self.inner),
            token: Some(token),
            access: PhantomData,
        }
    }

    fn require_law_eq(
        &self,
        left: &SemiringWeight<M>,
        right: &SemiringWeight<M>,
        epsilon: f64,
        law: &'static str,
    ) -> Result<(), DynamicSemiringError> {
        if self.approx_equal(left, right, epsilon)? {
            Ok(())
        } else {
            Err(DynamicSemiringError::LawViolation(law))
        }
    }

    fn token<'a>(
        &self,
        value: &'a SemiringWeight<M>,
    ) -> Result<&'a VtSemiringValue, DynamicSemiringError> {
        if !Arc::ptr_eq(&self.inner, &value.inner) {
            return Err(DynamicSemiringError::ContextMismatch);
        }
        value
            .token
            .as_ref()
            .ok_or(DynamicSemiringError::InvalidArgument("weight is closed"))
    }

    fn binary_tokens<'a>(
        &self,
        left: &'a SemiringWeight<M>,
        right: &'a SemiringWeight<M>,
    ) -> Result<(&'a VtSemiringValue, &'a VtSemiringValue), DynamicSemiringError> {
        Ok((self.token(left)?, self.token(right)?))
    }

    fn optional_output(
        &self,
        operation: &'static str,
        callback: impl FnOnce(*mut VtSemiringValue) -> u32,
    ) -> Result<Option<SemiringWeight<M>>, DynamicSemiringError> {
        let result = self.inner.invoke(|| {
            let mut output = VtSemiringValue {
                word0: u64::MAX,
                word1: u64::MAX,
            };
            match decode_status(operation, callback(&mut output))? {
                VtStatus::Ok => Ok(Some(output)),
                VtStatus::End => Ok(None),
                status => Err(DynamicSemiringError::Provider { operation, status }),
            }
        })?;
        Ok(result.map(|token| self.own(token)))
    }

    fn copy_bytes(
        &self,
        operation: &'static str,
        callback: impl Fn(*mut u8, usize, *mut usize, *mut usize) -> u32,
    ) -> Result<Vec<u8>, DynamicSemiringError> {
        copy_provider_bytes(&self.inner.gate, operation, callback).map_err(Into::into)
    }

    fn fold_many(
        &self,
        values: &[SemiringWeight<M>],
        operation: FoldOperation,
    ) -> Result<SemiringWeight<M>, DynamicSemiringError> {
        for value in values {
            self.token(value)?;
        }
        if values.is_empty() {
            return match operation {
                FoldOperation::Plus => self.zero(),
                FoldOperation::Times => self.one(),
            };
        }

        let table = self.inner.table();
        let batch_callback = match operation {
            FoldOperation::Plus => table.plus_many,
            FoldOperation::Times => table.times_many,
        };
        if table.flags & semiring_flags::BATCH == 0 || batch_callback.is_none() {
            let mut accumulator = values[0].try_clone()?;
            for value in &values[1..] {
                accumulator = match operation {
                    FoldOperation::Plus => self.plus(&accumulator, value)?,
                    FoldOperation::Times => self.times(&accumulator, value)?,
                };
            }
            return Ok(accumulator);
        }

        let callback = batch_callback.unwrap();
        let boundary = VT_RECOMMENDED_SEMIRING_BATCH.max(2);
        let first_end = values.len().min(boundary);
        let first: Vec<_> = values[..first_end]
            .iter()
            .map(|value| *self.token(value).unwrap())
            .collect();
        let mut accumulator = self.batch_output(operation.name(), callback, &first)?;
        let mut start = first_end;
        while start < values.len() {
            let end = (start + boundary - 1).min(values.len());
            let mut tokens = Vec::with_capacity(1 + end - start);
            tokens.push(*self.token(&accumulator)?);
            for value in &values[start..end] {
                tokens.push(*self.token(value)?);
            }
            accumulator = self.batch_output(operation.name(), callback, &tokens)?;
            start = end;
        }
        Ok(accumulator)
    }

    fn batch_output(
        &self,
        operation: &'static str,
        callback: unsafe extern "C" fn(
            *mut c_void,
            *const VtSemiringValue,
            usize,
            *mut VtSemiringValue,
        ) -> u32,
        values: &[VtSemiringValue],
    ) -> Result<SemiringWeight<M>, DynamicSemiringError> {
        let token = self.inner.output_value(operation, |output| unsafe {
            callback(self.inner.context(), values.as_ptr(), values.len(), output)
        })?;
        Ok(self.own(token))
    }
}

enum FoldOperation {
    Plus,
    Times,
}

impl FoldOperation {
    const fn name(&self) -> &'static str {
        match self {
            Self::Plus => "plus_many",
            Self::Times => "times_many",
        }
    }
}

/// One owned dynamic-semiring value token.
///
/// This type intentionally does not implement `Copy` or `Clone`. Use
/// [`Self::try_clone`] so provider failure remains explicit, and [`Self::close`]
/// when deterministic release reporting is required.
pub struct SemiringWeight<M: SemiringAccess = LocalSemiringAccess> {
    inner: Arc<DynamicSemiringInner>,
    token: Option<VtSemiringValue>,
    access: PhantomData<M>,
}

/// Same-thread owned dynamic-semiring weight.
pub type DynamicSemiringWeight = SemiringWeight<LocalSemiringAccess>;

/// Cross-thread owned dynamic-semiring weight.
pub type ParallelDynamicSemiringWeight = SemiringWeight<ParallelSemiringAccess>;

impl<M: SemiringAccess> fmt::Debug for SemiringWeight<M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemiringWeight")
            .field("domain_id", &self.inner.table().domain_id)
            .field("open", &self.token.is_some())
            .finish_non_exhaustive()
    }
}

impl<M: SemiringAccess> SemiringWeight<M> {
    /// Duplicate this owned token through the provider.
    pub fn try_clone(&self) -> Result<Self, DynamicSemiringError> {
        let token = self
            .token
            .as_ref()
            .ok_or(DynamicSemiringError::InvalidArgument("weight is closed"))?;
        let cloned = self.inner.output_value("clone_value", |output| unsafe {
            (self.inner.table().clone_value.unwrap())(self.inner.context(), token, output)
        })?;
        Ok(Self {
            inner: Arc::clone(&self.inner),
            token: Some(cloned),
            access: PhantomData,
        })
    }

    /// Return whether this wrapper still owns a live token.
    pub fn is_open(&self) -> bool {
        self.token.is_some()
    }

    /// Release the token deterministically and report provider failures.
    pub fn close(mut self) -> Result<(), DynamicSemiringError> {
        if let Some(token) = self.token.as_mut() {
            self.inner.release(token)?;
            self.token = None;
        }
        Ok(())
    }
}

impl<M: SemiringAccess> Drop for SemiringWeight<M> {
    fn drop(&mut self) {
        if let Some(token) = self.token.as_mut() {
            // A hostile provider can fail release. Drop cannot report that
            // failure, so explicit `close` is available for audited lifetimes.
            // Retrying after a failed status is the ABI's conservative rule.
            if self.inner.release(token).is_ok() {
                self.token = None;
            } else {
                // Keep the context and its foreign arena alive with the
                // unreleased token. A bounded leak is safer than destroying
                // provider storage whose release outcome is unknown.
                std::mem::forget(Arc::clone(&self.inner));
            }
        }
    }
}

fn validate_semiring(table: &VtSemiringVTable) -> Result<(), DynamicSemiringError> {
    validate_prefix(
        table,
        table.struct_size,
        table.interface_version,
        table.reserved,
        size_of::<VtSemiringVTable>(),
        VT_SEMIRING_INTERFACE_VERSION,
        "base",
    )?;
    if table.zero.is_none()
        || table.one.is_none()
        || table.clone_value.is_none()
        || table.release_values.is_none()
        || table.plus.is_none()
        || table.times.is_none()
        || table.equal.is_none()
        || table.approx_equal.is_none()
        || table.natural_order.is_none()
    {
        return Err(DynamicSemiringError::IncompatibleInterface("base"));
    }
    if (table.flags & semiring_flags::STABLE_BYTES != 0) != table.stable_bytes.is_some() {
        return Err(DynamicSemiringError::IncompatibleInterface("stable bytes"));
    }
    if table.flags & semiring_flags::BATCH != 0
        && (table.plus_many.is_none() || table.times_many.is_none())
    {
        return Err(DynamicSemiringError::IncompatibleInterface("batch"));
    }
    Ok(())
}

fn validate_prefix<T>(
    _table: &T,
    struct_size: usize,
    interface_version: u32,
    reserved: u32,
    minimum_size: usize,
    minimum_version: u32,
    name: &'static str,
) -> Result<(), DynamicSemiringError> {
    if struct_size < minimum_size || interface_version < minimum_version || reserved != 0 {
        Err(DynamicSemiringError::IncompatibleInterface(name))
    } else {
        Ok(())
    }
}

/// Known law bits understood by this adapter.
pub const KNOWN_SEMIRING_PROPERTIES: u64 = semiring_properties::HASHABLE
    | semiring_properties::IDEMPOTENT_PLUS
    | semiring_properties::K_CLOSED
    | semiring_properties::ZERO_SUM_FREE
    | semiring_properties::COMMUTATIVE_TIMES
    | semiring_properties::TOTALLY_ORDERED
    | semiring_properties::NONNEGATIVE;
