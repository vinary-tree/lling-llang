//! Checked, single-pass adapters between the scalar resource ABI and native WFSTs.
//!
//! The scalar ABI has runtime label and weight domains. Native algorithms use
//! concrete Rust types, so the domain choice must be checked before traversal
//! and each value must be checked again at the conversion boundary. Algorithms
//! can then operate on an ordinary `VectorWfst` without reimplementing their
//! mathematics in the FFI layer.

use super::{
    discover_wfst, scalar_zero, valid_scalar_label, valid_scalar_weight, BindingError,
    CapturedWfst, OwnedWfstResource, ScalarStateData, ScalarWfstGraph, ScalarWfstProvider,
    ScalarWfstState, MAX_EXACT_F64_INTEGER,
};
use crate::semiring::{
    ArcticWeight, BoolWeight, CountWeight, LogWeight, ProbabilityWeight, Semiring,
    SignedTropicalWeight, TropicalWeight,
};
use crate::wfst::{
    compute_state_at_snapshot, CancellationToken, ExpansionError, ExpansionFailureKind,
    MutableWfst, SharedCachePolicy, SourceSnapshot, StateExpansion, StateId, StateSource,
    VectorWfst, WeightedTransition, Wfst, WfstState, NO_STATE,
};
use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use vinary_tree_interop::{VtResource, VtStatus, VtUnitDomain, VtWeightDomain, VtWfstArc};

/// A native label that can be represented exactly by the scalar WFST ABI.
pub trait AbiScalarLabel: Clone + Send + Sync + 'static {
    /// The label's scalar ABI domain.
    const DOMAIN: VtUnitDomain;

    /// Decode one present (non-epsilon) ABI label.
    fn decode(value: u64) -> Option<Self>;

    /// Encode one present (non-epsilon) native label.
    fn encode(&self) -> u64;
}

impl AbiScalarLabel for u8 {
    const DOMAIN: VtUnitDomain = VtUnitDomain::Byte;

    fn decode(value: u64) -> Option<Self> {
        value.try_into().ok()
    }

    fn encode(&self) -> u64 {
        u64::from(*self)
    }
}

impl AbiScalarLabel for char {
    const DOMAIN: VtUnitDomain = VtUnitDomain::UnicodeScalar;

    fn decode(value: u64) -> Option<Self> {
        u32::try_from(value).ok().and_then(char::from_u32)
    }

    fn encode(&self) -> u64 {
        u64::from(u32::from(*self))
    }
}

impl AbiScalarLabel for u64 {
    const DOMAIN: VtUnitDomain = VtUnitDomain::U64;

    fn decode(value: u64) -> Option<Self> {
        Some(value)
    }

    fn encode(&self) -> u64 {
        *self
    }
}

/// A native semiring whose carrier can be represented exactly by an ABI `f64`.
///
/// `encode` is fallible because a native computation can leave the ABI carrier
/// (for example, a count greater than $`2^{53}`$ cannot be encoded exactly).
pub trait AbiScalarWeight: Semiring {
    /// The semiring's scalar ABI domain.
    const DOMAIN: VtWeightDomain;

    /// Decode one validated ABI weight.
    fn decode(value: f64) -> Option<Self>;

    /// Encode one native weight without rounding or leaving the ABI carrier.
    fn encode(self) -> Option<f64>;
}

macro_rules! scalar_float_weight {
    ($weight:ty, $domain:expr) => {
        impl AbiScalarWeight for $weight {
            const DOMAIN: VtWeightDomain = $domain;

            fn decode(value: f64) -> Option<Self> {
                valid_scalar_weight(Self::DOMAIN, value).then(|| Self::new(value))
            }

            fn encode(self) -> Option<f64> {
                let value = self.value();
                valid_scalar_weight(Self::DOMAIN, value).then_some(value)
            }
        }
    };
}

scalar_float_weight!(TropicalWeight, VtWeightDomain::TropicalF64);
scalar_float_weight!(LogWeight, VtWeightDomain::LogF64);
scalar_float_weight!(ProbabilityWeight, VtWeightDomain::ProbabilityF64);
scalar_float_weight!(ArcticWeight, VtWeightDomain::ArcticF64);
scalar_float_weight!(SignedTropicalWeight, VtWeightDomain::SignedTropicalF64);

impl AbiScalarWeight for CountWeight {
    const DOMAIN: VtWeightDomain = VtWeightDomain::CountF64;

    fn decode(value: f64) -> Option<Self> {
        valid_scalar_weight(Self::DOMAIN, value).then(|| Self::new(value as u64))
    }

    fn encode(self) -> Option<f64> {
        let value = self.value() as f64;
        valid_scalar_weight(Self::DOMAIN, value)
            .then_some(value)
            .filter(|_| self.value() <= MAX_EXACT_F64_INTEGER as u64)
    }
}

impl AbiScalarWeight for BoolWeight {
    const DOMAIN: VtWeightDomain = VtWeightDomain::BooleanF64;

    fn decode(value: f64) -> Option<Self> {
        valid_scalar_weight(Self::DOMAIN, value).then(|| Self::new(value == 1.0))
    }

    fn encode(self) -> Option<f64> {
        Some(f64::from(self.value()))
    }
}

fn encode_weight<W: AbiScalarWeight>(weight: W) -> Result<f64, BindingError> {
    let value = weight.encode().ok_or(BindingError::RepresentationLimit)?;
    valid_scalar_weight(W::DOMAIN, value)
        .then_some(value)
        .ok_or(BindingError::RepresentationLimit)
}

/// Cumulative, deterministic graph-payload limits for one native operation.
/// The byte axis counts native/scalar state payloads and both ABI/native arc
/// payloads; it is not a process-RSS cap over foreign provider allocations.
#[derive(Clone, Debug)]
pub(crate) struct GraphBudget {
    limits: [Option<u64>; 4],
    used: [u64; 4],
}

impl GraphBudget {
    pub(crate) fn new(
        max_states: Option<u64>,
        max_arcs: Option<u64>,
        max_bytes: Option<u64>,
        max_work: Option<u64>,
    ) -> Self {
        Self {
            limits: [max_states, max_arcs, max_bytes, max_work],
            used: [0; 4],
        }
    }

    fn unlimited() -> Self {
        Self::new(None, None, None, None)
    }

    pub(crate) fn charge(
        &mut self,
        states: u64,
        arcs: u64,
        bytes: u64,
        work: u64,
    ) -> Result<(), BindingError> {
        let increments = [states, arcs, bytes, work];
        let axes = ["states", "arcs", "bytes", "work"];
        let mut next = self.used;
        for index in 0..4 {
            next[index] = next[index]
                .checked_add(increments[index])
                .ok_or(BindingError::BudgetExceeded(axes[index]))?;
            if self.limits[index].is_some_and(|limit| next[index] > limit) {
                return Err(BindingError::BudgetExceeded(axes[index]));
            }
        }
        self.used = next;
        Ok(())
    }

    fn remaining(&self, axis: usize) -> u64 {
        self.limits[axis].map_or(u64::MAX, |limit| limit.saturating_sub(self.used[axis]))
    }

    fn max_arcs_for_state<L: AbiScalarLabel, W: AbiScalarWeight>(&self) -> (usize, &'static str) {
        let arc_bytes = arc_payload_bytes::<L, W>();
        let choices = [
            (self.remaining(1), "arcs"),
            (self.remaining(2) / arc_bytes, "bytes"),
            (self.remaining(3), "work"),
        ];
        let (limit, axis) = choices.into_iter().min_by_key(|choice| choice.0).unwrap();
        (limit.min(usize::MAX as u64) as usize, axis)
    }

    /// Reserve the complete potential output graph before constructing or
    /// publishing it. For lazy transforms this is deliberately conservative:
    /// callers can never receive a graph whose full expansion exceeds the
    /// declared logical graph budget.
    pub(crate) fn charge_output<L: AbiScalarLabel, W: AbiScalarWeight>(
        &mut self,
        states: u64,
        arcs: u64,
    ) -> Result<(), BindingError> {
        let state_bytes = states
            .checked_mul(state_payload_bytes::<L, W>())
            .ok_or(BindingError::BudgetExceeded("bytes"))?;
        let arc_bytes = arcs
            .checked_mul(arc_payload_bytes::<L, W>())
            .ok_or(BindingError::BudgetExceeded("bytes"))?;
        let bytes = state_bytes
            .checked_add(arc_bytes)
            .ok_or(BindingError::BudgetExceeded("bytes"))?;
        let work = states
            .checked_add(arcs)
            .ok_or(BindingError::BudgetExceeded("work"))?;
        self.charge(states, arcs, bytes, work)
    }
}

fn state_payload_bytes<L: AbiScalarLabel, W: AbiScalarWeight>() -> u64 {
    (std::mem::size_of::<WfstState<L, W>>() + std::mem::size_of::<ScalarStateData>()) as u64
}

fn arc_payload_bytes<L: AbiScalarLabel, W: AbiScalarWeight>() -> u64 {
    (std::mem::size_of::<VtWfstArc>() + std::mem::size_of::<WeightedTransition<L, W>>()) as u64
}

fn encode_arc<L: AbiScalarLabel, W: AbiScalarWeight>(
    state_count: Option<usize>,
    source: StateId,
    transition: &WeightedTransition<L, W>,
) -> Result<VtWfstArc, BindingError> {
    if transition.from != source
        || transition.to == NO_STATE
        || state_count.is_some_and(|count| transition.to as usize >= count)
    {
        return Err(BindingError::RepresentationLimit);
    }
    let weight = encode_weight(transition.weight)?;
    let input_label = transition.input.as_ref().map_or(0, L::encode);
    let output_label = transition.output.as_ref().map_or(0, L::encode);
    if (transition.input.is_some() && !valid_scalar_label(L::DOMAIN, input_label))
        || (transition.output.is_some() && !valid_scalar_label(L::DOMAIN, output_label))
    {
        return Err(BindingError::RepresentationLimit);
    }
    Ok(VtWfstArc {
        input_label,
        output_label,
        target_state: u64::from(transition.to),
        weight,
        has_input: u8::from(transition.input.is_some()),
        has_output: u8::from(transition.output.is_some()),
        reserved: [0; 6],
    })
}

struct NativeStateSourceProvider<L, W, S>
where
    L: AbiScalarLabel,
    W: AbiScalarWeight,
    S: StateSource<L, W>,
{
    source: S,
    snapshot: SourceSnapshot,
    states_hint: Option<usize>,
    start: u64,
    has_dead_start: bool,
    attempts: AtomicU64,
    _types: PhantomData<(L, W)>,
}

impl<L, W, S> NativeStateSourceProvider<L, W, S>
where
    L: AbiScalarLabel,
    W: AbiScalarWeight,
    S: StateSource<L, W>,
{
    fn new(source: S) -> Result<Self, BindingError> {
        let states_hint = source.num_states_hint();
        if states_hint.is_some_and(|count| count >= NO_STATE as usize) {
            return Err(BindingError::RepresentationLimit);
        }
        let raw_start = source.start();
        let has_dead_start = raw_start == NO_STATE;
        if !has_dead_start && states_hint.is_some_and(|count| raw_start as usize >= count) {
            return Err(BindingError::RepresentationLimit);
        }
        Ok(Self {
            snapshot: source.snapshot(),
            source,
            states_hint,
            start: u64::from(raw_start),
            has_dead_start,
            attempts: AtomicU64::new(0),
            _types: PhantomData,
        })
    }

    fn invalid_state() -> ScalarWfstState {
        ScalarWfstState {
            valid: false,
            is_final: false,
            final_weight: scalar_zero(W::DOMAIN),
            arcs: Vec::new(),
        }
    }
}

impl<L, W, S> ScalarWfstProvider for NativeStateSourceProvider<L, W, S>
where
    L: AbiScalarLabel,
    W: AbiScalarWeight,
    S: StateSource<L, W> + 'static,
{
    fn unit_domain(&self) -> VtUnitDomain {
        L::DOMAIN
    }

    fn weight_domain(&self) -> VtWeightDomain {
        W::DOMAIN
    }

    fn start(&self) -> Result<u64, VtStatus> {
        Ok(self.start)
    }

    fn num_states(&self) -> Result<Option<usize>, VtStatus> {
        // StateSource offers an upper bound, not necessarily an exact count.
        Ok(None)
    }

    fn state(&self, raw_state: u64) -> Result<ScalarWfstState, VtStatus> {
        if self.has_dead_start && raw_state == u64::from(NO_STATE) {
            return Ok(ScalarWfstState {
                valid: true,
                is_final: false,
                final_weight: scalar_zero(W::DOMAIN),
                arcs: Vec::new(),
            });
        }
        let Ok(state) = StateId::try_from(raw_state) else {
            return Ok(Self::invalid_state());
        };
        if state == NO_STATE
            || self
                .states_hint
                .is_some_and(|count| state as usize >= count)
        {
            return Ok(Self::invalid_state());
        }
        let attempt = self
            .attempts
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
                Some(old.saturating_add(1))
            })
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let cancellation = CancellationToken::new();
        let expansion = match compute_state_at_snapshot(
            &self.source,
            self.snapshot,
            state,
            attempt,
            &cancellation,
        ) {
            Ok(StateExpansion::Expanded {
                is_final,
                final_weight,
                transitions,
            }) => (is_final, final_weight, transitions),
            Err(ExpansionError::Failure(failure))
                if failure.kind() == ExpansionFailureKind::InvalidState =>
            {
                return Ok(Self::invalid_state());
            }
            Err(ExpansionError::Failure(failure))
                if failure.kind() == ExpansionFailureKind::ResourceExhausted =>
            {
                return Err(VtStatus::LimitExceeded);
            }
            Err(_) => return Err(VtStatus::ProviderError),
            Ok(_) => return Err(VtStatus::ProviderError),
        };
        let (is_final, final_weight, transitions) = expansion;
        let final_weight = if is_final {
            encode_weight(final_weight).map_err(|_| VtStatus::LimitExceeded)?
        } else {
            scalar_zero(W::DOMAIN)
        };
        let arcs = transitions
            .iter()
            .map(|arc| {
                encode_arc(self.states_hint, state, arc).map_err(|_| VtStatus::LimitExceeded)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ScalarWfstState {
            valid: true,
            is_final,
            final_weight,
            arcs,
        })
    }
}

/// Publish an immutable native state source without expanding any state.
/// The default outer cache retains no state payloads; callers may opt into a
/// bounded LRU through [`export_native_lazy_wfst_with_cache`]. Sources are
/// evaluated independently and in parallel against one captured snapshot.
///
/// # Example
///
/// ```
/// use lling_llang::bindings::{export_native_lazy_wfst, import_native_wfst};
/// use lling_llang::semiring::BoolWeight;
/// use lling_llang::wfst::{union, MutableWfst, VectorWfst, Wfst};
///
/// let mut operand = VectorWfst::<u8, BoolWeight>::new();
/// let start = operand.add_state();
/// operand.set_start(start);
/// operand.set_final(start, BoolWeight::new(true));
/// let resource = export_native_lazy_wfst(union(&operand, &operand).into_source())?;
/// let restored: VectorWfst<u8, BoolWeight> = import_native_wfst(resource.as_raw())?;
/// assert_eq!(restored.transitions(restored.start()).len(), 2);
/// # Ok::<(), lling_llang::bindings::BindingError>(())
/// ```
pub fn export_native_lazy_wfst<L, W, S>(source: S) -> Result<OwnedWfstResource, BindingError>
where
    L: AbiScalarLabel,
    W: AbiScalarWeight,
    S: StateSource<L, W> + 'static,
{
    export_native_lazy_wfst_with_cache(source, SharedCachePolicy::NoCache)
}

/// Publish a native state source with an explicit outer cache-residency policy.
/// The source's own cache, if any, remains independently configured.
pub fn export_native_lazy_wfst_with_cache<L, W, S>(
    source: S,
    policy: SharedCachePolicy,
) -> Result<OwnedWfstResource, BindingError>
where
    L: AbiScalarLabel,
    W: AbiScalarWeight,
    S: StateSource<L, W> + 'static,
{
    let provider = NativeStateSourceProvider::new(source)?;
    Ok(OwnedWfstResource::from_provider_with_cache(
        Arc::new(provider),
        policy,
    ))
}

/// Capture a scalar resource and convert its reachable states directly into a
/// typed native graph, without first allocating a second eager scalar graph.
///
/// The input is borrowed. A snapshot is retained for the whole traversal and
/// released before this function returns. Domain mismatches are rejected
/// before asking the provider to snapshot itself.
pub fn import_native_wfst<L, W>(resource: VtResource) -> Result<VectorWfst<L, W>, BindingError>
where
    L: AbiScalarLabel,
    W: AbiScalarWeight,
{
    import_native_wfst_with_budget(resource, &mut GraphBudget::unlimited())
}

pub(crate) fn import_native_wfst_with_budget<L, W>(
    resource: VtResource,
    budget: &mut GraphBudget,
) -> Result<VectorWfst<L, W>, BindingError>
where
    L: AbiScalarLabel,
    W: AbiScalarWeight,
{
    let live = unsafe { discover_wfst(resource)? };
    let (unit_domain, weight_domain) = unsafe { ((*live).unit_domain, (*live).weight_domain) };
    if unit_domain != L::DOMAIN {
        return Err(BindingError::UnitDomainMismatch(unit_domain));
    }
    if weight_domain != W::DOMAIN {
        return Err(BindingError::WeightDomainMismatch(weight_domain));
    }

    let captured = unsafe { CapturedWfst::capture(resource)? };
    let mut graph = VectorWfst::new();
    budget.charge(1, 0, state_payload_bytes::<L, W>(), 0)?;
    let start = graph.add_state();
    graph.set_start(start);
    let mut ids = HashMap::from([(captured.start, start)]);
    let mut queue = VecDeque::from([captured.start]);

    while let Some(raw_state) = queue.pop_front() {
        let local_state = ids[&raw_state];
        budget.charge(0, 0, 0, 1)?;
        let (arc_limit, limiting_axis) = budget.max_arcs_for_state::<L, W>();
        let state = captured
            .state_uncached_with_arc_limit(raw_state, arc_limit)
            .map_err(|error| match error {
                BindingError::BudgetExceeded(_) => BindingError::BudgetExceeded(limiting_axis),
                other => other,
            })?;
        if !state.valid {
            return Err(BindingError::InvalidProviderOutput(
                "reachable state is reported invalid",
            ));
        }
        if state.is_final {
            // `MutableWfst::set_final` clears a semiring-zero final. The ABI
            // carries finality separately, so preserve it through state_mut.
            let final_state = graph
                .state_mut(local_state)
                .ok_or(BindingError::RepresentationLimit)?;
            final_state.is_final = true;
            final_state.final_weight = W::decode(state.final_weight)
                .ok_or(BindingError::InvalidProviderOutput("invalid final weight"))?;
        }
        let arc_count =
            u64::try_from(state.arcs.len()).map_err(|_| BindingError::RepresentationLimit)?;
        let arc_bytes = arc_count
            .checked_mul(arc_payload_bytes::<L, W>())
            .ok_or(BindingError::RepresentationLimit)?;
        budget.charge(0, arc_count, arc_bytes, arc_count)?;
        graph.reserve_transitions(local_state, state.arcs.len());
        for arc in state.arcs.iter() {
            let input = if arc.has_input == 0 {
                None
            } else {
                Some(
                    L::decode(arc.input_label)
                        .ok_or(BindingError::InvalidProviderOutput("invalid input label"))?,
                )
            };
            let output = if arc.has_output == 0 {
                None
            } else {
                Some(
                    L::decode(arc.output_label)
                        .ok_or(BindingError::InvalidProviderOutput("invalid output label"))?,
                )
            };
            let target = if let Some(target) = ids.get(&arc.target_state) {
                *target
            } else {
                if graph.num_states() >= NO_STATE as usize {
                    return Err(BindingError::RepresentationLimit);
                }
                budget.charge(1, 0, state_payload_bytes::<L, W>(), 0)?;
                let target = graph.add_state();
                ids.insert(arc.target_state, target);
                queue.push_back(arc.target_state);
                target
            };
            graph
                .try_add_transition(WeightedTransition::new(
                    local_state,
                    input,
                    output,
                    target,
                    W::decode(arc.weight)
                        .ok_or(BindingError::InvalidProviderOutput("invalid arc weight"))?,
                ))
                .map_err(|_| BindingError::RepresentationLimit)?;
        }
    }
    Ok(graph)
}

/// Export any native scalar WFST into a retained, immutable ABI resource.
///
/// All states and arcs are checked before publication. If a native algorithm
/// returns no start state, the ABI's mandatory start is represented by a new
/// unreachable-to-finals dead state; the accepted weighted relation is empty.
///
/// # Example
///
/// ```
/// use lling_llang::bindings::{export_native_wfst, import_native_wfst};
/// use lling_llang::semiring::BoolWeight;
/// use lling_llang::wfst::{MutableWfst, VectorWfst, Wfst};
///
/// let mut native = VectorWfst::<u8, BoolWeight>::new();
/// let start = native.add_state();
/// native.set_start(start);
/// native.set_final(start, BoolWeight::new(true));
/// let resource = export_native_wfst(&native)?;
/// let restored: VectorWfst<u8, BoolWeight> = import_native_wfst(resource.as_raw())?;
/// assert!(restored.is_final(restored.start()));
/// # Ok::<(), lling_llang::bindings::BindingError>(())
/// ```
pub fn export_native_wfst<L, W, F>(fst: &F) -> Result<OwnedWfstResource, BindingError>
where
    L: AbiScalarLabel,
    W: AbiScalarWeight,
    F: Wfst<L, W>,
{
    let state_count = fst.num_states();
    if state_count >= NO_STATE as usize {
        return Err(BindingError::RepresentationLimit);
    }
    let mut graph = ScalarWfstGraph::new(L::DOMAIN, W::DOMAIN);
    graph.reserve_states(state_count + usize::from(fst.start() == NO_STATE));
    for _ in 0..state_count {
        graph.add_state()?;
    }
    if fst.start() == NO_STATE {
        let dead_start = graph.add_state()?;
        graph.set_start(dead_start);
    } else if !graph.set_start(fst.start()) {
        return Err(BindingError::RepresentationLimit);
    }

    for index in 0..state_count {
        let state = StateId::try_from(index).map_err(|_| BindingError::RepresentationLimit)?;
        if fst.is_final(state) {
            let weight = encode_weight(fst.final_weight(state))?;
            graph.set_final(state, weight);
        }
        for transition in fst.transitions(state) {
            let arc = encode_arc(Some(state_count), state, transition)?;
            if !graph.add_arc(state, arc) {
                return Err(BindingError::RepresentationLimit);
            }
        }
    }
    Ok(OwnedWfstResource::from_scalar_wfst(graph))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::{ExpansionFailure, ExpansionRequest};
    use std::fmt::Debug;
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct VectorStateSource<L: AbiScalarLabel, W: AbiScalarWeight> {
        fst: VectorWfst<L, W>,
    }

    impl<L: AbiScalarLabel, W: AbiScalarWeight> StateSource<L, W> for VectorStateSource<L, W> {
        fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<L, W> {
            let state = request.state();
            if !self.fst.is_valid_state(state) {
                return StateExpansion::failed(ExpansionFailure::invalid_state(state));
            }
            StateExpansion::Expanded {
                is_final: self.fst.is_final(state),
                final_weight: self.fst.final_weight(state),
                transitions: self.fst.transitions(state).iter().cloned().collect(),
            }
        }

        fn start(&self) -> StateId {
            self.fst.start()
        }

        fn num_states_hint(&self) -> Option<usize> {
            Some(self.fst.num_states())
        }
    }

    #[derive(Clone)]
    struct CountingSource {
        inner: VectorStateSource<u8, BoolWeight>,
        expansions: Arc<AtomicUsize>,
    }

    impl StateSource<u8, BoolWeight> for CountingSource {
        fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<u8, BoolWeight> {
            self.expansions.fetch_add(1, Ordering::SeqCst);
            self.inner.compute_state(request)
        }

        fn start(&self) -> StateId {
            self.inner.start()
        }

        fn num_states_hint(&self) -> Option<usize> {
            self.inner.num_states_hint()
        }
    }

    #[derive(Clone)]
    struct EpochSource {
        inner: VectorStateSource<u8, BoolWeight>,
        epoch: Arc<AtomicUsize>,
        mutate_during_expansion: bool,
    }

    impl StateSource<u8, BoolWeight> for EpochSource {
        fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<u8, BoolWeight> {
            let result = self.inner.compute_state(request);
            if self.mutate_during_expansion {
                self.epoch.fetch_add(1, Ordering::SeqCst);
            }
            result
        }

        fn snapshot(&self) -> SourceSnapshot {
            let mut bytes = [0; 32];
            bytes[..8].copy_from_slice(&(self.epoch.load(Ordering::SeqCst) as u64).to_le_bytes());
            SourceSnapshot::from_bytes(bytes)
        }

        fn start(&self) -> StateId {
            self.inner.start()
        }

        fn num_states_hint(&self) -> Option<usize> {
            self.inner.num_states_hint()
        }
    }

    #[derive(Clone)]
    struct InvalidByteLabel;

    impl AbiScalarLabel for InvalidByteLabel {
        const DOMAIN: VtUnitDomain = VtUnitDomain::Byte;

        fn decode(_: u64) -> Option<Self> {
            Some(Self)
        }

        fn encode(&self) -> u64 {
            256
        }
    }

    fn round_trip<L, W>(label: L)
    where
        L: AbiScalarLabel + Copy + Debug + PartialEq,
        W: AbiScalarWeight,
    {
        let mut source = VectorWfst::<L, W>::new();
        let start = source.add_state();
        let end = source.add_state();
        source.set_start(start);
        // Finality is an independent bit in the ABI, even at semiring zero.
        let final_state = source.state_mut(end).unwrap();
        final_state.is_final = true;
        final_state.final_weight = W::zero();
        source.add_arc(start, None, Some(label), end, W::one());

        let eager = export_native_wfst(&source).unwrap();
        let lazy = export_native_lazy_wfst(VectorStateSource { fst: source }).unwrap();
        for exported in [&eager, &lazy] {
            let restored: VectorWfst<L, W> = import_native_wfst(exported.as_raw()).unwrap();
            assert_eq!(restored.num_states(), 2);
            assert_eq!(restored.start(), start);
            assert!(restored.is_final(end));
            assert_eq!(restored.final_weight(end), W::zero());
            let arc = &restored.transitions(start)[0];
            assert_eq!(arc.input, None);
            assert_eq!(arc.output, Some(label));
            assert_eq!(arc.weight, W::one());
            assert_eq!(arc.to, end);
        }
    }

    macro_rules! every_weight {
        ($label:expr, $label_type:ty) => {{
            round_trip::<$label_type, TropicalWeight>($label);
            round_trip::<$label_type, LogWeight>($label);
            round_trip::<$label_type, ProbabilityWeight>($label);
            round_trip::<$label_type, ArcticWeight>($label);
            round_trip::<$label_type, SignedTropicalWeight>($label);
            round_trip::<$label_type, CountWeight>($label);
            round_trip::<$label_type, BoolWeight>($label);
        }};
    }

    #[test]
    fn every_scalar_domain_pair_preserves_epsilon_and_zero_finality() {
        every_weight!(b'a', u8);
        every_weight!('🦀', char);
        every_weight!(u64::MAX, u64);
    }

    #[test]
    fn domains_are_checked_before_snapshot_and_count_encoding_is_exact() {
        let mut source = VectorWfst::<u8, CountWeight>::new();
        let start = source.add_state();
        source.set_start(start);
        source.set_final(start, CountWeight::new(1));
        let exported = export_native_wfst(&source).unwrap();
        assert_eq!(
            import_native_wfst::<char, CountWeight>(exported.as_raw()).unwrap_err(),
            BindingError::UnitDomainMismatch(VtUnitDomain::Byte)
        );
        assert_eq!(
            import_native_wfst::<u8, TropicalWeight>(exported.as_raw()).unwrap_err(),
            BindingError::WeightDomainMismatch(VtWeightDomain::CountF64)
        );

        source.set_final(start, CountWeight::new(9_007_199_254_740_993));
        assert!(matches!(
            export_native_wfst(&source),
            Err(BindingError::RepresentationLimit)
        ));
    }

    #[test]
    fn input_materialization_enforces_each_budget_axis_before_publication() {
        let mut source = VectorWfst::<u8, BoolWeight>::new();
        let start = source.add_state();
        let end = source.add_state();
        source.set_start(start);
        source.set_final(end, BoolWeight::one());
        source.add_arc(start, Some(b'a'), None, end, BoolWeight::one());
        let resource = export_native_wfst(&source).unwrap();
        let state_bytes = state_payload_bytes::<u8, BoolWeight>();
        let arc_bytes = arc_payload_bytes::<u8, BoolWeight>();
        let cases = [
            (GraphBudget::new(Some(1), None, None, None), "states"),
            (GraphBudget::new(None, Some(0), None, None), "arcs"),
            (
                GraphBudget::new(None, None, Some(state_bytes), None),
                "bytes",
            ),
            (GraphBudget::new(None, None, None, Some(1)), "work"),
        ];
        for (mut budget, axis) in cases {
            assert_eq!(
                import_native_wfst_with_budget::<u8, BoolWeight>(resource.as_raw(), &mut budget)
                    .unwrap_err(),
                BindingError::BudgetExceeded(axis)
            );
        }
        let mut exact =
            GraphBudget::new(Some(2), Some(1), Some(2 * state_bytes + arc_bytes), Some(3));
        let imported: VectorWfst<u8, BoolWeight> =
            import_native_wfst_with_budget(resource.as_raw(), &mut exact).unwrap();
        assert_eq!(imported.num_states(), 2);
        assert_eq!(imported.transitions(imported.start()).len(), 1);
    }

    #[test]
    fn no_start_is_exported_as_an_empty_weighted_relation() {
        let mut source = VectorWfst::<u8, BoolWeight>::new();
        let inaccessible = source.add_state();
        source.set_final(inaccessible, BoolWeight::one());
        let exported = export_native_wfst(&source).unwrap();
        let restored: VectorWfst<u8, BoolWeight> = import_native_wfst(exported.as_raw()).unwrap();
        assert_eq!(restored.num_states(), 1);
        assert!(!restored.is_final(restored.start()));
        assert!(restored.transitions(restored.start()).is_empty());

        let lazy = export_native_lazy_wfst(VectorStateSource { fst: source }).unwrap();
        let restored: VectorWfst<u8, BoolWeight> = import_native_wfst(lazy.as_raw()).unwrap();
        assert_eq!(restored.num_states(), 1);
        assert!(!restored.is_final(restored.start()));
    }

    #[test]
    fn malformed_native_target_cannot_be_published() {
        let mut source = VectorWfst::<u8, BoolWeight>::new();
        let start = source.add_state();
        source.set_start(start);
        source.add_arc(start, Some(b'a'), None, 10, BoolWeight::one());
        assert!(matches!(
            export_native_wfst(&source),
            Err(BindingError::RepresentationLimit)
        ));
    }

    #[test]
    fn custom_codec_cannot_publish_values_outside_its_claimed_domain() {
        let mut source = VectorWfst::<InvalidByteLabel, BoolWeight>::new();
        let start = source.add_state();
        source.set_start(start);
        source.add_arc(
            start,
            Some(InvalidByteLabel),
            None,
            start,
            BoolWeight::one(),
        );
        assert!(matches!(
            export_native_wfst(&source),
            Err(BindingError::RepresentationLimit)
        ));

        let mut invalid_weight = VectorWfst::<u8, TropicalWeight>::new();
        let start = invalid_weight.add_state();
        invalid_weight.set_start(start);
        let state = invalid_weight.state_mut(start).unwrap();
        state.is_final = true;
        state.final_weight = TropicalWeight::new_unchecked(f64::NEG_INFINITY);
        assert!(matches!(
            export_native_wfst(&invalid_weight),
            Err(BindingError::RepresentationLimit)
        ));
    }

    #[test]
    fn lazy_publication_does_not_expand_and_uses_configurable_bounded_residency() {
        let mut fst = VectorWfst::<u8, BoolWeight>::new();
        let first = fst.add_state();
        let second = fst.add_state();
        fst.set_start(first);
        fst.set_final(second, BoolWeight::one());
        fst.add_arc(first, Some(b'a'), None, second, BoolWeight::one());
        let expansions = Arc::new(AtomicUsize::new(0));
        let counting = CountingSource {
            inner: VectorStateSource { fst },
            expansions: Arc::clone(&expansions),
        };
        let policy = SharedCachePolicy::Lru {
            capacity: NonZeroUsize::new(1).unwrap(),
        };
        let resource = export_native_lazy_wfst_with_cache(counting, policy).unwrap();
        let control = resource.provider_cache().unwrap();
        assert_eq!(control.policy(), policy);
        assert_eq!(expansions.load(Ordering::SeqCst), 0);

        let restored: VectorWfst<u8, BoolWeight> = import_native_wfst(resource.as_raw()).unwrap();
        assert_eq!(restored.num_states(), 2);
        assert_eq!(restored.transitions(restored.start()).len(), 1);
        assert!(expansions.load(Ordering::SeqCst) > 0);
        assert!(control.statistics().resident_states <= 1);
    }

    #[test]
    fn lazy_invalid_result_fails_when_expanded_without_partial_publication() {
        let mut source = VectorWfst::<u8, BoolWeight>::new();
        let start = source.add_state();
        source.set_start(start);
        source.add_arc(start, Some(b'a'), None, 10, BoolWeight::one());
        let resource = export_native_lazy_wfst(VectorStateSource { fst: source }).unwrap();
        assert_eq!(
            import_native_wfst::<u8, BoolWeight>(resource.as_raw()).unwrap_err(),
            BindingError::Provider(VtStatus::LimitExceeded)
        );
    }

    #[test]
    fn native_rational_union_source_expands_through_the_scalar_resource() {
        use crate::wfst::rational::union;

        let mut left = VectorWfst::<u8, BoolWeight>::new();
        let left_start = left.add_state();
        let left_end = left.add_state();
        left.set_start(left_start);
        left.set_final(left_end, BoolWeight::one());
        left.add_arc(left_start, Some(b'a'), None, left_end, BoolWeight::one());

        let mut right = VectorWfst::<u8, BoolWeight>::new();
        let right_start = right.add_state();
        let right_end = right.add_state();
        right.set_start(right_start);
        right.set_final(right_end, BoolWeight::one());
        right.add_arc(right_start, Some(b'b'), None, right_end, BoolWeight::one());

        let source = union(&left, &right).into_source();
        let resource = export_native_lazy_wfst(source).unwrap();
        let native: VectorWfst<u8, BoolWeight> = import_native_wfst(resource.as_raw()).unwrap();
        assert_eq!(native.num_states(), 5);
        assert_eq!(native.transitions(native.start()).len(), 2);
        let mut inputs: Vec<_> = (0..native.num_states() as StateId)
            .flat_map(|state| native.transitions(state).iter().filter_map(|arc| arc.input))
            .collect();
        inputs.sort_unstable();
        assert_eq!(inputs, [b'a', b'b']);
        assert_eq!(native.final_states().count(), 2);
    }

    #[test]
    fn changed_source_snapshot_cannot_publish_a_state() {
        let mut fst = VectorWfst::<u8, BoolWeight>::new();
        let start = fst.add_state();
        fst.set_start(start);
        fst.set_final(start, BoolWeight::one());
        let epoch = Arc::new(AtomicUsize::new(0));
        let source = EpochSource {
            inner: VectorStateSource { fst },
            epoch: Arc::clone(&epoch),
            mutate_during_expansion: false,
        };
        let resource = export_native_lazy_wfst(source).unwrap();
        epoch.store(1, Ordering::SeqCst);
        assert_eq!(
            import_native_wfst::<u8, BoolWeight>(resource.as_raw()).unwrap_err(),
            BindingError::Provider(VtStatus::ProviderError)
        );

        let mut fst = VectorWfst::<u8, BoolWeight>::new();
        let start = fst.add_state();
        fst.set_start(start);
        fst.set_final(start, BoolWeight::one());
        let source = EpochSource {
            inner: VectorStateSource { fst },
            epoch: Arc::new(AtomicUsize::new(0)),
            mutate_during_expansion: true,
        };
        let resource = export_native_lazy_wfst(source).unwrap();
        assert_eq!(
            import_native_wfst::<u8, BoolWeight>(resource.as_raw()).unwrap_err(),
            BindingError::Provider(VtStatus::ProviderError)
        );
    }

    #[test]
    fn concurrent_imports_observe_one_immutable_source_snapshot() {
        let mut fst = VectorWfst::<u8, BoolWeight>::new();
        let first = fst.add_state();
        let second = fst.add_state();
        fst.set_start(first);
        fst.set_final(second, BoolWeight::one());
        fst.add_arc(first, Some(b'z'), None, second, BoolWeight::one());
        let resource = Arc::new(
            export_native_lazy_wfst_with_cache(
                VectorStateSource { fst },
                SharedCachePolicy::Lru {
                    capacity: NonZeroUsize::new(1).unwrap(),
                },
            )
            .unwrap(),
        );
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    let resource = Arc::clone(&resource);
                    scope.spawn(move || {
                        let graph: VectorWfst<u8, BoolWeight> =
                            import_native_wfst(resource.as_raw()).unwrap();
                        assert_eq!(graph.num_states(), 2);
                        assert_eq!(graph.transitions(graph.start())[0].input, Some(b'z'));
                        assert_eq!(graph.final_states().count(), 1);
                    })
                })
                .collect();
            for worker in workers {
                worker.join().unwrap();
            }
        });
        assert!(
            resource
                .provider_cache()
                .unwrap()
                .statistics()
                .resident_states
                <= 1
        );
    }
}
