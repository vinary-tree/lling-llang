use proptest::prelude::*;
use vinary_requirements::{RequirementHistory, RequirementRevision, SourceAccounting};

fn requirements_contract() {
    let _ = RequirementHistory::new_for_test();
    let _ = SourceAccounting::new_for_test();
}

proptest! {
    #[test]
    fn prop_revision_preserves_stable_requirement_identity(id in any::<u64>(), payload in any::<u64>()) { requirements_contract(); prop_assert_eq!(RequirementRevision::initial(id, 0).revise(payload).stable_id(), id); }
    #[test]
    fn prop_revision_strictly_advances(id in any::<u64>(), payload in any::<u64>()) { requirements_contract(); let initial = RequirementRevision::initial(id, 0); prop_assert!(initial.clone().revise(payload).revision() > initial.revision()); }
    #[test]
    fn prop_tombstone_is_not_active(id in any::<u64>()) { requirements_contract(); prop_assert!(!RequirementRevision::initial(id, 0).tombstone().is_active()); }
    #[test]
    fn prop_source_accounting_is_total(spans in proptest::collection::vec(any::<u64>(), 0..128)) { requirements_contract(); let accounting = SourceAccounting::classify_even_for_test(&spans); prop_assert_eq!(accounting.classified_len() + accounting.unclassified_len(), spans.len()); }
    #[test]
    fn prop_unclassified_source_is_preserved(spans in proptest::collection::vec(any::<u64>(), 0..128)) { requirements_contract(); let accounting = SourceAccounting::classify_even_for_test(&spans); prop_assert!(spans.iter().filter(|span| **span % 2 == 1).all(|span| accounting.contains_unclassified(*span))); }
    #[test]
    fn prop_history_validation_uses_constant_native_stack(depth in 0_usize..100_000) { requirements_contract(); prop_assert_eq!(RequirementHistory::native_frame_bound(depth), 1); }
    #[test]
    fn prop_tombstones_are_not_active(id in any::<u64>()) { requirements_contract(); prop_assert!(!RequirementRevision::initial(id, 0).tombstone().is_active()); }
    #[test]
    fn prop_source_accounting_never_drops_unclassified_text(spans in proptest::collection::vec(any::<u64>(), 0..128)) { requirements_contract(); let accounting = SourceAccounting::classify_even_for_test(&spans); prop_assert!(spans.iter().all(|span| accounting.contains(*span))); }
    #[test]
    fn prop_e9_nf_smt_tombstone_not_active(id in any::<u64>()) { requirements_contract(); prop_assert!(!RequirementRevision::initial(id, 0).tombstone().is_active()); }
    #[test]
    fn prop_e9_nf_smt_unclassified_source_retained(spans in proptest::collection::vec(any::<u64>(), 0..128)) { requirements_contract(); let accounting = SourceAccounting::classify_even_for_test(&spans); prop_assert!(spans.iter().all(|span| accounting.contains(*span))); }
}
