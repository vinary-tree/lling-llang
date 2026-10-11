//! Independent small-graph oracle and exact/incomplete boundary checks.

use lling_llang::algorithms::{BoundedTopK, ShortestWitnessError, TopKLimits};
use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationCheckpoint, OperationError, OperationLimits,
    OperationOutcome, OperationPlan,
};
use lling_llang::wfst::{
    CancellationReason, CancellationToken, SourceSnapshot, VectorWfst, VectorWfstBuilder, Wfst,
};

const BINDING: [u8; 32] = [97; 32];
type SourceArc = (u32, usize, u32);
type OraclePath = (f64, Vec<SourceArc>);

fn graph() -> VectorWfst<char, TropicalWeight> {
    VectorWfstBuilder::new()
        .add_states(4)
        .start(0)
        .final_state(1, TropicalWeight::new(2.0))
        .final_state(2, TropicalWeight::new(2.0))
        .final_state(3, TropicalWeight::new(1.0))
        .arc(0, Some('a'), Some('A'), 1, TropicalWeight::new(1.0))
        .arc(0, Some('b'), Some('B'), 2, TropicalWeight::new(1.0))
        .arc(0, Some('c'), Some('C'), 3, TropicalWeight::new(2.0))
        .arc(1, Some('d'), Some('D'), 3, TropicalWeight::new(1.0))
        .build()
}

fn machine(
    limits: OperationLimits,
    paths: TopKLimits,
) -> BoundedTopK<VectorWfst<char, TropicalWeight>, char> {
    BoundedTopK::new(graph(), BINDING, limits, paths, CancellationToken::new()).unwrap()
}

// Deliberately independent recursive oracle, restricted to this acyclic shallow fixture.
fn oracle(source: &VectorWfst<char, TropicalWeight>) -> Vec<OraclePath> {
    fn walk(
        source: &VectorWfst<char, TropicalWeight>,
        state: u32,
        cost: f64,
        path: &mut Vec<SourceArc>,
        out: &mut Vec<OraclePath>,
    ) {
        if source.is_final(state) {
            out.push((cost + source.final_weight(state).value(), path.clone()));
        }
        for (index, arc) in source.transitions(state).iter().enumerate() {
            path.push((state, index, arc.to));
            walk(source, arc.to, cost + arc.weight.value(), path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(source, source.start(), 0.0, &mut Vec::new(), &mut out);
    // The fixture's deterministic equal-cost policy is depth then source-arc order.
    out.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then_with(|| a.1.len().cmp(&b.1.len()))
            .then_with(|| a.1.cmp(&b.1))
    });
    out
}

#[test]
fn oracle_ranking_and_full_source_provenance() {
    let mut search = machine(OperationLimits::default(), TopKLimits::default());
    let paths = search.run(BINDING, |_| 0).unwrap().into_complete().unwrap();
    let source = graph();
    let expected = oracle(&source);
    assert_eq!(paths.len(), expected.len());
    for (path, (cost, arc_path)) in paths.iter().zip(expected) {
        assert_eq!(path.total_weight.value(), cost);
        assert_eq!(
            path.steps
                .iter()
                .map(|s| (s.from, s.arc_index, s.to))
                .collect::<Vec<_>>(),
            arc_path
        );
        assert_eq!(path.final_state, arc_path.last().unwrap().2);
        assert_eq!(
            path.final_weight.value(),
            source.final_weight(path.final_state).value()
        );
        for step in &path.steps {
            let arc = &source.transitions(step.from)[step.arc_index];
            assert_eq!(step.input, arc.input);
            assert_eq!(step.output, arc.output);
            assert_eq!(step.weight, arc.weight);
        }
    }
}

#[test]
fn path_cap_is_incomplete_and_resume_preserves_exact_prefix() {
    let mut search = machine(
        OperationLimits::default(),
        TopKLimits {
            max_paths: 1,
            ..TopKLimits::default()
        },
    );
    let checkpoint = match search.run(BINDING, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert_eq!(reason, IncompleteReason::PathLimit);
            assert_eq!(partial.len(), 1);
            assert!(partial[0].steps[0].input == Some('a'));
            checkpoint
        }
        _ => panic!("remaining frontier must prevent complete"),
    };
    assert!(matches!(
        search.resume(
            OperationCheckpoint {
                next_index: checkpoint.next_index + 1,
                ..checkpoint
            },
            OperationLimits::default(),
            TopKLimits::default(),
            CancellationToken::new()
        ),
        Err(ShortestWitnessError::Operation(
            OperationError::StaleCheckpoint
        ))
    ));
    search
        .resume(
            checkpoint,
            OperationLimits::default(),
            TopKLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    let resumed = search.run(BINDING, |_| 0).unwrap().into_complete().unwrap();
    assert_eq!(resumed.len(), 4);
    assert_eq!(resumed[0].steps[0].input, Some('a'));
}

#[test]
fn depth_and_frontier_caps_retain_unexplored_work() {
    for (cap, expected) in [
        (
            TopKLimits {
                max_depth: 0,
                ..TopKLimits::default()
            },
            IncompleteReason::DepthLimit,
        ),
        (
            TopKLimits {
                max_frontier: 1,
                ..TopKLimits::default()
            },
            IncompleteReason::FrontierLimit,
        ),
    ] {
        let mut search = machine(OperationLimits::default(), cap);
        let checkpoint = match search.run(BINDING, |_| 0).unwrap() {
            OperationOutcome::Incomplete {
                partial,
                reason,
                checkpoint,
            } => {
                assert!(partial.is_empty());
                assert_eq!(reason, expected);
                assert_eq!(checkpoint.next_index, 0);
                checkpoint
            }
            _ => panic!("cap cannot prove exhaustion"),
        };
        search
            .resume(
                checkpoint,
                OperationLimits::default(),
                TopKLimits::default(),
                CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(
            search
                .run(BINDING, |_| 0)
                .unwrap()
                .into_complete()
                .unwrap()
                .len(),
            4
        );
    }
}

#[test]
fn shared_limits_cancel_binding_and_cache_exclusion() {
    for (limits, expected) in [
        (
            OperationLimits {
                max_states: 0,
                ..OperationLimits::default()
            },
            IncompleteReason::StateLimit,
        ),
        (
            OperationLimits {
                max_arcs: 0,
                ..OperationLimits::default()
            },
            IncompleteReason::ArcLimit,
        ),
        (
            OperationLimits {
                max_work: 0,
                ..OperationLimits::default()
            },
            IncompleteReason::WorkLimit,
        ),
        (
            OperationLimits {
                max_heap_bytes: 0,
                ..OperationLimits::default()
            },
            IncompleteReason::HeapLimit,
        ),
        (
            OperationLimits {
                max_elapsed_ns: 0,
                ..OperationLimits::default()
            },
            IncompleteReason::TimeLimit,
        ),
    ] {
        let mut search = machine(limits, TopKLimits::default());
        assert!(
            matches!(search.run(BINDING, |_| 0).unwrap(), OperationOutcome::Incomplete { reason, .. } if reason == expected)
        );
    }
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut search = BoundedTopK::new(
        graph(),
        BINDING,
        OperationLimits::default(),
        TopKLimits::default(),
        token,
    )
    .unwrap();
    let outcome = search.run(BINDING, |_| 0).unwrap();
    assert!(matches!(
        &outcome,
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            ..
        }
    ));
    let plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        BINDING,
        "lling.topk.nonnegative-tropical-witness/v1",
    )
    .unwrap();
    let mut cache = CompleteResultCache::default();
    assert!(cache.insert(&plan, outcome).is_err());
    assert!(matches!(
        search.run([98; 32], |_| 0),
        Err(ShortestWitnessError::Operation(OperationError::StaleSource))
    ));
}

#[test]
fn zero_cost_cycle_is_fair_but_finite_path_cap_never_claims_exhaustion() {
    let cyclic = VectorWfstBuilder::new()
        .add_states(2)
        .start(0)
        .final_state(1, TropicalWeight::one())
        .arc(0, Some('z'), Some('z'), 0, TropicalWeight::one())
        .arc(0, Some('a'), Some('a'), 1, TropicalWeight::one())
        .build();
    let mut search = BoundedTopK::new(
        cyclic,
        BINDING,
        OperationLimits::default(),
        TopKLimits {
            max_paths: 3,
            ..TopKLimits::default()
        },
        CancellationToken::new(),
    )
    .unwrap();
    match search.run(BINDING, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial, reason, ..
        } => {
            assert_eq!(reason, IncompleteReason::PathLimit);
            assert_eq!(
                partial.iter().map(|p| p.steps.len()).collect::<Vec<_>>(),
                vec![1, 2, 3]
            );
            assert!(partial.iter().all(|p| p.total_weight.value() == 0.0));
        }
        _ => panic!("infinite language must not complete"),
    }
}

#[test]
fn malformed_and_empty_sources_fail_closed_or_complete_exactly() {
    let negative = VectorWfstBuilder::new()
        .add_states(2)
        .start(0)
        .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(-1.0))
        .build();
    let mut search = BoundedTopK::new(
        negative,
        BINDING,
        OperationLimits::default(),
        TopKLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        search.run(BINDING, |_| 0),
        Err(ShortestWitnessError::UnsupportedWeight {
            state: 0,
            arc_index: Some(0)
        })
    ));
    let malformed = VectorWfstBuilder::new()
        .add_states(1)
        .start(0)
        .arc(0, Some('a'), Some('a'), 7, TropicalWeight::one())
        .build();
    let mut search = BoundedTopK::new(
        malformed,
        BINDING,
        OperationLimits::default(),
        TopKLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        search.run(BINDING, |_| 0),
        Err(ShortestWitnessError::InvalidTarget {
            state: 0,
            arc_index: 0
        })
    ));
    let empty: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let mut search = BoundedTopK::new(
        empty,
        BINDING,
        OperationLimits::default(),
        TopKLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(search
        .run(BINDING, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap()
        .is_empty());
}
