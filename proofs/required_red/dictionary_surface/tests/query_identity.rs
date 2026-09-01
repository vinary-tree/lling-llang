use proptest::prelude::*;
use vinary_dictionary_pipeline::{CandidateFeed, Coverage, DictionaryQueryIdentity};

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
    #[test]
    fn prop_query_identity_is_reflexive(s in any::<u64>(), q in any::<Vec<u8>>()) {
        let value = id(s, q, 1, 2, 3);
        prop_assert!(value.same_semantics(&value));
    }

    #[test]
    fn prop_query_identity_exactly_refines_fuzzy_index(s in any::<u64>(), q in any::<Vec<u8>>()) {
        let left = id(s, q, 1, 2, 3);
        let right = left.clone();
        prop_assert_eq!(left.same_semantics(&right), left.as_fuzzy_index() == right.as_fuzzy_index());
    }

    #[test]
    fn prop_changed_snapshot_is_rejected(v in any::<u64>()) {
        prop_assert!(!id(v, vec![], 1, 2, 3).same_semantics(&id(v.wrapping_add(1), vec![], 1, 2, 3)));
    }

    #[test]
    fn prop_changed_normalization_is_rejected(v in any::<u64>()) {
        prop_assert!(!id(0, vec![], v, 2, 3).same_semantics(&id(0, vec![], v.wrapping_add(1), 2, 3)));
    }

    #[test]
    fn prop_changed_edit_profile_is_rejected(v in any::<u64>()) {
        prop_assert!(!id(0, vec![], 1, v, 3).same_semantics(&id(0, vec![], 1, v.wrapping_add(1), 3)));
    }

    #[test]
    fn prop_changed_bound_is_rejected(v in any::<u32>()) {
        prop_assert!(!id(0, vec![], 1, 2, v).same_semantics(&id(0, vec![], 1, 2, v.wrapping_add(1))));
    }

    #[test]
    fn prop_candidate_identity_matches_capture(s in any::<u64>(), values in any::<Vec<u16>>()) {
        let capture = id(s, vec![], 1, 2, 3);
        let feed = CandidateFeed::new(capture.clone(), values, Coverage::Complete);
        prop_assert!(feed.identity().same_semantics(&capture));
    }
}
