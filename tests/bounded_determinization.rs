//! Independent shallow weighted-language and interruption checks.

use lling_llang::algorithms::{is_deterministic, BoundedDeterminization, BoundedDeterminizeError};
use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationError, OperationLimits, OperationOutcome,
    OperationPlan,
};
use lling_llang::wfst::{
    CancellationReason, CancellationToken, SourceSnapshot, VectorWfst, VectorWfstBuilder, Wfst,
};

const BINDING: [u8; 32] = [43; 32];

fn source() -> VectorWfst<char, TropicalWeight> {
    VectorWfstBuilder::new()
        .add_states(4)
        .start(0)
        .final_state(1, TropicalWeight::new(4.0))
        .final_state(2, TropicalWeight::new(5.0))
        .final_state(3, TropicalWeight::new(6.0))
        // Deliberately reverse label order: ordered subset construction emits a,b.
        .arc(0, Some('b'), Some('y'), 3, TropicalWeight::new(3.0))
        .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
        .arc(0, Some('a'), Some('x'), 2, TropicalWeight::new(2.0))
        .build()
}

fn new_bounded(
    limits: OperationLimits,
    token: CancellationToken,
) -> BoundedDeterminization<VectorWfst<char, TropicalWeight>, char, TropicalWeight> {
    BoundedDeterminization::new(source(), BINDING, limits, token).unwrap()
}

fn complete(
    mut machine: BoundedDeterminization<VectorWfst<char, TropicalWeight>, char, TropicalWeight>,
) -> VectorWfst<char, TropicalWeight> {
    machine
        .run(BINDING, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap()
}

#[test]
fn shallow_weighted_oracle_and_branch_order() {
    let result = complete(new_bounded(
        OperationLimits::default(),
        CancellationToken::new(),
    ));
    assert!(is_deterministic(&result));
    assert_eq!(result.num_states(), 3);
    let arcs = result.transitions(0);
    assert_eq!(arcs.len(), 2);
    assert_eq!(
        (
            arcs[0].input,
            arcs[0].output,
            arcs[0].to,
            arcs[0].weight.value()
        ),
        (Some('a'), Some('x'), 1, 1.0)
    );
    assert_eq!(
        (
            arcs[1].input,
            arcs[1].output,
            arcs[1].to,
            arcs[1].weight.value()
        ),
        (Some('b'), Some('y'), 2, 3.0)
    );
    assert_eq!(result.final_weight(1).value(), 4.0);
    assert_eq!(result.final_weight(2).value(), 6.0);
    // Independent source-path oracle: a costs min(1+4, 2+5)=5; b costs 3+6=9.
    assert_eq!(
        arcs[0].weight.value() + result.final_weight(arcs[0].to).value(),
        5.0
    );
    assert_eq!(
        arcs[1].weight.value() + result.final_weight(arcs[1].to).value(),
        9.0
    );
    let another = complete(new_bounded(
        OperationLimits::default(),
        CancellationToken::new(),
    ));
    assert_eq!(
        another
            .transitions(0)
            .iter()
            .map(|arc| arc.input)
            .collect::<Vec<_>>(),
        arcs.iter().map(|arc| arc.input).collect::<Vec<_>>()
    );
}

#[test]
fn state_cap_resume_and_stale_source() {
    let mut machine = new_bounded(
        OperationLimits {
            max_states: 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    let checkpoint = match machine.run(BINDING, |_| 0, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert_eq!(reason, IncompleteReason::StateLimit);
            assert_eq!(checkpoint.next_index, 1);
            assert_eq!(partial.num_states(), 3);
            assert_eq!(partial.transitions(0).len(), 2);
            checkpoint
        }
        _ => panic!("state cap must be incomplete"),
    };
    assert!(matches!(
        machine.run([44; 32], |_| 0, |_| 0),
        Err(BoundedDeterminizeError::Operation(
            OperationError::StaleSource
        ))
    ));
    assert!(matches!(
        machine.resume(
            lling_llang::wfst::operation::OperationCheckpoint {
                next_index: 0,
                ..checkpoint
            },
            OperationLimits::default(),
            CancellationToken::new()
        ),
        Err(BoundedDeterminizeError::Operation(
            OperationError::StaleCheckpoint
        ))
    ));
    machine
        .resume(
            checkpoint,
            OperationLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    let result = machine
        .run(BINDING, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    assert!(is_deterministic(&result));
    assert_eq!(result.final_weight(1).value(), 4.0);
    assert_eq!(result.final_weight(2).value(), 6.0);
}

#[test]
fn arc_cap_and_cancel_never_return_false_complete() {
    let mut capped = new_bounded(
        OperationLimits {
            max_arcs: 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    match capped.run(BINDING, |_| 0, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert_eq!(reason, IncompleteReason::ArcLimit);
            assert_eq!(checkpoint.next_index, 0);
            assert!(partial.transitions(0).is_empty());
            assert_eq!(partial.num_states(), 1);
        }
        _ => panic!("arc cap must be incomplete"),
    }
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut cancelled = new_bounded(OperationLimits::default(), token);
    match cancelled.run(BINDING, |_| 0, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            reason, checkpoint, ..
        } => {
            assert_eq!(reason, IncompleteReason::Cancelled);
            assert_eq!(checkpoint.next_index, 0);
        }
        _ => panic!("cancelled machine must be incomplete"),
    }
}

#[test]
fn unsupported_epsilon_and_conflicting_outputs_are_explicit_errors() {
    let epsilon = VectorWfstBuilder::new()
        .add_states(2)
        .start(0)
        .arc(0, None, Some('x'), 1, TropicalWeight::one())
        .build();
    let mut machine = BoundedDeterminization::new(
        epsilon,
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(BINDING, |_| 0, |_| 0),
        Err(BoundedDeterminizeError::InputEpsilon)
    ));

    let conflicting = VectorWfstBuilder::new()
        .add_states(3)
        .start(0)
        .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
        .arc(0, Some('a'), Some('y'), 2, TropicalWeight::one())
        .build();
    let mut machine = BoundedDeterminization::new(
        conflicting,
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(BINDING, |_| 0, |_| 0),
        Err(BoundedDeterminizeError::ConflictingOutput)
    ));
}

#[test]
fn every_resource_axis_is_incomplete_and_dynamic_results_are_uncacheable() {
    for (limits, expected) in [
        (
            OperationLimits {
                max_work: 1,
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
        let mut machine = new_bounded(limits, CancellationToken::new());
        match machine.run(BINDING, |_| 0, |_| 0).unwrap() {
            OperationOutcome::Incomplete {
                partial,
                reason,
                checkpoint,
            } => {
                assert_eq!(reason, expected);
                assert_eq!(checkpoint.next_index, 0);
                assert_eq!(partial.num_states(), 1);
                assert!(partial.transitions(0).is_empty());
            }
            _ => panic!("resource limit must not claim completion"),
        }
    }
    let mut machine = new_bounded(OperationLimits::default(), CancellationToken::new());
    let complete = machine.run(BINDING, |_| 0, |_| 0).unwrap();
    let plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        BINDING,
        "lling.determinize.ordered-subsets/v1",
    )
    .unwrap();
    let mut cache = CompleteResultCache::default();
    assert!(cache.insert(&plan, complete).is_err());
    assert!(cache.is_empty());
}
