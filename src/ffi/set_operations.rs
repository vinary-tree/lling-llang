//! Domain-qualified scalar-WFST set operations. General difference is absent.

use super::*;
use crate::wfst::Wfst;

fn checked_mul(a: u64, b: u64, axis: &'static str) -> Result<u64, BindingError> {
    a.checked_mul(b).ok_or(BindingError::BudgetExceeded(axis))
}

fn checked_add(a: u64, b: u64, axis: &'static str) -> Result<u64, BindingError> {
    a.checked_add(b).ok_or(BindingError::BudgetExceeded(axis))
}

fn is_acceptor<L: AbiScalarLabel + PartialEq, W: AbiScalarWeight>(
    graph: &VectorWfst<L, W>,
) -> bool {
    (0..graph.num_states() as u32).all(|state| {
        graph
            .transitions(state)
            .iter()
            .all(|arc| arc.input == arc.output)
    })
}

fn intersect_typed<L: AbiScalarLabel + PartialEq, W: AbiScalarWeight>(
    first: VtResource,
    second: VtResource,
    mut budget: GraphBudget,
) -> Result<OwnedWfstResource, LlingLlangStatus> {
    let (first_graph, first_stats): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(first, &mut budget).map_err(map_error)?;
    let (second_graph, second_stats): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(second, &mut budget).map_err(map_error)?;
    if !is_acceptor(&first_graph) || !is_acceptor(&second_graph) {
        set_error("acceptor intersection requires equal input/output labels on every reachable arc, including epsilon");
        return Err(LlingLlangStatus::InvalidArgument);
    }

    let (n1, a1) = (first_stats.states, first_stats.arcs);
    let (n2, a2) = (second_stats.states, second_stats.arcs);
    // The two exported, immutable operand copies coexist with the imported
    // native graphs and composition. Reserve them before allocation.
    budget.charge_output::<L, W>(n1, a1).map_err(map_error)?;
    budget.charge_output::<L, W>(n2, a2).map_err(map_error)?;

    // Native composition has three epsilon-filter states. Across each filter
    // state there are at most n1*n2 state pairs, a1*a2 matching-arc pairs,
    // a1*n2 unilateral left-epsilon moves, and a2*n1 right-epsilon moves.
    // Matching requires scanning the arc pairs, even if labels do not match.
    let pairs = checked_mul(n1, n2, "states").map_err(map_error)?;
    let output_states = checked_mul(3, pairs, "states").map_err(map_error)?;
    if output_states >= u64::from(NO_STATE) {
        return Err(map_error(BindingError::RepresentationLimit));
    }
    let candidates = checked_add(
        checked_add(
            checked_mul(a1, a2, "arcs").map_err(map_error)?,
            checked_mul(a1, n2, "arcs").map_err(map_error)?,
            "arcs",
        )
        .map_err(map_error)?,
        checked_mul(a2, n1, "arcs").map_err(map_error)?,
        "arcs",
    )
    .map_err(map_error)?;
    let output_arcs = checked_mul(3, candidates, "arcs").map_err(map_error)?;
    // Count one scan per candidate plus one visit per product state. Three
    // graph reservations cover the composition cache, checked output import,
    // and independently exported result. They also reserve their visits.
    let scan_work = checked_add(output_states, output_arcs, "work").map_err(map_error)?;
    budget.charge(0, 0, 0, scan_work).map_err(map_error)?;
    budget
        .charge_output::<L, W>(output_states, output_arcs)
        .map_err(map_error)?;
    budget
        .charge_output::<L, W>(output_states, output_arcs)
        .map_err(map_error)?;
    budget
        .charge_output::<L, W>(output_states, output_arcs)
        .map_err(map_error)?;

    let first_copy = export_native_wfst(&first_graph).map_err(map_error)?;
    let second_copy = export_native_wfst(&second_graph).map_err(map_error)?;
    let composed =
        OwnedWfstResource::compose(first_copy.as_raw(), second_copy.as_raw()).map_err(map_error)?;
    let mut expansion_budget = GraphBudget::new(
        Some(output_states),
        Some(output_arcs),
        None,
        Some(scan_work),
    );
    let (output, _): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(composed.as_raw(), &mut expansion_budget)
            .map_err(map_error)?;
    export_native_wfst(&output).map_err(map_error)
}

fn intersect_dispatch(
    first: VtResource,
    second: VtResource,
    budget: GraphBudget,
) -> Result<OwnedWfstResource, LlingLlangStatus> {
    let domains = wfst_domains(first).map_err(map_error)?;
    let other_domains = wfst_domains(second).map_err(map_error)?;
    if domains != other_domains {
        set_error("acceptor intersection requires equal scalar label and semiring domains");
        return Err(LlingLlangStatus::IncompatibleResource);
    }
    dispatch_scalar_domains!(domains.0, domains.1, intersect_typed, first, second, budget)
}

pub(super) fn acceptor_intersect(
    first: VtResource,
    second: VtResource,
    budget: *const LlingBudgetV2,
    out_wfst: *mut *mut LlingWfst,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_wfst, "out_wfst")?;
        let budget = transforms::materializing_budget(budget)?;
        let resource = intersect_dispatch(first, second, budget)?;
        *output = Box::into_raw(Box::new(LlingWfst { resource }));
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::import_native_wfst;
    use crate::semiring::Semiring;
    use crate::wfst::{MutableWfst, WeightedTransition};
    use std::ptr;

    fn budget() -> LlingBudgetV2 {
        LlingBudgetV2 {
            header: LlingAbiV2Header {
                struct_size: std::mem::size_of::<LlingBudgetV2>() as u32,
                abi_version: LLING_ABI_V2,
                flags: LLING_BUDGET_STATES
                    | LLING_BUDGET_ARCS
                    | LLING_BUDGET_BYTES
                    | LLING_BUDGET_WORK,
                ..Default::default()
            },
            max_states: 1_000,
            max_arcs: 10_000,
            max_bytes: 10_000_000,
            max_work: 100_000,
            ..Default::default()
        }
    }

    fn one_arc<L: AbiScalarLabel, W: AbiScalarWeight>(
        label: Option<L>,
        output: Option<L>,
        weight: W,
    ) -> VectorWfst<L, W> {
        let mut graph = VectorWfst::new();
        let start = graph.add_state();
        let finish = graph.add_state();
        graph.set_start(start);
        graph.set_final(finish, W::one());
        graph.add_transition(WeightedTransition::new(
            start, label, output, finish, weight,
        ));
        graph
    }

    fn result_graph<L: AbiScalarLabel, W: AbiScalarWeight>(
        handle: *mut LlingWfst,
    ) -> VectorWfst<L, W> {
        let mut raw = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(handle, &mut raw) },
            LlingLlangStatus::Ok
        );
        unsafe { lling_wfst_free(handle) };
        let graph = import_native_wfst(raw).unwrap();
        lling_resource_release(raw);
        graph
    }

    fn count_paths(graph: &VectorWfst<u8, CountWeight>) -> Vec<(Vec<u8>, u64)> {
        fn visit(
            graph: &VectorWfst<u8, CountWeight>,
            state: u32,
            label: Vec<u8>,
            weight: CountWeight,
            out: &mut Vec<(Vec<u8>, u64)>,
        ) {
            if graph.is_final(state) {
                out.push((
                    label.clone(),
                    weight.times(&graph.final_weight(state)).value(),
                ));
            }
            for arc in graph.transitions(state) {
                let mut next_label = label.clone();
                if let Some(unit) = arc.input {
                    next_label.push(unit);
                }
                visit(graph, arc.to, next_label, weight.times(&arc.weight), out);
            }
        }
        let mut out = Vec::new();
        visit(
            graph,
            graph.start(),
            Vec::new(),
            CountWeight::one(),
            &mut out,
        );
        out
    }

    fn count_chain(
        epsilons: usize,
        epsilon_weight: u64,
        label_weight: u64,
    ) -> VectorWfst<u8, CountWeight> {
        let mut graph = VectorWfst::new();
        let start = graph.add_state();
        graph.set_start(start);
        let mut previous = start;
        for _ in 0..epsilons {
            let next = graph.add_state();
            graph.add_transition(WeightedTransition::epsilon(
                previous,
                next,
                CountWeight::new(epsilon_weight),
            ));
            previous = next;
        }
        let finish = graph.add_state();
        graph.add_transition(WeightedTransition::new(
            previous,
            Some(b'a'),
            Some(b'a'),
            finish,
            CountWeight::new(label_weight),
        ));
        graph.set_final(finish, CountWeight::one());
        graph
    }

    fn probability_chain(
        epsilons: usize,
        epsilon_weight: f64,
        label_weight: f64,
    ) -> VectorWfst<u8, ProbabilityWeight> {
        let mut graph = VectorWfst::new();
        let start = graph.add_state();
        graph.set_start(start);
        let mut previous = start;
        for _ in 0..epsilons {
            let next = graph.add_state();
            graph.add_transition(WeightedTransition::epsilon(
                previous,
                next,
                ProbabilityWeight::new(epsilon_weight),
            ));
            previous = next;
        }
        let finish = graph.add_state();
        graph.add_transition(WeightedTransition::new(
            previous,
            Some(b'a'),
            Some(b'a'),
            finish,
            ProbabilityWeight::new(label_weight),
        ));
        graph.set_final(finish, ProbabilityWeight::one());
        graph
    }

    fn probability_paths(graph: &VectorWfst<u8, ProbabilityWeight>) -> Vec<f64> {
        fn visit(
            graph: &VectorWfst<u8, ProbabilityWeight>,
            state: u32,
            weight: ProbabilityWeight,
            out: &mut Vec<f64>,
        ) {
            if graph.is_final(state) {
                out.push(weight.times(&graph.final_weight(state)).value());
            }
            for arc in graph.transitions(state) {
                visit(graph, arc.to, weight.times(&arc.weight), out);
            }
        }
        let mut out = Vec::new();
        visit(graph, graph.start(), ProbabilityWeight::one(), &mut out);
        out
    }

    fn matrix_case<L: AbiScalarLabel + PartialEq, W: AbiScalarWeight>() {
        let label = L::decode(65).unwrap();
        let graph = one_arc(Some(label.clone()), Some(label), W::one());
        let source = export_native_wfst(&graph).unwrap();
        let mut handle = ptr::null_mut();
        assert_eq!(
            lling_wfst_acceptor_intersect_refs_for_test(source.as_raw(), &budget(), &mut handle),
            LlingLlangStatus::Ok
        );
        drop(source);
        let result: VectorWfst<L, W> = result_graph(handle);
        let arcs = result.transitions(result.start());
        assert_eq!(arcs.len(), 1);
        assert!(arcs[0].input == arcs[0].output);
        assert_eq!(arcs[0].weight.encode(), W::one().encode());
        assert!(result.is_final(arcs[0].to));
    }

    fn lling_wfst_acceptor_intersect_refs_for_test(
        resource: VtResource,
        budget: &LlingBudgetV2,
        output: &mut *mut LlingWfst,
    ) -> LlingLlangStatus {
        unsafe { lling_wfst_acceptor_intersect_refs(&resource, &resource, budget, output) }
    }

    #[test]
    fn all_three_label_domains_and_seven_semirings_are_supported() {
        macro_rules! weights {
            ($label:ty) => {
                matrix_case::<$label, TropicalWeight>();
                matrix_case::<$label, LogWeight>();
                matrix_case::<$label, ProbabilityWeight>();
                matrix_case::<$label, ArcticWeight>();
                matrix_case::<$label, SignedTropicalWeight>();
                matrix_case::<$label, CountWeight>();
                matrix_case::<$label, BoolWeight>();
            };
        }
        weights!(u8);
        weights!(char);
        weights!(u64);
    }

    #[test]
    fn weighted_products_and_epsilon_acceptors_use_native_composition() {
        for (left, right, expected) in [(2.0, 3.0, 5.0), (-4.0, 7.0, 3.0)] {
            let first =
                export_native_wfst(&one_arc(Some(b'a'), Some(b'a'), TropicalWeight::new(left)))
                    .unwrap();
            let second =
                export_native_wfst(&one_arc(Some(b'a'), Some(b'a'), TropicalWeight::new(right)))
                    .unwrap();
            let mut handle = ptr::null_mut();
            assert_eq!(
                lling_wfst_acceptor_intersect(
                    first.as_raw(),
                    second.as_raw(),
                    &budget(),
                    &mut handle
                ),
                LlingLlangStatus::Ok
            );
            let graph: VectorWfst<u8, TropicalWeight> = result_graph(handle);
            assert_eq!(graph.transitions(graph.start())[0].weight.value(), expected);
        }

        let first =
            export_native_wfst(&one_arc(Some(b'a'), Some(b'a'), CountWeight::new(3))).unwrap();
        let second =
            export_native_wfst(&one_arc(Some(b'a'), Some(b'a'), CountWeight::new(4))).unwrap();
        let mut handle = ptr::null_mut();
        assert_eq!(
            lling_wfst_acceptor_intersect(first.as_raw(), second.as_raw(), &budget(), &mut handle),
            LlingLlangStatus::Ok
        );
        let graph: VectorWfst<u8, CountWeight> = result_graph(handle);
        assert_eq!(graph.transitions(graph.start())[0].weight.value(), 12);

        let epsilon = export_native_wfst(&one_arc::<u8, TropicalWeight>(
            None,
            None,
            TropicalWeight::new(2.0),
        ))
        .unwrap();
        let mut handle = ptr::null_mut();
        assert_eq!(
            lling_wfst_acceptor_intersect(
                epsilon.as_raw(),
                epsilon.as_raw(),
                &budget(),
                &mut handle
            ),
            LlingLlangStatus::Ok
        );
        let graph: VectorWfst<u8, TropicalWeight> = result_graph(handle);
        assert!(graph
            .transitions(graph.start())
            .iter()
            .all(|arc| arc.input.is_none() && arc.output.is_none()));
        assert!(!graph.transitions(graph.start()).is_empty());
        let accepting_paths: Vec<_> = graph
            .transitions(graph.start())
            .iter()
            .flat_map(|first| {
                graph
                    .transitions(first.to)
                    .iter()
                    .filter(|second| graph.is_final(second.to))
                    .map(|second| first.weight.times(&second.weight))
            })
            .collect();
        assert_eq!(accepting_paths, vec![TropicalWeight::new(4.0)]);
    }

    #[test]
    fn epsilon_interleavings_preserve_non_idempotent_path_multiplicity() {
        for left_epsilons in 0..=3 {
            for right_epsilons in 0..=3 {
                let left = export_native_wfst(&count_chain(left_epsilons, 2, 5)).unwrap();
                let right = export_native_wfst(&count_chain(right_epsilons, 3, 7)).unwrap();
                let mut handle = ptr::null_mut();
                assert_eq!(
                    lling_wfst_acceptor_intersect(
                        left.as_raw(),
                        right.as_raw(),
                        &budget(),
                        &mut handle
                    ),
                    LlingLlangStatus::Ok
                );
                let result: VectorWfst<u8, CountWeight> = result_graph(handle);
                assert_eq!(
                    count_paths(&result),
                    vec![(
                        vec![b'a'],
                        5 * 7 * 2u64.pow(left_epsilons as u32) * 3u64.pow(right_epsilons as u32)
                    )],
                    "left epsilon count={left_epsilons}, right epsilon count={right_epsilons}"
                );
            }
        }

        // Two independent left derivations and three right derivations of
        // epsilon must yield exactly six products, not fewer or more.
        let mut left = VectorWfst::<u8, CountWeight>::new();
        left.add_state();
        left.add_state();
        left.set_start(0);
        left.set_final(1, CountWeight::one());
        for weight in [2, 3] {
            left.add_transition(WeightedTransition::epsilon(0, 1, CountWeight::new(weight)));
        }
        let mut right = VectorWfst::<u8, CountWeight>::new();
        right.add_state();
        right.add_state();
        right.set_start(0);
        right.set_final(1, CountWeight::one());
        for weight in [4, 5, 6] {
            right.add_transition(WeightedTransition::epsilon(0, 1, CountWeight::new(weight)));
        }
        let left = export_native_wfst(&left).unwrap();
        let right = export_native_wfst(&right).unwrap();
        let mut handle = ptr::null_mut();
        assert_eq!(
            lling_wfst_acceptor_intersect(left.as_raw(), right.as_raw(), &budget(), &mut handle),
            LlingLlangStatus::Ok
        );
        let result: VectorWfst<u8, CountWeight> = result_graph(handle);
        let mut weights: Vec<_> = count_paths(&result)
            .into_iter()
            .map(|(word, weight)| {
                assert!(word.is_empty());
                weight
            })
            .collect();
        weights.sort_unstable();
        assert_eq!(weights, [8, 10, 12, 12, 15, 18]);

        for left_epsilons in 0..=3 {
            for right_epsilons in 0..=3 {
                let left = export_native_wfst(&probability_chain(left_epsilons, 0.5, 0.8)).unwrap();
                let right =
                    export_native_wfst(&probability_chain(right_epsilons, 0.25, 0.6)).unwrap();
                let mut handle = ptr::null_mut();
                assert_eq!(
                    lling_wfst_acceptor_intersect(
                        left.as_raw(),
                        right.as_raw(),
                        &budget(),
                        &mut handle
                    ),
                    LlingLlangStatus::Ok
                );
                let result: VectorWfst<u8, ProbabilityWeight> = result_graph(handle);
                let paths = probability_paths(&result);
                assert_eq!(paths.len(), 1);
                let expected = 0.8
                    * 0.6
                    * 0.5f64.powi(left_epsilons as i32)
                    * 0.25f64.powi(right_epsilons as i32);
                assert!((paths[0] - expected).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn domain_acceptor_and_budget_failures_preserve_output() {
        let acceptor =
            export_native_wfst(&one_arc(Some(b'a'), Some(b'a'), TropicalWeight::one())).unwrap();
        let non_acceptor =
            export_native_wfst(&one_arc(Some(b'a'), None, TropicalWeight::one())).unwrap();
        let other_domain =
            export_native_wfst(&one_arc(Some('a'), Some('a'), TropicalWeight::one())).unwrap();
        let sentinel = ptr::dangling_mut::<LlingWfst>();
        let mut handle = sentinel;
        assert_eq!(
            lling_wfst_acceptor_intersect(
                acceptor.as_raw(),
                non_acceptor.as_raw(),
                &budget(),
                &mut handle
            ),
            LlingLlangStatus::InvalidArgument
        );
        assert_eq!(handle, sentinel);
        assert_eq!(
            lling_wfst_acceptor_intersect(
                acceptor.as_raw(),
                other_domain.as_raw(),
                &budget(),
                &mut handle
            ),
            LlingLlangStatus::IncompatibleResource
        );
        assert_eq!(handle, sentinel);
        let mut malformed = budget();
        malformed.header.flags &= !LLING_BUDGET_WORK;
        assert_eq!(
            lling_wfst_acceptor_intersect(
                VtResource::NULL,
                VtResource::NULL,
                &malformed,
                &mut handle
            ),
            LlingLlangStatus::InvalidArgument
        );
        assert_eq!(handle, sentinel);
        for axis in 0..4 {
            let mut limited = budget();
            match axis {
                0 => limited.max_states = 1,
                1 => limited.max_arcs = 1,
                2 => limited.max_bytes = 1,
                _ => limited.max_work = 1,
            }
            assert_eq!(
                lling_wfst_acceptor_intersect(
                    acceptor.as_raw(),
                    acceptor.as_raw(),
                    &limited,
                    &mut handle
                ),
                LlingLlangStatus::LimitExceeded
            );
            assert_eq!(handle, sentinel);
        }
        assert_eq!(
            unsafe {
                lling_wfst_acceptor_intersect_refs(
                    ptr::null(),
                    &acceptor.as_raw(),
                    &budget(),
                    &mut handle,
                )
            },
            LlingLlangStatus::NullPointer
        );
        assert_eq!(handle, sentinel);
    }

    #[test]
    fn unencodable_count_product_fails_before_publishing() {
        let first = export_native_wfst(&one_arc(
            Some(b'a'),
            Some(b'a'),
            CountWeight::new(1u64 << 53),
        ))
        .unwrap();
        let second =
            export_native_wfst(&one_arc(Some(b'a'), Some(b'a'), CountWeight::new(2))).unwrap();
        let sentinel = ptr::dangling_mut::<LlingWfst>();
        let mut handle = sentinel;
        assert_eq!(
            lling_wfst_acceptor_intersect(first.as_raw(), second.as_raw(), &budget(), &mut handle),
            LlingLlangStatus::LimitExceeded
        );
        assert_eq!(handle, sentinel);
    }
}
