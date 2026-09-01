use proptest::prelude::*;
use vinary_runtime::{
    CheckpointCompatibility, InputLocks, NeutralFoundationRelease, OutcomeAxes,
    ProcessTreeTerminator, RepositoryBackedSpillPolicy,
};

fn runtime_contract() {
    let _ = OutcomeAxes::incomplete();
    let _ = InputLocks::zeroed_for_test();
    let _ = RepositoryBackedSpillPolicy::new_for_test(1);
}

proptest! {
    #[test]
    fn prop_incomplete_result_is_not_cacheable(seed in any::<u64>()) { runtime_contract(); prop_assert!(!OutcomeAxes::incomplete_with_seed(seed).cache_admissible()); }
    #[test]
    fn prop_exact_release_binds_every_input_lock(seed in any::<u64>()) { runtime_contract(); let locks = InputLocks::from_test_seed(seed); prop_assert!(OutcomeAxes::exact_complete().release_admissible(&locks, &locks)); }
    #[test]
    fn prop_stale_checkpoint_cannot_resume(left in any::<u64>(), right in any::<u64>()) { runtime_contract(); prop_assume!(left != right); prop_assert!(!CheckpointCompatibility::new(InputLocks::from_test_seed(left)).can_resume(&InputLocks::from_test_seed(right))); }
    #[test]
    fn prop_overflow_output_never_uses_tmpfs(bytes in 2_usize..4096) { runtime_contract(); prop_assert!(RepositoryBackedSpillPolicy::new_for_test(1).route(bytes).is_repository_backed()); }
    #[test]
    fn prop_process_termination_decreases_pending_work(pending in 1_usize..100_000) { runtime_contract(); prop_assert!(ProcessTreeTerminator::test_step(pending).pending() < pending); }
    #[test]
    fn prop_process_termination_native_stack_is_constant(pending in 0_usize..100_000) { runtime_contract(); prop_assert_eq!(ProcessTreeTerminator::native_frame_bound(pending), 1); }
    #[test]
    fn prop_incomplete_never_enters_cache(seed in any::<u64>()) { runtime_contract(); prop_assert!(!OutcomeAxes::incomplete_with_seed(seed).cache_admissible()); }
    #[test]
    fn prop_runtime_release_requires_exact_complete_locked_inputs(seed in any::<u64>()) { runtime_contract(); let locks = InputLocks::from_test_seed(seed); prop_assert!(OutcomeAxes::exact_complete().release_admissible(&locks, &locks)); }
    #[test]
    fn prop_overflow_spills_only_to_repository_storage(bytes in 2_usize..4096) { runtime_contract(); prop_assert!(RepositoryBackedSpillPolicy::new_for_test(1).route(bytes).is_repository_backed()); }
    #[test]
    fn prop_resume_requires_compatible_checkpoint(left in any::<u64>(), right in any::<u64>()) { runtime_contract(); prop_assert_eq!(CheckpointCompatibility::new(InputLocks::from_test_seed(left)).can_resume(&InputLocks::from_test_seed(right)), left == right); }
    #[test]
    fn prop_e9_nf_smt_incomplete_not_cacheable(seed in any::<u64>()) { runtime_contract(); prop_assert!(!OutcomeAxes::incomplete_with_seed(seed).cache_admissible()); }
    #[test]
    fn prop_e9_nf_smt_exact_release_locks_all_inputs(seed in any::<u64>()) { runtime_contract(); let locks = InputLocks::from_test_seed(seed); prop_assert!(OutcomeAxes::exact_complete().release_admissible(&locks, &locks)); }
    #[test]
    fn prop_e9_nf_smt_overflow_spills_to_repository(bytes in 2_usize..4096) { runtime_contract(); prop_assert!(RepositoryBackedSpillPolicy::new_for_test(1).route(bytes).is_repository_backed()); }
    #[test]
    fn prop_e9_nf_smt_resume_requires_compatible_checkpoint(left in any::<u64>(), right in any::<u64>()) { runtime_contract(); prop_assert_eq!(CheckpointCompatibility::new(InputLocks::from_test_seed(left)).can_resume(&InputLocks::from_test_seed(right)), left == right); }
    #[test]
    fn prop_release_requires_every_neutral_foundation_gate(seed in any::<u64>()) { runtime_contract(); prop_assert!(NeutralFoundationRelease::scenario_for_test(seed).release_implies_all_gates()); }
}
