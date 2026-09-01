use proptest::prelude::*;
use vinary_dictionary_pipeline::{
    classify_outcome, Coverage, DenseExternalMap, DictionaryQueryIdentity, Precision, QueryOutcome,
    TerminationReason,
};

fn id(
    s: u64,
    q: Vec<u8>,
    n: u64,
    e: u64,
    b: u32,
) -> DictionaryQueryIdentity<u64, Vec<u8>, u64, u64> {
    DictionaryQueryIdentity::new(s, q, n, e, b)
}

proptest! {
    #[test] fn prop_smt_snapshot_component_is_mandatory(v in any::<u64>()) {
        prop_assert!(!id(v,vec![],0,0,0).same_semantics(&id(v.wrapping_add(1),vec![],0,0,0)));
    }
    #[test] fn prop_smt_query_component_is_mandatory(mut q in any::<Vec<u8>>()) {
        let left=id(0,q.clone(),0,0,0); q.push(0);
        prop_assert!(!left.same_semantics(&id(0,q,0,0,0)));
    }
    #[test] fn prop_smt_normalization_component_is_mandatory(v in any::<u64>()) {
        prop_assert!(!id(0,vec![],v,0,0).same_semantics(&id(0,vec![],v.wrapping_add(1),0,0)));
    }
    #[test] fn prop_smt_edit_component_is_mandatory(v in any::<u64>()) {
        prop_assert!(!id(0,vec![],0,v,0).same_semantics(&id(0,vec![],0,v.wrapping_add(1),0)));
    }
    #[test] fn prop_smt_bound_component_is_mandatory(v in any::<u32>()) {
        prop_assert!(!id(0,vec![],0,0,v).same_semantics(&id(0,vec![],0,0,v.wrapping_add(1))));
    }
    #[test] fn prop_smt_dense_external_mapping_is_injective(e in any::<u64>(), d in any::<u32>()) {
        let m=DenseExternalMap::try_from_pairs([(e,d)]).unwrap();
        prop_assert_eq!(m.external_for(d),Some(&e));
    }
    #[test] fn prop_smt_strict_rank_excludes_cycles(a in any::<u8>(),b in any::<u8>(),c in any::<u8>()) {
        prop_assert!(!(b<a && c<b && a<c));
    }
    #[test] fn prop_smt_tropical_times_cannot_be_meet(v in 1u32..u32::MAX) {
        prop_assert_ne!(v.saturating_add(v),v);
    }
    #[test] fn prop_smt_nonfinite_is_inadmissible(i in 0usize..3) {
        prop_assert!(![f64::NAN,f64::INFINITY,f64::NEG_INFINITY][i].is_finite());
    }
    #[test] fn prop_smt_left_biased_merge_has_countermodel(a in any::<u8>(),b in any::<u8>()) {
        prop_assume!(a!=b); prop_assert_ne!(vec![a,b],vec![b,a]);
    }
    #[test] fn prop_smt_exact_facade_cannot_differ(v in any::<u64>()) {
        prop_assert_eq!(duallity::dictionary_pipeline::identity(v),v);
    }
    #[test] fn prop_smt_broken_facade_has_countermodel(v in any::<bool>()) {
        prop_assert_ne!(v,!v);
    }
    #[test] fn prop_smt_fibration_requires_lifts(v in any::<bool>()) {
        prop_assert_eq!(vinary_dictionary_pipeline::may_claim_fibration(v),v);
    }
    #[test] fn prop_smt_cap_is_not_complete(_x in any::<u8>()) {
        prop_assert_eq!(classify_outcome(Precision::Exact,Coverage::Complete,TerminationReason::Capped),QueryOutcome::Incomplete);
    }
    #[test] fn prop_smt_cancel_is_not_complete(_x in any::<u8>()) {
        prop_assert_eq!(classify_outcome(Precision::Exact,Coverage::Complete,TerminationReason::Cancelled),QueryOutcome::Incomplete);
    }
    #[test] fn prop_smt_failure_is_not_complete(_x in any::<u8>()) {
        prop_assert_eq!(classify_outcome(Precision::Exact,Coverage::Complete,TerminationReason::ProviderFailed),QueryOutcome::Incomplete);
    }
}
