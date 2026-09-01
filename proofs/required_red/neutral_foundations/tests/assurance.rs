use proptest::prelude::*;
use vinary_assurance::{
    Applicability, AssuranceDecision, EvidenceAuthority, EvidenceContext, ObligationKind,
    ReviewerAttestation,
};

fn assurance_contract() {
    let _ = EvidenceAuthority::TheoremProof;
    let _ = EvidenceContext::zeroed_for_test();
}

proptest! {
    #[test]
    fn prop_statistics_do_not_discharge_theorem_obligations(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::evaluate_for_test(EvidenceAuthority::StatisticalInference, ObligationKind::Theorem, seed).verified()); }
    #[test]
    fn prop_bounded_models_do_not_discharge_unbounded_theorems(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::evaluate_for_test(EvidenceAuthority::BoundedModelCheck, ObligationKind::Theorem, seed).verified()); }
    #[test]
    fn prop_changed_subject_invalidates_evidence(left in any::<u64>(), right in any::<u64>()) { assurance_contract(); prop_assume!(left != right); prop_assert!(!EvidenceContext::from_test_seed(left).fresh_for(&EvidenceContext::from_test_seed(right))); }
    #[test]
    fn prop_attestation_is_revision_bound(revision in any::<u64>(), other in any::<u64>()) { assurance_contract(); let attestation = ReviewerAttestation::for_test_revision(revision); prop_assert_eq!(attestation.applies_to_revision(other), revision == other); }
    #[test]
    fn prop_verified_assurance_requires_negative_control(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::theorem_for_test(seed).without_negative_control().verified()); }
    #[test]
    fn prop_inapplicable_evidence_cannot_verify(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::theorem_for_test(seed).with_applicability(Applicability::NotApplicable).verified()); }
    #[test]
    fn prop_statistics_never_discharge_theorem_obligations(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::evaluate_for_test(EvidenceAuthority::StatisticalInference, ObligationKind::Theorem, seed).verified()); }
    #[test]
    fn prop_verified_assurance_requires_negative_control_lifecycle(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::theorem_for_test(seed).without_negative_control().verified()); }
    #[test]
    fn prop_stale_evidence_cannot_verify(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::theorem_for_test(seed).mark_stale().verified()); }
    #[test]
    fn prop_verified_assurance_requires_revision_attestation(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::theorem_for_test(seed).without_attestation().verified()); }
    #[test]
    fn prop_e9_nf_smt_statistics_not_theorem(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::evaluate_for_test(EvidenceAuthority::StatisticalInference, ObligationKind::Theorem, seed).verified()); }
    #[test]
    fn prop_e9_nf_smt_stale_evidence_not_verified(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::theorem_for_test(seed).mark_stale().verified()); }
    #[test]
    fn prop_e9_nf_smt_negative_control_required(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::theorem_for_test(seed).without_negative_control().verified()); }
    #[test]
    fn prop_e9_nf_smt_attestation_revision_required(seed in any::<u64>()) { assurance_contract(); prop_assert!(!AssuranceDecision::theorem_for_test(seed).without_attestation().verified()); }
}
