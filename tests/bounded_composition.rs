//! Independent shallow and interruption checks for bounded BFS composition.

use lling_llang::composition::{compose, materialize, BoundedComposition};
use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationError, OperationLimits, OperationOutcome,
    OperationPlan,
};
use lling_llang::wfst::{
    CancellationReason, CancellationToken, SourceSnapshot, VectorWfst, VectorWfstBuilder, Wfst,
};

const BINDING: [u8; 32] = [37; 32];
type ArcSignature = (Option<char>, Option<char>, u32, f64);
type StateSignature = (bool, Vec<ArcSignature>);

fn operands() -> (
    VectorWfst<char, TropicalWeight>,
    VectorWfst<char, TropicalWeight>,
) {
    let left = VectorWfstBuilder::new()
        .add_states(3)
        .start(0)
        .final_state(1, TropicalWeight::one())
        .final_state(2, TropicalWeight::one())
        .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
        .arc(0, Some('b'), Some('y'), 2, TropicalWeight::new(2.0))
        .arc(0, Some('c'), Some('x'), 1, TropicalWeight::new(3.0))
        .build();
    let right = VectorWfstBuilder::new()
        .add_states(3)
        .start(0)
        .final_state(1, TropicalWeight::one())
        .final_state(2, TropicalWeight::one())
        .arc(0, Some('x'), Some('p'), 1, TropicalWeight::new(0.5))
        .arc(0, Some('y'), Some('q'), 2, TropicalWeight::new(0.25))
        .build();
    (left, right)
}

fn signature(graph: &VectorWfst<char, TropicalWeight>) -> Vec<StateSignature> {
    (0..graph.num_states() as u32)
        .map(|state| {
            (
                graph.is_final(state),
                graph
                    .transitions(state)
                    .iter()
                    .map(|arc| (arc.input, arc.output, arc.to, arc.weight.value()))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn shallow_oracle_and_legacy_order_match() {
    let (left, right) = operands();
    let legacy = materialize(compose(left.clone(), right.clone()));
    let mut bounded = BoundedComposition::new(
        compose(left, right),
        BINDING,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let result = bounded
        .run(BINDING, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    assert_eq!(signature(&result), signature(&legacy));
    assert_eq!(result.start(), 0);
    assert_eq!(result.num_states(), 3);
    assert_eq!(
        signature(&result)[0].1,
        vec![
            (Some('a'), Some('p'), 1, 1.5),
            (Some('b'), Some('q'), 2, 2.25),
            (Some('c'), Some('p'), 1, 3.5),
        ]
    );
}

#[test]
fn state_limit_checkpoint_resume_and_partial_cache_rejection() {
    let (left, right) = operands();
    let mut bounded = BoundedComposition::new(
        compose(left.clone(), right.clone()),
        BINDING,
        OperationLimits {
            max_states: 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    )
    .unwrap();
    let outcome = bounded.run(BINDING, |_| 0).unwrap();
    let checkpoint = match outcome {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert_eq!(reason, IncompleteReason::StateLimit);
            assert_eq!(checkpoint.next_index, 1);
            assert_eq!(checkpoint.usage.states, 1);
            assert_eq!(partial.num_states(), 3);
            assert_eq!(partial.transitions(0).len(), 3);
            checkpoint
        }
        _ => panic!("state cap must be incomplete"),
    };
    assert!(matches!(
        bounded.run([38; 32], |_| 0),
        Err(OperationError::StaleSource)
    ));
    assert!(matches!(
        bounded.resume(
            lling_llang::wfst::operation::OperationCheckpoint {
                next_index: 0,
                ..checkpoint
            },
            OperationLimits::default(),
            CancellationToken::new(),
        ),
        Err(OperationError::StaleCheckpoint)
    ));

    let partial = bounded.run(BINDING, |_| 0).unwrap();
    let checkpoint = match &partial {
        OperationOutcome::Incomplete { checkpoint, .. } => *checkpoint,
        _ => panic!("expected partial"),
    };
    let dynamic_plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        BINDING,
        "lling.composition.materialize-bfs/v1",
    )
    .unwrap();
    let mut cache = CompleteResultCache::default();
    assert!(cache.insert(&dynamic_plan, partial).is_err());
    assert!(cache.is_empty());
    bounded
        .resume(
            checkpoint,
            OperationLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    let completed = bounded.run(BINDING, |_| 0).unwrap();
    let terminal = match &completed {
        OperationOutcome::Complete { checkpoint, .. } => *checkpoint,
        _ => panic!("resume must complete"),
    };
    assert_eq!(terminal.next_index, 3);
    assert_eq!(terminal.usage.states, 3);
    assert_eq!(
        signature(&completed.into_complete().unwrap()),
        signature(&materialize(compose(left, right)))
    );
}

#[test]
fn arc_cap_and_cancellation_do_not_commit_a_partial_state() {
    let (left, right) = operands();
    let mut capped = BoundedComposition::new(
        compose(left.clone(), right.clone()),
        BINDING,
        OperationLimits {
            max_arcs: 2,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    )
    .unwrap();
    match capped.run(BINDING, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert_eq!(reason, IncompleteReason::ArcLimit);
            assert_eq!(checkpoint.next_index, 0);
            assert_eq!(partial.num_states(), 1);
            assert!(partial.transitions(0).is_empty());
        }
        _ => panic!("arc cap must be incomplete"),
    }
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut cancelled = BoundedComposition::new(
        compose(left, right),
        BINDING,
        OperationLimits::default(),
        token,
    )
    .unwrap();
    match cancelled.run(BINDING, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            reason, checkpoint, ..
        } => {
            assert_eq!(reason, IncompleteReason::Cancelled);
            assert_eq!(checkpoint.next_index, 0);
        }
        _ => panic!("cancelled operation must be incomplete"),
    }
}

#[test]
fn cyclic_product_is_visited_once_and_complete_is_not_dynamic_cacheable() {
    let left = VectorWfstBuilder::new()
        .add_states(1)
        .start(0)
        .final_state(0, TropicalWeight::one())
        .arc(0, Some('a'), Some('x'), 0, TropicalWeight::one())
        .build();
    let right = VectorWfstBuilder::new()
        .add_states(1)
        .start(0)
        .final_state(0, TropicalWeight::one())
        .arc(0, Some('x'), Some('b'), 0, TropicalWeight::one())
        .build();
    let mut bounded = BoundedComposition::new(
        compose(left, right),
        BINDING,
        OperationLimits {
            max_states: 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    )
    .unwrap();
    let complete = bounded.run(BINDING, |_| 0).unwrap();
    match &complete {
        OperationOutcome::Complete { value, checkpoint } => {
            assert_eq!(value.num_states(), 1);
            assert_eq!(value.transitions(0).len(), 1);
            assert_eq!(value.transitions(0)[0].to, 0);
            assert_eq!(checkpoint.next_index, 1);
        }
        _ => panic!("finite cyclic product must complete"),
    }
    let plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        BINDING,
        "lling.composition.materialize-bfs/v1",
    )
    .unwrap();
    let mut cache = CompleteResultCache::default();
    assert!(cache.insert(&plan, complete).is_err());
}
