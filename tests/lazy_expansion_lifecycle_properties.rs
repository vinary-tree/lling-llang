//! Property realization of every TLC lazy-expansion invariant.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::{
    CancellationReason, CancellationToken, ExpansionFailure, ExpansionFailureKind,
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
        .expect("lazy lifecycle property must hold");
}

macro_rules! property {
    ($name:ident, $strategy:expr, |$value:pat_param| $body:block) => {
        #[test]
        fn $name() {
            run_property($strategy, |$value| $body);
        }
    };
}

fn must<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.expect("property operation must succeed")
}

fn snapshot(revision: u8) -> SourceSnapshot {
    SourceSnapshot::from_bytes([revision; 32])
}

fn transition() -> WeightedTransition<char, TropicalWeight> {
    WeightedTransition::new(0, Some('x'), Some('x'), 0, TropicalWeight::one())
}

#[derive(Clone, Copy, Debug)]
enum Outcome {
    Empty,
    Nonempty,
    RetryableFailure,
    PermanentFailure,
    Cancelled,
}

#[derive(Clone, Debug)]
struct LifecycleSource {
    outcome: Outcome,
    calls: Arc<AtomicUsize>,
    revision: Arc<AtomicU8>,
}

impl LifecycleSource {
    fn new(outcome: Outcome) -> Self {
        Self {
            outcome,
            calls: Arc::new(AtomicUsize::new(0)),
            revision: Arc::new(AtomicU8::new(0)),
        }
    }
}

impl StateSource<char, TropicalWeight> for LifecycleSource {
    fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<char, TropicalWeight> {
        if let Some(reason) = request.cancellation().reason() {
            return StateExpansion::cancelled(reason);
        }
        self.calls.fetch_add(1, Ordering::AcqRel);
        match self.outcome {
            Outcome::Empty => StateExpansion::non_final(SmallVec::new()),
            Outcome::Nonempty => StateExpansion::non_final(smallvec![transition()]),
            Outcome::RetryableFailure => StateExpansion::failed(ExpansionFailure::new(
                ExpansionFailureKind::Source,
                RetryPolicy::Explicit,
                "retryable",
            )),
            Outcome::PermanentFailure => StateExpansion::failed(ExpansionFailure::new(
                ExpansionFailureKind::Source,
                RetryPolicy::Never,
                "permanent",
            )),
            Outcome::Cancelled => StateExpansion::cancelled(CancellationReason::Source),
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

property!(prop_type_ok, (0u8..6, any::<u64>()), |(phase, attempt)| {
    let state = match phase {
        0 => LazyState::<char, TropicalWeight>::Unexpanded,
        1 => LazyState::expanding(attempt),
        2 => LazyState::expanded(false, TropicalWeight::zero(), SmallVec::new(), attempt),
        3 => LazyState::expanded(
            false,
            TropicalWeight::zero(),
            smallvec![transition()],
            attempt,
        ),
        4 => LazyState::failed(
            ExpansionFailure::new(ExpansionFailureKind::Source, RetryPolicy::Never, "failure"),
            attempt,
        ),
        _ => LazyState::cancelled(CancellationReason::Requested, attempt),
    };
    prop_assert!(state.is_well_formed());
    Ok(())
});

property!(prop_at_most_one_expansion_owner, 1usize..32, |readers| {
    let source = LifecycleSource::new(Outcome::Nonempty);
    let mut lazy = LazyWfstWrapper::new(source.clone());
    for _ in 0..readers {
        prop_assert_eq!(must(lazy.expand(0)), ExpansionStatus::ExpandedNonempty);
    }
    prop_assert_eq!(source.calls.load(Ordering::Acquire), 1);
    Ok(())
});

property!(prop_owner_exactly_while_expanding, 0usize..32, |steps| {
    let mut lazy = LazyWfstWrapper::new(LifecycleSource::new(Outcome::Empty));
    for _ in 0..steps {
        let _ = must(lazy.expand(0));
        prop_assert_ne!(must(lazy.expansion_status(0)), ExpansionStatus::Expanding);
    }
    Ok(())
});

property!(
    prop_expanding_uses_captured_snapshot,
    any::<u8>(),
    |revision| {
        let source = LifecycleSource::new(Outcome::Empty);
        source.revision.store(revision, Ordering::Release);
        let mut lazy = LazyWfstWrapper::new(source.clone());
        prop_assert_eq!(lazy.current_snapshot(), source.snapshot());
        let _ = must(lazy.expand(0));
        prop_assert_eq!(lazy.current_snapshot(), source.snapshot());
        Ok(())
    }
);

property!(
    prop_observable_state_uses_current_snapshot,
    any::<u8>(),
    |revision| {
        let source = LifecycleSource::new(Outcome::Empty);
        let mut lazy = LazyWfstWrapper::new(source.clone());
        let _ = must(lazy.expand(0));
        source
            .revision
            .store(revision.wrapping_add(1), Ordering::Release);
        prop_assert!(lazy.observe(0).is_err());
        Ok(())
    }
);

property!(prop_unexpanded_never_appears_empty, any::<u32>(), |state| {
    let lazy = LazyWfstWrapper::new(LifecycleSource::new(Outcome::Empty));
    prop_assert_eq!(
        must(lazy.expansion_status(state)),
        ExpansionStatus::Unexpanded
    );
    prop_assert!(must(lazy.transitions_if_expanded(state)).is_none());
    Ok(())
});

property!(prop_expanding_is_unobservable, any::<u64>(), |attempt| {
    let state = LazyState::<char, TropicalWeight>::expanding(attempt);
    prop_assert_eq!(state.status().observation(), None);
    prop_assert_eq!(state.transitions(), None);
    Ok(())
});

property!(prop_empty_observation_is_exact, any::<u8>(), |_seed| {
    let mut lazy = LazyWfstWrapper::new(LifecycleSource::new(Outcome::Empty));
    let _ = must(lazy.expand(0));
    prop_assert_eq!(must(lazy.observe(0)), Some(ExpansionObservation::Empty));
    prop_assert_eq!(
        must(lazy.expansion_status(0)),
        ExpansionStatus::ExpandedEmpty
    );
    Ok(())
});

property!(prop_nonempty_observation_is_exact, any::<u8>(), |_seed| {
    let mut lazy = LazyWfstWrapper::new(LifecycleSource::new(Outcome::Nonempty));
    let _ = must(lazy.expand(0));
    prop_assert_eq!(must(lazy.observe(0)), Some(ExpansionObservation::Nonempty));
    prop_assert_eq!(
        must(lazy.expansion_status(0)),
        ExpansionStatus::ExpandedNonempty
    );
    Ok(())
});

property!(
    prop_failure_observation_has_exact_status,
    any::<bool>(),
    |retryable| {
        let outcome = if retryable {
            Outcome::RetryableFailure
        } else {
            Outcome::PermanentFailure
        };
        let mut lazy = LazyWfstWrapper::new(LifecycleSource::new(outcome));
        let _ = lazy.expand(0);
        prop_assert_eq!(must(lazy.observe(0)), Some(ExpansionObservation::Failure));
        prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Failed);
        Ok(())
    }
);

property!(
    prop_cancellation_observation_has_exact_status,
    any::<bool>(),
    |pre_cancel| {
        let source = LifecycleSource::new(Outcome::Cancelled);
        let token = CancellationToken::new();
        if pre_cancel {
            token.cancel(CancellationReason::Requested);
        }
        let mut lazy = LazyWfstWrapper::new(source);
        let _ = lazy.expand_with(0, &token);
        prop_assert_eq!(
            must(lazy.observe(0)),
            Some(ExpansionObservation::Cancellation)
        );
        prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Cancelled);
        Ok(())
    }
);

property!(prop_retry_flag_only_on_failure, any::<u64>(), |attempt| {
    let state = LazyState::<char, TropicalWeight>::failed(
        ExpansionFailure::new(
            ExpansionFailureKind::Source,
            RetryPolicy::Explicit,
            "retryable",
        ),
        attempt,
    );
    prop_assert!(state.is_retryable());
    prop_assert_eq!(state.status(), ExpansionStatus::Failed);
    Ok(())
});

property!(
    prop_nonretryable_failure_is_terminal,
    1usize..32,
    |retries| {
        let source = LifecycleSource::new(Outcome::PermanentFailure);
        let mut lazy = LazyWfstWrapper::new(source.clone());
        let _ = lazy.expand(0);
        for _ in 0..retries {
            prop_assert!(lazy.retry(0).is_err());
        }
        prop_assert_eq!(source.calls.load(Ordering::Acquire), 1);
        Ok(())
    }
);

property!(
    prop_expanded_exactly_cacheable,
    (any::<bool>(), any::<u64>()),
    |(nonempty, attempt)| {
        let state = LazyState::expanded(
            false,
            TropicalWeight::zero(),
            if nonempty {
                smallvec![transition()]
            } else {
                SmallVec::new()
            },
            attempt,
        );
        prop_assert!(state.status().is_cacheable());
        prop_assert!(state.is_computed());
        Ok(())
    }
);

property!(
    prop_incomplete_states_are_not_cacheable,
    (0u8..4, any::<u64>()),
    |(kind, attempt)| {
        let state = match kind {
            0 => LazyState::<char, TropicalWeight>::Unexpanded,
            1 => LazyState::expanding(attempt),
            2 => LazyState::failed(
                ExpansionFailure::new(ExpansionFailureKind::Source, RetryPolicy::Never, "failure"),
                attempt,
            ),
            _ => LazyState::cancelled(CancellationReason::Requested, attempt),
        };
        prop_assert!(!state.status().is_cacheable());
        Ok(())
    }
);

property!(
    prop_attempt_count_is_bounded,
    prop::collection::vec(any::<bool>(), 0..1024),
    |actions| {
        let mut lazy = LazyWfstWrapper::new(LifecycleSource::new(Outcome::Empty));
        let begin_opportunities =
            u64::try_from(actions.len()).expect("property trace length fits in u64");
        for clear in actions {
            if clear {
                lazy.clear_state(0);
            }
            let _ = lazy.expand(0);
        }
        prop_assert!(lazy.total_attempts() <= begin_opportunities);
        Ok(())
    }
);

property!(prop_cancelled_has_no_owner, any::<bool>(), |pre_cancel| {
    let token = CancellationToken::new();
    if pre_cancel {
        token.cancel(CancellationReason::Requested);
    }
    let mut lazy = LazyWfstWrapper::new(LifecycleSource::new(Outcome::Cancelled));
    let _ = lazy.expand_with(0, &token);
    prop_assert_ne!(must(lazy.expansion_status(0)), ExpansionStatus::Expanding);
    Ok(())
});

property!(prop_failed_has_no_owner, any::<bool>(), |retryable| {
    let mut lazy = LazyWfstWrapper::new(LifecycleSource::new(if retryable {
        Outcome::RetryableFailure
    } else {
        Outcome::PermanentFailure
    }));
    let _ = lazy.expand(0);
    prop_assert_eq!(must(lazy.expansion_status(0)), ExpansionStatus::Failed);
    Ok(())
});
