//! Revision-10 bounded, materializing native WFST transforms.

use super::*;
use crate::algorithms::{
    connect, determinize, is_deterministic, minimize_distance_iteration_cap,
    minimize_with_distance_config, remove_epsilon, worst_case_work, CheckedMinimizeError,
    ConnectConfig, DeterminizeConfig, DeterminizeError, EpsilonRemovalConfig, MinimizeConfig,
    NativeTransformKind, ShortestDistanceConfig,
};
use crate::semiring::{DivisibleSemiring, QuantizableSemiring, TotallyOrderedSemiring};
use crate::wfst::Wfst;
use std::fmt::Debug;
use std::hash::Hash;

const ALL_BUDGET_FLAGS: u64 =
    LLING_BUDGET_STATES | LLING_BUDGET_ARCS | LLING_BUDGET_BYTES | LLING_BUDGET_WORK;

#[derive(Clone, Copy)]
pub(super) enum MaterializingTransform {
    Determinize,
    Minimize,
    RemoveEpsilon,
    Connect,
}

pub(super) fn materializing_budget(
    budget: *const LlingBudgetV2,
) -> Result<GraphBudget, LlingLlangStatus> {
    let raw = read_v2_struct(budget, "budget", ALL_BUDGET_FLAGS)?;
    if !validate_budget_v2(&raw) || raw.header.flags != ALL_BUDGET_FLAGS {
        set_error("bounded graph operations require a canonical budget with all four limit flags");
        return Err(LlingLlangStatus::InvalidArgument);
    }
    graph_budget_from_v2(budget)
}

fn mul_budget(a: u64, b: u64, axis: &'static str) -> Result<u64, BindingError> {
    a.checked_mul(b).ok_or(BindingError::BudgetExceeded(axis))
}

fn native_shape<L: AbiScalarLabel, W: AbiScalarWeight>(graph: &VectorWfst<L, W>) -> (u64, u64) {
    let states = graph.num_states() as u64;
    let arcs = (0..graph.num_states() as u32)
        .map(|state| graph.transitions(state).len() as u64)
        .sum();
    (states, arcs)
}

// Check conservative potential output without consuming the actual budget.
// The native algorithm is invoked only after this admission check; its exact
// result is separately charged before exporting the handle.
fn preflight<L: AbiScalarLabel, W: AbiScalarWeight>(
    budget: &GraphBudget,
    states: u64,
    arcs: u64,
) -> Result<(), BindingError> {
    budget.clone().charge_output::<L, W>(states, arcs)
}

fn reserve_native_work(
    budget: &mut GraphBudget,
    kind: NativeTransformKind,
    states: u64,
    arcs: u64,
) -> Result<(), BindingError> {
    let units = worst_case_work(kind, states, arcs).ok_or(BindingError::BudgetExceeded("work"))?;
    budget.charge(0, 0, 0, units)
}

fn transform_common<L, W>(
    resource: VtResource,
    mut budget: GraphBudget,
    operation: MaterializingTransform,
) -> Result<OwnedWfstResource, BindingError>
where
    L: AbiScalarLabel + PartialEq,
    W: AbiScalarWeight,
{
    let (mut graph, stats): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(resource, &mut budget)?;
    let n = stats.states;
    let a = stats.arcs;
    match operation {
        MaterializingTransform::Connect => {
            // Trimming never adds a native state or arc.
            reserve_native_work(&mut budget, NativeTransformKind::Connect, n, a)?;
            preflight::<L, W>(&budget, n, a)?;
            connect(&mut graph, ConnectConfig::trim());
        }
        MaterializingTransform::RemoveEpsilon => {
            // Each source closure has at most n members; each retained arc may
            // traverse a destination closure of at most n members.
            reserve_native_work(&mut budget, NativeTransformKind::RemoveEpsilon, n, a)?;
            let bound = mul_budget(mul_budget(n, n, "arcs")?, a, "arcs")?;
            preflight::<L, W>(&budget, n, bound)?;
            remove_epsilon(&mut graph, EpsilonRemovalConfig::default()).map_err(|error| {
                BindingError::InvalidProviderOutput(match error {
                    crate::algorithms::EpsilonRemovalError::NoStartState => {
                        "native epsilon removal input has no start state"
                    }
                    crate::algorithms::EpsilonRemovalError::NonConvergentCycle => {
                        "native epsilon cycle does not converge"
                    }
                })
            })?;
        }
        _ => unreachable!(),
    }
    let (states, arcs) = native_shape(&graph);
    budget.charge_output::<L, W>(states, arcs)?;
    export_native_wfst(&graph)
}

fn transform_divisible<L, W>(
    resource: VtResource,
    mut budget: GraphBudget,
    operation: MaterializingTransform,
) -> Result<OwnedWfstResource, BindingError>
where
    L: AbiScalarLabel + Eq + Hash + Ord + Debug,
    W: AbiScalarWeight
        + DivisibleSemiring
        + TotallyOrderedSemiring
        + QuantizableSemiring
        + PartialOrd
        + Hash
        + Eq
        + Debug,
{
    let (mut graph, stats): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(resource, &mut budget)?;
    let n = stats.states;
    let a = stats.arcs;
    match operation {
        MaterializingTransform::Determinize => {
            if crate::algorithms::has_epsilon_transitions(&graph) {
                reserve_native_work(&mut budget, NativeTransformKind::RemoveEpsilon, n, a)?;
                let bound = mul_budget(mul_budget(n, n, "arcs")?, a, "arcs")?;
                preflight::<L, W>(&budget, n, bound)?;
                remove_epsilon(
                    &mut graph,
                    EpsilonRemovalConfig {
                        connect: false,
                        ..Default::default()
                    },
                )
                .map_err(|error| {
                    BindingError::InvalidProviderOutput(match error {
                        crate::algorithms::EpsilonRemovalError::NoStartState => {
                            "native epsilon removal input has no start state"
                        }
                        crate::algorithms::EpsilonRemovalError::NonConvergentCycle => {
                            "native epsilon cycle does not converge"
                        }
                    })
                })?;
                let (states, arcs) = native_shape(&graph);
                budget.charge_output::<L, W>(states, arcs)?;
            }
            let (n, a) = native_shape(&graph);
            let deterministic = is_deterministic(&graph);
            let state_cap = if deterministic {
                n
            } else {
                budget.remaining(0)
            };
            let arc_cap = if deterministic {
                a
            } else {
                mul_budget(state_cap, a, "arcs")?
            };
            reserve_native_work(
                &mut budget,
                NativeTransformKind::Determinize {
                    output_state_cap: state_cap,
                },
                n,
                a,
            )?;
            preflight::<L, W>(&budget, state_cap, arc_cap)?;
            let max_states = usize::try_from(state_cap.min(u64::from(NO_STATE) - 1))
                .map_err(|_| BindingError::RepresentationLimit)?;
            graph = determinize(
                &graph,
                DeterminizeConfig {
                    max_states: Some(max_states),
                    remove_epsilon_first: false,
                    connect_after: true,
                },
            )
            .map_err(|error| match error {
                DeterminizeError::StateLimitExceeded { .. } => {
                    BindingError::BudgetExceeded("states")
                }
                _ => BindingError::InvalidProviderOutput("native input is not determinizable"),
            })?;
        }
        MaterializingTransform::Minimize => {
            // Minimized graph cannot exceed the input shape. Native pushing is
            // iteration-bounded here even when the public Rust default is not.
            let iterations =
                minimize_distance_iteration_cap(n).ok_or(BindingError::BudgetExceeded("work"))?;
            reserve_native_work(
                &mut budget,
                NativeTransformKind::Minimize {
                    distance_iterations: iterations,
                },
                n,
                a,
            )?;
            preflight::<L, W>(&budget, n, a)?;
            let mut distance_config = ShortestDistanceConfig::default();
            distance_config.max_iterations = Some(
                usize::try_from(iterations).map_err(|_| BindingError::BudgetExceeded("work"))?,
            );
            graph =
                minimize_with_distance_config(&graph, MinimizeConfig::default(), distance_config)
                    .map_err(|error| match error {
                    CheckedMinimizeError::DistanceLimitExceeded => {
                        BindingError::BudgetExceeded("work")
                    }
                    CheckedMinimizeError::Native(error) => {
                        BindingError::InvalidProviderOutput(match error {
                            crate::algorithms::MinimizeError::NotDeterministic => {
                                "native input is not deterministic"
                            }
                            crate::algorithms::MinimizeError::NoStartState => {
                                "native minimization input has no start state"
                            }
                            crate::algorithms::MinimizeError::InvalidWeightEpsilon { .. } => {
                                "native minimization weight epsilon is invalid"
                            }
                            crate::algorithms::MinimizeError::PushError(_) => {
                                "native minimization weight pushing failed"
                            }
                        })
                    }
                })?;
        }
        _ => unreachable!(),
    }
    let (states, arcs) = native_shape(&graph);
    budget.charge_output::<L, W>(states, arcs)?;
    export_native_wfst(&graph)
}

fn transform_dispatch(
    resource: VtResource,
    budget: GraphBudget,
    operation: MaterializingTransform,
) -> Result<OwnedWfstResource, LlingLlangStatus> {
    let (unit, weight) = wfst_domains(resource).map_err(map_error)?;
    match operation {
        MaterializingTransform::Connect | MaterializingTransform::RemoveEpsilon => {
            dispatch_scalar_domains!(unit, weight, transform_common, resource, budget, operation)
                .map_err(map_transform_error)
        }
        MaterializingTransform::Determinize | MaterializingTransform::Minimize => {
            macro_rules! divisible_weight {
                ($label:ty) => {
                    match weight {
                        VtWeightDomain::TropicalF64 => {
                            transform_divisible::<$label, TropicalWeight>(
                                resource, budget, operation,
                            )
                        }
                        VtWeightDomain::LogF64 => {
                            transform_divisible::<$label, LogWeight>(resource, budget, operation)
                        }
                        VtWeightDomain::ProbabilityF64 => transform_divisible::<
                            $label,
                            ProbabilityWeight,
                        >(resource, budget, operation),
                        VtWeightDomain::SignedTropicalF64 => transform_divisible::<
                            $label,
                            SignedTropicalWeight,
                        >(resource, budget, operation),
                        VtWeightDomain::CountF64 => {
                            transform_divisible::<$label, CountWeight>(resource, budget, operation)
                        }
                        VtWeightDomain::ArcticF64 | VtWeightDomain::BooleanF64 => {
                            set_error(
                                "transform requires a divisible, ordered, quantizable semiring",
                            );
                            return Err(LlingLlangStatus::IncompatibleResource);
                        }
                    }
                };
            }
            let result = match unit {
                VtUnitDomain::Byte => divisible_weight!(u8),
                VtUnitDomain::UnicodeScalar => divisible_weight!(char),
                VtUnitDomain::U64 => divisible_weight!(u64),
            };
            result.map_err(map_transform_error)
        }
    }
}

fn map_transform_error(error: BindingError) -> LlingLlangStatus {
    match error {
        BindingError::InvalidProviderOutput(message) if message.starts_with("native ") => {
            set_error(message);
            LlingLlangStatus::InvalidArgument
        }
        other => map_error(other),
    }
}

pub(super) fn materialize(
    resource: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
    operation: MaterializingTransform,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_wfst, "out_wfst")?;
        let budget = materializing_budget(budget)?;
        let resource = transform_dispatch(resource, budget, operation)?;
        *output = Box::into_raw(Box::new(LlingWfst { resource }));
        Ok(())
    })
}

pub(super) fn materialize_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
    operation: MaterializingTransform,
) -> LlingLlangStatus {
    if resource.is_null() {
        set_error("resource is null");
        return LlingLlangStatus::NullPointer;
    }
    materialize(unsafe { *resource }, budget, out_wfst, operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::minimize;
    use crate::bindings::import_native_wfst;
    use crate::semiring::Semiring;
    use crate::wfst::{MutableWfst, WeightedTransition};
    use std::ptr;

    fn budget() -> LlingBudgetV2 {
        LlingBudgetV2 {
            header: LlingAbiV2Header {
                struct_size: std::mem::size_of::<LlingBudgetV2>() as u32,
                abi_version: LLING_ABI_V2,
                flags: ALL_BUDGET_FLAGS,
                ..Default::default()
            },
            max_states: 16,
            max_arcs: 256,
            max_bytes: 100_000,
            max_work: 100_000,
            ..Default::default()
        }
    }

    fn graph<W: AbiScalarWeight>() -> VectorWfst<u8, W> {
        let mut graph = VectorWfst::new();
        let start = graph.add_state();
        let finish = graph.add_state();
        graph.set_start(start);
        graph.set_final(finish, W::one());
        graph.add_transition(WeightedTransition {
            from: start,
            to: finish,
            input: Some(b'a'),
            output: Some(b'a'),
            weight: W::one(),
        });
        graph
    }

    fn output_graph<W: AbiScalarWeight>(handle: *mut LlingWfst) -> VectorWfst<u8, W> {
        let mut raw = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(handle, &mut raw) },
            LlingLlangStatus::Ok
        );
        unsafe { lling_wfst_free(handle) };
        let output = import_native_wfst(raw).unwrap();
        lling_resource_release(raw);
        output
    }

    #[test]
    fn four_transforms_match_native_results() {
        let source = graph::<TropicalWeight>();
        let resource = export_native_wfst(&source).unwrap();
        let mut handle = ptr::null_mut();
        let b = budget();

        assert_eq!(
            lling_wfst_determinize(resource.as_raw(), &b, &mut handle),
            LlingLlangStatus::Ok
        );
        let expected = determinize(&source, DeterminizeConfig::default()).unwrap();
        let actual = output_graph::<TropicalWeight>(handle);
        assert_eq!(native_shape(&actual), native_shape(&expected));
        assert_eq!(actual.transitions(0), expected.transitions(0));

        assert_eq!(
            lling_wfst_minimize(resource.as_raw(), &b, &mut handle),
            LlingLlangStatus::Ok
        );
        let expected = minimize(&source, MinimizeConfig::default()).unwrap();
        let actual = output_graph::<TropicalWeight>(handle);
        assert_eq!(native_shape(&actual), native_shape(&expected));
        assert_eq!(actual.transitions(0), expected.transitions(0));

        assert_eq!(
            lling_wfst_remove_epsilon(resource.as_raw(), &b, &mut handle),
            LlingLlangStatus::Ok
        );
        let mut expected = source.clone();
        remove_epsilon(&mut expected, EpsilonRemovalConfig::default()).unwrap();
        let actual = output_graph::<TropicalWeight>(handle);
        assert_eq!(native_shape(&actual), native_shape(&expected));
        assert_eq!(actual.transitions(0), expected.transitions(0));

        assert_eq!(
            lling_wfst_connect(resource.as_raw(), &b, &mut handle),
            LlingLlangStatus::Ok
        );
        let mut expected = source.clone();
        connect(&mut expected, ConnectConfig::trim());
        let actual = output_graph::<TropicalWeight>(handle);
        assert_eq!(native_shape(&actual), native_shape(&expected));
        assert_eq!(actual.transitions(0), expected.transitions(0));
    }

    #[test]
    fn supported_and_unsupported_semirings_follow_native_traits() {
        let b = budget();
        for source in [
            export_native_wfst(&graph::<CountWeight>()).unwrap(),
            export_native_wfst(&graph::<SignedTropicalWeight>()).unwrap(),
        ] {
            let mut output = ptr::null_mut();
            assert_eq!(
                lling_wfst_determinize(source.as_raw(), &b, &mut output),
                LlingLlangStatus::Ok
            );
            unsafe { lling_wfst_free(output) };
            assert_eq!(
                lling_wfst_minimize(source.as_raw(), &b, &mut output),
                LlingLlangStatus::Ok
            );
            unsafe { lling_wfst_free(output) };
        }
        for source in [
            export_native_wfst(&graph::<ArcticWeight>()).unwrap(),
            export_native_wfst(&graph::<BoolWeight>()).unwrap(),
        ] {
            let sentinel = ptr::dangling_mut::<LlingWfst>();
            let mut output = sentinel;
            assert_eq!(
                lling_wfst_determinize(source.as_raw(), &b, &mut output),
                LlingLlangStatus::IncompatibleResource
            );
            assert_eq!(output, sentinel);
            assert_eq!(
                lling_wfst_minimize(source.as_raw(), &b, &mut output),
                LlingLlangStatus::IncompatibleResource
            );
            assert_eq!(output, sentinel);
            assert_eq!(
                lling_wfst_connect(source.as_raw(), &b, &mut output),
                LlingLlangStatus::Ok
            );
            unsafe { lling_wfst_free(output) };
        }
    }

    #[test]
    fn flags_limits_and_nulls_fail_atomically() {
        let source = export_native_wfst(&graph::<TropicalWeight>()).unwrap();
        let sentinel = ptr::dangling_mut::<LlingWfst>();
        let mut output = sentinel;
        let mut b = budget();
        b.header.flags &= !LLING_BUDGET_WORK;
        assert_eq!(
            lling_wfst_connect(source.as_raw(), &b, &mut output),
            LlingLlangStatus::InvalidArgument
        );
        assert_eq!(output, sentinel);
        b = budget();
        b.max_states = 1;
        assert_eq!(
            lling_wfst_connect(source.as_raw(), &b, &mut output),
            LlingLlangStatus::LimitExceeded
        );
        assert_eq!(output, sentinel);
        assert_eq!(
            unsafe { lling_wfst_connect_ref(ptr::null(), &budget(), &mut output) },
            LlingLlangStatus::NullPointer
        );
        assert_eq!(output, sentinel);
        assert_eq!(
            lling_wfst_connect(source.as_raw(), &budget(), ptr::null_mut()),
            LlingLlangStatus::NullPointer
        );
    }

    #[test]
    fn work_reservation_is_exact_for_connect_and_epsilon_preflight_is_conservative() {
        let source = export_native_wfst(&graph::<TropicalWeight>()).unwrap();
        let sentinel = ptr::dangling_mut::<LlingWfst>();
        let mut output = sentinel;
        let mut b = budget();
        // Import: 2 states + 1 arc = 3 work; source-derived connect
        // reservation: 8((2+1)+(1+1)) = 40; output: 2+1 = 3.
        b.max_work = 45;
        assert_eq!(
            lling_wfst_connect(source.as_raw(), &b, &mut output),
            LlingLlangStatus::LimitExceeded
        );
        assert_eq!(output, sentinel);
        b.max_work = 46;
        assert_eq!(
            lling_wfst_connect(source.as_raw(), &b, &mut output),
            LlingLlangStatus::Ok
        );
        unsafe { lling_wfst_free(output) };

        // A no-epsilon graph would in fact keep its one arc, but the
        // source-derived potential n²*a = 4 is intentionally checked first.
        b = budget();
        b.max_arcs = 4; // one imported arc leaves only three for output
        output = sentinel;
        assert_eq!(
            lling_wfst_remove_epsilon(source.as_raw(), &b, &mut output),
            LlingLlangStatus::LimitExceeded
        );
        assert_eq!(output, sentinel);
        b.max_arcs = 5;
        assert_eq!(
            lling_wfst_remove_epsilon(source.as_raw(), &b, &mut output),
            LlingLlangStatus::Ok
        );
        unsafe { lling_wfst_free(output) };
    }

    #[test]
    fn native_preconditions_and_state_cap_are_classified() {
        let mut nondeterministic = graph::<TropicalWeight>();
        nondeterministic.add_transition(WeightedTransition {
            from: 0,
            to: 1,
            input: Some(b'a'),
            output: Some(b'b'),
            weight: TropicalWeight::one(),
        });
        let source = export_native_wfst(&nondeterministic).unwrap();
        let sentinel = ptr::dangling_mut::<LlingWfst>();
        let mut output = sentinel;
        assert_eq!(
            lling_wfst_minimize(source.as_raw(), &budget(), &mut output),
            LlingLlangStatus::InvalidArgument
        );
        assert_eq!(output, sentinel);
        assert_eq!(
            lling_wfst_determinize(source.as_raw(), &budget(), &mut output),
            LlingLlangStatus::InvalidArgument
        );
        assert_eq!(output, sentinel);

        let mut b = budget();
        b.max_states = 3; // two imported states leave a one-state output cap
        let source = export_native_wfst(&graph::<TropicalWeight>()).unwrap();
        assert_eq!(
            lling_wfst_determinize(source.as_raw(), &b, &mut output),
            LlingLlangStatus::LimitExceeded
        );
        assert_eq!(output, sentinel);

        let mut cyclic = graph::<TropicalWeight>();
        cyclic.add_transition(WeightedTransition {
            from: 0,
            to: 1,
            input: None,
            output: None,
            weight: TropicalWeight::one(),
        });
        cyclic.add_transition(WeightedTransition {
            from: 1,
            to: 0,
            input: None,
            output: None,
            weight: TropicalWeight::one(),
        });
        let source = export_native_wfst(&cyclic).unwrap();
        assert_eq!(
            lling_wfst_remove_epsilon(source.as_raw(), &budget(), &mut output),
            LlingLlangStatus::InvalidArgument
        );
        assert_eq!(output, sentinel);
    }
}
