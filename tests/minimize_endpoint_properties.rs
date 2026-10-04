//! Executable refinement checks for `MinimizeEndpointValidation.v`.

use lling_llang::algorithms::{
    estimate_reduction, estimate_reduction_with_epsilon_and_input_identity, minimize,
    minimize_with_input_identity, MinimizeConfig, MinimizeError, MinimizeInputIdentity,
};
use lling_llang::semiring::TropicalWeight;
use lling_llang::wfst::{MutableWfst, VectorWfst, WeightedTransition, Wfst};
use proptest::prelude::*;

type TestWfst = VectorWfst<char, TropicalWeight>;

fn base_wfst(states: usize) -> TestWfst {
    let mut fst = TestWfst::new();
    fst.add_states(states);
    if states != 0 {
        fst.set_start(0);
        fst.set_final((states - 1) as u32, TropicalWeight::new(0.0));
    }
    fst
}

fn no_transforms() -> MinimizeConfig {
    MinimizeConfig {
        push_weights: false,
        connect_first: false,
        ..MinimizeConfig::default()
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // valid_arcb_exact; first_invalid_none_iff; checked_then_preserves_valid_input.
    #[test]
    fn valid_endpoints_are_accepted_without_mutating_the_input(
        states in 1usize..20,
        seed in proptest::collection::vec(any::<u16>(), 0..60),
    ) {
        let mut fst = base_wfst(states);
        for (index, raw) in seed.iter().enumerate() {
            let owner = index % states;
            let target = (*raw as usize) % states;
            let label = char::from_u32(0x1000 + index as u32).unwrap();
            fst.add_arc(owner as u32, Some(label), Some(label), target as u32,
                TropicalWeight::new(1.0));
        }
        let before = format!("{fst:?}");
        let minimized = minimize(&fst, no_transforms()).unwrap();
        prop_assert_eq!(format!("{fst:?}"), before);
        prop_assert!(minimized.num_states() <= states);
        for state in 0..minimized.num_states() {
            for arc in minimized.transitions(state as u32) {
                prop_assert_eq!(arc.from as usize, state);
                prop_assert!((arc.to as usize) < minimized.num_states());
            }
        }
        prop_assert!(estimate_reduction(&fst).unwrap() <= states);
    }

    // first_invalid_complete; first_invalid_first_error; first_invalid_error_identity.
    #[test]
    fn malformed_target_reports_the_original_arc_before_any_transform(
        states in 1usize..20,
        owner_seed in any::<usize>(),
        target_delta in 0u32..1000,
        connect_first in any::<bool>(),
        push_weights in any::<bool>(),
    ) {
        let owner = owner_seed % states;
        let target = states as u32 + target_delta;
        let mut fst = base_wfst(states);
        fst.add_arc(owner as u32, Some('x'), Some('x'), target,
            TropicalWeight::new(1.0));
        let before = format!("{fst:?}");
        let identity = MinimizeInputIdentity::BorrowedWfst(&fst as *const TestWfst as usize);
        let config = MinimizeConfig { connect_first, push_weights, ..no_transforms() };
        let result = minimize(&fst, config);
        let rejected_correctly = matches!(result,
            Err(MinimizeError::InvalidTransition {
                input_identity,
                state_count,
                owner_state,
                source_state,
                target_state,
                transition_index: 0,
            }) if input_identity == identity && state_count == states &&
                owner_state == owner as u32 && source_state == owner as u32 &&
                target_state == target
        );
        prop_assert!(rejected_correctly);
        prop_assert_eq!(format!("{fst:?}"), before);
        let estimate_rejected = matches!(estimate_reduction(&fst),
            Err(MinimizeError::InvalidTransition { .. }));
        prop_assert!(estimate_rejected);
    }

    // valid_arcb_exact includes ownership, not only endpoint bounds.
    #[test]
    fn source_owner_mismatch_is_rejected(
        states in 2usize..20,
        owner_seed in any::<usize>(),
        target_seed in any::<usize>(),
    ) {
        let owner = owner_seed % states;
        let source = (owner + 1) % states;
        let target = target_seed % states;
        let mut fst = base_wfst(states);
        fst.state_mut(owner as u32).unwrap().transitions.push(
            WeightedTransition::new(source as u32, Some('x'), Some('x'),
                target as u32, TropicalWeight::new(1.0)));
        let rejected_correctly = matches!(minimize(&fst, no_transforms()),
            Err(MinimizeError::InvalidTransition {
                owner_state, source_state, target_state, transition_index: 0, ..
            }) if owner_state == owner as u32 && source_state == source as u32 &&
                target_state == target as u32);
        prop_assert!(rejected_correctly);
    }

    // A causal target mutant differs from the accepted graph in one endpoint.
    #[test]
    fn target_endpoint_mutant_is_detected_and_repair_restores_acceptance(
        states in 2usize..20,
        owner_seed in any::<usize>(),
        target_seed in any::<usize>(),
    ) {
        let owner = owner_seed % states;
        let target = target_seed % states;
        let mut fst = base_wfst(states);
        fst.add_arc(owner as u32, Some('x'), Some('x'), target as u32,
            TropicalWeight::new(1.0));
        let input_identity = MinimizeInputIdentity::Snapshot(57);
        prop_assert!(minimize_with_input_identity(&fst, input_identity, no_transforms()).is_ok());
        fst.state_mut(owner as u32).unwrap().transitions[0].to = states as u32;
        let malformed = minimize_with_input_identity(&fst, input_identity, no_transforms());
        let rejected = matches!(malformed,
            Err(MinimizeError::InvalidTransition {
                input_identity: MinimizeInputIdentity::Snapshot(57),
                target_state, ..
            }) if target_state == states as u32);
        prop_assert!(rejected);
        let estimated = estimate_reduction_with_epsilon_and_input_identity(
            &fst, 1e-10, input_identity);
        let estimate_rejected = matches!(estimated,
            Err(MinimizeError::InvalidTransition {
                input_identity: MinimizeInputIdentity::Snapshot(57), ..
            }));
        prop_assert!(estimate_rejected);
        fst.state_mut(owner as u32).unwrap().transitions[0].to = target as u32;
        prop_assert!(minimize_with_input_identity(&fst, input_identity, no_transforms()).is_ok());
    }

    // A causal source mutant retains a valid target but violates ownership.
    #[test]
    fn source_endpoint_mutant_is_detected_and_repair_restores_acceptance(
        states in 2usize..20,
        owner_seed in any::<usize>(),
    ) {
        let owner = owner_seed % states;
        let mut fst = base_wfst(states);
        fst.add_arc(owner as u32, Some('x'), Some('x'), owner as u32,
            TropicalWeight::new(1.0));
        prop_assert!(minimize(&fst, no_transforms()).is_ok());
        fst.state_mut(owner as u32).unwrap().transitions[0].from = states as u32;
        let rejected = matches!(minimize(&fst, no_transforms()),
            Err(MinimizeError::InvalidTransition { source_state, .. })
                if source_state == states as u32);
        prop_assert!(rejected);
        fst.state_mut(owner as u32).unwrap().transitions[0].from = owner as u32;
        prop_assert!(minimize(&fst, no_transforms()).is_ok());
    }

    // first_invalid_first_error; first_invalid_error_identity; first_invalid_complete.
    #[test]
    fn generated_first_error_wins_in_state_and_slice_order(
        states in 1usize..20,
        owner_seed in any::<usize>(),
        ordinal in 0usize..3,
        malformed_source in any::<bool>(),
        excess in 0u32..1000,
        snapshot in any::<u64>(),
    ) {
        let owner = owner_seed % states;
        let mut fst = base_wfst(states);
        for state in 0..states {
            for arc_index in 0..3 {
                let label = char::from_u32(0x1000 + (state * 3 + arc_index) as u32).unwrap();
                fst.add_arc(state as u32, Some(label), Some(label), state as u32,
                    TropicalWeight::new(1.0));
            }
        }
        let bad_endpoint = states as u32 + excess;
        let first = &mut fst.state_mut(owner as u32).unwrap().transitions[ordinal];
        if malformed_source {
            first.from = bad_endpoint;
        } else {
            first.to = bad_endpoint;
        }
        if (owner, ordinal) != (states - 1, 2) {
            fst.state_mut((states - 1) as u32).unwrap().transitions[2].to = states as u32;
        }
        let before = format!("{fst:?}");
        let identity = MinimizeInputIdentity::Snapshot(snapshot);
        let expected_source = if malformed_source { bad_endpoint } else { owner as u32 };
        let expected_target = if malformed_source { owner as u32 } else { bad_endpoint };
        let rejected = matches!(minimize_with_input_identity(&fst, identity, no_transforms()),
            Err(MinimizeError::InvalidTransition {
                input_identity, state_count, owner_state, source_state,
                target_state, transition_index,
            }) if input_identity == identity && state_count == states &&
                owner_state == owner as u32 &&
                source_state == expected_source &&
                target_state == expected_target &&
                transition_index == ordinal);
        prop_assert!(rejected);
        let estimate_rejected_first = matches!(
            estimate_reduction_with_epsilon_and_input_identity(&fst, 1e-10, identity),
            Err(MinimizeError::InvalidTransition {
                owner_state, transition_index, ..
            }) if owner_state == owner as u32 && transition_index == ordinal
        );
        prop_assert!(estimate_rejected_first);
        prop_assert_eq!(format!("{fst:?}"), before);
    }

    // invalid_start_rejected_before_arcs; validate_input_none_iff.
    #[test]
    fn generated_invalid_start_precedes_malformed_arc(
        states in 1usize..20,
        excess in 0u32..1000,
        snapshot in any::<u64>(),
    ) {
        let mut fst = base_wfst(states);
        fst.add_arc(0, Some('x'), Some('x'), states as u32,
            TropicalWeight::new(1.0));
        let bad_start = states as u32 + excess;
        let reported = ReportedWfst {
            inner: fst,
            reported_start: bad_start,
            reported_count: states,
        };
        let identity = MinimizeInputIdentity::Snapshot(snapshot);
        let rejected_start_first = matches!(
            estimate_reduction_with_epsilon_and_input_identity(&reported, 1e-10, identity),
            Err(MinimizeError::InvalidStartState {
                input_identity, state_count, start_state,
            }) if input_identity == identity && state_count == states && start_state == bad_start
        );
        prop_assert!(rejected_start_first);
    }

    // invalid_state_count_rejected_first; validate_input_none_iff.
    #[cfg(target_pointer_width = "64")]
    #[test]
    fn generated_unrepresentable_count_precedes_start_and_arc(
        excess in 0usize..1000,
        snapshot in any::<u64>(),
    ) {
        let mut fst = base_wfst(1);
        fst.add_arc(0, Some('x'), Some('x'), 99,
            TropicalWeight::new(1.0));
        let reported_count = u32::MAX as usize + 1 + excess;
        let reported = ReportedWfst {
            inner: fst,
            reported_start: 99,
            reported_count,
        };
        let identity = MinimizeInputIdentity::Snapshot(snapshot);
        let rejected_count_first = matches!(
            estimate_reduction_with_epsilon_and_input_identity(&reported, 1e-10, identity),
            Err(MinimizeError::InvalidStateCount {
                input_identity, state_count,
            }) if input_identity == identity && state_count == reported_count
        );
        prop_assert!(rejected_count_first);
    }

    // valid_startb's empty-input branch; checked_then_preserves_valid_input.
    #[test]
    fn empty_input_with_sentinel_start_is_accepted(snapshot in any::<u64>()) {
        let fst = base_wfst(0);
        let identity = MinimizeInputIdentity::Snapshot(snapshot);
        prop_assert_eq!(
            minimize_with_input_identity(&fst, identity, no_transforms()).unwrap().num_states(),
            0,
        );
        prop_assert_eq!(
            estimate_reduction_with_epsilon_and_input_identity(&fst, 1e-10, identity).unwrap(),
            0,
        );
    }
}

#[derive(Clone)]
struct ReportedWfst {
    inner: TestWfst,
    reported_start: u32,
    reported_count: usize,
}

impl Wfst<char, TropicalWeight> for ReportedWfst {
    fn start(&self) -> u32 {
        self.reported_start
    }
    fn is_final(&self, state: u32) -> bool {
        self.inner.is_final(state)
    }
    fn final_weight(&self, state: u32) -> TropicalWeight {
        self.inner.final_weight(state)
    }
    fn transitions(&self, state: u32) -> &[WeightedTransition<char, TropicalWeight>] {
        self.inner.transitions(state)
    }
    fn num_states(&self) -> usize {
        self.reported_count
    }
}

#[test]
fn invalid_start_precedes_invalid_transition() {
    let mut fst = base_wfst(2);
    fst.add_arc(0, Some('x'), Some('x'), 99, TropicalWeight::new(1.0));
    let fst = ReportedWfst {
        inner: fst,
        reported_start: 99,
        reported_count: 2,
    };
    let identity = MinimizeInputIdentity::Snapshot(901);
    assert!(matches!(
        estimate_reduction_with_epsilon_and_input_identity(&fst, 1e-10, identity),
        Err(MinimizeError::InvalidStartState {
            input_identity: MinimizeInputIdentity::Snapshot(901),
            state_count: 2,
            start_state: 99,
        })
    ));
}

#[test]
#[cfg(target_pointer_width = "64")]
fn unrepresentable_state_count_rejected_before_narrowing_or_scan() {
    let fst = ReportedWfst {
        inner: base_wfst(0),
        reported_start: u32::MAX,
        reported_count: u32::MAX as usize + 1,
    };
    let identity = MinimizeInputIdentity::Snapshot(902);
    assert!(
        matches!(estimate_reduction_with_epsilon_and_input_identity(&fst, 1e-10, identity),
        Err(MinimizeError::InvalidStateCount {
            input_identity: MinimizeInputIdentity::Snapshot(902), state_count,
        })
            if state_count == u32::MAX as usize + 1)
    );
}

#[test]
fn first_invalid_arc_wins_in_state_then_slice_order() {
    let mut fst = base_wfst(3);
    fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
    fst.add_arc(0, Some('b'), Some('b'), 30, TropicalWeight::new(1.0));
    fst.add_arc(1, Some('c'), Some('c'), 31, TropicalWeight::new(1.0));
    assert!(matches!(
        minimize(&fst, no_transforms()),
        Err(MinimizeError::InvalidTransition {
            owner_state: 0,
            source_state: 0,
            target_state: 30,
            transition_index: 1,
            ..
        })
    ));
}

#[test]
fn malformed_endpoint_scan_is_stack_safe() {
    let mut fst = base_wfst(50_000);
    for state in 0..49_999 {
        fst.add_arc(
            state,
            Some('a'),
            Some('a'),
            state + 1,
            TropicalWeight::new(1.0),
        );
    }
    fst.add_arc(
        49_999,
        Some('b'),
        Some('b'),
        50_000,
        TropicalWeight::new(1.0),
    );
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            assert!(matches!(
                minimize(&fst, no_transforms()),
                Err(MinimizeError::InvalidTransition {
                    owner_state: 49_999,
                    target_state: 50_000,
                    ..
                })
            ));
            assert!(estimate_reduction(&fst).is_err());
        })
        .unwrap()
        .join()
        .unwrap();
}
