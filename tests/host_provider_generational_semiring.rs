#![cfg(feature = "bindings-core")]

//! Generated stale-token checks through the public Vinary Tree Interop vtable
//! and the real lling-llang dynamic-semiring consumer.

use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use lling_llang::dynamic_semiring::DynamicSemiringContext;
use proptest::prelude::*;
use vinary_tree_interop::{
    semiring_order, VtInterfaceId, VtResource, VtResourceVTable, VtSemiringVTable, VtSemiringValue,
    VtStatus, VT_ABI_VERSION, VT_SEMIRING_INTERFACE_ID, VT_SEMIRING_INTERFACE_VERSION,
};

const DOMAIN: VtInterfaceId = VtInterfaceId {
    bytes: *b"gen.tokens.test1",
};

struct Slot {
    generation: u64,
    value: Option<f64>,
}

#[derive(Default)]
struct Arena {
    slots: Vec<Slot>,
}

impl Arena {
    fn allocate(&mut self, value: f64) -> VtSemiringValue {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.value.is_none() {
                slot.generation = slot.generation.checked_add(1).unwrap();
                slot.value = Some(value);
                return VtSemiringValue {
                    word0: (index + 1) as u64,
                    word1: slot.generation,
                };
            }
        }
        self.slots.push(Slot {
            generation: 1,
            value: Some(value),
        });
        VtSemiringValue {
            word0: self.slots.len() as u64,
            word1: 1,
        }
    }

    fn read(&self, token: VtSemiringValue, accept_stale: bool) -> Result<f64, VtStatus> {
        let index = usize::try_from(token.word0)
            .ok()
            .and_then(|word| word.checked_sub(1))
            .ok_or(VtStatus::InvalidArgument)?;
        let slot = self.slots.get(index).ok_or(VtStatus::InvalidArgument)?;
        if slot.generation != token.word1 && !accept_stale {
            return Err(VtStatus::InvalidArgument);
        }
        slot.value.ok_or(VtStatus::InvalidArgument)
    }

    fn release(&mut self, token: VtSemiringValue) -> Result<(), VtStatus> {
        let index = usize::try_from(token.word0)
            .ok()
            .and_then(|word| word.checked_sub(1))
            .ok_or(VtStatus::InvalidArgument)?;
        let slot = self.slots.get_mut(index).ok_or(VtStatus::InvalidArgument)?;
        if slot.generation != token.word1 || slot.value.is_none() {
            return Err(VtStatus::InvalidArgument);
        }
        slot.value = None;
        Ok(())
    }

    fn live(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.value.is_some())
            .count()
    }
}

struct State {
    references: AtomicUsize,
    accept_stale_mutant: AtomicBool,
    arena: Mutex<Arena>,
}

struct Resource {
    raw: VtResource,
    state: *mut State,
}

impl Resource {
    fn new() -> Self {
        let state = Box::into_raw(Box::new(State {
            references: AtomicUsize::new(1),
            accept_stale_mutant: AtomicBool::new(false),
            arena: Mutex::new(Arena::default()),
        }));
        Self {
            raw: VtResource {
                context: state.cast(),
                vtable: &RESOURCE_VTABLE,
            },
            state,
        }
    }

    fn state(&self) -> &State {
        // SAFETY: this wrapper holds the original resource retain.
        unsafe { &*self.state }
    }
}

impl Drop for Resource {
    fn drop(&mut self) {
        // SAFETY: this consumes exactly the wrapper's original retain.
        unsafe { release_resource(self.raw.context) };
    }
}

unsafe fn state(context: *mut c_void) -> &'static State {
    // SAFETY: callers hold an owned or borrowed live resource retain.
    unsafe { &*context.cast::<State>() }
}

unsafe extern "C" fn retain_resource(context: *mut c_void) {
    unsafe { state(context) }
        .references
        .fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C" fn release_resource(context: *mut c_void) {
    let current = unsafe { state(context) }
        .references
        .fetch_sub(1, Ordering::AcqRel);
    assert!(current > 0);
    if current == 1 {
        drop(unsafe { Box::from_raw(context.cast::<State>()) });
    }
}

unsafe extern "C" fn query_interface(
    _context: *mut c_void,
    interface_id: *const VtInterfaceId,
    minimum_version: u32,
    output: *mut *const c_void,
) -> u32 {
    if interface_id.is_null() || output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    if unsafe { *interface_id } == VT_SEMIRING_INTERFACE_ID
        && minimum_version <= VT_SEMIRING_INTERFACE_VERSION
    {
        unsafe { *output = (&SEMIRING_VTABLE as *const VtSemiringVTable).cast() };
        VtStatus::Ok.to_raw()
    } else {
        VtStatus::Unsupported.to_raw()
    }
}

unsafe fn write_value(context: *mut c_void, output: *mut VtSemiringValue, value: f64) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let token = unsafe { state(context) }
        .arena
        .lock()
        .unwrap()
        .allocate(value);
    unsafe { *output = token };
    VtStatus::Ok.to_raw()
}

unsafe fn read_value(context: *mut c_void, input: *const VtSemiringValue) -> Result<f64, VtStatus> {
    if input.is_null() {
        return Err(VtStatus::NullPointer);
    }
    let state = unsafe { state(context) };
    state.arena.lock().unwrap().read(
        unsafe { *input },
        state.accept_stale_mutant.load(Ordering::Relaxed),
    )
}

unsafe extern "C" fn zero(context: *mut c_void, output: *mut VtSemiringValue) -> u32 {
    unsafe { write_value(context, output, 0.0) }
}

unsafe extern "C" fn one(context: *mut c_void, output: *mut VtSemiringValue) -> u32 {
    unsafe { write_value(context, output, 1.0) }
}

unsafe extern "C" fn clone_value(
    context: *mut c_void,
    input: *const VtSemiringValue,
    output: *mut VtSemiringValue,
) -> u32 {
    match unsafe { read_value(context, input) } {
        Ok(value) => unsafe { write_value(context, output, value) },
        Err(status) => status.to_raw(),
    }
}

unsafe extern "C" fn release_values(
    context: *mut c_void,
    values: *mut VtSemiringValue,
    count: usize,
) -> u32 {
    if count > 0 && values.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let mut arena = unsafe { state(context) }.arena.lock().unwrap();
    let mut seen = HashSet::with_capacity(count);
    for index in 0..count {
        let token = unsafe { *values.add(index) };
        if arena.read(token, false).is_err() || !seen.insert((token.word0, token.word1)) {
            return VtStatus::InvalidArgument.to_raw();
        }
    }
    for index in 0..count {
        let token = unsafe { *values.add(index) };
        if let Err(status) = arena.release(token) {
            return status.to_raw();
        }
        unsafe { *values.add(index) = VtSemiringValue { word0: 0, word1: 0 } };
    }
    VtStatus::Ok.to_raw()
}

unsafe fn binary_values(
    context: *mut c_void,
    left: *const VtSemiringValue,
    right: *const VtSemiringValue,
) -> Result<(f64, f64), VtStatus> {
    Ok((unsafe { read_value(context, left) }?, unsafe {
        read_value(context, right)
    }?))
}

unsafe extern "C" fn plus(
    context: *mut c_void,
    left: *const VtSemiringValue,
    right: *const VtSemiringValue,
    output: *mut VtSemiringValue,
) -> u32 {
    match unsafe { binary_values(context, left, right) } {
        Ok((left, right)) => unsafe { write_value(context, output, left + right) },
        Err(status) => status.to_raw(),
    }
}

unsafe extern "C" fn times(
    context: *mut c_void,
    left: *const VtSemiringValue,
    right: *const VtSemiringValue,
    output: *mut VtSemiringValue,
) -> u32 {
    match unsafe { binary_values(context, left, right) } {
        Ok((left, right)) => unsafe { write_value(context, output, left * right) },
        Err(status) => status.to_raw(),
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
    match unsafe { binary_values(context, left, right) } {
        Ok((left, right)) => {
            unsafe { *output = u8::from(left == right) };
            VtStatus::Ok.to_raw()
        }
        Err(status) => status.to_raw(),
    }
}

unsafe extern "C" fn approx_equal(
    context: *mut c_void,
    left: *const VtSemiringValue,
    right: *const VtSemiringValue,
    epsilon: f64,
    output: *mut u8,
) -> u32 {
    if output.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    match unsafe { binary_values(context, left, right) } {
        Ok((left, right)) => {
            unsafe { *output = u8::from((left - right).abs() <= epsilon) };
            VtStatus::Ok.to_raw()
        }
        Err(status) => status.to_raw(),
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
    match unsafe { binary_values(context, left, right) } {
        Ok((left, right)) => {
            unsafe {
                *output = if left < right {
                    semiring_order::BETTER
                } else if left > right {
                    semiring_order::WORSE
                } else {
                    semiring_order::EQUAL
                }
            };
            VtStatus::Ok.to_raw()
        }
        Err(status) => status.to_raw(),
    }
}

static RESOURCE_VTABLE: VtResourceVTable = VtResourceVTable {
    struct_size: size_of::<VtResourceVTable>(),
    abi_version: VT_ABI_VERSION,
    reserved: 0,
    retain: Some(retain_resource),
    release: Some(release_resource),
    query_interface: Some(query_interface),
};

static SEMIRING_VTABLE: VtSemiringVTable = VtSemiringVTable {
    struct_size: size_of::<VtSemiringVTable>(),
    interface_version: VT_SEMIRING_INTERFACE_VERSION,
    reserved: 0,
    flags: 0,
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
    stable_bytes: None,
    diagnostic: None,
    plus_many: None,
    times_many: None,
};

fn raw_zero(resource: &Resource) -> VtSemiringValue {
    let mut token = VtSemiringValue { word0: 0, word1: 0 };
    let status = unsafe { zero(resource.raw.context, &mut token) };
    assert_eq!(status, VtStatus::Ok.to_raw());
    token
}

fn raw_release(resource: &Resource, token: &mut VtSemiringValue) -> VtStatus {
    VtStatus::from_raw(unsafe { release_values(resource.raw.context, token, 1) }).unwrap()
}

fn stale_status(
    resource: &Resource,
    stale: VtSemiringValue,
    current: VtSemiringValue,
) -> (u32, u8) {
    let mut result = 7;
    let status = unsafe { equal(resource.raw.context, &stale, &current, &mut result) };
    (status, result)
}

// INVARIANT-HOOK: LLING-HOST-4..6 — a real ABI arena reuses slots but never
// accepts an earlier generation or publishes output for a failed operation.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn host_provider_generated_token_generations(reuses in 1usize..32) {
        let resource = Resource::new();
        let context = unsafe { DynamicSemiringContext::borrow_raw(resource.raw) }.unwrap();
        prop_assert_eq!(resource.state().references.load(Ordering::SeqCst), 2);
        let mut previous_generation = 0;
        for _ in 0..reuses {
            let mut stale = raw_zero(&resource);
            prop_assert_eq!(stale.word0, 1);
            prop_assert!(stale.word1 > previous_generation);
            let old = stale;
            prop_assert_eq!(raw_release(&resource, &mut stale), VtStatus::Ok);
            let mut current = raw_zero(&resource);
            prop_assert_eq!(current.word0, 1);
            prop_assert!(current.word1 > old.word1);
            let (status, output) = stale_status(&resource, old, current);
            prop_assert_eq!(status, VtStatus::InvalidArgument.to_raw());
            prop_assert_eq!(output, 7);
            let current_generation = current.word1;
            prop_assert_eq!(raw_release(&resource, &mut current), VtStatus::Ok);
            previous_generation = current_generation;
        }
        let weight = context.one().unwrap();
        prop_assert_eq!(resource.state().arena.lock().unwrap().live(), 1);
        let cloned = weight.try_clone().unwrap();
        prop_assert_eq!(context.equal(&weight, &cloned).unwrap(), true);
        drop(context);
        prop_assert_eq!(resource.state().references.load(Ordering::SeqCst), 2);
        drop((weight, cloned));
        prop_assert_eq!(resource.state().arena.lock().unwrap().live(), 0);
        prop_assert_eq!(resource.state().references.load(Ordering::SeqCst), 1);
    }
}

#[test]
#[should_panic(expected = "stale semiring generation accepted")]
fn host_provider_stale_generation_mutant_is_detected() {
    let resource = Resource::new();
    let mut old = raw_zero(&resource);
    let old_bits = old;
    assert_eq!(raw_release(&resource, &mut old), VtStatus::Ok);
    let mut current = raw_zero(&resource);
    resource
        .state()
        .accept_stale_mutant
        .store(true, Ordering::SeqCst);
    let (status, _) = stale_status(&resource, old_bits, current);
    assert_eq!(raw_release(&resource, &mut current), VtStatus::Ok);
    assert_eq!(
        status,
        VtStatus::InvalidArgument.to_raw(),
        "stale semiring generation accepted"
    );
}
