//! Hand-rolled `vt.scalar-wfst.1` (and minimal `vt.dictionary.v1`) producers
//! for exercising lling-llang's ABI consumer chokepoints from outside the
//! crate boundary.
//!
//! [`TestWfst`] wraps an adjacency-list model behind real extern "C"
//! callbacks, with:
//! - a [`Metrics`] ledger (retains/releases/snapshots/state_info/state_arcs)
//!   shared by the provider and every snapshot it hands out, so tests can
//!   prove laziness (INVARIANT-HOOK: LLING-LAZY-1) and retain/release balance
//!   after the resource is gone;
//! - configurable interface flags (`LAZY` / `IMMUTABLE` / `PARALLEL_REENTRANT`
//!   / `ACYCLIC`), unit domain, and weight domain (for non-tropical rejection
//!   coverage);
//! - raw, ABI-shaped arcs ([`TestArc`]), so adversarial payloads — NaN or
//!   -inf weights (the F1 shape), `has_input == 2`, labels beyond
//!   `char::MAX` — are expressed directly in the model; and
//! - call-level [`Misbehavior`] modes (`out_written > capacity`, unstable
//!   `out_total`, injected status codes, including out-of-range raw values).
//!
//! Status wire rule: every callback returns the RAW `u32` status. Producers
//! here encode through typed `*_status` inner functions plus a `.to_raw()`
//! shim — the same idiom as lling-llang's own producers — and misbehavior
//! modes may inject arbitrary raw values to prove the consumer decodes with
//! `VtStatus::from_raw` instead of trusting the wire.
//!
//! Panic discipline: an unwind across an extern "C" boundary aborts the
//! process, so NOTHING in these callbacks may panic. All model access is
//! bounds-checked with pattern matching and every fault is signalled through
//! status codes or data, never by unwinding.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use vinary_tree_interop::{
    dictionary_flags, wfst_flags, VtDictionaryEdge, VtDictionaryVTable, VtInterfaceId,
    VtOptionalU64, VtResource, VtResourceVTable, VtStatus, VtUnitDomain, VtValueDomain,
    VtWeightDomain, VtWfstArc, VtWfstVTable, VT_ABI_VERSION, VT_DICTIONARY_INTERFACE_ID,
    VT_DICTIONARY_INTERFACE_VERSION, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
};

// ─────────────────────────────────────────────────────────────────────────────
// Metrics ledger
// ─────────────────────────────────────────────────────────────────────────────

/// Callback ledger shared by a [`TestWfst`] and every snapshot it hands out.
///
/// `retains` counts every ownership acquisition, INCLUDING the constructor's
/// initial owned reference and the internal retain taken by each `snapshot`
/// call, so `balance() == 0` holds exactly when every owner has released.
#[derive(Default)]
pub struct Metrics {
    retains: AtomicUsize,
    releases: AtomicUsize,
    snapshots: AtomicUsize,
    state_info_calls: AtomicUsize,
    state_arcs_calls: AtomicUsize,
}

impl Metrics {
    /// Total ownership acquisitions (constructor + retain + snapshot-retain).
    pub fn retains(&self) -> usize {
        self.retains.load(Ordering::SeqCst)
    }

    /// Total `release` callback invocations.
    pub fn releases(&self) -> usize {
        self.releases.load(Ordering::SeqCst)
    }

    /// Total `snapshot` callback invocations.
    pub fn snapshots(&self) -> usize {
        self.snapshots.load(Ordering::SeqCst)
    }

    /// Total `state_info` callback invocations.
    pub fn state_info_calls(&self) -> usize {
        self.state_info_calls.load(Ordering::SeqCst)
    }

    /// Total `state_arcs` callback invocations.
    pub fn state_arcs_calls(&self) -> usize {
        self.state_arcs_calls.load(Ordering::SeqCst)
    }

    /// Outstanding owned references: retains minus releases.
    pub fn balance(&self) -> isize {
        let retains =
            isize::try_from(self.retains()).expect("retain count fits the signed balance");
        let releases =
            isize::try_from(self.releases()).expect("release count fits the signed balance");
        retains - releases
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Adjacency-list model
// ─────────────────────────────────────────────────────────────────────────────

/// One raw, ABI-shaped outgoing arc of the test model.
///
/// The fields mirror [`VtWfstArc`] exactly, so adversarial payloads (labels
/// beyond `char::MAX`, presence flags outside {0, 1}, NaN or -inf weights)
/// are expressed directly in the model rather than through fault switches.
#[derive(Clone, Copy, Debug)]
pub struct TestArc {
    /// Raw input label word (meaningful only when `has_input` is one).
    pub input_label: u64,
    /// Input presence flag as it will cross the wire (adversarial values ok).
    pub has_input: u8,
    /// Raw output label word (meaningful only when `has_output` is one).
    pub output_label: u64,
    /// Output presence flag as it will cross the wire.
    pub has_output: u8,
    /// Target state identifier.
    pub to: u64,
    /// Raw scalar weight as it will cross the wire (NaN/-inf expressible).
    pub weight: f64,
}

impl TestArc {
    /// Well-formed labelled arc `input:output/weight` to `to`.
    pub fn pair(input: char, output: char, to: u64, weight: f64) -> Self {
        Self {
            input_label: u64::from(input),
            has_input: 1,
            output_label: u64::from(output),
            has_output: 1,
            to,
            weight,
        }
    }

    /// Well-formed input-epsilon arc `ε:output/weight`.
    pub fn input_epsilon(output: char, to: u64, weight: f64) -> Self {
        Self {
            input_label: 0,
            has_input: 0,
            output_label: u64::from(output),
            has_output: 1,
            to,
            weight,
        }
    }

    /// Well-formed output-epsilon arc `input:ε/weight`.
    pub fn output_epsilon(input: char, to: u64, weight: f64) -> Self {
        Self {
            input_label: u64::from(input),
            has_input: 1,
            output_label: 0,
            has_output: 0,
            to,
            weight,
        }
    }
}

/// One state of the adjacency-list model.
#[derive(Clone, Debug)]
pub struct TestState {
    /// Whether the state accepts.
    pub is_final: bool,
    /// Raw final weight as it will cross the wire (NaN/-inf expressible).
    pub final_weight: f64,
    /// Outgoing arcs in emission order.
    pub arcs: Vec<TestArc>,
}

impl TestState {
    /// Non-final interior state with the given arcs.
    pub fn interior(arcs: Vec<TestArc>) -> Self {
        Self {
            is_final: false,
            final_weight: f64::INFINITY,
            arcs,
        }
    }

    /// Accepting state with the given final weight and arcs.
    pub fn accepting(final_weight: f64, arcs: Vec<TestArc>) -> Self {
        Self {
            is_final: true,
            final_weight,
            arcs,
        }
    }
}

/// Linear chain over `pairs`: state `i` carries `pairs[i].0 : pairs[i].1`
/// to `i + 1` at `arc_weight`; the last state accepts at `final_weight`.
pub fn chain_states(pairs: &[(char, char)], arc_weight: f64, final_weight: f64) -> Vec<TestState> {
    let mut states = Vec::with_capacity(pairs.len() + 1);
    for (index, &(input, output)) in pairs.iter().enumerate() {
        let target = u64::try_from(index + 1).expect("chain fixture fits u64");
        states.push(TestState::interior(vec![TestArc::pair(
            input, output, target, arc_weight,
        )]));
    }
    states.push(TestState::accepting(final_weight, Vec::new()));
    states
}

// ─────────────────────────────────────────────────────────────────────────────
// Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Call-level provider misbehavior, injected via status codes or counts only
/// (payload-level misbehavior lives directly in the [`TestArc`] model).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Misbehavior {
    /// Honest provider.
    None,
    /// `state_arcs` reports `out_written = capacity + 1` while never writing
    /// past `capacity` (the lie is in the count, not the memory).
    OvershootWritten,
    /// `state_arcs` inflates `out_total` by one more on every call, so no
    /// paging loop can ever terminate honestly.
    UnstableOutTotal,
    /// `state_info` returns this raw status (out-of-range values allowed).
    StateInfoStatus(u32),
    /// `state_arcs` returns this raw status (out-of-range values allowed).
    StateArcsStatus(u32),
}

/// Interface-level configuration of a [`TestWfst`].
#[derive(Clone, Copy, Debug)]
pub struct TestWfstConfig {
    /// Bitset from [`wfst_flags`].
    pub flags: u64,
    /// Advertised label domain.
    pub unit_domain: VtUnitDomain,
    /// Advertised scalar semiring.
    pub weight_domain: VtWeightDomain,
    /// Call-level misbehavior mode.
    pub misbehavior: Misbehavior,
}

impl Default for TestWfstConfig {
    fn default() -> Self {
        Self {
            flags: wfst_flags::PARALLEL_REENTRANT | wfst_flags::IMMUTABLE | wfst_flags::LAZY,
            unit_domain: VtUnitDomain::UnicodeScalar,
            weight_domain: VtWeightDomain::TropicalF64,
            misbehavior: Misbehavior::None,
        }
    }
}

impl TestWfstConfig {
    /// Default configuration WITHOUT `PARALLEL_REENTRANT`: the consumer must
    /// serialize every callback through its provider call gate.
    pub fn serial() -> Self {
        Self {
            flags: wfst_flags::IMMUTABLE | wfst_flags::LAZY,
            ..Self::default()
        }
    }

    /// Replace the advertised weight domain.
    pub fn with_weight_domain(mut self, weight_domain: VtWeightDomain) -> Self {
        self.weight_domain = weight_domain;
        self
    }

    /// Replace the advertised unit domain.
    pub fn with_unit_domain(mut self, unit_domain: VtUnitDomain) -> Self {
        self.unit_domain = unit_domain;
        self
    }

    /// Replace the misbehavior mode.
    pub fn with_misbehavior(mut self, misbehavior: Misbehavior) -> Self {
        self.misbehavior = misbehavior;
        self
    }

    /// Replace the interface flag bitset.
    pub fn with_flags(mut self, flags: u64) -> Self {
        self.flags = flags;
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The scalar-WFST provider
// ─────────────────────────────────────────────────────────────────────────────

struct WfstContext {
    start: u64,
    states: Vec<TestState>,
    misbehavior: Misbehavior,
    metrics: Arc<Metrics>,
    /// Monotone `state_arcs` call sequence backing `UnstableOutTotal`.
    arcs_call_sequence: AtomicUsize,
    /// Per-instance interface vtable (flags and domains vary per test), owned
    /// by this context so the pointer handed out by `query_interface` stays
    /// valid exactly as long as the resource is retained.
    vtable: VtWfstVTable,
}

/// One owned retain of a hand-rolled `vt.scalar-wfst.1` test resource.
pub struct TestWfst {
    raw: VtResource,
    metrics: Arc<Metrics>,
}

// The context is an Arc of atomics + immutable model data; the raw vtable
// pointer is immutable for the Arc's lifetime.
unsafe impl Send for TestWfst {}
unsafe impl Sync for TestWfst {}

impl TestWfst {
    /// Publish `states` (with `start` as the initial state) under `config`.
    pub fn new(states: Vec<TestState>, start: u64, config: TestWfstConfig) -> Self {
        let metrics = Arc::new(Metrics::default());
        let context = Arc::new(WfstContext {
            start,
            states,
            misbehavior: config.misbehavior,
            metrics: Arc::clone(&metrics),
            arcs_call_sequence: AtomicUsize::new(0),
            vtable: VtWfstVTable {
                struct_size: std::mem::size_of::<VtWfstVTable>(),
                interface_version: VT_WFST_INTERFACE_VERSION,
                unit_domain: config.unit_domain,
                weight_domain: config.weight_domain,
                reserved: 0,
                flags: config.flags,
                snapshot: Some(wfst_snapshot),
                start: Some(wfst_start),
                num_states: Some(wfst_num_states),
                state_info: Some(wfst_state_info),
                state_arcs: Some(wfst_state_arcs),
            },
        });
        // The constructor's initial owned reference is part of the ledger:
        // balance() == 0 exactly when every owner (including us) released.
        metrics.retains.fetch_add(1, Ordering::SeqCst);
        Self {
            raw: VtResource {
                context: Arc::into_raw(context).cast_mut().cast(),
                vtable: &WFST_RESOURCE_VTABLE,
            },
            metrics,
        }
    }

    /// Convenience constructor with the default (parallel/immutable/lazy,
    /// Unicode/tropical, honest) configuration.
    pub fn tropical(states: Vec<TestState>, start: u64) -> Self {
        Self::new(states, start, TestWfstConfig::default())
    }

    /// Borrow the two-word ABI value (the wrapper keeps its owned retain).
    pub fn as_raw(&self) -> VtResource {
        self.raw
    }

    /// The shared callback ledger; it outlives the provider.
    pub fn metrics(&self) -> Arc<Metrics> {
        Arc::clone(&self.metrics)
    }
}

impl Drop for TestWfst {
    fn drop(&mut self) {
        unsafe { wfst_release(self.raw.context) }
    }
}

static WFST_RESOURCE_VTABLE: VtResourceVTable = VtResourceVTable {
    struct_size: std::mem::size_of::<VtResourceVTable>(),
    abi_version: VT_ABI_VERSION,
    reserved: 0,
    retain: Some(wfst_retain),
    release: Some(wfst_release),
    query_interface: Some(wfst_query_interface),
};

unsafe extern "C" fn wfst_retain(context: *mut c_void) {
    if !context.is_null() {
        let shared = context.cast::<WfstContext>();
        {
            let context_ref = &*shared;
            context_ref.metrics.retains.fetch_add(1, Ordering::SeqCst);
        }
        Arc::increment_strong_count(shared);
    }
}

unsafe extern "C" fn wfst_release(context: *mut c_void) {
    if !context.is_null() {
        let shared = context.cast::<WfstContext>();
        // Record BEFORE decrementing: the caller's retain keeps the context
        // alive until the decrement itself runs, so the reference below is
        // valid for exactly this window.
        {
            let context_ref = &*shared;
            context_ref.metrics.releases.fetch_add(1, Ordering::SeqCst);
        }
        Arc::decrement_strong_count(shared);
    }
}

unsafe extern "C" fn wfst_query_interface(
    context: *mut c_void,
    interface_id: *const VtInterfaceId,
    minimum_version: u32,
    out_vtable: *mut *const c_void,
) -> u32 {
    wfst_query_interface_status(context, interface_id, minimum_version, out_vtable).to_raw()
}

unsafe fn wfst_query_interface_status(
    context: *mut c_void,
    interface_id: *const VtInterfaceId,
    minimum_version: u32,
    out_vtable: *mut *const c_void,
) -> VtStatus {
    if context.is_null() || interface_id.is_null() || out_vtable.is_null() {
        return VtStatus::NullPointer;
    }
    if (*interface_id).bytes != VT_WFST_INTERFACE_ID.bytes
        || minimum_version > VT_WFST_INTERFACE_VERSION
    {
        return VtStatus::Unsupported;
    }
    let shared = &*context.cast::<WfstContext>();
    out_vtable.write(std::ptr::from_ref(&shared.vtable).cast());
    VtStatus::Ok
}

unsafe extern "C" fn wfst_snapshot(context: *mut c_void, out_snapshot: *mut VtResource) -> u32 {
    wfst_snapshot_status(context, out_snapshot).to_raw()
}

unsafe fn wfst_snapshot_status(context: *mut c_void, out_snapshot: *mut VtResource) -> VtStatus {
    if context.is_null() || out_snapshot.is_null() {
        return VtStatus::NullPointer;
    }
    let shared = &*context.cast::<WfstContext>();
    shared.metrics.snapshots.fetch_add(1, Ordering::SeqCst);
    // The model is immutable, so the snapshot retains the same context.
    wfst_retain(context);
    out_snapshot.write(VtResource {
        context,
        vtable: &WFST_RESOURCE_VTABLE,
    });
    VtStatus::Ok
}

unsafe extern "C" fn wfst_start(context: *mut c_void, out_state: *mut u64) -> u32 {
    wfst_start_status(context, out_state).to_raw()
}

unsafe fn wfst_start_status(context: *mut c_void, out_state: *mut u64) -> VtStatus {
    if context.is_null() || out_state.is_null() {
        return VtStatus::NullPointer;
    }
    out_state.write((*context.cast::<WfstContext>()).start);
    VtStatus::Ok
}

unsafe extern "C" fn wfst_num_states(
    context: *mut c_void,
    out_count: *mut usize,
    out_known: *mut u8,
) -> u32 {
    wfst_num_states_status(context, out_count, out_known).to_raw()
}

unsafe fn wfst_num_states_status(
    context: *mut c_void,
    out_count: *mut usize,
    out_known: *mut u8,
) -> VtStatus {
    if context.is_null() || out_count.is_null() || out_known.is_null() {
        return VtStatus::NullPointer;
    }
    out_count.write((*context.cast::<WfstContext>()).states.len());
    out_known.write(1);
    VtStatus::Ok
}

unsafe extern "C" fn wfst_state_info(
    context: *mut c_void,
    state: u64,
    out_valid: *mut u8,
    out_is_final: *mut u8,
    out_final_weight: *mut f64,
) -> u32 {
    wfst_state_info_status(context, state, out_valid, out_is_final, out_final_weight)
        .map_or_else(|raw| raw, VtStatus::to_raw)
}

/// `Err(raw)` carries an injected raw wire value that may lie OUTSIDE the
/// published `VtStatus` range (the consumer must treat it as a value, never
/// as undefined behavior).
unsafe fn wfst_state_info_status(
    context: *mut c_void,
    state: u64,
    out_valid: *mut u8,
    out_is_final: *mut u8,
    out_final_weight: *mut f64,
) -> Result<VtStatus, u32> {
    if context.is_null()
        || out_valid.is_null()
        || out_is_final.is_null()
        || out_final_weight.is_null()
    {
        return Ok(VtStatus::NullPointer);
    }
    let shared = &*context.cast::<WfstContext>();
    shared
        .metrics
        .state_info_calls
        .fetch_add(1, Ordering::SeqCst);
    if let Misbehavior::StateInfoStatus(raw) = shared.misbehavior {
        return Err(raw);
    }
    match usize::try_from(state)
        .ok()
        .and_then(|index| shared.states.get(index))
    {
        Some(data) => {
            out_valid.write(1);
            out_is_final.write(u8::from(data.is_final));
            out_final_weight.write(data.final_weight);
        }
        None => {
            out_valid.write(0);
            out_is_final.write(0);
            out_final_weight.write(f64::INFINITY);
        }
    }
    Ok(VtStatus::Ok)
}

unsafe extern "C" fn wfst_state_arcs(
    context: *mut c_void,
    state: u64,
    start: usize,
    out_arcs: *mut VtWfstArc,
    capacity: usize,
    out_written: *mut usize,
    out_total: *mut usize,
) -> u32 {
    wfst_state_arcs_status(
        context,
        state,
        start,
        out_arcs,
        capacity,
        out_written,
        out_total,
    )
    .map_or_else(|raw| raw, VtStatus::to_raw)
}

/// `Err(raw)` carries an injected raw wire value that may lie OUTSIDE the
/// published `VtStatus` range (the consumer must treat it as a value, never
/// as undefined behavior).
unsafe fn wfst_state_arcs_status(
    context: *mut c_void,
    state: u64,
    start: usize,
    out_arcs: *mut VtWfstArc,
    capacity: usize,
    out_written: *mut usize,
    out_total: *mut usize,
) -> Result<VtStatus, u32> {
    if context.is_null()
        || out_written.is_null()
        || out_total.is_null()
        || (capacity != 0 && out_arcs.is_null())
    {
        return Ok(VtStatus::NullPointer);
    }
    let shared = &*context.cast::<WfstContext>();
    shared
        .metrics
        .state_arcs_calls
        .fetch_add(1, Ordering::SeqCst);
    if let Misbehavior::StateArcsStatus(raw) = shared.misbehavior {
        return Err(raw);
    }
    let Some(data) = usize::try_from(state)
        .ok()
        .and_then(|index| shared.states.get(index))
    else {
        return Ok(VtStatus::InvalidArgument);
    };

    let total = data.arcs.len();
    let written = total.saturating_sub(start).min(capacity);
    for (offset, arc) in data.arcs.iter().skip(start).take(written).enumerate() {
        out_arcs.add(offset).write(VtWfstArc {
            input_label: arc.input_label,
            output_label: arc.output_label,
            target_state: arc.to,
            weight: arc.weight,
            has_input: arc.has_input,
            has_output: arc.has_output,
            reserved: [0; 6],
        });
    }

    let call_index = shared.arcs_call_sequence.fetch_add(1, Ordering::SeqCst);
    let (reported_written, reported_total) = match shared.misbehavior {
        // The lie lives in the COUNT; memory past `capacity` is never touched.
        Misbehavior::OvershootWritten => (capacity + 1, total),
        // Every call reports one more arc than can ever be delivered, so an
        // honest paging loop can never terminate on `offset == total`.
        Misbehavior::UnstableOutTotal => (written, total + call_index + 1),
        _ => (written, total),
    };
    out_written.write(reported_written);
    out_total.write(reported_total);
    Ok(VtStatus::Ok)
}

// ─────────────────────────────────────────────────────────────────────────────
// Minimal vt.dictionary.v1 provider (wrong-interface rejection fixture)
// ─────────────────────────────────────────────────────────────────────────────

struct DictionaryContext {
    vtable: VtDictionaryVTable,
}

/// One owned retain of a minimal, empty `vt.dictionary.v1` resource.
///
/// It implements the dictionary interface honestly (one non-final root, no
/// edges) and answers `Unsupported` for every other interface id — exactly
/// what a well-behaved NON-WFST resource looks like to lling-llang.
pub struct TestDictionaryResource {
    raw: VtResource,
}

unsafe impl Send for TestDictionaryResource {}
unsafe impl Sync for TestDictionaryResource {}

impl TestDictionaryResource {
    /// Publish the empty dictionary.
    pub fn new() -> Self {
        let context = Arc::new(DictionaryContext {
            vtable: VtDictionaryVTable {
                struct_size: std::mem::size_of::<VtDictionaryVTable>(),
                interface_version: VT_DICTIONARY_INTERFACE_VERSION,
                unit_domain: VtUnitDomain::UnicodeScalar,
                value_domain: VtValueDomain::Unit,
                flags: dictionary_flags::PARALLEL_REENTRANT | dictionary_flags::IMMUTABLE,
                snapshot: Some(dictionary_snapshot),
                root: Some(dictionary_root),
                len: Some(dictionary_len),
                node_is_final: Some(dictionary_node_is_final),
                node_value_u64: Some(dictionary_node_value_u64),
                node_transition: Some(dictionary_node_transition),
                node_edges: Some(dictionary_node_edges),
            },
        });
        Self {
            raw: VtResource {
                context: Arc::into_raw(context).cast_mut().cast(),
                vtable: &DICTIONARY_RESOURCE_VTABLE,
            },
        }
    }

    /// Borrow the two-word ABI value.
    pub fn as_raw(&self) -> VtResource {
        self.raw
    }
}

impl Default for TestDictionaryResource {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestDictionaryResource {
    fn drop(&mut self) {
        unsafe { dictionary_release(self.raw.context) }
    }
}

static DICTIONARY_RESOURCE_VTABLE: VtResourceVTable = VtResourceVTable {
    struct_size: std::mem::size_of::<VtResourceVTable>(),
    abi_version: VT_ABI_VERSION,
    reserved: 0,
    retain: Some(dictionary_retain),
    release: Some(dictionary_release),
    query_interface: Some(dictionary_query_interface),
};

unsafe extern "C" fn dictionary_retain(context: *mut c_void) {
    if !context.is_null() {
        Arc::increment_strong_count(context.cast::<DictionaryContext>());
    }
}

unsafe extern "C" fn dictionary_release(context: *mut c_void) {
    if !context.is_null() {
        Arc::decrement_strong_count(context.cast::<DictionaryContext>());
    }
}

unsafe extern "C" fn dictionary_query_interface(
    context: *mut c_void,
    interface_id: *const VtInterfaceId,
    minimum_version: u32,
    out_vtable: *mut *const c_void,
) -> u32 {
    dictionary_query_interface_status(context, interface_id, minimum_version, out_vtable).to_raw()
}

unsafe fn dictionary_query_interface_status(
    context: *mut c_void,
    interface_id: *const VtInterfaceId,
    minimum_version: u32,
    out_vtable: *mut *const c_void,
) -> VtStatus {
    if context.is_null() || interface_id.is_null() || out_vtable.is_null() {
        return VtStatus::NullPointer;
    }
    if (*interface_id).bytes != VT_DICTIONARY_INTERFACE_ID.bytes
        || minimum_version > VT_DICTIONARY_INTERFACE_VERSION
    {
        return VtStatus::Unsupported;
    }
    let shared = &*context.cast::<DictionaryContext>();
    out_vtable.write(std::ptr::from_ref(&shared.vtable).cast());
    VtStatus::Ok
}

unsafe extern "C" fn dictionary_snapshot(
    context: *mut c_void,
    out_snapshot: *mut VtResource,
) -> u32 {
    if context.is_null() || out_snapshot.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    dictionary_retain(context);
    out_snapshot.write(VtResource {
        context,
        vtable: &DICTIONARY_RESOURCE_VTABLE,
    });
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn dictionary_root(context: *mut c_void, out_node: *mut u64) -> u32 {
    if context.is_null() || out_node.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    out_node.write(0);
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn dictionary_len(
    context: *mut c_void,
    out_len: *mut usize,
    out_known: *mut u8,
) -> u32 {
    if context.is_null() || out_len.is_null() || out_known.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    out_len.write(0);
    out_known.write(1);
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn dictionary_node_is_final(
    context: *mut c_void,
    _node: u64,
    out_is_final: *mut u8,
) -> u32 {
    if context.is_null() || out_is_final.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    out_is_final.write(0);
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn dictionary_node_value_u64(
    context: *mut c_void,
    _node: u64,
    out_value: *mut VtOptionalU64,
) -> u32 {
    if context.is_null() || out_value.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    out_value.write(VtOptionalU64::default());
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn dictionary_node_transition(
    context: *mut c_void,
    _node: u64,
    _label: u64,
    out_child: *mut u64,
    out_found: *mut u8,
) -> u32 {
    if context.is_null() || out_child.is_null() || out_found.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    out_child.write(0);
    out_found.write(0);
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn dictionary_node_edges(
    context: *mut c_void,
    _node: u64,
    _start: usize,
    _out_edges: *mut VtDictionaryEdge,
    _capacity: usize,
    out_written: *mut usize,
    out_total: *mut usize,
) -> u32 {
    if context.is_null() || out_written.is_null() || out_total.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    out_written.write(0);
    out_total.write(0);
    VtStatus::Ok.to_raw()
}

// ─────────────────────────────────────────────────────────────────────────────
// Consumer-side walking and canonicalization helpers (test harness side —
// these MAY panic, they never run inside an extern "C" frame)
// ─────────────────────────────────────────────────────────────────────────────

use lling_llang::semiring::TropicalWeight;
use lling_llang::wfst::{VectorWfst, Wfst};
use std::collections::{BTreeMap, VecDeque};

/// One fully paged state observed through a `vt.scalar-wfst.1` interface.
#[derive(Clone, Debug)]
pub struct WalkedState {
    /// Whether the state accepts.
    pub is_final: bool,
    /// Scalar final weight.
    pub final_weight: f64,
    /// Outgoing arcs in provider emission order.
    pub arcs: Vec<VtWfstArc>,
}

/// Discover the scalar-WFST interface vtable of a live resource.
///
/// # Safety
/// `resource` must be a live resource whose vtable outlives the returned
/// pointer's use.
pub unsafe fn discover_scalar_wfst(resource: VtResource) -> *const VtWfstVTable {
    assert!(!resource.is_null(), "resource words must be non-null");
    let mut interface = std::ptr::null();
    let raw = (*resource.vtable)
        .query_interface
        .expect("resource must publish query_interface")(
        resource.context,
        &VT_WFST_INTERFACE_ID,
        VT_WFST_INTERFACE_VERSION,
        &mut interface,
    );
    assert_eq!(
        raw,
        VtStatus::Ok.to_raw(),
        "query_interface must accept vt.scalar-wfst.1 v{VT_WFST_INTERFACE_VERSION}"
    );
    assert!(!interface.is_null(), "query_interface must write a vtable");
    interface.cast::<VtWfstVTable>()
}

/// BFS the reachable graph of a live scalar-WFST resource, paging every
/// state's arcs `page_capacity` at a time and asserting the paging law along
/// the way (`out_total` stable across pages, `out_written <= capacity`,
/// pages concatenate losslessly) — the consumer-observable face of VT-PAGE.
///
/// Returns the start state and every reachable state keyed by provider id.
///
/// # Safety
/// `resource` must be a live `vt.scalar-wfst.1` resource.
pub unsafe fn walk_reachable(
    resource: VtResource,
    page_capacity: usize,
) -> (u64, BTreeMap<u64, WalkedState>) {
    assert!(page_capacity > 0, "walker needs a positive page capacity");
    let table = &*discover_scalar_wfst(resource);
    let mut start = 0;
    assert_eq!(
        table.start.expect("start must be published")(resource.context, &mut start),
        VtStatus::Ok.to_raw()
    );

    let mut states = BTreeMap::new();
    let mut queue = VecDeque::from([start]);
    let mut page = vec![VtWfstArc::default(); page_capacity];
    while let Some(state) = queue.pop_front() {
        if states.contains_key(&state) {
            continue;
        }
        let mut valid = 0;
        let mut is_final = 0;
        let mut final_weight = f64::NAN;
        assert_eq!(
            table.state_info.expect("state_info must be published")(
                resource.context,
                state,
                &mut valid,
                &mut is_final,
                &mut final_weight,
            ),
            VtStatus::Ok.to_raw(),
            "state_info({state}) must succeed"
        );
        assert_eq!(valid, 1, "reachable state {state} must be valid");

        let mut arcs = Vec::new();
        let mut offset = 0usize;
        let mut expected_total = None;
        loop {
            let mut written = usize::MAX;
            let mut total = usize::MAX;
            assert_eq!(
                table.state_arcs.expect("state_arcs must be published")(
                    resource.context,
                    state,
                    offset,
                    page.as_mut_ptr(),
                    page.len(),
                    &mut written,
                    &mut total,
                ),
                VtStatus::Ok.to_raw(),
                "state_arcs({state}, {offset}) must succeed"
            );
            assert!(written <= page.len(), "out_written must respect capacity");
            match expected_total {
                None => expected_total = Some(total),
                Some(expected) => {
                    assert_eq!(total, expected, "out_total must be stable across pages");
                }
            }
            arcs.extend_from_slice(&page[..written]);
            offset += written;
            if offset >= total {
                assert_eq!(offset, total, "pages must concatenate losslessly");
                break;
            }
            assert!(written > 0, "provider must make progress before the end");
        }
        for arc in &arcs {
            queue.push_back(arc.target_state);
        }
        states.insert(
            state,
            WalkedState {
                is_final: is_final == 1,
                final_weight,
                arcs,
            },
        );
    }
    (start, states)
}

/// Numbering-invariant canonical arc: labels as optional scalar values, the
/// target as a canonical (BFS discovery order) index.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalArc {
    /// Input label scalar, `None` for epsilon.
    pub input: Option<u32>,
    /// Output label scalar, `None` for epsilon.
    pub output: Option<u32>,
    /// Raw scalar weight.
    pub weight: f64,
    /// Canonical target index.
    pub target: usize,
}

/// Numbering-invariant canonical state.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalState {
    /// Whether the state accepts.
    pub is_final: bool,
    /// Scalar final weight.
    pub final_weight: f64,
    /// Outgoing arcs in emission order with canonical targets.
    pub arcs: Vec<CanonicalArc>,
}

/// Canonical (BFS-discovery-order) form of a WFST: state 0 is the start and
/// every id is the order of first discovery following arcs in emission
/// order. Two structurally identical machines canonicalize identically no
/// matter how their concrete state ids were assigned.
pub type CanonicalWfst = Vec<CanonicalState>;

/// Canonicalize a walked resource graph.
pub fn canonical_of_walk(start: u64, states: &BTreeMap<u64, WalkedState>) -> CanonicalWfst {
    let mut order = BTreeMap::new();
    let mut sequence = Vec::with_capacity(states.len());
    let mut queue = VecDeque::from([start]);
    while let Some(state) = queue.pop_front() {
        if order.contains_key(&state) {
            continue;
        }
        order.insert(state, sequence.len());
        sequence.push(state);
        let walked = states
            .get(&state)
            .expect("walk must contain every reachable state");
        for arc in &walked.arcs {
            if !order.contains_key(&arc.target_state) {
                queue.push_back(arc.target_state);
            }
        }
    }
    // Second pass: now that discovery order is fixed, remap arc targets.
    let mut canonical = Vec::with_capacity(sequence.len());
    for state in &sequence {
        let walked = &states[state];
        let arcs = walked
            .arcs
            .iter()
            .map(|arc| CanonicalArc {
                input: (arc.has_input == 1)
                    .then(|| u32::try_from(arc.input_label).expect("walked input label fits u32")),
                output: (arc.has_output == 1).then(|| {
                    u32::try_from(arc.output_label).expect("walked output label fits u32")
                }),
                weight: arc.weight,
                target: order[&arc.target_state],
            })
            .collect();
        canonical.push(CanonicalState {
            is_final: walked.is_final,
            final_weight: walked.final_weight,
            arcs,
        });
    }
    canonical
}

/// Canonicalize the reachable part of a native Unicode/tropical `VectorWfst`
/// with the same discovery rule as [`canonical_of_walk`].
pub fn canonical_of_vector(wfst: &VectorWfst<char, TropicalWeight>) -> CanonicalWfst {
    let mut order = BTreeMap::new();
    let mut sequence = Vec::with_capacity(wfst.num_states());
    let mut queue = VecDeque::from([wfst.start()]);
    while let Some(state) = queue.pop_front() {
        if order.contains_key(&state) {
            continue;
        }
        order.insert(state, sequence.len());
        sequence.push(state);
        for transition in wfst.transitions(state) {
            if !order.contains_key(&transition.to) {
                queue.push_back(transition.to);
            }
        }
    }
    let mut canonical = Vec::with_capacity(sequence.len());
    for state in &sequence {
        let arcs = wfst
            .transitions(*state)
            .iter()
            .map(|transition| CanonicalArc {
                input: transition.input.map(u32::from),
                output: transition.output.map(u32::from),
                weight: transition.weight.value(),
                target: order[&transition.to],
            })
            .collect();
        canonical.push(CanonicalState {
            is_final: wfst.is_final(*state),
            final_weight: wfst.final_weight(*state).value(),
            arcs,
        });
    }
    canonical
}
