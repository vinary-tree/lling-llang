//! Independent acceptor-intersection oracles and interruption checks.

use lling_llang::composition::{BoundedIntersection, IntersectionError};
use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::operation::{
    IncompleteReason, OperationError, OperationLimits, OperationOutcome,
};
use lling_llang::wfst::{
    CancellationReason, CancellationToken, VectorWfst, VectorWfstBuilder, Wfst,
};

const BINDING: [u8; 32] = [53; 32];

fn operands() -> (
    VectorWfst<char, TropicalWeight>,
    VectorWfst<char, TropicalWeight>,
) {
    let first = VectorWfstBuilder::new()
        .add_states(3)
        .start(0)
        .final_state(1, TropicalWeight::one())
        .final_state(2, TropicalWeight::one())
        .arc(0, Some('b'), Some('b'), 2, TropicalWeight::new(2.0))
        .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
        .build();
    let second = VectorWfstBuilder::new()
        .add_states(3)
        .start(0)
        .final_state(1, TropicalWeight::one())
        .final_state(2, TropicalWeight::one())
        .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(3.0))
        .arc(0, Some('b'), Some('b'), 2, TropicalWeight::new(4.0))
        .build();
    (first, second)
}

fn intersection(
    limits: OperationLimits,
    token: CancellationToken,
) -> BoundedIntersection<
    VectorWfst<char, TropicalWeight>,
    VectorWfst<char, TropicalWeight>,
    char,
    TropicalWeight,
> {
    let (first, second) = operands();
    BoundedIntersection::new(first, second, BINDING, limits, token).unwrap()
}

#[test]
fn shallow_oracle_preserves_branch_order_and_weights() {
    let mut machine = intersection(OperationLimits::default(), CancellationToken::new());
    let result = machine
        .run(BINDING, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    assert_eq!(result.num_states(), 3);
    let arcs = result.transitions(0);
    assert_eq!(arcs.len(), 2);
    assert_eq!(
        (arcs[0].input, arcs[0].output, arcs[0].weight.value()),
        (Some('b'), Some('b'), 6.0)
    );
    assert_eq!(
        (arcs[1].input, arcs[1].output, arcs[1].weight.value()),
        (Some('a'), Some('a'), 4.0)
    );
    assert!(result.is_final(arcs[0].to));
    assert!(result.is_final(arcs[1].to));
}

#[test]
fn limit_cancel_stale_binding_and_resume_are_typed() {
    let mut machine = intersection(
        OperationLimits {
            max_states: 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    let checkpoint = match machine.run(BINDING, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert_eq!(reason, IncompleteReason::StateLimit);
            assert_eq!(checkpoint.next_index, 1);
            assert_eq!(partial.num_states(), 3);
            checkpoint
        }
        _ => panic!("state cap must be incomplete"),
    };
    assert!(matches!(
        machine.run([54; 32], |_| 0),
        Err(IntersectionError::Operation(OperationError::StaleSource))
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
        Err(IntersectionError::Operation(
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
        .run(BINDING, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    assert_eq!(result.num_states(), 3);
    assert!(result.is_final(1));
    assert!(result.is_final(2));

    let mut capped = intersection(
        OperationLimits {
            max_arcs: 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    match capped.run(BINDING, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert_eq!(reason, IncompleteReason::ArcLimit);
            assert_eq!(checkpoint.next_index, 0);
            assert!(partial.transitions(0).is_empty());
        }
        _ => panic!("arc cap must be incomplete"),
    }
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut cancelled = intersection(OperationLimits::default(), token);
    assert!(matches!(
        cancelled.run(BINDING, |_| 0).unwrap(),
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            ..
        }
    ));
}

#[test]
fn invalid_even_unreachable_arc_is_rejected_before_result() {
    let (first, _) = operands();
    let second = VectorWfstBuilder::new()
        .add_states(2)
        .start(0)
        .arc(1, Some('a'), Some('b'), 1, TropicalWeight::one())
        .build();
    assert!(matches!(
        BoundedIntersection::new(
            first,
            second,
            BINDING,
            OperationLimits::default(),
            CancellationToken::new()
        ),
        Err(IntersectionError::NotAcceptor {
            operand: 2,
            state: 1,
            arc_index: 0
        })
    ));
    let (first, _) = operands();
    let invalid_target = VectorWfstBuilder::new()
        .add_states(1)
        .start(0)
        .arc(0, Some('a'), Some('a'), 7, TropicalWeight::one())
        .build();
    assert!(matches!(
        BoundedIntersection::new(
            first,
            invalid_target,
            BINDING,
            OperationLimits::default(),
            CancellationToken::new()
        ),
        Err(IntersectionError::InvalidTarget {
            operand: 2,
            state: 0,
            arc_index: 0
        })
    ));
}

#[test]
fn independent_epsilon_path_oracle() {
    let first = VectorWfstBuilder::new()
        .add_states(3)
        .start(0)
        .final_state(2, TropicalWeight::one())
        .arc(0, None, None, 1, TropicalWeight::new(1.0))
        .arc(1, Some('a'), Some('a'), 2, TropicalWeight::new(2.0))
        .build();
    let second = VectorWfstBuilder::new()
        .add_states(3)
        .start(0)
        .final_state(2, TropicalWeight::one())
        .arc(0, None, None, 1, TropicalWeight::new(3.0))
        .arc(1, Some('a'), Some('a'), 2, TropicalWeight::new(4.0))
        .build();
    let mut machine = BoundedIntersection::new(
        first,
        second,
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let result = machine
        .run(BINDING, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    let mut stack = vec![(result.start(), 0_u8, 0.0_f64, 0_u8)];
    let mut costs = Vec::new();
    while let Some((state, seen, cost, depth)) = stack.pop() {
        if result.is_final(state) && seen == 1 {
            costs.push(cost + result.final_weight(state).value());
        }
        if depth >= 5 {
            continue;
        }
        for arc in result.transitions(state) {
            assert_eq!(arc.input, arc.output);
            let next_seen = match arc.input {
                None => seen,
                Some('a') if seen == 0 => 1,
                _ => continue,
            };
            stack.push((arc.to, next_seen, cost + arc.weight.value(), depth + 1));
        }
    }
    assert_eq!(costs.iter().copied().reduce(f64::min), Some(10.0));
}
