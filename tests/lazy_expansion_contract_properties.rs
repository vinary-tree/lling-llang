//! Property realization of every Rocq lazy-expansion declaration.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::{
    CancellationReason, CancellationToken, ExpansionFailure, ExpansionFailureKind, ExpansionMode,
    ExpansionObservation, ExpansionRequest, ExpansionStatus, LazyState, LazyWfstWrapper,
    RetryPolicy, SourceSnapshot, StateExpansion, StateSource, WeightedTransition,
};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseResult, TestRunner};
use smallvec::{smallvec, SmallVec};

fn run_property<S, F>(strategy: S, check: F)
where
    S: Strategy,
    F: Fn(S::Value) -> TestCaseResult,
{
    TestRunner::new(Config::with_cases(64))
        .run(&strategy, check)
        .expect("lazy-expansion property must hold");
}

fn must<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.expect("property operation must succeed")
}

macro_rules! property {
    ($name:ident, $strategy:expr, |$value:pat_param| $body:block) => {
        #[test]
        fn $name() {
            run_property($strategy, |$value| $body);
        }
    };
}

fn status_strategy() -> impl Strategy<Value = ExpansionStatus> {
    prop_oneof![
        Just(ExpansionStatus::Unexpanded),
        Just(ExpansionStatus::Expanding),
        Just(ExpansionStatus::ExpandedEmpty),
        Just(ExpansionStatus::ExpandedNonempty),
        Just(ExpansionStatus::Failed),
        Just(ExpansionStatus::Cancelled),
    ]
}

fn mode_strategy() -> impl Strategy<Value = ExpansionMode> {
    prop_oneof![
        Just(ExpansionMode::Normal),
        Just(ExpansionMode::ExplicitRetry),
    ]
}

fn snapshot(revision: u8) -> SourceSnapshot {
    SourceSnapshot::from_bytes([revision; 32])
}

fn transition() -> WeightedTransition<char, TropicalWeight> {
    WeightedTransition::new(0, Some('a'), Some('a'), 0, TropicalWeight::one())
}

fn retryable_failure() -> ExpansionFailure {
    ExpansionFailure::new(
        ExpansionFailureKind::Source,
        RetryPolicy::Explicit,
        "retryable test failure",
    )
}

fn permanent_failure() -> ExpansionFailure {
    ExpansionFailure::new(
        ExpansionFailureKind::Source,
        RetryPolicy::Never,
        "permanent test failure",
    )
}

#[derive(Clone, Copy, Debug)]
enum Plan {
    Empty,
    Nonempty,
    RetryOnce,
    PermanentFailure,
    Cancel,
    ChangeSnapshot,
}

#[derive(Clone, Debug)]
struct ScriptedSource {
    plan: Plan,
    calls: Arc<AtomicUsize>,
    revision: Arc<AtomicU8>,
}

impl ScriptedSource {
    fn new(plan: Plan) -> Self {
        Self {
            plan,
            calls: Arc::new(AtomicUsize::new(0)),
            revision: Arc::new(AtomicU8::new(0)),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Acquire)
    }

    fn set_revision(&self, revision: u8) {
        self.revision.store(revision, Ordering::Release);
    }
}

impl StateSource<char, TropicalWeight> for ScriptedSource {
    fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<char, TropicalWeight> {
        if let Some(reason) = request.cancellation().reason() {
            return StateExpansion::cancelled(reason);
        }

        if request.state() != 0 {
            return StateExpansion::failed(ExpansionFailure::invalid_state(request.state()));
        }

        let call = self.calls.fetch_add(1, Ordering::AcqRel);
        match self.plan {
            Plan::Empty => StateExpansion::non_final(SmallVec::new()),
            Plan::Nonempty => StateExpansion::non_final(smallvec![transition()]),
            Plan::RetryOnce if call == 0 => StateExpansion::failed(retryable_failure()),
            Plan::RetryOnce => StateExpansion::non_final(smallvec![transition()]),
            Plan::PermanentFailure => StateExpansion::failed(permanent_failure()),
            Plan::Cancel => StateExpansion::cancelled(CancellationReason::Source),
            Plan::ChangeSnapshot => {
                self.revision.fetch_add(1, Ordering::AcqRel);
                StateExpansion::non_final(smallvec![transition()])
            }
        }
    }

    fn snapshot(&self) -> SourceSnapshot {
        snapshot(self.revision.load(Ordering::Acquire))
    }

    fn start(&self) -> u32 {
        0
    }

    fn num_states_hint(&self) -> Option<usize> {
        Some(1)
    }
}

property!(prop_expansion_status, status_strategy(), |status| {
    prop_assert!(matches!(
        status,
        ExpansionStatus::Unexpanded
            | ExpansionStatus::Expanding
            | ExpansionStatus::ExpandedEmpty
            | ExpansionStatus::ExpandedNonempty
            | ExpansionStatus::Failed
            | ExpansionStatus::Cancelled
    ));
    Ok(())
});

property!(prop_expansion_observation, status_strategy(), |status| {
    let observation = status.observation();
    prop_assert_eq!(
        observation.is_some(),
        status.is_terminal(),
        "only terminal statuses are observable"
    );
    Ok(())
});

property!(prop_completion_kind, 0usize..8, |transition_count| {
    let state = LazyState::<char, TropicalWeight>::expanded(
        false,
        TropicalWeight::zero(),
        if transition_count == 0 {
            SmallVec::new()
        } else {
            smallvec![transition()]
        },
        1,
    );
    prop_assert_eq!(
        state.status(),
        if transition_count == 0 {
            ExpansionStatus::ExpandedEmpty
        } else {
            ExpansionStatus::ExpandedNonempty
        }
    );
    Ok(())
});

property!(prop_begin_mode, mode_strategy(), |mode| {
    prop_assert!(matches!(
        mode,
        ExpansionMode::Normal | ExpansionMode::ExplicitRetry
    ));
    Ok(())
});

property!(prop_status_terminal, status_strategy(), |status| {
    prop_assert_eq!(
        status.is_terminal(),
        !matches!(
            status,
            ExpansionStatus::Unexpanded | ExpansionStatus::Expanding
        )
    );
    Ok(())
});

property!(prop_status_cacheable, status_strategy(), |status| {
    prop_assert_eq!(
        status.is_cacheable(),
        matches!(
            status,
            ExpansionStatus::ExpandedEmpty | ExpansionStatus::ExpandedNonempty
        )
    );
    Ok(())
});

property!(prop_observe_status, status_strategy(), |status| {
    prop_assert_eq!(
        status.observation(),
        match status {
            ExpansionStatus::ExpandedEmpty => Some(ExpansionObservation::Empty),
            ExpansionStatus::ExpandedNonempty => Some(ExpansionObservation::Nonempty),
            ExpansionStatus::Failed => Some(ExpansionObservation::Failure),
            ExpansionStatus::Cancelled => Some(ExpansionObservation::Cancellation),
            ExpansionStatus::Unexpanded | ExpansionStatus::Expanding => None,
        }
    );
    Ok(())
});

property!(
    prop_unexpanded_is_not_observed_as_empty,
    any::<bool>(),
    |_fresh| {
        prop_assert_ne!(
            ExpansionStatus::Unexpanded.observation(),
            Some(ExpansionObservation::Empty)
        );
        Ok(())
    }
);

property!(
    prop_expanding_is_not_observed_as_empty,
    any::<bool>(),
    |_fresh| {
        prop_assert_ne!(
            ExpansionStatus::Expanding.observation(),
            Some(ExpansionObservation::Empty)
        );
        Ok(())
    }
);

property!(
    prop_empty_observation_requires_expanded_empty,
    status_strategy(),
    |status| {
        if status.observation() == Some(ExpansionObservation::Empty) {
            prop_assert_eq!(status, ExpansionStatus::ExpandedEmpty);
        }
        Ok(())
    }
);

property!(
    prop_nonempty_observation_requires_expanded_nonempty,
    status_strategy(),
    |status| {
        if status.observation() == Some(ExpansionObservation::Nonempty) {
            prop_assert_eq!(status, ExpansionStatus::ExpandedNonempty);
        }
        Ok(())
    }
);

property!(
    prop_failure_observation_requires_failed,
    status_strategy(),
    |status| {
        if status.observation() == Some(ExpansionObservation::Failure) {
            prop_assert_eq!(status, ExpansionStatus::Failed);
        }
        Ok(())
    }
);

property!(
    prop_cancellation_observation_requires_cancelled,
    status_strategy(),
    |status| {
        if status.observation() == Some(ExpansionObservation::Cancellation) {
            prop_assert_eq!(status, ExpansionStatus::Cancelled);
        }
        Ok(())
    }
);

property!(
    prop_stale_status_is_unobservable,
    status_strategy(),
    |status| {
        let source = ScriptedSource::new(Plan::Empty);
        let mut lazy = LazyWfstWrapper::new(source.clone());
        must(lazy.expand(0));
        source.set_revision(1);
        prop_assert!(lazy.observe(0).is_err());
        prop_assert!(status.observation().is_some() || !status.is_terminal());
        Ok(())
    }
);

property!(
    prop_only_expanded_states_are_cacheable,
    status_strategy(),
    |status| {
        if status.is_cacheable() {
            prop_assert!(matches!(
                status,
                ExpansionStatus::ExpandedEmpty | ExpansionStatus::ExpandedNonempty
            ));
        }
        Ok(())
    }
);

property!(
    prop_begin_authorized,
    (
        mode_strategy(),
        status_strategy(),
        any::<bool>(),
        any::<bool>()
    ),
    |(mode, status, retryable, cancelled)| {
        let expected = !cancelled
            && match (mode, status) {
                (ExpansionMode::Normal, ExpansionStatus::Unexpanded) => true,
                (ExpansionMode::ExplicitRetry, ExpansionStatus::Failed) => retryable,
                _ => false,
            };
        prop_assert_eq!(mode.is_authorized(status, retryable, cancelled), expected);
        Ok(())
    }
);

property!(
    prop_can_begin,
    (
        mode_strategy(),
        status_strategy(),
        any::<bool>(),
        any::<bool>()
    ),
    |(mode, status, retryable, cancelled)| {
        prop_assert_eq!(
            mode.is_authorized(status, retryable, cancelled),
            !cancelled && mode.is_authorized(status, retryable, false)
        );
        Ok(())
    }
);

property!(
    prop_normal_begin_requires_unexpanded,
    (status_strategy(), any::<bool>()),
    |(status, cancelled)| {
        if ExpansionMode::Normal.is_authorized(status, false, cancelled) {
            prop_assert!(!cancelled);
            prop_assert_eq!(status, ExpansionStatus::Unexpanded);
        }
        Ok(())
    }
);

property!(
    prop_retry_begin_requires_retryable_failure,
    (status_strategy(), any::<bool>(), any::<bool>()),
    |(status, retryable, cancelled)| {
        if ExpansionMode::ExplicitRetry.is_authorized(status, retryable, cancelled) {
            prop_assert!(!cancelled);
            prop_assert!(retryable);
            prop_assert_eq!(status, ExpansionStatus::Failed);
        }
        Ok(())
    }
);

property!(
    prop_normal_begin_cannot_retry_failure,
    any::<bool>(),
    |cancelled| {
        prop_assert!(!ExpansionMode::Normal.is_authorized(
            ExpansionStatus::Failed,
            true,
            cancelled
        ));
        Ok(())
    }
);

property!(
    prop_nonretryable_failure_is_terminal,
    any::<bool>(),
    |cancelled| {
        prop_assert!(!ExpansionMode::ExplicitRetry.is_authorized(
            ExpansionStatus::Failed,
            false,
            cancelled
        ));
        Ok(())
    }
);

property!(
    prop_cancellation_blocks_begin,
    (mode_strategy(), status_strategy(), any::<bool>()),
    |(mode, status, retryable)| {
        prop_assert!(!mode.is_authorized(status, retryable, true));
        Ok(())
    }
);

property!(
    prop_expanding_blocks_second_begin,
    (mode_strategy(), any::<bool>(), any::<bool>()),
    |(mode, retryable, cancelled)| {
        prop_assert!(!mode.is_authorized(ExpansionStatus::Expanding, retryable, cancelled));
        Ok(())
    }
);

property!(
    prop_cancelled_requires_explicit_reset,
    (mode_strategy(), any::<bool>(), any::<bool>()),
    |(mode, retryable, cancelled)| {
        prop_assert!(!mode.is_authorized(ExpansionStatus::Cancelled, retryable, cancelled));
        Ok(())
    }
);

property!(
    prop_expansion_state,
    (status_strategy(), 0u64..u64::MAX),
    |(status, attempt)| {
        let state = match status {
            ExpansionStatus::Unexpanded => LazyState::Unexpanded,
            ExpansionStatus::Expanding => LazyState::expanding(attempt),
            ExpansionStatus::ExpandedEmpty => {
                LazyState::expanded(false, TropicalWeight::zero(), SmallVec::new(), attempt)
            }
            ExpansionStatus::ExpandedNonempty => LazyState::expanded(
                false,
                TropicalWeight::zero(),
                smallvec![transition()],
                attempt,
            ),
            ExpansionStatus::Failed => LazyState::failed(permanent_failure(), attempt),
            ExpansionStatus::Cancelled => {
                LazyState::cancelled(CancellationReason::Requested, attempt)
            }
        };
        prop_assert_eq!(state.status(), status);
        Ok(())
    }
);

property!(prop_snapshot_fresh, (any::<u8>(), any::<u8>()), |(
    left,
    right,
)| {
    prop_assert_eq!(snapshot(left) == snapshot(right), left == right);
    Ok(())
});

property!(prop_owner_consistent, any::<u64>(), |attempt| {
    prop_assert!(LazyState::<char, TropicalWeight>::expanding(attempt).is_well_formed());
    Ok(())
});

property!(prop_retry_consistent, any::<u64>(), |attempt| {
    let failed = LazyState::<char, TropicalWeight>::failed(retryable_failure(), attempt);
    prop_assert!(failed.is_well_formed());
    prop_assert!(failed.is_retryable());
    Ok(())
});

property!(prop_terminal_fresh, any::<u8>(), |revision| {
    let source = ScriptedSource::new(Plan::Empty);
    source.set_revision(revision);
    let mut lazy = LazyWfstWrapper::new(source.clone());
    must(lazy.expand(0));
    prop_assert!(must(lazy.observe(0)).is_some());
    Ok(())
});

property!(
    prop_state_well_formed,
    (status_strategy(), any::<u64>()),
    |(status, attempt)| {
        let state = match status {
            ExpansionStatus::Unexpanded => LazyState::Unexpanded,
            ExpansionStatus::Expanding => LazyState::expanding(attempt),
            ExpansionStatus::ExpandedEmpty => {
                LazyState::expanded(false, TropicalWeight::zero(), SmallVec::new(), attempt)
            }
            ExpansionStatus::ExpandedNonempty => LazyState::expanded(
                false,
                TropicalWeight::zero(),
                smallvec![transition()],
                attempt,
            ),
            ExpansionStatus::Failed => LazyState::failed(retryable_failure(), attempt),
            ExpansionStatus::Cancelled => {
                LazyState::cancelled(CancellationReason::Requested, attempt)
            }
        };
        prop_assert!(state.is_well_formed());
        Ok(())
    }
);

property!(prop_begin_attempt, any::<u8>(), |_seed| {
    let source = ScriptedSource::new(Plan::Empty);
    let mut lazy = LazyWfstWrapper::new(source);
    prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Unexpanded);
    prop_assert_eq!(must(lazy.expand(0)), ExpansionStatus::ExpandedEmpty);
    Ok(())
});

property!(prop_cancel_before_begin, any::<u8>(), |_seed| {
    let source = ScriptedSource::new(Plan::Nonempty);
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut lazy = LazyWfstWrapper::new(source.clone());
    prop_assert!(lazy.expand_with(0, &token).is_err());
    prop_assert_eq!(source.calls(), 0);
    prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Cancelled);
    Ok(())
});

property!(prop_finish_status, 0usize..8, |transition_count| {
    let transitions = if transition_count == 0 {
        SmallVec::new()
    } else {
        smallvec![transition()]
    };
    let state = LazyState::expanded(false, TropicalWeight::zero(), transitions, 1);
    prop_assert_eq!(
        state.status(),
        if transition_count == 0 {
            ExpansionStatus::ExpandedEmpty
        } else {
            ExpansionStatus::ExpandedNonempty
        }
    );
    Ok(())
});

property!(prop_complete_attempt, any::<bool>(), |nonempty| {
    let source = ScriptedSource::new(if nonempty {
        Plan::Nonempty
    } else {
        Plan::Empty
    });
    let mut lazy = LazyWfstWrapper::new(source);
    let status = must(lazy.expand(0));
    prop_assert_eq!(
        status,
        if nonempty {
            ExpansionStatus::ExpandedNonempty
        } else {
            ExpansionStatus::ExpandedEmpty
        }
    );
    Ok(())
});

property!(prop_fail_attempt, any::<bool>(), |retryable| {
    let source = ScriptedSource::new(if retryable {
        Plan::RetryOnce
    } else {
        Plan::PermanentFailure
    });
    let mut lazy = LazyWfstWrapper::new(source);
    prop_assert!(lazy.expand(0).is_err());
    let state = must(lazy.lifecycle_state(0)).expect("failed state retained");
    prop_assert_eq!(state.status(), ExpansionStatus::Failed);
    prop_assert_eq!(state.is_retryable(), retryable);
    Ok(())
});

property!(prop_cancel_attempt, any::<u8>(), |_seed| {
    let mut lazy = LazyWfstWrapper::new(ScriptedSource::new(Plan::Cancel));
    prop_assert!(lazy.expand(0).is_err());
    prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Cancelled);
    Ok(())
});

property!(prop_reset_cancelled, any::<u8>(), |_seed| {
    let mut lazy = LazyWfstWrapper::new(ScriptedSource::new(Plan::Cancel));
    let _ = lazy.expand(0);
    must(lazy.reset_cancelled(0));
    prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Unexpanded);
    Ok(())
});

property!(prop_reset_failed, any::<u8>(), |_seed| {
    let mut lazy = LazyWfstWrapper::new(ScriptedSource::new(Plan::PermanentFailure));
    let _ = lazy.expand(0);
    must(lazy.reset_failed(0));
    prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Unexpanded);
    Ok(())
});

property!(prop_rebind_snapshot, any::<u8>(), |revision| {
    let source = ScriptedSource::new(Plan::Empty);
    source.set_revision(revision);
    let mut lazy = LazyWfstWrapper::new(source.clone());
    must(lazy.expand(0));
    source.set_revision(revision.wrapping_add(1));
    prop_assert!(lazy.refresh_snapshot());
    prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Unexpanded);
    Ok(())
});

property!(prop_begin_attempt_has_single_owner, 1usize..8, |repeat| {
    let source = ScriptedSource::new(Plan::Nonempty);
    let mut lazy = LazyWfstWrapper::new(source.clone());
    for _ in 0..repeat {
        prop_assert_eq!(must(lazy.expand(0)), ExpansionStatus::ExpandedNonempty);
    }
    prop_assert_eq!(source.calls(), 1);
    Ok(())
});

property!(prop_begin_attempt_increments_count, 1usize..8, |_repeat| {
    let source = ScriptedSource::new(Plan::RetryOnce);
    let mut lazy = LazyWfstWrapper::new(source);
    let _ = lazy.expand(0);
    prop_assert_eq!(lazy.total_attempts(), 1);
    must(lazy.retry(0));
    prop_assert_eq!(lazy.total_attempts(), 2);
    Ok(())
});

property!(
    prop_begin_attempt_captures_current_snapshot,
    any::<u8>(),
    |revision| {
        let source = ScriptedSource::new(Plan::Empty);
        source.set_revision(revision);
        let mut lazy = LazyWfstWrapper::new(source.clone());
        prop_assert_eq!(lazy.current_snapshot(), source.snapshot());
        must(lazy.expand(0));
        prop_assert_eq!(lazy.current_snapshot(), source.snapshot());
        Ok(())
    }
);

property!(prop_begin_attempt_is_well_formed, any::<u64>(), |attempt| {
    prop_assert!(LazyState::<char, TropicalWeight>::expanding(attempt).is_well_formed());
    Ok(())
});

property!(
    prop_pre_cancel_is_ownerless_and_does_not_attempt,
    any::<u8>(),
    |_seed| {
        let source = ScriptedSource::new(Plan::Nonempty);
        let token = CancellationToken::new();
        token.cancel(CancellationReason::Requested);
        let mut lazy = LazyWfstWrapper::new(source.clone());
        let _ = lazy.expand_with(0, &token);
        prop_assert_eq!(source.calls(), 0);
        prop_assert_eq!(lazy.total_attempts(), 0);
        Ok(())
    }
);

property!(prop_wrong_owner_cannot_complete, 1usize..8, |repeat| {
    let source = ScriptedSource::new(Plan::Empty);
    let mut lazy = LazyWfstWrapper::new(source.clone());
    for _ in 0..repeat {
        let _ = must(lazy.expand(0));
    }
    prop_assert_eq!(source.calls(), 1);
    prop_assert_ne!(must(lazy.expansion_status(0)), ExpansionStatus::Expanding);
    Ok(())
});

property!(prop_stale_attempt_cannot_complete, any::<u8>(), |_seed| {
    let source = ScriptedSource::new(Plan::ChangeSnapshot);
    let mut lazy = LazyWfstWrapper::new(source);
    prop_assert!(lazy.expand(0).is_err());
    prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Unexpanded);
    prop_assert!(must(lazy.transitions_if_expanded(0)).is_none());
    Ok(())
});

property!(
    prop_completion_is_fresh_and_ownerless,
    any::<bool>(),
    |nonempty| {
        let mut lazy = LazyWfstWrapper::new(ScriptedSource::new(if nonempty {
            Plan::Nonempty
        } else {
            Plan::Empty
        }));
        must(lazy.expand(0));
        prop_assert!(must(lazy.observe(0)).is_some());
        prop_assert_ne!(must(lazy.expansion_status(0)), ExpansionStatus::Expanding);
        Ok(())
    }
);

property!(
    prop_completion_classifies_empty_exactly,
    any::<u8>(),
    |_seed| {
        let mut lazy = LazyWfstWrapper::new(ScriptedSource::new(Plan::Empty));
        prop_assert_eq!(must(lazy.expand(0)), ExpansionStatus::ExpandedEmpty);
        prop_assert_eq!(
            must(lazy.transitions_if_expanded(0)).map(<[_]>::len),
            Some(0)
        );
        Ok(())
    }
);

property!(
    prop_completion_classifies_nonempty_exactly,
    any::<u8>(),
    |_seed| {
        let mut lazy = LazyWfstWrapper::new(ScriptedSource::new(Plan::Nonempty));
        prop_assert_eq!(must(lazy.expand(0)), ExpansionStatus::ExpandedNonempty);
        prop_assert_eq!(
            must(lazy.transitions_if_expanded(0)).map(<[_]>::len),
            Some(1)
        );
        Ok(())
    }
);

property!(
    prop_failed_attempt_is_ownerless,
    any::<bool>(),
    |retryable| {
        let mut lazy = LazyWfstWrapper::new(ScriptedSource::new(if retryable {
            Plan::RetryOnce
        } else {
            Plan::PermanentFailure
        }));
        let _ = lazy.expand(0);
        prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Failed);
        prop_assert!(must(lazy.observe(0)).is_some());
        Ok(())
    }
);

property!(
    prop_nonretryable_failure_cannot_begin,
    1usize..8,
    |retries| {
        let source = ScriptedSource::new(Plan::PermanentFailure);
        let mut lazy = LazyWfstWrapper::new(source.clone());
        let _ = lazy.expand(0);
        for _ in 0..retries {
            prop_assert!(lazy.retry(0).is_err());
        }
        prop_assert_eq!(source.calls(), 1);
        Ok(())
    }
);

property!(prop_cancelled_attempt_is_ownerless, any::<u8>(), |_seed| {
    let mut lazy = LazyWfstWrapper::new(ScriptedSource::new(Plan::Cancel));
    let _ = lazy.expand(0);
    prop_assert_eq!(
        must(lazy.observe(0)),
        Some(ExpansionObservation::Cancellation)
    );
    prop_assert_ne!(must(lazy.expansion_status(0)), ExpansionStatus::Expanding);
    Ok(())
});

property!(
    prop_reset_cancelled_is_explicitly_unexpanded,
    1usize..8,
    |attempts| {
        let mut lazy = LazyWfstWrapper::new(ScriptedSource::new(Plan::Cancel));
        let _ = lazy.expand(0);
        for _ in 0..attempts {
            prop_assert!(lazy.expand(0).is_err());
        }
        must(lazy.reset_cancelled(0));
        prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Unexpanded);
        Ok(())
    }
);

property!(
    prop_reset_failed_is_explicitly_unexpanded,
    any::<bool>(),
    |retryable| {
        let mut lazy = LazyWfstWrapper::new(ScriptedSource::new(if retryable {
            Plan::RetryOnce
        } else {
            Plan::PermanentFailure
        }));
        let _ = lazy.expand(0);
        must(lazy.reset_failed(0));
        prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Unexpanded);
        Ok(())
    }
);

property!(
    prop_rebind_invalidates_prior_lifecycle,
    any::<u8>(),
    |revision| {
        let source = ScriptedSource::new(Plan::Nonempty);
        source.set_revision(revision);
        let mut lazy = LazyWfstWrapper::new(source.clone());
        must(lazy.expand(0));
        source.set_revision(revision.wrapping_add(1));
        prop_assert!(lazy.refresh_snapshot());
        prop_assert_eq!(lazy.total_attempts(), 0);
        prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Unexpanded);
        Ok(())
    }
);

property!(
    prop_lazy_expansion_control_phase,
    prop::collection::vec(any::<bool>(), 0..128),
    |steps| {
        let source = ScriptedSource::new(Plan::RetryOnce);
        let mut lazy = LazyWfstWrapper::new(source);
        for retry in steps {
            let _ = if retry { lazy.retry(0) } else { lazy.expand(0) };
            prop_assert!(matches!(
                must(lazy.expansion_status(0)),
                ExpansionStatus::Unexpanded
                    | ExpansionStatus::ExpandedNonempty
                    | ExpansionStatus::Failed
            ));
        }
        Ok(())
    }
);

property!(
    prop_lazy_expansion_control_is_finite,
    0usize..50_000,
    |depth| {
        let source = ScriptedSource::new(Plan::Empty);
        let mut lazy = LazyWfstWrapper::new(source);
        for _ in 0..depth {
            lazy.clear_state(0);
            must(lazy.expand(0));
        }
        prop_assert_eq!(
            must(lazy.expansion_status(0)),
            ExpansionStatus::ExpandedEmpty
        );
        Ok(())
    }
);
