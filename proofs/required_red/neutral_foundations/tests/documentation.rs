use proptest::prelude::*;
use vinary_doc_lint::{ClaimAuthority, ClaimKind, GeneratedAssetManifest, LintExecutionMode};

fn documentation_contract() {
    let _ = GeneratedAssetManifest::zeroed_for_test();
    let _ = LintExecutionMode::CheckOnly;
}

proptest! {
    #[test]
    fn prop_changed_source_marks_generated_asset_stale(left in any::<u64>(), right in any::<u64>()) { documentation_contract(); prop_assume!(left != right); prop_assert!(!GeneratedAssetManifest::from_test_seed(left).current_against(&GeneratedAssetManifest::from_test_seed(right))); }
    #[test]
    fn prop_changed_generator_marks_generated_asset_stale(left in any::<u64>(), right in any::<u64>()) { documentation_contract(); prop_assume!(left != right); prop_assert!(!GeneratedAssetManifest::from_generator_test_seed(left).current_against(&GeneratedAssetManifest::from_generator_test_seed(right))); }
    #[test]
    fn prop_statistical_wording_cannot_claim_a_theorem(seed in any::<u64>()) { documentation_contract(); prop_assert!(!ClaimAuthority::StatisticalInference.authorizes(ClaimKind::Theorem, seed)); }
    #[test]
    fn prop_check_only_lint_is_non_mutating(document in proptest::collection::vec(any::<u8>(), 0..1024)) { documentation_contract(); prop_assert_eq!(LintExecutionMode::CheckOnly.apply_for_test(&document), document); }
    #[test]
    fn prop_documentation_traversal_uses_constant_native_stack(depth in 0_usize..100_000) { documentation_contract(); prop_assert_eq!(LintExecutionMode::CheckOnly.native_frame_bound(depth), 1); }
    #[test]
    fn prop_check_only_lint_never_mutates_documentation(document in proptest::collection::vec(any::<u8>(), 0..1024)) { documentation_contract(); prop_assert_eq!(LintExecutionMode::CheckOnly.apply_for_test(&document), document); }
    #[test]
    fn prop_stale_manifest_cannot_pass_lint(left in any::<u64>(), right in any::<u64>()) { documentation_contract(); prop_assume!(left != right); prop_assert!(!GeneratedAssetManifest::from_test_seed(left).lint_passes_against(&GeneratedAssetManifest::from_test_seed(right))); }
    #[test]
    fn prop_e9_nf_smt_stale_manifest_not_linted(left in any::<u64>(), right in any::<u64>()) { documentation_contract(); prop_assume!(left != right); prop_assert!(!GeneratedAssetManifest::from_test_seed(left).lint_passes_against(&GeneratedAssetManifest::from_test_seed(right))); }
    #[test]
    fn prop_e9_nf_smt_check_only_nonmutating(document in proptest::collection::vec(any::<u8>(), 0..1024)) { documentation_contract(); prop_assert_eq!(LintExecutionMode::CheckOnly.apply_for_test(&document), document); }
}
