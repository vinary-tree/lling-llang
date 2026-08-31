#![cfg(feature = "bindings-core")]

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use lling_llang::dynamic_semiring::{
    DynamicSemiringContext, DynamicSemiringError, NaturalOrder, ParallelDynamicSemiringContext,
};
#[cfg(feature = "ffi")]
use lling_llang::ffi::{
    lling_semiring_divide, lling_semiring_equal, lling_semiring_free, lling_semiring_natural_order,
    lling_semiring_numerical_value, lling_semiring_one, lling_semiring_open, lling_semiring_plus,
    lling_semiring_properties, lling_semiring_stable_bytes, lling_semiring_validate_laws,
    lling_semiring_weight_free, lling_semiring_zero, LlingSemiring, LlingSemiringWeight,
    LlingStatus,
};
use vinary_tree_interop::{
    semiring_flags, semiring_order, semiring_properties, VtInterfaceId, VtResource,
    VtResourceVTable, VtSemiringDivisionVTable, VtSemiringNumericVTable,
    VtSemiringPropertiesVTable, VtSemiringStarVTable, VtSemiringVTable, VtSemiringValue, VtStatus,
    VT_ABI_VERSION, VT_SEMIRING_DIVISION_INTERFACE_ID, VT_SEMIRING_DIVISION_INTERFACE_VERSION,
    VT_SEMIRING_INTERFACE_ID, VT_SEMIRING_INTERFACE_VERSION, VT_SEMIRING_NUMERIC_INTERFACE_ID,
    VT_SEMIRING_NUMERIC_INTERFACE_VERSION, VT_SEMIRING_PROPERTIES_INTERFACE_ID,
    VT_SEMIRING_PROPERTIES_INTERFACE_VERSION, VT_SEMIRING_STAR_INTERFACE_ID,
    VT_SEMIRING_STAR_INTERFACE_VERSION,
};

const TOKEN_TAG: u64 = 0x5649_4e41_5259_534d;
const DOMAIN: VtInterfaceId = VtInterfaceId {
    bytes: *b"test.real.sum.v1",
};

struct MockState {
    references: AtomicUsize,
    live_tokens: AtomicUsize,
    batch_calls: AtomicUsize,
    parallel: bool,
    hostile: AtomicU8,
}

impl MockState {
    fn new(parallel: bool) -> Self {
        Self {
            references: AtomicUsize::new(1),
            live_tokens: AtomicUsize::new(0),
            batch_calls: AtomicUsize::new(0),
            parallel,
            hostile: AtomicU8::new(0),
        }
    }
}

struct TestResource {
    raw: VtResource,
    state: *mut MockState,
}

impl TestResource {
    fn new(parallel: bool) -> Self {
        let state = Box::into_raw(Box::new(MockState::new(parallel)));
        Self {
            raw: VtResource {
                context: state.cast(),
                vtable: &RESOURCE_VTABLE,
            },
            state,
        }
    }

    fn state(&self) -> &MockState {
        // SAFETY: this wrapper owns the original resource retain.
        unsafe { &*self.state }
    }
}

impl Drop for TestResource {
    fn drop(&mut self) {
        // SAFETY: this consumes the wrapper's original retain.
        unsafe { mock_release(self.raw.context) };
    }
}

fn state(context: *mut c_void) -> &'static MockState {
    // SAFETY: every callback receives the live resource's MockState pointer.
    unsafe { &*context.cast::<MockState>() }
}

fn encode(value: f64) -> VtSemiringValue {
    VtSemiringValue {
        word0: value.to_bits(),
        word1: TOKEN_TAG,
    }
}

unsafe fn decode(value: *const VtSemiringValue) -> Result<f64, VtStatus> {
    if value.is_null() || (*value).word1 != TOKEN_TAG {
        Err(VtStatus::InvalidArgument)
    } else {
        Ok(f64::from_bits((*value).word0))
    }
}

unsafe fn write_owned(context: *mut c_void, output: *mut VtSemiringValue, value: f64) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    *output = encode(value);
    state(context).live_tokens.fetch_add(1, Ordering::Relaxed);
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn mock_retain(context: *mut c_void) {
    state(context).references.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C" fn mock_release(context: *mut c_void) {
    let current = state(context).references.fetch_sub(1, Ordering::AcqRel);
    assert!(current > 0, "resource retain count underflow");
    if current == 1 {
        drop(Box::from_raw(context.cast::<MockState>()));
    }
}

unsafe extern "C" fn mock_query(
    context: *mut c_void,
    interface_id: *const VtInterfaceId,
    minimum_version: u32,
    output: *mut *const c_void,
) -> u32 {
    if interface_id.is_null() || output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    if state(context).hostile.load(Ordering::Relaxed) == 5 {
        return 42;
    }
    let id = *interface_id;
    let table =
        if id == VT_SEMIRING_INTERFACE_ID && minimum_version <= VT_SEMIRING_INTERFACE_VERSION {
            if state(context).parallel {
                (&PARALLEL_SEMIRING_VTABLE as *const VtSemiringVTable).cast()
            } else {
                (&SERIAL_SEMIRING_VTABLE as *const VtSemiringVTable).cast()
            }
        } else if id == VT_SEMIRING_DIVISION_INTERFACE_ID
            && minimum_version <= VT_SEMIRING_DIVISION_INTERFACE_VERSION
        {
            (&DIVISION_VTABLE as *const VtSemiringDivisionVTable).cast()
        } else if id == VT_SEMIRING_STAR_INTERFACE_ID
            && minimum_version <= VT_SEMIRING_STAR_INTERFACE_VERSION
        {
            (&STAR_VTABLE as *const VtSemiringStarVTable).cast()
        } else if id == VT_SEMIRING_NUMERIC_INTERFACE_ID
            && minimum_version <= VT_SEMIRING_NUMERIC_INTERFACE_VERSION
        {
            (&NUMERIC_VTABLE as *const VtSemiringNumericVTable).cast()
        } else if id == VT_SEMIRING_PROPERTIES_INTERFACE_ID
            && minimum_version <= VT_SEMIRING_PROPERTIES_INTERFACE_VERSION
        {
            (&PROPERTIES_VTABLE as *const VtSemiringPropertiesVTable).cast()
        } else {
            return VtStatus::Unsupported.to_raw();
        };
    *output = table;
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn zero(context: *mut c_void, output: *mut VtSemiringValue) -> u32 {
    write_owned(context, output, 0.0)
}

unsafe extern "C" fn one(context: *mut c_void, output: *mut VtSemiringValue) -> u32 {
    write_owned(context, output, 1.0)
}

unsafe extern "C" fn clone_value(
    context: *mut c_void,
    value: *const VtSemiringValue,
    output: *mut VtSemiringValue,
) -> u32 {
    match decode(value) {
        Ok(value) => write_owned(context, output, value),
        Err(status) => status.to_raw(),
    }
}

unsafe extern "C" fn release_values(
    context: *mut c_void,
    values: *mut VtSemiringValue,
    count: usize,
) -> u32 {
    if count != 0 && values.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    for index in 0..count {
        let value = values.add(index);
        if decode(value).is_err() {
            return VtStatus::InvalidArgument.to_raw();
        }
        (*value).word1 = 0;
    }
    let current = state(context)
        .live_tokens
        .fetch_sub(count, Ordering::AcqRel);
    assert!(current >= count, "token ownership underflow");
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn plus(
    context: *mut c_void,
    left: *const VtSemiringValue,
    right: *const VtSemiringValue,
    output: *mut VtSemiringValue,
) -> u32 {
    match (decode(left), decode(right)) {
        (Ok(left), Ok(right)) => {
            let value = if state(context).hostile.load(Ordering::Relaxed) == 6 {
                left - right
            } else {
                left + right
            };
            write_owned(context, output, value)
        }
        _ => VtStatus::InvalidArgument.to_raw(),
    }
}

unsafe extern "C" fn times(
    context: *mut c_void,
    left: *const VtSemiringValue,
    right: *const VtSemiringValue,
    output: *mut VtSemiringValue,
) -> u32 {
    match (decode(left), decode(right)) {
        (Ok(left), Ok(right)) => write_owned(context, output, left * right),
        _ => VtStatus::InvalidArgument.to_raw(),
    }
}

unsafe extern "C" fn equal(
    context: *mut c_void,
    left: *const VtSemiringValue,
    right: *const VtSemiringValue,
    output: *mut u8,
) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    match (decode(left), decode(right)) {
        (Ok(left), Ok(right)) => {
            *output = if state(context).hostile.load(Ordering::Relaxed) == 1 {
                7
            } else {
                u8::from(left == right)
            };
            VtStatus::Ok.to_raw()
        }
        _ => VtStatus::InvalidArgument.to_raw(),
    }
}

unsafe extern "C" fn approx_equal(
    _context: *mut c_void,
    left: *const VtSemiringValue,
    right: *const VtSemiringValue,
    epsilon: f64,
    output: *mut u8,
) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    match (decode(left), decode(right)) {
        (Ok(left), Ok(right)) => {
            *output = u8::from((left - right).abs() <= epsilon);
            VtStatus::Ok.to_raw()
        }
        _ => VtStatus::InvalidArgument.to_raw(),
    }
}

unsafe extern "C" fn natural_order(
    context: *mut c_void,
    left: *const VtSemiringValue,
    right: *const VtSemiringValue,
    output: *mut i32,
) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    match (decode(left), decode(right)) {
        (Ok(left), Ok(right)) => {
            *output = if state(context).hostile.load(Ordering::Relaxed) == 2 {
                99
            } else if left < right {
                semiring_order::BETTER
            } else if left > right {
                semiring_order::WORSE
            } else {
                semiring_order::EQUAL
            };
            VtStatus::Ok.to_raw()
        }
        _ => VtStatus::InvalidArgument.to_raw(),
    }
}

unsafe extern "C" fn stable_bytes(
    _context: *mut c_void,
    value: *const VtSemiringValue,
    output: *mut u8,
    capacity: usize,
    written: *mut usize,
    required: *mut usize,
) -> u32 {
    if written.is_null() || required.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let value = match decode(value) {
        Ok(value) => value,
        Err(status) => return status.to_raw(),
    };
    let bytes = value.to_bits().to_be_bytes();
    *required = bytes.len();
    *written = capacity.min(bytes.len());
    if *written != 0 {
        if output.is_null() {
            return VtStatus::NullPointer.to_raw();
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, *written);
    }
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn diagnostic(
    _context: *mut c_void,
    _value: *const VtSemiringValue,
    output: *mut u8,
    capacity: usize,
    written: *mut usize,
    required: *mut usize,
) -> u32 {
    let bytes = b"mock real semiring";
    if written.is_null() || required.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    *required = bytes.len();
    *written = capacity.min(bytes.len());
    if *written != 0 {
        if output.is_null() {
            return VtStatus::NullPointer.to_raw();
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, *written);
    }
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn plus_many(
    context: *mut c_void,
    values: *const VtSemiringValue,
    count: usize,
    output: *mut VtSemiringValue,
) -> u32 {
    state(context).batch_calls.fetch_add(1, Ordering::Relaxed);
    let mut result = 0.0;
    for index in 0..count {
        match decode(values.add(index)) {
            Ok(value) => result += value,
            Err(status) => return status.to_raw(),
        }
    }
    write_owned(context, output, result)
}

unsafe extern "C" fn times_many(
    context: *mut c_void,
    values: *const VtSemiringValue,
    count: usize,
    output: *mut VtSemiringValue,
) -> u32 {
    state(context).batch_calls.fetch_add(1, Ordering::Relaxed);
    let mut result = 1.0;
    for index in 0..count {
        match decode(values.add(index)) {
            Ok(value) => result *= value,
            Err(status) => return status.to_raw(),
        }
    }
    write_owned(context, output, result)
}

unsafe extern "C" fn divide(
    context: *mut c_void,
    left: *const VtSemiringValue,
    right: *const VtSemiringValue,
    output: *mut VtSemiringValue,
) -> u32 {
    match (decode(left), decode(right)) {
        (Ok(_), Ok(0.0)) => VtStatus::End.to_raw(),
        (Ok(left), Ok(right)) => write_owned(context, output, left / right),
        _ => VtStatus::InvalidArgument.to_raw(),
    }
}

unsafe extern "C" fn star(
    context: *mut c_void,
    value: *const VtSemiringValue,
    output: *mut VtSemiringValue,
) -> u32 {
    match decode(value) {
        Ok(value) if value < 1.0 => write_owned(context, output, 1.0 / (1.0 - value)),
        Ok(_) => VtStatus::End.to_raw(),
        Err(status) => status.to_raw(),
    }
}

unsafe extern "C" fn numerical_value(
    _context: *mut c_void,
    value: *const VtSemiringValue,
    output: *mut f64,
) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    match decode(value) {
        Ok(value) => {
            *output = value;
            VtStatus::Ok.to_raw()
        }
        Err(status) => status.to_raw(),
    }
}

unsafe extern "C" fn quantize(
    _context: *mut c_void,
    value: *const VtSemiringValue,
    epsilon: f64,
    output: *mut i64,
) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    match decode(value) {
        Ok(value) => {
            *output = (value / epsilon).round() as i64;
            VtStatus::Ok.to_raw()
        }
        Err(status) => status.to_raw(),
    }
}

unsafe extern "C" fn to_probability(
    context: *mut c_void,
    value: *const VtSemiringValue,
    output: *mut f64,
) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    if state(context).hostile.load(Ordering::Relaxed) == 3 {
        *output = f64::NAN;
        return VtStatus::Ok.to_raw();
    }
    match decode(value) {
        Ok(value) => {
            *output = value.max(0.0);
            VtStatus::Ok.to_raw()
        }
        Err(status) => status.to_raw(),
    }
}

unsafe extern "C" fn closure_bound(
    _context: *mut c_void,
    output: *mut usize,
    known: *mut u8,
) -> u32 {
    if output.is_null() || known.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    *output = 7;
    *known = 1;
    VtStatus::Ok.to_raw()
}

static RESOURCE_VTABLE: VtResourceVTable = VtResourceVTable {
    struct_size: size_of::<VtResourceVTable>(),
    abi_version: VT_ABI_VERSION,
    reserved: 0,
    retain: Some(mock_retain),
    release: Some(mock_release),
    query_interface: Some(mock_query),
};

const BASE_FLAGS: u64 = semiring_flags::STABLE_BYTES | semiring_flags::BATCH;

macro_rules! semiring_vtable {
    ($flags:expr) => {
        VtSemiringVTable {
            struct_size: size_of::<VtSemiringVTable>(),
            interface_version: VT_SEMIRING_INTERFACE_VERSION,
            reserved: 0,
            flags: $flags,
            domain_id: DOMAIN,
            zero: Some(zero),
            one: Some(one),
            clone_value: Some(clone_value),
            release_values: Some(release_values),
            plus: Some(plus),
            times: Some(times),
            equal: Some(equal),
            approx_equal: Some(approx_equal),
            natural_order: Some(natural_order),
            stable_bytes: Some(stable_bytes),
            diagnostic: Some(diagnostic),
            plus_many: Some(plus_many),
            times_many: Some(times_many),
        }
    };
}

static SERIAL_SEMIRING_VTABLE: VtSemiringVTable = semiring_vtable!(BASE_FLAGS);
static PARALLEL_SEMIRING_VTABLE: VtSemiringVTable =
    semiring_vtable!(BASE_FLAGS | semiring_flags::PARALLEL_REENTRANT);

static DIVISION_VTABLE: VtSemiringDivisionVTable = VtSemiringDivisionVTable {
    struct_size: size_of::<VtSemiringDivisionVTable>(),
    interface_version: VT_SEMIRING_DIVISION_INTERFACE_VERSION,
    reserved: 0,
    divide: Some(divide),
    left_divide: Some(divide),
};

static STAR_VTABLE: VtSemiringStarVTable = VtSemiringStarVTable {
    struct_size: size_of::<VtSemiringStarVTable>(),
    interface_version: VT_SEMIRING_STAR_INTERFACE_VERSION,
    reserved: 0,
    star: Some(star),
};

static NUMERIC_VTABLE: VtSemiringNumericVTable = VtSemiringNumericVTable {
    struct_size: size_of::<VtSemiringNumericVTable>(),
    interface_version: VT_SEMIRING_NUMERIC_INTERFACE_VERSION,
    reserved: 0,
    numerical_value: Some(numerical_value),
    quantize: Some(quantize),
    to_probability: Some(to_probability),
};

static PROPERTIES_VTABLE: VtSemiringPropertiesVTable = VtSemiringPropertiesVTable {
    struct_size: size_of::<VtSemiringPropertiesVTable>(),
    interface_version: VT_SEMIRING_PROPERTIES_INTERFACE_VERSION,
    reserved: 0,
    properties: semiring_properties::HASHABLE
        | semiring_properties::COMMUTATIVE_TIMES
        | semiring_properties::TOTALLY_ORDERED
        | semiring_properties::NONNEGATIVE,
    closure_bound: Some(closure_bound),
};

#[test]
fn ownership_algebra_refinements_and_bounded_batches_are_exact() {
    let resource = TestResource::new(false);
    let context = unsafe { DynamicSemiringContext::borrow_raw(resource.raw) }.unwrap();
    assert_eq!(resource.state().references.load(Ordering::Relaxed), 2);
    assert_eq!(context.domain_id(), DOMAIN);
    assert!(context.supports_division());
    assert!(context.supports_left_division());
    assert!(context.supports_star());

    let zero = context.zero().unwrap();
    let one = context.one().unwrap();
    let two = context.plus(&one, &one).unwrap();
    let copied = two.try_clone().unwrap();
    assert!(context.equal(&two, &copied).unwrap());
    assert!(context.approx_equal(&two, &copied, 0.0).unwrap());
    assert_eq!(
        context.natural_order(&one, &two).unwrap(),
        NaturalOrder::Better
    );
    assert_eq!(
        context.stable_bytes(&two).unwrap(),
        2.0_f64.to_bits().to_be_bytes()
    );
    assert_eq!(
        context.diagnostic(Some(&two)).unwrap(),
        "mock real semiring"
    );
    assert_eq!(context.numerical_value(&two).unwrap(), 2.0);
    assert_eq!(context.quantize(&two, 0.25).unwrap(), 8);
    assert_eq!(context.to_probability(&two).unwrap(), 2.0);
    assert_eq!(context.closure_bound().unwrap(), Some(7));
    assert!(context.divide(&two, &zero).unwrap().is_none());
    assert!(context.star(&one).unwrap().is_none());
    let quotient = context.divide(&two, &two).unwrap().unwrap();
    assert_eq!(context.numerical_value(&quotient).unwrap(), 1.0);
    let law_samples = [
        zero.try_clone().unwrap(),
        one.try_clone().unwrap(),
        two.try_clone().unwrap(),
    ];
    context.validate_declared_laws(&law_samples, 0.0).unwrap();

    let values: Vec<_> = (0..600).map(|_| one.try_clone().unwrap()).collect();
    let sum = context.plus_many(&values).unwrap();
    assert_eq!(context.numerical_value(&sum).unwrap(), 600.0);
    assert_eq!(resource.state().batch_calls.load(Ordering::Relaxed), 3);

    drop((
        sum,
        values,
        law_samples,
        quotient,
        copied,
        two,
        one,
        zero,
        context,
    ));
    assert_eq!(resource.state().live_tokens.load(Ordering::Relaxed), 0);
    assert_eq!(resource.state().references.load(Ordering::Relaxed), 1);
}

#[test]
fn context_identity_and_hostile_outputs_are_rejected_before_safe_use() {
    let first = TestResource::new(false);
    let second = TestResource::new(false);
    let left_context = unsafe { DynamicSemiringContext::borrow_raw(first.raw) }.unwrap();
    let right_context = unsafe { DynamicSemiringContext::borrow_raw(second.raw) }.unwrap();
    let left = left_context.one().unwrap();
    let right = right_context.one().unwrap();
    assert!(matches!(
        left_context.plus(&left, &right),
        Err(DynamicSemiringError::ContextMismatch)
    ));

    first.state().hostile.store(1, Ordering::Relaxed);
    assert!(matches!(
        left_context.equal(&left, &left),
        Err(DynamicSemiringError::InvalidProviderOutput {
            operation: "equal",
            ..
        })
    ));
    first.state().hostile.store(2, Ordering::Relaxed);
    assert!(matches!(
        left_context.natural_order(&left, &left),
        Err(DynamicSemiringError::InvalidProviderOutput {
            operation: "natural_order",
            ..
        })
    ));
    first.state().hostile.store(3, Ordering::Relaxed);
    assert!(matches!(
        left_context.to_probability(&left),
        Err(DynamicSemiringError::InvalidProviderOutput {
            operation: "to_probability",
            ..
        })
    ));
    assert!(matches!(
        left_context.approx_equal(&left, &left, f64::NAN),
        Err(DynamicSemiringError::InvalidArgument(_))
    ));
    assert!(matches!(
        left_context.quantize(&left, 0.0),
        Err(DynamicSemiringError::InvalidArgument(_))
    ));
}

#[test]
fn only_parallel_reentrant_contexts_cross_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ParallelDynamicSemiringContext>();

    let serial_resource = TestResource::new(false);
    let serial = unsafe { DynamicSemiringContext::borrow_raw(serial_resource.raw) }.unwrap();
    assert!(matches!(
        serial.parallel(),
        Err(DynamicSemiringError::MissingCapability(
            "parallel reentrancy"
        ))
    ));

    let parallel_resource = TestResource::new(true);
    let local = unsafe { DynamicSemiringContext::borrow_raw(parallel_resource.raw) }.unwrap();
    let parallel = local.parallel().unwrap();
    let value = std::thread::spawn(move || {
        let one = parallel.one().unwrap();
        let value = parallel.numerical_value(&one).unwrap();
        one.close().unwrap();
        value
    })
    .join()
    .unwrap();
    assert_eq!(value, 1.0);
    drop(local);
    assert_eq!(
        parallel_resource
            .state()
            .live_tokens
            .load(Ordering::Relaxed),
        0
    );
}

#[test]
fn unknown_status_during_discovery_releases_the_borrowed_retain() {
    let resource = TestResource::new(false);
    resource.state().hostile.store(5, Ordering::Relaxed);
    assert!(matches!(
        unsafe { DynamicSemiringContext::borrow_raw(resource.raw) },
        Err(DynamicSemiringError::InvalidProviderOutput { .. })
    ));
    assert_eq!(resource.state().references.load(Ordering::Relaxed), 1);
}

#[test]
fn law_validation_rejects_a_misbehaving_base_algebra() {
    let resource = TestResource::new(false);
    let context = unsafe { DynamicSemiringContext::borrow_raw(resource.raw) }.unwrap();
    let one = context.one().unwrap();
    let two = context.plus(&one, &one).unwrap();
    resource.state().hostile.store(6, Ordering::Relaxed);
    assert!(matches!(
        context.validate_declared_laws(&[one, two], 0.0),
        Err(DynamicSemiringError::LawViolation(_))
    ));
}

#[cfg(feature = "ffi")]
#[test]
fn c_surface_consumes_the_same_validated_context_and_owned_weights() {
    let resource = TestResource::new(false);
    let mut semiring: *mut LlingSemiring = std::ptr::null_mut();
    assert_eq!(
        unsafe { lling_semiring_open(&resource.raw, &mut semiring) },
        LlingStatus::Ok
    );
    assert!(!semiring.is_null());

    let mut properties = 0;
    assert_eq!(
        lling_semiring_properties(semiring, &mut properties),
        LlingStatus::Ok
    );
    assert_ne!(properties & semiring_properties::HASHABLE, 0);

    let mut zero: *mut LlingSemiringWeight = std::ptr::null_mut();
    let mut one: *mut LlingSemiringWeight = std::ptr::null_mut();
    let mut two: *mut LlingSemiringWeight = std::ptr::null_mut();
    assert_eq!(lling_semiring_zero(semiring, &mut zero), LlingStatus::Ok);
    assert_eq!(lling_semiring_one(semiring, &mut one), LlingStatus::Ok);
    assert_eq!(
        lling_semiring_plus(semiring, one, one, &mut two),
        LlingStatus::Ok
    );

    let mut numerical = f64::NAN;
    assert_eq!(
        lling_semiring_numerical_value(semiring, two, &mut numerical),
        LlingStatus::Ok
    );
    assert_eq!(numerical, 2.0);

    let mut equal = u8::MAX;
    assert_eq!(
        lling_semiring_equal(semiring, one, two, &mut equal),
        LlingStatus::Ok
    );
    assert_eq!(equal, 0);
    let mut order = i32::MIN;
    assert_eq!(
        lling_semiring_natural_order(semiring, one, two, &mut order),
        LlingStatus::Ok
    );
    assert_eq!(order, semiring_order::BETTER);

    let mut written = usize::MAX;
    let mut required = usize::MAX;
    assert_eq!(
        unsafe {
            lling_semiring_stable_bytes(
                semiring,
                two,
                std::ptr::null_mut(),
                0,
                &mut written,
                &mut required,
            )
        },
        LlingStatus::Ok
    );
    assert_eq!((written, required), (0, 8));
    let mut bytes = [0_u8; 8];
    assert_eq!(
        unsafe {
            lling_semiring_stable_bytes(
                semiring,
                two,
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut written,
                &mut required,
            )
        },
        LlingStatus::Ok
    );
    assert_eq!(bytes, 2.0_f64.to_bits().to_be_bytes());

    let mut undefined: *mut LlingSemiringWeight = std::ptr::null_mut();
    let mut defined = u8::MAX;
    assert_eq!(
        lling_semiring_divide(semiring, two, zero, &mut undefined, &mut defined),
        LlingStatus::Ok
    );
    assert_eq!(defined, 0);
    assert!(undefined.is_null());

    let samples = [one as *const _, two as *const _];
    assert_eq!(
        unsafe { lling_semiring_validate_laws(semiring, samples.as_ptr(), samples.len(), 0.0) },
        LlingStatus::Ok
    );

    unsafe {
        lling_semiring_weight_free(two);
        lling_semiring_weight_free(one);
        lling_semiring_weight_free(zero);
        lling_semiring_free(semiring);
    }
    assert_eq!(resource.state().live_tokens.load(Ordering::Relaxed), 0);
    assert_eq!(resource.state().references.load(Ordering::Relaxed), 1);
}
