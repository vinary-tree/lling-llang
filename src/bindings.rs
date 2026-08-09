//! Stable scalar-WFST resource producers and consumers for language bindings.
//!
//! This module intentionally exposes only lling-llang's generic WFST model.
//! Dictionary construction remains in libdictenstein and Levenshtein adapters
//! remain in duallity. Imported resources are captured before traversal, and
//! lazy composition uses independently computed/cacheable product states rather
//! than a resource-wide sequential call gate.

use crate::composition::{EpsilonFilter, FilterState};
use crate::semiring::TropicalWeight;
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst, NO_STATE};
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::fmt;
use std::sync::{Arc, Mutex, RwLock};
use vinary_tree_interop::{
    wfst_flags, VtInterfaceId, VtResource, VtResourceVTable, VtStatus, VtUnitDomain,
    VtWeightDomain, VtWfstArc, VtWfstVTable, VT_ABI_VERSION, VT_RECOMMENDED_ARC_BATCH,
    VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
};

/// Binding failures raised while validating or traversing a foreign WFST.
#[derive(Clone, Debug, PartialEq)]
pub enum BindingError {
    /// One or both resource words were null.
    NullResource,
    /// The base resource contract is incomplete or incompatible.
    IncompatibleResourceAbi,
    /// The resource does not implement `vt.scalar-wfst.1`.
    MissingWfstInterface,
    /// The WFST interface is incomplete or incompatible.
    IncompatibleWfstInterface,
    /// Only Unicode scalar labels are accepted by this binding specialization.
    UnitDomainMismatch(VtUnitDomain),
    /// Only tropical f64 weights are accepted by this binding specialization.
    WeightDomainMismatch(VtWeightDomain),
    /// A provider callback failed.
    Provider(VtStatus),
    /// A provider returned inconsistent or malformed output.
    InvalidProviderOutput(&'static str),
    /// A state, label, or state count cannot fit lling-llang's native model.
    RepresentationLimit,
}

impl fmt::Display for BindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullResource => formatter.write_str("resource context or vtable is null"),
            Self::IncompatibleResourceAbi => formatter.write_str("incompatible resource ABI"),
            Self::MissingWfstInterface => {
                formatter.write_str("resource has no scalar WFST interface")
            }
            Self::IncompatibleWfstInterface => {
                formatter.write_str("incompatible scalar WFST interface")
            }
            Self::UnitDomainMismatch(domain) => {
                write!(formatter, "expected Unicode scalar labels, got {domain:?}")
            }
            Self::WeightDomainMismatch(domain) => {
                write!(formatter, "expected tropical f64 weights, got {domain:?}")
            }
            Self::Provider(status) => write!(formatter, "WFST provider returned {status:?}"),
            Self::InvalidProviderOutput(message) => write!(
                formatter,
                "WFST provider returned invalid output: {message}"
            ),
            Self::RepresentationLimit => {
                formatter.write_str("WFST exceeds the native state or label representation")
            }
        }
    }
}

impl std::error::Error for BindingError {}

fn check_status(raw: u32) -> Result<(), BindingError> {
    // The wire carries a raw u32 (interop status rule): decode before any
    // enum-typed use; an out-of-range discriminant is provider misbehavior,
    // never undefined behavior (family hardening LLEV-B6).
    let Some(status) = VtStatus::from_raw(raw) else {
        return Err(BindingError::InvalidProviderOutput(
            "provider returned an out-of-range status code",
        ));
    };
    if status.is_ok() {
        Ok(())
    } else {
        Err(BindingError::Provider(status))
    }
}

#[derive(Clone)]
struct StateData {
    valid: bool,
    is_final: bool,
    final_weight: f64,
    arcs: Arc<[VtWfstArc]>,
}

/// One fully expanded state returned by a project-owned lazy provider.
#[derive(Clone, Debug)]
pub struct ScalarWfstState {
    /// Whether the requested state identifier is valid.
    pub valid: bool,
    /// Whether this is an accepting state.
    pub is_final: bool,
    /// Scalar final weight in the provider's declared semiring.
    pub final_weight: f64,
    /// Contiguous outgoing arcs. This is one state, never a global graph.
    pub arcs: Vec<VtWfstArc>,
}

/// Project-owned lazy Unicode/tropical WFST exposed through the common ABI.
///
/// Implementations must be safe for concurrent calls. Expensive state
/// expansion should be local or sharded; a resource-wide evaluation mutex
/// would violate the parallel/reentrant contract published by the wrapper.
pub trait ScalarWfstProvider: Send + Sync + 'static {
    /// Scalar semiring represented by the returned weights.
    fn weight_domain(&self) -> VtWeightDomain {
        VtWeightDomain::TropicalF64
    }
    /// Return the initial state identifier.
    fn start(&self) -> Result<u64, VtStatus>;
    /// Return the state count when it is already known without materialization.
    fn num_states(&self) -> Result<Option<usize>, VtStatus>;
    /// Expand one state and return its complete, bounded outgoing arc set.
    fn state(&self, state: u64) -> Result<ScalarWfstState, VtStatus>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct AbiProductState {
    left: u64,
    right: u64,
    filter: FilterState,
}

struct ProductRegistry {
    ids: HashMap<AbiProductState, u64>,
    states: Vec<AbiProductState>,
}

impl ProductRegistry {
    fn new(start: AbiProductState) -> Self {
        let mut ids = HashMap::new();
        ids.insert(start, 0);
        Self {
            ids,
            states: vec![start],
        }
    }

    fn get(&self, id: u64) -> Option<AbiProductState> {
        usize::try_from(id)
            .ok()
            .and_then(|id| self.states.get(id).copied())
    }

    fn register(&mut self, state: AbiProductState) -> Result<u64, BindingError> {
        if let Some(id) = self.ids.get(&state) {
            return Ok(*id);
        }
        let id = u64::try_from(self.states.len()).map_err(|_| BindingError::RepresentationLimit)?;
        self.states.push(state);
        self.ids.insert(state, id);
        Ok(id)
    }
}

enum ProviderCallGate {
    Parallel,
    Serial(Mutex<()>),
}

impl ProviderCallGate {
    fn for_flags(flags: u64) -> Self {
        if flags & wfst_flags::PARALLEL_REENTRANT != 0 {
            Self::Parallel
        } else {
            Self::Serial(Mutex::new(()))
        }
    }

    fn call<T>(&self, callback: impl FnOnce() -> T) -> T {
        match self {
            Self::Parallel => callback(),
            Self::Serial(lock) => {
                let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                callback()
            }
        }
    }
}

struct CapturedWfst {
    resource: RawOwnedResource,
    table: *const VtWfstVTable,
    start: u64,
    gate: ProviderCallGate,
    states: RwLock<HashMap<u64, Arc<StateData>>>,
}

// The ABI resource is immutable after snapshot capture. Providers advertising
// PARALLEL_REENTRANT promise concurrent callback safety; all other providers
// are protected by this adapter's callback gate.
unsafe impl Send for CapturedWfst {}
unsafe impl Sync for CapturedWfst {}

impl CapturedWfst {
    unsafe fn capture(resource: VtResource) -> Result<Self, BindingError> {
        let live_table = discover_wfst(resource)?;
        let live_gate = ProviderCallGate::for_flags((*live_table).flags);
        let mut snapshot = VtResource::NULL;
        check_status(
            live_gate.call(|| (*live_table).snapshot.unwrap()(resource.context, &mut snapshot)),
        )?;
        if snapshot.is_null() {
            return Err(BindingError::InvalidProviderOutput(
                "snapshot returned null",
            ));
        }
        let snapshot = RawOwnedResource(snapshot);
        let table = discover_wfst(snapshot.0)?;
        let gate = ProviderCallGate::for_flags((*table).flags);
        let mut start = 0;
        check_status(gate.call(|| (*table).start.unwrap()(snapshot.0.context, &mut start)))?;
        Ok(Self {
            resource: snapshot,
            table,
            start,
            gate,
            states: RwLock::new(HashMap::new()),
        })
    }

    fn state(&self, state: u64) -> Result<Arc<StateData>, BindingError> {
        if let Some(cached) = self
            .states
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&state)
            .cloned()
        {
            return Ok(cached);
        }
        let expanded = unsafe { self.gate.call(|| self.expand_state(state)) }?;
        let expanded = Arc::new(expanded);
        let mut cache = self
            .states
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(cache
            .entry(state)
            .or_insert_with(|| Arc::clone(&expanded))
            .clone())
    }

    unsafe fn expand_state(&self, state: u64) -> Result<StateData, BindingError> {
        let table = &*self.table;
        let mut valid = 0;
        let mut is_final = 0;
        let mut final_weight = f64::INFINITY;
        check_status(table.state_info.unwrap()(
            self.resource.0.context,
            state,
            &mut valid,
            &mut is_final,
            &mut final_weight,
        ))?;
        // TropicalWeight admits finite costs or +inf (the zero) only: -inf and
        // NaN are invalid and would poison composition with +inf + -inf = NaN
        // (finding LLING-B2/F1). The composition/import paths are tropical-only.
        if valid > 1 || is_final > 1 || !TropicalWeight::is_valid_raw(final_weight) {
            return Err(BindingError::InvalidProviderOutput(
                "invalid state_info fields",
            ));
        }
        if valid == 0 {
            return Ok(StateData {
                valid: false,
                is_final: false,
                final_weight: f64::INFINITY,
                arcs: Arc::from([]),
            });
        }

        let mut arcs = Vec::new();
        let mut page = vec![VtWfstArc::default(); VT_RECOMMENDED_ARC_BATCH];
        let mut offset = 0usize;
        loop {
            let mut written = 0usize;
            let mut total = 0usize;
            check_status(table.state_arcs.unwrap()(
                self.resource.0.context,
                state,
                offset,
                page.as_mut_ptr(),
                page.len(),
                &mut written,
                &mut total,
            ))?;
            if written > page.len()
                || offset > total
                || offset.saturating_add(written) > total
                || (written == 0 && offset < total)
            {
                return Err(BindingError::InvalidProviderOutput(
                    "invalid arc page counts",
                ));
            }
            for arc in page.iter().take(written) {
                if arc.has_input > 1
                    || arc.has_output > 1
                    || arc.reserved != [0; 6]
                    || !TropicalWeight::is_valid_raw(arc.weight)
                    || (arc.has_input == 1
                        && u32::try_from(arc.input_label)
                            .ok()
                            .and_then(char::from_u32)
                            .is_none())
                    || (arc.has_output == 1
                        && u32::try_from(arc.output_label)
                            .ok()
                            .and_then(char::from_u32)
                            .is_none())
                {
                    return Err(BindingError::InvalidProviderOutput("invalid arc fields"));
                }
                arcs.push(*arc);
            }
            offset = offset
                .checked_add(written)
                .ok_or(BindingError::RepresentationLimit)?;
            if offset == total {
                break;
            }
        }
        Ok(StateData {
            valid: true,
            is_final: is_final == 1,
            final_weight,
            arcs: arcs.into(),
        })
    }
}

struct CompositionResource {
    left: Arc<CapturedWfst>,
    right: Arc<CapturedWfst>,
    registry: RwLock<ProductRegistry>,
    cache: RwLock<HashMap<u64, Arc<StateData>>>,
    filter: EpsilonFilter,
}

impl CompositionResource {
    unsafe fn capture(first: VtResource, second: VtResource) -> Result<Self, BindingError> {
        let left = Arc::new(CapturedWfst::capture(first)?);
        let right = Arc::new(CapturedWfst::capture(second)?);
        let start = AbiProductState {
            left: left.start,
            right: right.start,
            filter: FilterState::None,
        };
        Ok(Self {
            left,
            right,
            registry: RwLock::new(ProductRegistry::new(start)),
            cache: RwLock::new(HashMap::new()),
            filter: EpsilonFilter::default(),
        })
    }

    fn state(&self, id: u64) -> Result<Arc<StateData>, BindingError> {
        if let Some(cached) = self
            .cache
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&id)
            .cloned()
        {
            return Ok(cached);
        }
        let product = self
            .registry
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .ok_or(BindingError::InvalidProviderOutput(
                "unknown composed state",
            ))?;
        let left = self.left.state(product.left)?;
        let right = self.right.state(product.right)?;
        if !left.valid || !right.valid {
            return Ok(Arc::new(StateData {
                valid: false,
                is_final: false,
                final_weight: f64::INFINITY,
                arcs: Arc::from([]),
            }));
        }
        let is_final = left.is_final && right.is_final;
        let final_weight = if is_final {
            left.final_weight + right.final_weight
        } else {
            f64::INFINITY
        };
        let (can_left_epsilon, can_right_epsilon, can_match) =
            self.filter.allowed_moves(product.filter);
        let mut registry = self.registry.write().unwrap_or_else(|p| p.into_inner());
        let mut arcs = Vec::new();

        if can_left_epsilon {
            for arc in left.arcs.iter().filter(|arc| arc.has_output == 0) {
                let target_state = registry.register(AbiProductState {
                    left: arc.target_state,
                    right: product.right,
                    filter: self.filter.next_state(product.filter, true, false),
                })?;
                arcs.push(VtWfstArc {
                    input_label: arc.input_label,
                    output_label: 0,
                    target_state,
                    weight: arc.weight,
                    has_input: arc.has_input,
                    has_output: 0,
                    reserved: [0; 6],
                });
            }
        }

        if can_right_epsilon {
            for arc in right.arcs.iter().filter(|arc| arc.has_input == 0) {
                let target_state = registry.register(AbiProductState {
                    left: product.left,
                    right: arc.target_state,
                    filter: self.filter.next_state(product.filter, false, true),
                })?;
                arcs.push(VtWfstArc {
                    input_label: 0,
                    output_label: arc.output_label,
                    target_state,
                    weight: arc.weight,
                    has_input: 0,
                    has_output: arc.has_output,
                    reserved: [0; 6],
                });
            }
        }

        if can_match {
            for left_arc in left.arcs.iter().filter(|arc| arc.has_output == 1) {
                for right_arc in right
                    .arcs
                    .iter()
                    .filter(|arc| arc.has_input == 1 && arc.input_label == left_arc.output_label)
                {
                    let target_state = registry.register(AbiProductState {
                        left: left_arc.target_state,
                        right: right_arc.target_state,
                        filter: self.filter.next_state(product.filter, false, false),
                    })?;
                    arcs.push(VtWfstArc {
                        input_label: left_arc.input_label,
                        output_label: right_arc.output_label,
                        target_state,
                        weight: left_arc.weight + right_arc.weight,
                        has_input: left_arc.has_input,
                        has_output: right_arc.has_output,
                        reserved: [0; 6],
                    });
                }
            }
        }
        drop(registry);
        let state = Arc::new(StateData {
            valid: true,
            is_final,
            final_weight,
            arcs: arcs.into(),
        });
        let mut cache = self.cache.write().unwrap_or_else(|p| p.into_inner());
        Ok(cache
            .entry(id)
            .or_insert_with(|| Arc::clone(&state))
            .clone())
    }
}

enum ResourcePayload {
    Eager(Arc<VectorWfst<char, TropicalWeight>>),
    Composition(Arc<CompositionResource>),
    Provider(Arc<ProviderResource>),
}

struct ProviderResource {
    provider: Arc<dyn ScalarWfstProvider>,
    states: RwLock<HashMap<u64, Arc<StateData>>>,
}

impl ProviderResource {
    fn state(&self, id: u64) -> Result<Arc<StateData>, BindingError> {
        if let Some(cached) = self
            .states
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&id)
            .cloned()
        {
            return Ok(cached);
        }
        let state = self.provider.state(id).map_err(BindingError::Provider)?;
        let state = Arc::new(StateData {
            valid: state.valid,
            is_final: state.is_final,
            final_weight: state.final_weight,
            arcs: state.arcs.into(),
        });
        let mut cache = self
            .states
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(cache
            .entry(id)
            .or_insert_with(|| Arc::clone(&state))
            .clone())
    }
}

struct ResourceContext {
    payload: ResourcePayload,
}

impl ResourceContext {
    fn weight_domain(&self) -> VtWeightDomain {
        match &self.payload {
            ResourcePayload::Eager(_) | ResourcePayload::Composition(_) => {
                VtWeightDomain::TropicalF64
            }
            ResourcePayload::Provider(provider) => provider.provider.weight_domain(),
        }
    }

    fn state(&self, id: u64) -> Result<Arc<StateData>, BindingError> {
        match &self.payload {
            ResourcePayload::Eager(wfst) => {
                let Ok(state) = StateId::try_from(id) else {
                    return Ok(Arc::new(StateData {
                        valid: false,
                        is_final: false,
                        final_weight: f64::INFINITY,
                        arcs: Arc::from([]),
                    }));
                };
                if !wfst.is_valid_state(state) {
                    return Ok(Arc::new(StateData {
                        valid: false,
                        is_final: false,
                        final_weight: f64::INFINITY,
                        arcs: Arc::from([]),
                    }));
                }
                let arcs = wfst
                    .transitions(state)
                    .iter()
                    .map(|arc| VtWfstArc {
                        input_label: arc.input.map_or(0, |label| u64::from(u32::from(label))),
                        output_label: arc.output.map_or(0, |label| u64::from(u32::from(label))),
                        target_state: u64::from(arc.to),
                        weight: arc.weight.value(),
                        has_input: u8::from(arc.input.is_some()),
                        has_output: u8::from(arc.output.is_some()),
                        reserved: [0; 6],
                    })
                    .collect::<Vec<_>>();
                Ok(Arc::new(StateData {
                    valid: true,
                    is_final: wfst.is_final(state),
                    final_weight: wfst.final_weight(state).value(),
                    arcs: arcs.into(),
                }))
            }
            ResourcePayload::Composition(composition) => composition.state(id),
            ResourcePayload::Provider(provider) => provider.state(id),
        }
    }
}

/// One owned retain of an immutable `vt.scalar-wfst.1` resource.
pub struct OwnedWfstResource {
    raw: VtResource,
}

// Every context constructible through this safe type is backed by an Arc of a
// Send + Sync payload. Raw vtable pointers are immutable for that Arc's life.
unsafe impl Send for OwnedWfstResource {}
unsafe impl Sync for OwnedWfstResource {}

impl OwnedWfstResource {
    fn new(payload: ResourcePayload) -> Self {
        let context = Arc::new(ResourceContext { payload });
        Self {
            raw: VtResource {
                context: Arc::into_raw(context).cast_mut().cast(),
                vtable: &RESOURCE_VTABLE,
            },
        }
    }

    /// Create a resource from an eager Unicode/tropical WFST without copying it.
    pub fn from_wfst(wfst: VectorWfst<char, TropicalWeight>) -> Self {
        Self::new(ResourcePayload::Eager(Arc::new(wfst)))
    }

    /// Wrap a related project's parallel/reentrant lazy WFST in O(1).
    pub fn from_provider(provider: Arc<dyn ScalarWfstProvider>) -> Self {
        Self::new(ResourcePayload::Provider(Arc::new(ProviderResource {
            provider,
            states: RwLock::new(HashMap::new()),
        })))
    }

    /// Borrow the stable two-word ABI value.
    pub fn as_raw(&self) -> VtResource {
        self.raw
    }

    /// Transfer this owned retain to a raw ABI value.
    pub fn into_raw(self) -> VtResource {
        let raw = self.raw;
        std::mem::forget(self);
        raw
    }

    /// Lazily compose two captured scalar-WFST resources.
    ///
    /// Each input snapshot is retained in O(1). Component and product states
    /// are expanded on demand and cached; neither input graph nor the full
    /// product is materialized at construction.
    pub fn compose(first: VtResource, second: VtResource) -> Result<Self, BindingError> {
        let composition = unsafe { CompositionResource::capture(first, second)? };
        Ok(Self::new(ResourcePayload::Composition(Arc::new(
            composition,
        ))))
    }
}

impl Clone for OwnedWfstResource {
    fn clone(&self) -> Self {
        unsafe { resource_retain(self.raw.context) };
        Self { raw: self.raw }
    }
}

impl Drop for OwnedWfstResource {
    fn drop(&mut self) {
        unsafe { resource_release(self.raw.context) }
    }
}

unsafe extern "C" fn resource_retain(context: *mut c_void) {
    if !context.is_null() {
        Arc::increment_strong_count(context.cast::<ResourceContext>());
    }
}

unsafe extern "C" fn resource_release(context: *mut c_void) {
    if !context.is_null() {
        Arc::decrement_strong_count(context.cast::<ResourceContext>());
    }
}

unsafe extern "C" fn query_interface(
    context: *mut c_void,
    interface_id: *const VtInterfaceId,
    minimum_version: u32,
    out_vtable: *mut *const c_void,
) -> u32 {
    query_interface_status(context, interface_id, minimum_version, out_vtable).to_raw()
}

unsafe fn query_interface_status(
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
    let context = &*context.cast::<ResourceContext>();
    out_vtable.write(wfst_vtable(context.weight_domain()).cast());
    VtStatus::Ok
}

unsafe extern "C" fn wfst_snapshot(context: *mut c_void, out_snapshot: *mut VtResource) -> u32 {
    wfst_snapshot_status(context, out_snapshot).to_raw()
}

unsafe fn wfst_snapshot_status(context: *mut c_void, out_snapshot: *mut VtResource) -> VtStatus {
    if context.is_null() || out_snapshot.is_null() {
        return VtStatus::NullPointer;
    }
    resource_retain(context);
    out_snapshot.write(VtResource {
        context,
        vtable: &RESOURCE_VTABLE,
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
    let context = &*context.cast::<ResourceContext>();
    match &context.payload {
        ResourcePayload::Eager(wfst) if wfst.start() != NO_STATE => {
            out_state.write(u64::from(wfst.start()));
            VtStatus::Ok
        }
        ResourcePayload::Eager(_) => VtStatus::InvalidArgument,
        ResourcePayload::Composition(_) => {
            out_state.write(0);
            VtStatus::Ok
        }
        ResourcePayload::Provider(provider) => match provider.provider.start() {
            Ok(state) => {
                out_state.write(state);
                VtStatus::Ok
            }
            Err(status) => status,
        },
    }
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
    let context = &*context.cast::<ResourceContext>();
    match &context.payload {
        ResourcePayload::Eager(wfst) => {
            out_count.write(wfst.num_states());
            out_known.write(1);
        }
        ResourcePayload::Composition(composition) => {
            out_count.write(
                composition
                    .registry
                    .read()
                    .unwrap_or_else(|p| p.into_inner())
                    .states
                    .len(),
            );
            out_known.write(0);
        }
        ResourcePayload::Provider(provider) => match provider.provider.num_states() {
            Ok(count) => {
                out_count.write(count.unwrap_or_default());
                out_known.write(u8::from(count.is_some()));
            }
            Err(status) => return status,
        },
    }
    VtStatus::Ok
}

unsafe extern "C" fn wfst_state_info(
    context: *mut c_void,
    state: u64,
    out_valid: *mut u8,
    out_is_final: *mut u8,
    out_final_weight: *mut f64,
) -> u32 {
    wfst_state_info_status(context, state, out_valid, out_is_final, out_final_weight).to_raw()
}

unsafe fn wfst_state_info_status(
    context: *mut c_void,
    state: u64,
    out_valid: *mut u8,
    out_is_final: *mut u8,
    out_final_weight: *mut f64,
) -> VtStatus {
    if context.is_null()
        || out_valid.is_null()
        || out_is_final.is_null()
        || out_final_weight.is_null()
    {
        return VtStatus::NullPointer;
    }
    match (&*context.cast::<ResourceContext>()).state(state) {
        Ok(data) => {
            out_valid.write(u8::from(data.valid));
            out_is_final.write(u8::from(data.is_final));
            out_final_weight.write(data.final_weight);
            VtStatus::Ok
        }
        Err(_) => VtStatus::ProviderError,
    }
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
    .to_raw()
}

unsafe fn wfst_state_arcs_status(
    context: *mut c_void,
    state: u64,
    start: usize,
    out_arcs: *mut VtWfstArc,
    capacity: usize,
    out_written: *mut usize,
    out_total: *mut usize,
) -> VtStatus {
    if context.is_null()
        || out_written.is_null()
        || out_total.is_null()
        || (capacity != 0 && out_arcs.is_null())
    {
        return VtStatus::NullPointer;
    }
    let data = match (&*context.cast::<ResourceContext>()).state(state) {
        Ok(data) => data,
        Err(_) => return VtStatus::ProviderError,
    };
    if !data.valid {
        return VtStatus::InvalidArgument;
    }
    out_total.write(data.arcs.len());
    let page = data.arcs.iter().skip(start).take(capacity);
    let mut written = 0;
    for (index, arc) in page.enumerate() {
        out_arcs.add(index).write(*arc);
        written += 1;
    }
    out_written.write(written);
    VtStatus::Ok
}

static RESOURCE_VTABLE: VtResourceVTable = VtResourceVTable {
    struct_size: std::mem::size_of::<VtResourceVTable>(),
    abi_version: VT_ABI_VERSION,
    reserved: 0,
    retain: Some(resource_retain),
    release: Some(resource_release),
    query_interface: Some(query_interface),
};

macro_rules! wfst_vtable {
    ($name:ident, $weight:expr) => {
        static $name: VtWfstVTable = VtWfstVTable {
            struct_size: std::mem::size_of::<VtWfstVTable>(),
            interface_version: VT_WFST_INTERFACE_VERSION,
            unit_domain: VtUnitDomain::UnicodeScalar,
            weight_domain: $weight,
            reserved: 0,
            flags: wfst_flags::PARALLEL_REENTRANT | wfst_flags::IMMUTABLE | wfst_flags::LAZY,
            snapshot: Some(wfst_snapshot),
            start: Some(wfst_start),
            num_states: Some(wfst_num_states),
            state_info: Some(wfst_state_info),
            state_arcs: Some(wfst_state_arcs),
        };
    };
}

wfst_vtable!(TROPICAL_WFST_VTABLE, VtWeightDomain::TropicalF64);
wfst_vtable!(LOG_WFST_VTABLE, VtWeightDomain::LogF64);
wfst_vtable!(PROBABILITY_WFST_VTABLE, VtWeightDomain::ProbabilityF64);
wfst_vtable!(ARCTIC_WFST_VTABLE, VtWeightDomain::ArcticF64);
wfst_vtable!(
    SIGNED_TROPICAL_WFST_VTABLE,
    VtWeightDomain::SignedTropicalF64
);
wfst_vtable!(COUNT_WFST_VTABLE, VtWeightDomain::CountF64);
wfst_vtable!(BOOLEAN_WFST_VTABLE, VtWeightDomain::BooleanF64);

fn wfst_vtable(weight: VtWeightDomain) -> *const VtWfstVTable {
    match weight {
        VtWeightDomain::TropicalF64 => &TROPICAL_WFST_VTABLE,
        VtWeightDomain::LogF64 => &LOG_WFST_VTABLE,
        VtWeightDomain::ProbabilityF64 => &PROBABILITY_WFST_VTABLE,
        VtWeightDomain::ArcticF64 => &ARCTIC_WFST_VTABLE,
        VtWeightDomain::SignedTropicalF64 => &SIGNED_TROPICAL_WFST_VTABLE,
        VtWeightDomain::CountF64 => &COUNT_WFST_VTABLE,
        VtWeightDomain::BooleanF64 => &BOOLEAN_WFST_VTABLE,
    }
}

struct RawOwnedResource(VtResource);
impl Drop for RawOwnedResource {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                if let Some(release) = (*self.0.vtable).release {
                    release(self.0.context);
                }
            }
        }
    }
}

unsafe fn discover_wfst(resource: VtResource) -> Result<*const VtWfstVTable, BindingError> {
    if resource.is_null() {
        return Err(BindingError::NullResource);
    }
    let base = &*resource.vtable;
    if base.struct_size < std::mem::size_of::<VtResourceVTable>()
        || base.abi_version != VT_ABI_VERSION
        || base.retain.is_none()
        || base.release.is_none()
        || base.query_interface.is_none()
    {
        return Err(BindingError::IncompatibleResourceAbi);
    }
    let mut interface: *const c_void = std::ptr::null();
    let result = base.query_interface.unwrap()(
        resource.context,
        &VT_WFST_INTERFACE_ID,
        VT_WFST_INTERFACE_VERSION,
        &mut interface,
    );
    if VtStatus::from_raw(result) == Some(VtStatus::Unsupported) {
        return Err(BindingError::MissingWfstInterface);
    }
    check_status(result)?;
    if interface.is_null() {
        return Err(BindingError::InvalidProviderOutput(
            "query_interface returned null",
        ));
    }
    let interface = interface.cast::<VtWfstVTable>();
    let table = &*interface;
    if table.struct_size < std::mem::size_of::<VtWfstVTable>()
        || table.interface_version < VT_WFST_INTERFACE_VERSION
        || table.snapshot.is_none()
        || table.start.is_none()
        || table.state_info.is_none()
        || table.state_arcs.is_none()
    {
        return Err(BindingError::IncompatibleWfstInterface);
    }
    if table.unit_domain != VtUnitDomain::UnicodeScalar {
        return Err(BindingError::UnitDomainMismatch(table.unit_domain));
    }
    if table.weight_domain != VtWeightDomain::TropicalF64 {
        return Err(BindingError::WeightDomainMismatch(table.weight_domain));
    }
    Ok(interface)
}

/// Capture and import a Unicode/tropical scalar-WFST resource.
///
/// The traversal copies each reachable state and arc exactly once into lling-
/// llang's compact eager representation. This is used at a cross-runtime
/// composition boundary; native resource production itself is O(1).
pub fn import_tropical_wfst(
    resource: VtResource,
) -> Result<VectorWfst<char, TropicalWeight>, BindingError> {
    unsafe {
        let live = discover_wfst(resource)?;
        let mut snapshot = VtResource::NULL;
        check_status((*live).snapshot.unwrap()(resource.context, &mut snapshot))?;
        if snapshot.is_null() {
            return Err(BindingError::InvalidProviderOutput(
                "snapshot returned null",
            ));
        }
        let snapshot = RawOwnedResource(snapshot);
        let table = discover_wfst(snapshot.0)?;
        let mut raw_start = 0;
        check_status((*table).start.unwrap()(snapshot.0.context, &mut raw_start))?;

        let mut graph = VectorWfst::new();
        let local_start = graph.add_state();
        graph.set_start(local_start);
        let mut ids = HashMap::from([(raw_start, local_start)]);
        let mut queue = VecDeque::from([raw_start]);
        let mut page = vec![VtWfstArc::default(); VT_RECOMMENDED_ARC_BATCH];

        while let Some(raw_state) = queue.pop_front() {
            let local_state = ids[&raw_state];
            let mut valid = 0;
            let mut is_final = 0;
            let mut final_weight = f64::INFINITY;
            check_status((*table).state_info.unwrap()(
                snapshot.0.context,
                raw_state,
                &mut valid,
                &mut is_final,
                &mut final_weight,
            ))?;
            if valid != 1 || is_final > 1 || !TropicalWeight::is_valid_raw(final_weight) {
                return Err(BindingError::InvalidProviderOutput(
                    "invalid state_info fields",
                ));
            }
            if is_final == 1 {
                let state = graph
                    .state_mut(local_state)
                    .ok_or(BindingError::RepresentationLimit)?;
                state.is_final = true;
                state.final_weight = TropicalWeight::new(final_weight);
            }

            let mut offset = 0usize;
            loop {
                let mut written = 0usize;
                let mut total = 0usize;
                check_status((*table).state_arcs.unwrap()(
                    snapshot.0.context,
                    raw_state,
                    offset,
                    page.as_mut_ptr(),
                    page.len(),
                    &mut written,
                    &mut total,
                ))?;
                // F3 harmonization: the same acceptance predicate `expand_state`
                // runs (the ConsumerAcceptance `accepts_dec` law). The
                // `offset + written > total` conjunct rejects an overshooting
                // final page immediately, rather than one iteration late after
                // buffering a page of extra arcs.
                if written > page.len()
                    || offset > total
                    || offset.saturating_add(written) > total
                    || (written == 0 && offset < total)
                {
                    return Err(BindingError::InvalidProviderOutput(
                        "invalid arc page counts",
                    ));
                }
                graph.reserve_transitions(local_state, written);
                for arc in page.iter().take(written) {
                    if arc.has_input > 1
                        || arc.has_output > 1
                        || arc.reserved != [0; 6]
                        || !TropicalWeight::is_valid_raw(arc.weight)
                    {
                        return Err(BindingError::InvalidProviderOutput("invalid arc fields"));
                    }
                    let input = if arc.has_input == 0 {
                        None
                    } else {
                        Some(
                            char::from_u32(
                                u32::try_from(arc.input_label)
                                    .map_err(|_| BindingError::RepresentationLimit)?,
                            )
                            .ok_or(BindingError::RepresentationLimit)?,
                        )
                    };
                    let output = if arc.has_output == 0 {
                        None
                    } else {
                        Some(
                            char::from_u32(
                                u32::try_from(arc.output_label)
                                    .map_err(|_| BindingError::RepresentationLimit)?,
                            )
                            .ok_or(BindingError::RepresentationLimit)?,
                        )
                    };
                    let target = if let Some(target) = ids.get(&arc.target_state) {
                        *target
                    } else {
                        if graph.num_states() >= StateId::MAX as usize {
                            return Err(BindingError::RepresentationLimit);
                        }
                        let target = graph.add_state();
                        ids.insert(arc.target_state, target);
                        queue.push_back(arc.target_state);
                        target
                    };
                    graph.add_arc(
                        local_state,
                        input,
                        output,
                        target,
                        TropicalWeight::new(arc.weight),
                    );
                }
                offset = offset
                    .checked_add(written)
                    .ok_or(BindingError::RepresentationLimit)?;
                if offset == total {
                    break;
                }
            }
        }
        Ok(graph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn left() -> VectorWfst<char, TropicalWeight> {
        let mut graph = VectorWfst::new();
        let a = graph.add_state();
        let b = graph.add_state();
        graph.set_start(a);
        graph.set_final(b, TropicalWeight::new(0.0));
        graph.add_arc(a, Some('a'), Some('x'), b, TropicalWeight::new(1.0));
        graph
    }

    #[test]
    fn resource_round_trip_and_lazy_composition() {
        let first = OwnedWfstResource::from_wfst(left());
        let imported = import_tropical_wfst(first.as_raw()).unwrap();
        assert_eq!(imported.transitions(imported.start())[0].output, Some('x'));

        let mut right = VectorWfst::new();
        let a = right.add_state();
        let b = right.add_state();
        right.set_start(a);
        right.set_final(b, TropicalWeight::new(0.0));
        right.add_arc(a, Some('x'), Some('z'), b, TropicalWeight::new(0.5));
        let second = OwnedWfstResource::from_wfst(right);
        let composed = OwnedWfstResource::compose(first.as_raw(), second.as_raw()).unwrap();
        let table = unsafe { discover_wfst(composed.as_raw()).unwrap().as_ref().unwrap() };
        assert_ne!(table.flags & wfst_flags::PARALLEL_REENTRANT, 0);
        let mut arcs = [VtWfstArc::default(); 4];
        let mut written = 0;
        let mut total = 0;
        assert_eq!(
            unsafe {
                table.state_arcs.unwrap()(
                    composed.as_raw().context,
                    0,
                    0,
                    arcs.as_mut_ptr(),
                    arcs.len(),
                    &mut written,
                    &mut total,
                )
            },
            VtStatus::Ok.to_raw()
        );
        assert_eq!((written, total), (1, 1));
        assert_eq!(char::from_u32(arcs[0].output_label as u32), Some('z'));
        assert_eq!(arcs[0].weight, 1.5);
    }

    struct CountingProvider {
        expansions: Arc<AtomicUsize>,
        input: char,
        output: char,
    }

    impl ScalarWfstProvider for CountingProvider {
        fn start(&self) -> Result<u64, VtStatus> {
            Ok(0)
        }

        fn num_states(&self) -> Result<Option<usize>, VtStatus> {
            Ok(Some(2))
        }

        fn state(&self, state: u64) -> Result<ScalarWfstState, VtStatus> {
            self.expansions.fetch_add(1, Ordering::Relaxed);
            Ok(match state {
                0 => ScalarWfstState {
                    valid: true,
                    is_final: false,
                    final_weight: f64::INFINITY,
                    arcs: vec![VtWfstArc {
                        input_label: u64::from(u32::from(self.input)),
                        output_label: u64::from(u32::from(self.output)),
                        target_state: 1,
                        weight: 0.0,
                        has_input: 1,
                        has_output: 1,
                        reserved: [0; 6],
                    }],
                },
                1 => ScalarWfstState {
                    valid: true,
                    is_final: true,
                    final_weight: 0.0,
                    arcs: vec![],
                },
                _ => ScalarWfstState {
                    valid: false,
                    is_final: false,
                    final_weight: f64::INFINITY,
                    arcs: vec![],
                },
            })
        }
    }

    #[test]
    fn composition_construction_retains_inputs_without_expanding_them() {
        let left_expansions = Arc::new(AtomicUsize::new(0));
        let right_expansions = Arc::new(AtomicUsize::new(0));
        let left = OwnedWfstResource::from_provider(Arc::new(CountingProvider {
            expansions: Arc::clone(&left_expansions),
            input: 'a',
            output: 'x',
        }));
        let right = OwnedWfstResource::from_provider(Arc::new(CountingProvider {
            expansions: Arc::clone(&right_expansions),
            input: 'x',
            output: 'z',
        }));

        let composed = OwnedWfstResource::compose(left.as_raw(), right.as_raw()).unwrap();
        assert_eq!(left_expansions.load(Ordering::Relaxed), 0);
        assert_eq!(right_expansions.load(Ordering::Relaxed), 0);

        let table = unsafe { discover_wfst(composed.as_raw()).unwrap().as_ref().unwrap() };
        let mut arc = VtWfstArc::default();
        let mut written = 0;
        let mut total = 0;
        assert_eq!(
            unsafe {
                table.state_arcs.unwrap()(
                    composed.as_raw().context,
                    0,
                    0,
                    &mut arc,
                    1,
                    &mut written,
                    &mut total,
                )
            },
            VtStatus::Ok.to_raw()
        );
        assert_eq!(
            (written, total, arc.output_label),
            (1, 1, u64::from(u32::from('z')))
        );
        assert_eq!(left_expansions.load(Ordering::Relaxed), 1);
        assert_eq!(right_expansions.load(Ordering::Relaxed), 1);
    }

    /// A provider that reports a -inf tropical arc weight: valid f64, not NaN,
    /// but NOT a valid TropicalWeight (only finite or +inf). Before the F1 fix
    /// this slipped the is_nan-only check and either produced +inf + -inf = NaN
    /// in composition or panicked in TropicalWeight::new. It must now be
    /// rejected as invalid provider output at ingestion.
    struct NegInfArcProvider;

    impl ScalarWfstProvider for NegInfArcProvider {
        fn start(&self) -> Result<u64, VtStatus> {
            Ok(0)
        }

        fn num_states(&self) -> Result<Option<usize>, VtStatus> {
            Ok(Some(2))
        }

        fn state(&self, state: u64) -> Result<ScalarWfstState, VtStatus> {
            Ok(match state {
                0 => ScalarWfstState {
                    valid: true,
                    is_final: false,
                    final_weight: f64::INFINITY,
                    arcs: vec![VtWfstArc {
                        input_label: u64::from(u32::from('a')),
                        output_label: u64::from(u32::from('x')),
                        target_state: 1,
                        weight: f64::NEG_INFINITY,
                        has_input: 1,
                        has_output: 1,
                        reserved: [0; 6],
                    }],
                },
                _ => ScalarWfstState {
                    valid: true,
                    is_final: true,
                    final_weight: 0.0,
                    arcs: vec![],
                },
            })
        }
    }

    /// LLING-B2/F1 regression: a -inf tropical weight is rejected as invalid
    /// provider output on import (no panic, no NaN), and on the composition
    /// expansion path (surfaced as a provider error during traversal).
    #[test]
    fn negative_infinity_tropical_weight_is_rejected_not_poisoned() {
        // Import path: reject cleanly rather than panic in TropicalWeight::new.
        let poison = OwnedWfstResource::from_provider(Arc::new(NegInfArcProvider));
        let imported = import_tropical_wfst(poison.as_raw());
        assert!(
            matches!(imported, Err(BindingError::InvalidProviderOutput(_))),
            "import must reject a -inf arc weight, got {imported:?}"
        );

        // Composition expansion path: the -inf surfaces as a provider error
        // when the composed state is expanded, never as a NaN arc weight.
        let clean = left();
        let clean_resource = OwnedWfstResource::from_wfst(clean);
        let poison2 = OwnedWfstResource::from_provider(Arc::new(NegInfArcProvider));
        let composed =
            OwnedWfstResource::compose(poison2.as_raw(), clean_resource.as_raw()).unwrap();
        let table = unsafe { discover_wfst(composed.as_raw()).unwrap().as_ref().unwrap() };
        let mut arc = VtWfstArc::default();
        let mut written = 0;
        let mut total = 0;
        let status = unsafe {
            table.state_arcs.unwrap()(
                composed.as_raw().context,
                0,
                0,
                &mut arc,
                1,
                &mut written,
                &mut total,
            )
        };
        assert_ne!(
            status,
            VtStatus::Ok.to_raw(),
            "expanding a -inf-weighted product state must fail, not yield a NaN arc"
        );
    }
}
