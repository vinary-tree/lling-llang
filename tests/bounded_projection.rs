//! Reachable input/output projection oracles and interruption checks.

use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationError, OperationLimits, OperationOutcome,
    OperationPlan,
};
use lling_llang::wfst::{
    BoundedInputProjection, BoundedOutputProjection, CancellationReason, CancellationToken,
    ProjectionError, SourceSnapshot, VectorWfst, VectorWfstBuilder, Wfst,
};

const BINDING: [u8; 32] = [61; 32];

fn source() -> VectorWfst<char, TropicalWeight> {
    VectorWfstBuilder::new()
        .add_states(5)
        .start(0)
        .final_state(2, TropicalWeight::new(4.0))
        .final_state(3, TropicalWeight::new(5.0))
        .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
        .arc(0, Some('b'), Some('y'), 2, TropicalWeight::new(2.0))
        .arc(1, None, Some('z'), 3, TropicalWeight::new(3.0))
        .arc(4, Some('q'), Some('q'), 4, TropicalWeight::one())
        .build()
}

#[test]
fn both_projection_oracles_preserve_order_weights_and_reachability() {
    let mut input = BoundedInputProjection::new(
        source(),
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let mut output = BoundedOutputProjection::new(
        source(),
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let input = input.run(BINDING, |_| 0).unwrap().into_complete().unwrap();
    let output = output.run(BINDING, |_| 0).unwrap().into_complete().unwrap();
    assert_eq!(input.num_states(), 4);
    assert_eq!(output.num_states(), 4);
    let input_arcs = input.transitions(0);
    let output_arcs = output.transitions(0);
    assert_eq!(
        (
            input_arcs[0].input,
            input_arcs[0].output,
            input_arcs[0].weight.value()
        ),
        (Some('a'), Some('a'), 1.0)
    );
    assert_eq!(
        (
            input_arcs[1].input,
            input_arcs[1].output,
            input_arcs[1].weight.value()
        ),
        (Some('b'), Some('b'), 2.0)
    );
    assert_eq!(
        (
            output_arcs[0].input,
            output_arcs[0].output,
            output_arcs[0].weight.value()
        ),
        (Some('x'), Some('x'), 1.0)
    );
    assert_eq!(
        (
            output_arcs[1].input,
            output_arcs[1].output,
            output_arcs[1].weight.value()
        ),
        (Some('y'), Some('y'), 2.0)
    );
    assert_eq!(
        (
            input.transitions(1)[0].input,
            input.transitions(1)[0].output
        ),
        (None, None)
    );
    assert_eq!(
        (
            output.transitions(1)[0].input,
            output.transitions(1)[0].output
        ),
        (Some('z'), Some('z'))
    );
    assert_eq!(input.final_weight(3).value(), 5.0);
    assert_eq!(output.final_weight(2).value(), 4.0);
}

#[test]
fn exact_resume_and_source_drift_for_both_directions() {
    let limits = OperationLimits {
        max_states: 1,
        ..OperationLimits::default()
    };
    let mut input =
        BoundedInputProjection::new(source(), BINDING, limits, CancellationToken::new()).unwrap();
    let mut output =
        BoundedOutputProjection::new(source(), BINDING, limits, CancellationToken::new()).unwrap();
    let input_cp = match input.run(BINDING, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert_eq!(reason, IncompleteReason::StateLimit);
            assert_eq!(partial.num_states(), 3);
            checkpoint
        }
        _ => panic!("input state cap must be incomplete"),
    };
    let output_cp = match output.run(BINDING, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            reason, checkpoint, ..
        } => {
            assert_eq!(reason, IncompleteReason::StateLimit);
            checkpoint
        }
        _ => panic!("output state cap must be incomplete"),
    };
    assert_ne!(
        input_cp.identity.plan_digest,
        output_cp.identity.plan_digest
    );
    assert!(matches!(
        input.run([62; 32], |_| 0),
        Err(ProjectionError::Operation(OperationError::StaleSource))
    ));
    assert!(matches!(
        input.resume(
            output_cp,
            OperationLimits::default(),
            CancellationToken::new()
        ),
        Err(ProjectionError::Operation(OperationError::StaleCheckpoint))
    ));
    input
        .resume(
            input_cp,
            OperationLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    output
        .resume(
            output_cp,
            OperationLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(
        input
            .run(BINDING, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .num_states(),
        4
    );
    assert_eq!(
        output
            .run(BINDING, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .num_states(),
        4
    );
}

#[test]
fn resource_caps_cancel_and_cache_rejection() {
    for (limits, expected) in [
        (
            OperationLimits {
                max_arcs: 1,
                ..OperationLimits::default()
            },
            IncompleteReason::ArcLimit,
        ),
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
        let mut machine =
            BoundedInputProjection::new(source(), BINDING, limits, CancellationToken::new())
                .unwrap();
        match machine.run(BINDING, |_| 0).unwrap() {
            OperationOutcome::Incomplete {
                partial,
                reason,
                checkpoint,
            } => {
                assert_eq!(reason, expected);
                assert_eq!(checkpoint.next_index, 0);
                assert!(partial.transitions(0).is_empty());
            }
            _ => panic!("resource cap must not claim completion"),
        }
    }
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut machine =
        BoundedOutputProjection::new(source(), BINDING, OperationLimits::default(), token).unwrap();
    let partial = machine.run(BINDING, |_| 0).unwrap();
    assert!(matches!(
        partial,
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            ..
        }
    ));
    let plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        BINDING,
        "lling.projection.output-reachable/v1",
    )
    .unwrap();
    let mut cache = CompleteResultCache::default();
    assert!(cache.insert(&plan, partial).is_err());
}

#[test]
fn malformed_reachable_target_fails_explicitly() {
    let malformed = VectorWfstBuilder::new()
        .add_states(1)
        .start(0)
        .arc(0, Some('a'), Some('x'), 7, TropicalWeight::one())
        .build();
    let mut machine = BoundedInputProjection::new(
        malformed,
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(BINDING, |_| 0),
        Err(ProjectionError::InvalidTarget {
            state: 0,
            arc_index: 0
        })
    ));
}
