//! Checked, single-pass adapters between the scalar resource ABI and native WFSTs.
//!
//! The scalar ABI has runtime label and weight domains. Native algorithms use
//! concrete Rust types, so the domain choice must be checked before traversal
//! and each value must be checked again at the conversion boundary. Algorithms
//! can then operate on an ordinary `VectorWfst` without reimplementing their
//! mathematics in the FFI layer.

use super::{
    discover_wfst, valid_scalar_label, valid_scalar_weight, BindingError, CapturedWfst,
    OwnedWfstResource, ScalarWfstGraph, MAX_EXACT_F64_INTEGER,
};
use crate::semiring::{
    ArcticWeight, BoolWeight, CountWeight, LogWeight, ProbabilityWeight, Semiring,
    SignedTropicalWeight, TropicalWeight,
};
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst, NO_STATE};
use std::collections::{HashMap, VecDeque};
use vinary_tree_interop::{VtResource, VtUnitDomain, VtWeightDomain, VtWfstArc};

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
    let start = graph.add_state();
    graph.set_start(start);
    let mut ids = HashMap::from([(captured.start, start)]);
    let mut queue = VecDeque::from([captured.start]);

    while let Some(raw_state) = queue.pop_front() {
        let local_state = ids[&raw_state];
        let state = captured.state(raw_state)?;
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
            if transition.from != state || transition.to as usize >= state_count {
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
            let arc = VtWfstArc {
                input_label,
                output_label,
                target_state: u64::from(transition.to),
                weight,
                has_input: u8::from(transition.input.is_some()),
                has_output: u8::from(transition.output.is_some()),
                reserved: [0; 6],
            };
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
    use std::fmt::Debug;

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

        let exported = export_native_wfst(&source).unwrap();
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
    fn no_start_is_exported_as_an_empty_weighted_relation() {
        let mut source = VectorWfst::<u8, BoolWeight>::new();
        let inaccessible = source.add_state();
        source.set_final(inaccessible, BoolWeight::one());
        let exported = export_native_wfst(&source).unwrap();
        let restored: VectorWfst<u8, BoolWeight> = import_native_wfst(exported.as_raw()).unwrap();
        assert_eq!(restored.num_states(), 1);
        assert!(!restored.is_final(restored.start()));
        assert!(restored.transitions(restored.start()).is_empty());
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
}
