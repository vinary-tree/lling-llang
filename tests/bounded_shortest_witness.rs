//! Independent shortest-path oracle, provenance, and interruption controls.

use lling_llang::algorithms::{BoundedShortestWitness, ShortestWitnessError};
use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationError, OperationLimits, OperationOutcome,
    OperationPlan,
};
use lling_llang::wfst::{
    CancellationReason, CancellationToken, SourceSnapshot, VectorWfst, VectorWfstBuilder,
};

const BINDING: [u8; 32] = [83; 32];

fn graph() -> VectorWfst<char, TropicalWeight> {
    VectorWfstBuilder::new()
        .add_states(3)
        .start(0)
        .final_state(1, TropicalWeight::new(4.0))
        .final_state(2, TropicalWeight::new(3.0))
        .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
        .arc(0, Some('b'), Some('y'), 2, TropicalWeight::new(2.0))
        .build()
}

fn machine(
    limits: OperationLimits,
    token: CancellationToken,
) -> BoundedShortestWitness<VectorWfst<char, TropicalWeight>, char> {
    BoundedShortestWitness::new(graph(), BINDING, limits, token).unwrap()
}

#[test]
fn hand_oracle_equal_cost_tie_uses_first_source_arc_and_full_provenance() {
    let mut machine = machine(OperationLimits::default(), CancellationToken::new());
    let outcome = machine.run(BINDING, |_| 0).unwrap();
    let witness = outcome.into_complete().unwrap().unwrap();
    // Both branches cost five, so first-discovered arc 0 must win.
    assert_eq!(witness.total_weight.value(), 5.0);
    assert_eq!(witness.final_state, 1);
    assert_eq!(witness.final_weight.value(), 4.0);
    assert_eq!(witness.steps.len(), 1);
    let step = &witness.steps[0];
    assert_eq!(
        (
            step.from,
            step.arc_index,
            step.input,
            step.output,
            step.to,
            step.weight.value()
        ),
        (0, 0, Some('a'), Some('x'), 1, 1.0)
    );
}

#[test]
fn state_cap_and_witness_materialization_cap_resume_exactly() {
    let mut capped = machine(
        OperationLimits {
            max_states: 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    let checkpoint = match capped.run(BINDING, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert!(partial.is_none());
            assert_eq!(reason, IncompleteReason::StateLimit);
            assert_eq!(checkpoint.next_index, 1);
            checkpoint
        }
        _ => panic!("state cap must be incomplete"),
    };
    assert!(matches!(
        capped.run([84; 32], |_| 0),
        Err(ShortestWitnessError::Operation(OperationError::StaleSource))
    ));
    assert!(matches!(
        capped.resume(
            lling_llang::wfst::operation::OperationCheckpoint {
                next_index: 0,
                ..checkpoint
            },
            OperationLimits::default(),
            CancellationToken::new()
        ),
        Err(ShortestWitnessError::Operation(
            OperationError::StaleCheckpoint
        ))
    ));
    capped
        .resume(
            checkpoint,
            OperationLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    let resumed = capped
        .run(BINDING, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap()
        .unwrap();
    assert_eq!(resumed.total_weight.value(), 5.0);

    // Search itself costs seven work units; witness copying costs one more.
    let mut late = machine(
        OperationLimits {
            max_work: 7,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    let checkpoint = match late.run(BINDING, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert!(partial.is_none());
            assert_eq!(reason, IncompleteReason::WorkLimit);
            assert_eq!(checkpoint.next_index, 3);
            checkpoint
        }
        _ => panic!("reconstruction cap must not become false complete"),
    };
    late.resume(
        checkpoint,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(
        late.run(BINDING, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .unwrap()
            .total_weight
            .value(),
        5.0
    );
}

#[test]
fn every_preflight_limit_cancel_and_cache_exclusion_are_typed() {
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
                max_arcs: 1,
                ..OperationLimits::default()
            },
            IncompleteReason::ArcLimit,
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
        let mut machine = machine(limits, CancellationToken::new());
        match machine.run(BINDING, |_| 0).unwrap() {
            OperationOutcome::Incomplete {
                partial,
                reason,
                checkpoint,
            } => {
                assert!(partial.is_none());
                assert_eq!(reason, expected);
                assert_eq!(checkpoint.next_index, 0);
            }
            _ => panic!("cap cannot prove exactness"),
        }
    }
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut cancelled = machine(OperationLimits::default(), token);
    let partial = cancelled.run(BINDING, |_| 0).unwrap();
    assert!(matches!(
        &partial,
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            ..
        }
    ));
    let plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        BINDING,
        "lling.shortest.nonnegative-tropical-witness/v1",
    )
    .unwrap();
    let mut cache = CompleteResultCache::default();
    assert!(cache.insert(&plan, partial).is_err());
}

#[test]
fn negative_weight_and_malformed_target_fail_closed() {
    let negative = VectorWfstBuilder::new()
        .add_states(2)
        .start(0)
        .final_state(1, TropicalWeight::one())
        .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(-1.0))
        .build();
    let mut machine = BoundedShortestWitness::new(
        negative,
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(BINDING, |_| 0),
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
    let mut machine = BoundedShortestWitness::new(
        malformed,
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(BINDING, |_| 0),
        Err(ShortestWitnessError::InvalidTarget {
            state: 0,
            arc_index: 0
        })
    ));
}

#[test]
fn zero_cycle_terminates_and_empty_language_is_exact_none() {
    let cyclic = VectorWfstBuilder::new()
        .add_states(2)
        .start(0)
        .final_state(1, TropicalWeight::new(2.0))
        .arc(0, Some('z'), Some('z'), 0, TropicalWeight::one())
        .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
        .build();
    let mut machine = BoundedShortestWitness::new(
        cyclic,
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let witness = machine
        .run(BINDING, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap()
        .unwrap();
    assert_eq!(witness.total_weight.value(), 3.0);
    assert_eq!(witness.steps.len(), 1);
    let empty = VectorWfstBuilder::<char, TropicalWeight>::new()
        .add_states(1)
        .start(0)
        .build();
    let mut machine = BoundedShortestWitness::new(
        empty,
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(machine
        .run(BINDING, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap()
        .is_none());
}
