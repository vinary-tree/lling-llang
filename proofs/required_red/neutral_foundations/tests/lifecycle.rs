use proptest::prelude::*;
use vinary_runtime::{InputLocks, NeutralFoundationRelease, OutcomeAxes};

fn release_contract() {
    let _ = NeutralFoundationRelease::builder();
}

proptest! {
    #[test]
    fn prop_type_ok(seed in any::<u64>()) { release_contract(); prop_assert!(NeutralFoundationRelease::scenario_for_test(seed).type_ok()); }
    #[test]
    fn prop_named_profile_is_not_rfc8785(seed in any::<u64>()) { release_contract(); prop_assert_ne!(NeutralFoundationRelease::scenario_for_test(seed).profile_id(), "RFC8785"); }
    #[test]
    fn prop_release_requires_every_neutral_foundation_gate(seed in any::<u64>()) { release_contract(); prop_assert!(NeutralFoundationRelease::scenario_for_test(seed).release_implies_all_gates()); }
    #[test]
    fn prop_native_stack_bound_is_input_independent(depth in 0_usize..100_000) { release_contract(); prop_assert_eq!(NeutralFoundationRelease::native_frame_bound(depth), 1); }
    #[test]
    fn prop_e9_nf_smt_release_requires_every_gate(seed in any::<u64>()) { release_contract(); prop_assert!(NeutralFoundationRelease::scenario_for_test(seed).release_implies_all_gates()); }
    #[test]
    fn prop_e9_nf_smt_native_stack_constant(depth in 0_usize..100_000) { release_contract(); prop_assert_eq!(NeutralFoundationRelease::native_frame_bound(depth), 1); }
    #[test]
    fn prop_e9_nf_smt_valid_exact_release_witness(seed in any::<u64>()) { release_contract(); let locks = InputLocks::from_test_seed(seed); prop_assert!(NeutralFoundationRelease::builder().canonical(true).patch_atomic(true).runtime(OutcomeAxes::exact_complete(), locks.clone(), locks).source_accounted(true).assurance_verified(true).lint_current(true).build().released()); }
    #[test]
    fn prop_e9_nf_smt_valid_complete_approximate_cache_witness(seed in any::<u64>()) { release_contract(); prop_assert!(OutcomeAxes::complete_approximate_for_test(seed).cache_admissible()); }
    #[test]
    fn prop_eventually_terminal(seed in any::<u64>()) { release_contract(); prop_assert!(NeutralFoundationRelease::scenario_for_test(seed).run_to_terminal().is_terminal()); }
}
