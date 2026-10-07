//! Contract tests for bounded lazy WFST operation sessions.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::operation::{
    ApproximationBound, CompleteResultCache, IncompleteReason, OperationCheckpoint, OperationError,
    OperationLimits, OperationOutcome, OperationPlan, OperationSession,
};
use lling_llang::wfst::{
    CancellationReason, CancellationToken, ExpansionRequest, ExpansionStatus, LazyState,
    LazyWfstWrapper, SourceSnapshot, StateExpansion, StateId, StateSource, WeightedTransition,
};
use smallvec::SmallVec;

#[derive(Clone, Debug)]
struct ChainSource {
    calls: Arc<AtomicUsize>,
    snapshot: SourceSnapshot,
}

impl StateSource<char, TropicalWeight> for ChainSource {
    fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<char, TropicalWeight> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let state = request.state();
        let mut transitions = SmallVec::new();
        if state < 2 {
            transitions.push(WeightedTransition::new(
                state,
                Some('a'),
                Some('a'),
                state + 1,
                TropicalWeight::one(),
            ));
        }
        if state == 2 {
            StateExpansion::final_state(TropicalWeight::one(), transitions)
        } else {
            StateExpansion::non_final(transitions)
        }
    }

    fn snapshot(&self) -> SourceSnapshot {
        self.snapshot
    }

    fn start(&self) -> StateId {
        0
    }

    fn num_states_hint(&self) -> Option<usize> {
        Some(3)
    }
}

fn source(calls: Arc<AtomicUsize>) -> ChainSource {
    ChainSource {
        calls,
        snapshot: SourceSnapshot::from_bytes([7; 32]),
    }
}

fn plan_for(states: Vec<StateId>) -> OperationPlan {
    OperationPlan::new(
        SourceSnapshot::from_bytes([7; 32]),
        [9; 32],
        "ordered-lazy-expansion/v1",
        states,
    )
    .expect("plan")
}

fn meter(state: &LazyState<char, TropicalWeight>) -> u64 {
    match state {
        LazyState::Expanded { transitions, .. } => 16 + transitions.len() as u64 * 32,
        _ => panic!("meter must see a completed state"),
    }
}

#[test]
fn exact_resume_matches_one_shot_and_checkpoint_identity_is_strict() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut wrapper = LazyWfstWrapper::new(source(calls.clone()));
    let plan = plan_for(vec![0, 1, 2]);
    let token = CancellationToken::new();
    let mut session = OperationSession::new(
        plan.clone(),
        OperationLimits {
            max_states: 1,
            ..OperationLimits::default()
        },
        token,
    );
    let incomplete = session
        .run_lazy(&mut wrapper, [9; 32], meter)
        .expect("limit outcome");
    let (checkpoint, receipt) = match incomplete {
        OperationOutcome::Incomplete {
            partial,
            reason,
            checkpoint,
        } => {
            assert_eq!(partial, [0]);
            assert_eq!(reason, IncompleteReason::StateLimit);
            let receipt = OperationOutcome::<Vec<StateId>>::Incomplete {
                partial: partial.clone(),
                reason,
                checkpoint,
            }
            .canonical_receipt_bytes();
            (checkpoint, receipt)
        }
        other => panic!("expected incomplete, got {other:?}"),
    };
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(checkpoint.next_index, 1);
    assert_eq!(checkpoint.usage.states, 1);
    let wire = checkpoint.canonical_bytes();
    assert_eq!(wire, checkpoint.canonical_bytes());
    assert_eq!(
        OperationCheckpoint::from_canonical_bytes(&wire).expect("roundtrip"),
        checkpoint
    );
    let same = OperationOutcome::<Vec<StateId>>::Incomplete {
        partial: vec![0],
        reason: IncompleteReason::StateLimit,
        checkpoint,
    };
    assert_eq!(same.canonical_receipt_bytes(), receipt);
    let mut damaged = wire.clone();
    damaged[20] ^= 1;
    assert!(matches!(
        OperationCheckpoint::from_canonical_bytes(&damaged),
        Err(OperationError::CorruptCheckpoint)
    ));
    let mut trailing = wire;
    trailing.push(0);
    assert!(matches!(
        OperationCheckpoint::from_canonical_bytes(&trailing),
        Err(OperationError::CorruptCheckpoint)
    ));

    let stale_order = plan_for(vec![1, 0, 2]);
    assert!(matches!(
        OperationSession::resume(
            stale_order,
            OperationLimits::default(),
            CancellationToken::new(),
            checkpoint
        ),
        Err(OperationError::StaleCheckpoint)
    ));
    let stale_binding = OperationPlan::new(
        SourceSnapshot::from_bytes([7; 32]),
        [8; 32],
        "ordered-lazy-expansion/v1",
        vec![0, 1, 2],
    )
    .expect("changed source binding");
    assert!(matches!(
        OperationSession::resume(
            stale_binding,
            OperationLimits::default(),
            CancellationToken::new(),
            checkpoint
        ),
        Err(OperationError::StaleCheckpoint)
    ));

    let mut resumed = OperationSession::resume(
        plan.clone(),
        OperationLimits::default(),
        CancellationToken::new(),
        checkpoint,
    )
    .expect("resume exact plan");
    let resumed = resumed
        .run_lazy(&mut wrapper, [9; 32], meter)
        .expect("resume run");
    let (resumed_value, resumed_checkpoint) = match resumed {
        OperationOutcome::Complete { value, checkpoint } => (value, checkpoint),
        other => panic!("expected complete, got {other:?}"),
    };
    assert_eq!(resumed_value, [0, 1, 2]);
    assert_eq!(resumed_checkpoint.usage.states, 3);
    assert_eq!(resumed_checkpoint.usage.arcs, 2);
    assert_eq!(resumed_checkpoint.usage.work, 5);

    let mut fresh_wrapper = LazyWfstWrapper::new(source(Arc::new(AtomicUsize::new(0))));
    let mut one_shot =
        OperationSession::new(plan, OperationLimits::default(), CancellationToken::new());
    let one_shot = one_shot
        .run_lazy(&mut fresh_wrapper, [9; 32], meter)
        .expect("one shot");
    let (value, checkpoint) = match one_shot {
        OperationOutcome::Complete { value, checkpoint } => (value, checkpoint),
        other => panic!("expected complete, got {other:?}"),
    };
    assert_eq!(resumed_value, value);
    assert_eq!(resumed_checkpoint.identity, checkpoint.identity);
    assert_eq!(resumed_checkpoint.usage, checkpoint.usage);
    assert_eq!(resumed_checkpoint.next_index, checkpoint.next_index);
}

#[test]
fn rejected_expansion_is_not_retained_and_only_exact_complete_enters_cache() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut wrapper = LazyWfstWrapper::new(source(calls.clone()));
    let plan = plan_for(vec![0]);
    let mut session = OperationSession::new(
        plan.clone(),
        OperationLimits {
            max_arcs: 0,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    let incomplete = session
        .run_lazy(&mut wrapper, [9; 32], meter)
        .expect("arc cap");
    assert!(matches!(
        &incomplete,
        OperationOutcome::Incomplete {
            reason: IncompleteReason::ArcLimit,
            ..
        }
    ));
    assert_eq!(
        wrapper.expansion_status(0).expect("status"),
        ExpansionStatus::Unexpanded
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut cache = CompleteResultCache::default();
    assert!(cache.insert(&plan, incomplete).is_err());
    assert!(cache.is_empty());

    let mut resumed = OperationSession::resume(
        plan.clone(),
        OperationLimits::default(),
        CancellationToken::new(),
        session.checkpoint(),
    )
    .expect("resume");
    let complete = resumed
        .run_lazy(&mut wrapper, [9; 32], meter)
        .expect("complete");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(cache.insert(&plan, complete).is_ok());
    assert_eq!(cache.get(&plan.identity.plan_digest), Some(&vec![0]));

    let approximation = OperationOutcome::Approximate {
        value: vec![0],
        bound: ApproximationBound {
            max_error_microunits: 1,
            method_digest: [4; 32],
        },
        checkpoint: resumed.checkpoint(),
    };
    assert!(cache.insert(&plan, approximation).is_err());
    assert_eq!(cache.len(), 1);
    let mislabeled_partial = OperationOutcome::Complete {
        value: vec![99],
        checkpoint: session.checkpoint(),
    };
    assert!(cache.insert(&plan, mislabeled_partial).is_err());
    assert_eq!(cache.get(&plan.identity.plan_digest), Some(&vec![0]));
}

#[test]
fn cancellation_time_heap_and_source_drift_are_not_complete() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut wrapper = LazyWfstWrapper::new(source(calls.clone()));
    let plan = plan_for(vec![0]);
    let cancelled = CancellationToken::new();
    assert!(cancelled.cancel(CancellationReason::Requested));
    let mut session = OperationSession::new(plan.clone(), OperationLimits::default(), cancelled);
    assert!(matches!(
        session
            .run_lazy(&mut wrapper, [9; 32], meter)
            .expect("cancelled"),
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            ..
        }
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut resumed = OperationSession::resume(
        plan.clone(),
        OperationLimits::default(),
        CancellationToken::new(),
        session.checkpoint(),
    )
    .expect("resume cancellation");
    assert!(matches!(
        resumed
            .run_lazy(&mut wrapper, [9; 32], meter)
            .expect("resume"),
        OperationOutcome::Complete { .. }
    ));

    let mut time_limited = OperationSession::new(
        plan.clone(),
        OperationLimits {
            max_elapsed_ns: 0,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    assert!(matches!(
        time_limited
            .run_lazy(&mut wrapper, [9; 32], meter)
            .expect("time cap"),
        OperationOutcome::Incomplete {
            reason: IncompleteReason::TimeLimit,
            ..
        }
    ));

    let mut heap_wrapper = LazyWfstWrapper::new(source(Arc::new(AtomicUsize::new(0))));
    let mut heap_limited = OperationSession::new(
        plan.clone(),
        OperationLimits {
            max_heap_bytes: 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    assert!(matches!(
        heap_limited
            .run_lazy(&mut heap_wrapper, [9; 32], meter)
            .expect("heap cap"),
        OperationOutcome::Incomplete {
            reason: IncompleteReason::HeapLimit,
            ..
        }
    ));
    assert_eq!(
        heap_wrapper.expansion_status(0).expect("status"),
        ExpansionStatus::Unexpanded
    );

    let mut work_wrapper = LazyWfstWrapper::new(source(Arc::new(AtomicUsize::new(0))));
    let mut work_limited = OperationSession::new(
        plan.clone(),
        OperationLimits {
            max_work: 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    assert!(matches!(
        work_limited
            .run_lazy(&mut work_wrapper, [9; 32], meter)
            .expect("work cap"),
        OperationOutcome::Incomplete {
            reason: IncompleteReason::WorkLimit,
            ..
        }
    ));
    assert_eq!(
        work_wrapper.expansion_status(0).expect("status"),
        ExpansionStatus::Unexpanded
    );

    let mut wrong_binding = OperationSession::new(
        plan.clone(),
        OperationLimits::default(),
        CancellationToken::new(),
    );
    assert!(matches!(
        wrong_binding.run_lazy(&mut wrapper, [8; 32], meter),
        Err(OperationError::StaleSource)
    ));

    let mut drifted = LazyWfstWrapper::new(ChainSource {
        calls: Arc::new(AtomicUsize::new(0)),
        snapshot: SourceSnapshot::from_bytes([8; 32]),
    });
    let mut drift_session =
        OperationSession::new(plan, OperationLimits::default(), CancellationToken::new());
    assert!(matches!(
        drift_session.run_lazy(&mut drifted, [9; 32], meter),
        Err(OperationError::StaleSource)
    ));
}
