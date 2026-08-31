#![cfg(feature = "bindings-core")]

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use lling_llang::dynamic_lattice::{
    DynamicLatticeError, DynamicLatticeValue, ParallelDynamicLatticeValue,
};
#[cfg(feature = "ffi")]
use lling_llang::ffi::{
    lling_lattice_domain_id, lling_lattice_equal, lling_lattice_flags, lling_lattice_free,
    lling_lattice_join, lling_lattice_join_many, lling_lattice_open, lling_lattice_stable_bytes,
    lling_lattice_validate_laws, LlingLatticeValue, LlingStatus,
};
use vinary_tree_interop::{
    lattice_flags, VtInterfaceId, VtLatticeVTable, VtResource, VtResourceVTable, VtStatus,
    VT_ABI_VERSION, VT_LATTICE_INTERFACE_ID, VT_LATTICE_INTERFACE_VERSION,
    VT_RECOMMENDED_LATTICE_BATCH,
};

const DOMAIN: VtInterfaceId = VtInterfaceId {
    bytes: *b"test.maxmin.v1..",
};
const OTHER_DOMAIN: VtInterfaceId = VtInterfaceId {
    bytes: *b"test.maxmin.v2..",
};

#[derive(Default)]
struct Metrics {
    live_resources: AtomicUsize,
    batch_calls: AtomicUsize,
    algebra_calls: AtomicUsize,
    hostile: AtomicU8,
}

struct ValueContext {
    references: AtomicUsize,
    value: u64,
    metrics: Arc<Metrics>,
    lattice: &'static VtLatticeVTable,
}

struct TestValue {
    raw: VtResource,
    context: *mut ValueContext,
}

impl TestValue {
    fn new(value: u64, lattice: &'static VtLatticeVTable, metrics: Arc<Metrics>) -> Self {
        let raw = allocate(value, lattice, metrics);
        Self {
            raw,
            context: raw.context.cast(),
        }
    }

    fn references(&self) -> usize {
        // SAFETY: the wrapper owns the original retain.
        unsafe { (*self.context).references.load(Ordering::Relaxed) }
    }
}

impl Drop for TestValue {
    fn drop(&mut self) {
        // SAFETY: this consumes the wrapper's original retain.
        unsafe { resource_release(self.raw.context) };
    }
}

fn allocate(value: u64, lattice: &'static VtLatticeVTable, metrics: Arc<Metrics>) -> VtResource {
    metrics.live_resources.fetch_add(1, Ordering::Relaxed);
    let context = Box::into_raw(Box::new(ValueContext {
        references: AtomicUsize::new(1),
        value,
        metrics,
        lattice,
    }));
    VtResource {
        context: context.cast(),
        vtable: &RESOURCE_VTABLE,
    }
}

unsafe fn value_context(context: *mut c_void) -> &'static ValueContext {
    &*context.cast::<ValueContext>()
}

unsafe fn value_from_resource(resource: *const VtResource) -> Result<u64, VtStatus> {
    if resource.is_null() || (*resource).is_null() {
        return Err(VtStatus::NullPointer);
    }
    Ok(value_context((*resource).context).value)
}

unsafe extern "C" fn resource_retain(context: *mut c_void) {
    value_context(context)
        .references
        .fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C" fn resource_release(context: *mut c_void) {
    let current = value_context(context)
        .references
        .fetch_sub(1, Ordering::AcqRel);
    assert!(current > 0, "resource retain count underflow");
    if current == 1 {
        let boxed = Box::from_raw(context.cast::<ValueContext>());
        boxed.metrics.live_resources.fetch_sub(1, Ordering::Relaxed);
        drop(boxed);
    }
}

unsafe extern "C" fn resource_query(
    context: *mut c_void,
    interface_id: *const VtInterfaceId,
    minimum_version: u32,
    output: *mut *const c_void,
) -> u32 {
    if interface_id.is_null() || output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let value = value_context(context);
    if value.metrics.hostile.load(Ordering::Relaxed) == 3 {
        *output = value.lattice as *const VtLatticeVTable as *const c_void;
        return VtStatus::Unsupported.to_raw();
    }
    if *interface_id != VT_LATTICE_INTERFACE_ID || minimum_version > VT_LATTICE_INTERFACE_VERSION {
        return VtStatus::Unsupported.to_raw();
    }
    *output = value.lattice as *const VtLatticeVTable as *const c_void;
    VtStatus::Ok.to_raw()
}

unsafe fn write_result(context: *mut c_void, output: *mut VtResource, result: u64) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let receiver = value_context(context);
    let hostile = receiver.metrics.hostile.load(Ordering::Relaxed);
    let lattice = match hostile {
        2 => &OTHER_LATTICE_VTABLE,
        6 => &SERIAL_LATTICE_VTABLE,
        _ => receiver.lattice,
    };
    *output = allocate(result, lattice, Arc::clone(&receiver.metrics));
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn join(
    context: *mut c_void,
    other: *const VtResource,
    output: *mut VtResource,
) -> u32 {
    let receiver = value_context(context);
    receiver
        .metrics
        .algebra_calls
        .fetch_add(1, Ordering::Relaxed);
    match value_from_resource(other) {
        Ok(other) => {
            let result = if receiver.metrics.hostile.load(Ordering::Relaxed) == 5 {
                receiver.value.wrapping_add(other)
            } else {
                receiver.value.max(other)
            };
            write_result(context, output, result)
        }
        Err(status) => status.to_raw(),
    }
}

unsafe extern "C" fn meet(
    context: *mut c_void,
    other: *const VtResource,
    output: *mut VtResource,
) -> u32 {
    let receiver = value_context(context);
    receiver
        .metrics
        .algebra_calls
        .fetch_add(1, Ordering::Relaxed);
    match value_from_resource(other) {
        Ok(other) => write_result(context, output, receiver.value.min(other)),
        Err(status) => status.to_raw(),
    }
}

unsafe extern "C" fn equal(context: *mut c_void, other: *const VtResource, output: *mut u8) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let receiver = value_context(context);
    match value_from_resource(other) {
        Ok(other) => {
            *output = if receiver.metrics.hostile.load(Ordering::Relaxed) == 1 {
                2
            } else {
                u8::from(receiver.value == other)
            };
            VtStatus::Ok.to_raw()
        }
        Err(status) => status.to_raw(),
    }
}

unsafe fn copy_bytes(
    bytes: &[u8],
    output: *mut u8,
    capacity: usize,
    written: *mut usize,
    required: *mut usize,
    short_final: bool,
) -> u32 {
    if written.is_null() || required.is_null() || (capacity != 0 && output.is_null()) {
        return VtStatus::NullPointer.to_raw();
    }
    *required = bytes.len();
    *written = if short_final && capacity >= bytes.len() {
        bytes.len().saturating_sub(1)
    } else {
        capacity.min(bytes.len())
    };
    if *written != 0 {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, *written);
    }
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn stable_bytes(
    context: *mut c_void,
    output: *mut u8,
    capacity: usize,
    written: *mut usize,
    required: *mut usize,
) -> u32 {
    let value = value_context(context);
    copy_bytes(
        &value.value.to_be_bytes(),
        output,
        capacity,
        written,
        required,
        value.metrics.hostile.load(Ordering::Relaxed) == 4,
    )
}

unsafe extern "C" fn diagnostic(
    _context: *mut c_void,
    output: *mut u8,
    capacity: usize,
    written: *mut usize,
    required: *mut usize,
) -> u32 {
    copy_bytes(
        b"mock max/min lattice",
        output,
        capacity,
        written,
        required,
        false,
    )
}

unsafe fn fold_many(
    context: *mut c_void,
    others: *const VtResource,
    count: usize,
    output: *mut VtResource,
    join_fold: bool,
) -> u32 {
    if count != 0 && others.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let receiver = value_context(context);
    receiver.metrics.batch_calls.fetch_add(1, Ordering::Relaxed);
    let mut result = receiver.value;
    for index in 0..count {
        match value_from_resource(others.add(index)) {
            Ok(value) if join_fold => result = result.max(value),
            Ok(value) => result = result.min(value),
            Err(status) => return status.to_raw(),
        }
    }
    write_result(context, output, result)
}

unsafe extern "C" fn join_many(
    context: *mut c_void,
    others: *const VtResource,
    count: usize,
    output: *mut VtResource,
) -> u32 {
    fold_many(context, others, count, output, true)
}

unsafe extern "C" fn meet_many(
    context: *mut c_void,
    others: *const VtResource,
    count: usize,
    output: *mut VtResource,
) -> u32 {
    fold_many(context, others, count, output, false)
}

static RESOURCE_VTABLE: VtResourceVTable = VtResourceVTable {
    struct_size: size_of::<VtResourceVTable>(),
    abi_version: VT_ABI_VERSION,
    reserved: 0,
    retain: Some(resource_retain),
    release: Some(resource_release),
    query_interface: Some(resource_query),
};

macro_rules! lattice_vtable {
    ($flags:expr, $domain:expr) => {
        VtLatticeVTable {
            struct_size: size_of::<VtLatticeVTable>(),
            interface_version: VT_LATTICE_INTERFACE_VERSION,
            reserved: 0,
            flags: $flags,
            domain_id: $domain,
            join: Some(join),
            meet: Some(meet),
            equal: Some(equal),
            stable_bytes: Some(stable_bytes),
            diagnostic: Some(diagnostic),
            join_many: Some(join_many),
            meet_many: Some(meet_many),
        }
    };
}

const BASE_FLAGS: u64 = lattice_flags::STABLE_BYTES | lattice_flags::BATCH;
static SERIAL_LATTICE_VTABLE: VtLatticeVTable = lattice_vtable!(BASE_FLAGS, DOMAIN);
static PARALLEL_LATTICE_VTABLE: VtLatticeVTable =
    lattice_vtable!(BASE_FLAGS | lattice_flags::PARALLEL_REENTRANT, DOMAIN);
static OTHER_LATTICE_VTABLE: VtLatticeVTable = lattice_vtable!(BASE_FLAGS, OTHER_DOMAIN);
static INVALID_FLAGS_VTABLE: VtLatticeVTable = lattice_vtable!(
    BASE_FLAGS | lattice_flags::THREAD_BOUND | lattice_flags::PARALLEL_REENTRANT,
    DOMAIN
);

fn decode_value(value: &DynamicLatticeValue) -> u64 {
    u64::from_be_bytes(value.stable_bytes().unwrap().try_into().unwrap())
}

#[test]
fn ownership_algebra_laws_and_bounded_batches_are_exact() {
    let metrics = Arc::new(Metrics::default());
    let zero = TestValue::new(0, &SERIAL_LATTICE_VTABLE, Arc::clone(&metrics));
    let two = TestValue::new(2, &SERIAL_LATTICE_VTABLE, Arc::clone(&metrics));
    let five = TestValue::new(5, &SERIAL_LATTICE_VTABLE, Arc::clone(&metrics));
    let zero_value = unsafe { DynamicLatticeValue::borrow_raw(zero.raw) }.unwrap();
    let two_value = unsafe { DynamicLatticeValue::borrow_raw(two.raw) }.unwrap();
    let five_value = unsafe { DynamicLatticeValue::borrow_raw(five.raw) }.unwrap();
    assert_eq!(zero.references(), 2);
    assert_eq!(two.references(), 2);
    assert_eq!(five.references(), 2);
    assert_eq!(zero_value.domain_id(), DOMAIN);
    assert_eq!(decode_value(&two_value.join(&five_value).unwrap()), 5);
    assert_eq!(decode_value(&two_value.meet(&five_value).unwrap()), 2);
    assert!(!two_value.equal(&five_value).unwrap());
    assert_eq!(two_value.diagnostic().unwrap(), "mock max/min lattice");
    DynamicLatticeValue::validate_laws(&[
        zero_value.clone(),
        two_value.clone(),
        five_value.clone(),
    ])
    .unwrap();

    let roots: Vec<_> = (0..600)
        .map(|value| TestValue::new(value, &SERIAL_LATTICE_VTABLE, Arc::clone(&metrics)))
        .collect();
    let values: Vec<_> = roots
        .iter()
        .map(|value| unsafe { DynamicLatticeValue::borrow_raw(value.raw) }.unwrap())
        .collect();
    let maximum = zero_value.join_many(&values).unwrap();
    assert_eq!(decode_value(&maximum), 599);
    assert_eq!(
        metrics.batch_calls.load(Ordering::Relaxed),
        600_usize.div_ceil(VT_RECOMMENDED_LATTICE_BATCH)
    );

    drop((maximum, values, roots, zero_value, two_value, five_value));
    assert_eq!(zero.references(), 1);
    assert_eq!(two.references(), 1);
    assert_eq!(five.references(), 1);
    drop((zero, two, five));
    assert_eq!(metrics.live_resources.load(Ordering::Relaxed), 0);
}

#[test]
fn incompatible_domains_flags_and_hostile_outputs_are_rejected() {
    let metrics = Arc::new(Metrics::default());
    let left = TestValue::new(2, &SERIAL_LATTICE_VTABLE, Arc::clone(&metrics));
    let right = TestValue::new(5, &SERIAL_LATTICE_VTABLE, Arc::clone(&metrics));
    let other_domain = TestValue::new(5, &OTHER_LATTICE_VTABLE, Arc::clone(&metrics));
    let left_value = unsafe { DynamicLatticeValue::borrow_raw(left.raw) }.unwrap();
    let right_value = unsafe { DynamicLatticeValue::borrow_raw(right.raw) }.unwrap();
    let other_value = unsafe { DynamicLatticeValue::borrow_raw(other_domain.raw) }.unwrap();
    let calls = metrics.algebra_calls.load(Ordering::Relaxed);
    assert!(matches!(
        left_value.join(&other_value),
        Err(DynamicLatticeError::DomainMismatch)
    ));
    assert_eq!(metrics.algebra_calls.load(Ordering::Relaxed), calls);

    metrics.hostile.store(1, Ordering::Relaxed);
    assert!(matches!(
        left_value.equal(&right_value),
        Err(DynamicLatticeError::InvalidProviderOutput {
            operation: "equal",
            ..
        })
    ));
    metrics.hostile.store(2, Ordering::Relaxed);
    assert!(matches!(
        left_value.join(&right_value),
        Err(DynamicLatticeError::InvalidProviderOutput {
            operation: "join",
            ..
        })
    ));
    metrics.hostile.store(4, Ordering::Relaxed);
    assert!(matches!(
        left_value.stable_bytes(),
        Err(DynamicLatticeError::InvalidProviderOutput {
            operation: "stable_bytes",
            ..
        })
    ));
    metrics.hostile.store(5, Ordering::Relaxed);
    assert!(matches!(
        DynamicLatticeValue::validate_laws(&[left_value.clone(), right_value.clone()]),
        Err(DynamicLatticeError::LawViolation(_))
    ));
    metrics.hostile.store(0, Ordering::Relaxed);

    let invalid = TestValue::new(1, &INVALID_FLAGS_VTABLE, Arc::clone(&metrics));
    assert!(matches!(
        unsafe { DynamicLatticeValue::borrow_raw(invalid.raw) },
        Err(DynamicLatticeError::IncompatibleInterface(_))
    ));
    assert_eq!(invalid.references(), 1);

    let discovery = TestValue::new(1, &SERIAL_LATTICE_VTABLE, Arc::clone(&metrics));
    metrics.hostile.store(3, Ordering::Relaxed);
    assert!(matches!(
        unsafe { DynamicLatticeValue::borrow_raw(discovery.raw) },
        Err(DynamicLatticeError::InvalidProviderOutput { .. })
    ));
    assert_eq!(discovery.references(), 1);
}

#[test]
fn only_parallel_reentrant_values_cross_threads_and_results_preserve_access() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ParallelDynamicLatticeValue>();

    let serial_metrics = Arc::new(Metrics::default());
    let serial_resource = TestValue::new(7, &SERIAL_LATTICE_VTABLE, serial_metrics);
    let serial = unsafe { DynamicLatticeValue::borrow_raw(serial_resource.raw) }.unwrap();
    assert!(matches!(
        serial.parallel(),
        Err(DynamicLatticeError::MissingCapability(
            "parallel reentrancy"
        ))
    ));

    let metrics = Arc::new(Metrics::default());
    let first = TestValue::new(7, &PARALLEL_LATTICE_VTABLE, Arc::clone(&metrics));
    let second = TestValue::new(11, &PARALLEL_LATTICE_VTABLE, Arc::clone(&metrics));
    let first = unsafe { DynamicLatticeValue::borrow_raw(first.raw) }
        .unwrap()
        .parallel()
        .unwrap();
    let second = unsafe { DynamicLatticeValue::borrow_raw(second.raw) }
        .unwrap()
        .parallel()
        .unwrap();
    let joined = std::thread::spawn(move || first.join(&second).unwrap())
        .join()
        .unwrap();
    assert_eq!(joined.stable_bytes().unwrap(), 11_u64.to_be_bytes());

    metrics.hostile.store(6, Ordering::Relaxed);
    assert!(matches!(
        joined.join(&joined),
        Err(DynamicLatticeError::IncompatibleInterface(
            "operation result lost parallel reentrancy"
        ))
    ));
}

#[cfg(feature = "ffi")]
#[test]
fn c_surface_owns_computes_batches_and_validates_the_same_dynamic_values() {
    let metrics = Arc::new(Metrics::default());
    let two = TestValue::new(2, &SERIAL_LATTICE_VTABLE, Arc::clone(&metrics));
    let five = TestValue::new(5, &SERIAL_LATTICE_VTABLE, Arc::clone(&metrics));
    let mut left: *mut LlingLatticeValue = std::ptr::null_mut();
    let mut right: *mut LlingLatticeValue = std::ptr::null_mut();
    assert_eq!(
        unsafe { lling_lattice_open(&two.raw, &mut left) },
        LlingStatus::Ok
    );
    assert_eq!(
        unsafe { lling_lattice_open(&five.raw, &mut right) },
        LlingStatus::Ok
    );

    let mut domain = VtInterfaceId { bytes: [0; 16] };
    let mut flags = 0;
    assert_eq!(lling_lattice_domain_id(left, &mut domain), LlingStatus::Ok);
    assert_eq!(lling_lattice_flags(left, &mut flags), LlingStatus::Ok);
    assert_eq!(domain, DOMAIN);
    assert_eq!(flags, BASE_FLAGS);

    let mut joined: *mut LlingLatticeValue = std::ptr::null_mut();
    assert_eq!(
        lling_lattice_join(left, right, &mut joined),
        LlingStatus::Ok
    );
    let mut equal = u8::MAX;
    assert_eq!(
        lling_lattice_equal(joined, right, &mut equal),
        LlingStatus::Ok
    );
    assert_eq!(equal, 1);

    let mut written = usize::MAX;
    let mut required = usize::MAX;
    assert_eq!(
        unsafe {
            lling_lattice_stable_bytes(joined, std::ptr::null_mut(), 0, &mut written, &mut required)
        },
        LlingStatus::Ok
    );
    assert_eq!((written, required), (0, 8));
    let mut bytes = [0_u8; 8];
    assert_eq!(
        unsafe {
            lling_lattice_stable_bytes(
                joined,
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut written,
                &mut required,
            )
        },
        LlingStatus::Ok
    );
    assert_eq!(bytes, 5_u64.to_be_bytes());

    let operands = [right as *const LlingLatticeValue; 2];
    let mut batched: *mut LlingLatticeValue = std::ptr::null_mut();
    assert_eq!(
        unsafe { lling_lattice_join_many(left, operands.as_ptr(), operands.len(), &mut batched) },
        LlingStatus::Ok
    );
    let samples = [left as *const LlingLatticeValue, right];
    assert_eq!(
        unsafe { lling_lattice_validate_laws(samples.as_ptr(), samples.len()) },
        LlingStatus::Ok
    );

    unsafe {
        lling_lattice_free(batched);
        lling_lattice_free(joined);
        lling_lattice_free(right);
        lling_lattice_free(left);
    }
    assert_eq!(two.references(), 1);
    assert_eq!(five.references(), 1);
}
