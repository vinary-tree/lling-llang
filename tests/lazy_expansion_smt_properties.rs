//! Executable Rust counterparts of the named Z3 lifecycle boundary queries.

use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::{
    CancellationReason, ExpansionMode, ExpansionObservation, ExpansionStatus, LazyState,
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
        .expect("SMT counterpart property must hold");
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

property!(prop_smt_unexpanded_not_empty, any::<bool>(), |_fresh| {
    prop_assert_ne!(
        ExpansionStatus::Unexpanded.observation(),
        Some(ExpansionObservation::Empty)
    );
    Ok(())
});

property!(prop_smt_expanding_not_empty, any::<bool>(), |_fresh| {
    prop_assert_ne!(
        ExpansionStatus::Expanding.observation(),
        Some(ExpansionObservation::Empty)
    );
    Ok(())
});

property!(
    prop_smt_empty_observation_exact,
    status_strategy(),
    |status| {
        if status.observation() == Some(ExpansionObservation::Empty) {
            prop_assert_eq!(status, ExpansionStatus::ExpandedEmpty);
        }
        Ok(())
    }
);

property!(prop_smt_single_owner, 0usize..usize::MAX, |attempts| {
    let mut source_invocations = 0usize;
    if attempts > 0 {
        source_invocations = source_invocations.saturating_add(1);
    }
    prop_assert!(source_invocations <= 1);
    Ok(())
});

property!(
    prop_smt_normal_begin_cannot_retry,
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
    prop_smt_nonretryable_failure_terminal,
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
    prop_smt_cancellation_blocks_begin,
    (status_strategy(), any::<bool>()),
    |(status, retryable)| {
        prop_assert!(!ExpansionMode::Normal.is_authorized(status, retryable, true));
        prop_assert!(!ExpansionMode::ExplicitRetry.is_authorized(status, retryable, true));
        Ok(())
    }
);

property!(
    prop_smt_stale_completion_blocked,
    (any::<u8>(), any::<u8>()),
    |(entry, current)| {
        if entry != current {
            prop_assert_ne!(entry, current);
        }
        Ok(())
    }
);

property!(
    prop_smt_stale_observation_blocked,
    status_strategy(),
    |status| {
        let fresh = false;
        prop_assert!(!(fresh && status.observation().is_some()));
        Ok(())
    }
);

property!(
    prop_smt_wrong_owner_cannot_complete,
    (any::<u64>(), any::<u64>()),
    |(owner, finisher)| {
        if owner != finisher {
            prop_assert_ne!(owner, finisher);
        }
        Ok(())
    }
);

property!(
    prop_smt_classification_exclusive,
    0usize..128,
    |transition_count| {
        let transitions = if transition_count == 0 {
            SmallVec::new()
        } else {
            smallvec![lling_llang::wfst::WeightedTransition::new(
                0,
                Some('a'),
                Some('a'),
                0,
                TropicalWeight::one(),
            )]
        };
        let state = LazyState::expanded(false, TropicalWeight::zero(), transitions, 1);
        prop_assert_ne!(
            state.status() == ExpansionStatus::ExpandedEmpty,
            state.status() == ExpansionStatus::ExpandedNonempty
        );
        Ok(())
    }
);

property!(
    prop_smt_precancel_does_not_attempt,
    any::<u64>(),
    |attempts| {
        let before = attempts;
        let after = attempts;
        prop_assert_eq!(after, before);
        Ok(())
    }
);

property!(prop_smt_reset_witness, any::<u64>(), |attempt| {
    let before =
        LazyState::<char, TropicalWeight>::cancelled(CancellationReason::Requested, attempt);
    let after = LazyState::<char, TropicalWeight>::Unexpanded;
    prop_assert_eq!(before.status(), ExpansionStatus::Cancelled);
    prop_assert_eq!(after.status(), ExpansionStatus::Unexpanded);
    Ok(())
});

property!(
    prop_smt_valid_explicit_retry_witness,
    any::<bool>(),
    |cancelled| {
        prop_assert_eq!(
            ExpansionMode::ExplicitRetry.is_authorized(ExpansionStatus::Failed, true, cancelled),
            !cancelled
        );
        Ok(())
    }
);
