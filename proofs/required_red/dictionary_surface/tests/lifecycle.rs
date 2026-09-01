use proptest::prelude::*;
use std::collections::BTreeSet;
use vinary_dictionary_pipeline::{
    classify_outcome, ConfirmationMachine, Coverage, Precision, QueryOutcome, TerminationReason,
};

fn machine(values: Vec<u8>) -> ConfirmationMachine<u8, impl Fn(&u8) -> bool> {
    ConfirmationMachine::new(values, |value: &u8| value % 2 == 0)
}

proptest! {
    #[test] fn prop_cap_is_incomplete(_x in any::<u8>()) {
        prop_assert!(!TerminationReason::Capped.is_complete());
    }
    #[test] fn prop_cancellation_is_incomplete(_x in any::<u8>()) {
        prop_assert!(!TerminationReason::Cancelled.is_complete());
    }
    #[test] fn prop_provider_failure_is_incomplete(_x in any::<u8>()) {
        prop_assert!(!TerminationReason::ProviderFailed.is_complete());
    }
    #[test] fn prop_confirmation_step_decreases_pending_work(values in proptest::collection::vec(any::<u8>(), 1..128)) {
        let mut m = machine(values); let before = m.pending_len();
        prop_assert!(m.step()); prop_assert!(m.pending_len() < before);
    }
    #[test] fn prop_dictionary_lifecycle_is_well_typed(values in any::<Vec<u8>>()) {
        let m = machine(values);
        prop_assert!(m.accepted_len() <= m.checked_len() && m.checked_len() <= m.feed_len());
    }
    #[test] fn prop_pending_and_checked_partition_feed(values in any::<Vec<u8>>(), steps in 0usize..128) {
        let mut m = machine(values);
        for _ in 0..steps { if !m.step() { break; } }
        prop_assert_eq!(m.pending_len() + m.checked_len(), m.feed_len());
    }
    #[test] fn prop_accepted_equals_checked_reference(values in any::<Vec<u8>>()) {
        let expected: BTreeSet<_> = values.iter().copied().filter(|v| v % 2 == 0).collect();
        let mut m = machine(values); m.run_to_completion();
        prop_assert_eq!(m.accepted().iter().copied().collect::<BTreeSet<_>>(), expected);
    }
    #[test] fn prop_accepted_is_reference_subset(values in any::<Vec<u8>>(), steps in 0usize..128) {
        let mut m = machine(values);
        for _ in 0..steps { if !m.step() { break; } }
        prop_assert!(m.accepted().iter().all(|v| v % 2 == 0));
    }
    #[test] fn prop_complete_feed_contains_reference(values in any::<Vec<u8>>()) {
        prop_assert!(values.iter().filter(|v| **v % 2 == 0).all(|v| values.contains(v)));
    }
    #[test] fn prop_nonexhaustive_termination_never_promotes(reason in 1u8..4) {
        let r = match reason { 1 => TerminationReason::Capped, 2 => TerminationReason::Cancelled,
                               _ => TerminationReason::ProviderFailed };
        prop_assert_eq!(classify_outcome(Precision::Exact,Coverage::Complete,r), QueryOutcome::Incomplete);
    }
    #[test] fn prop_complete_exact_requires_all_evidence(_x in any::<u8>()) {
        prop_assert_eq!(classify_outcome(Precision::Exact,Coverage::Complete,TerminationReason::Exhausted),
                        QueryOutcome::CompleteExact);
    }
    #[test] fn prop_complete_exact_equals_reference(values in any::<Vec<u8>>()) {
        let expected: BTreeSet<_> = values.iter().copied().filter(|v| v % 2 == 0).collect();
        let mut m = machine(values); m.run_to_completion();
        prop_assert_eq!(m.accepted().iter().copied().collect::<BTreeSet<_>>(), expected);
    }
    #[test] fn prop_published_output_equals_accepted(values in any::<Vec<u8>>()) {
        let mut m = machine(values); m.run_to_completion();
        prop_assert_eq!(m.publish(), m.accepted());
    }
}
