use proptest::prelude::*;
use vinary_dictionary_pipeline::DenseExternalMap;

proptest! {
    #[test]
    fn prop_external_key_has_at_most_one_dense_id(e in any::<u64>(), d in any::<u32>()) {
        let map = DenseExternalMap::try_from_pairs([(e, d)]).unwrap();
        prop_assert_eq!(map.dense_for(&e), Some(d));
    }

    #[test]
    fn prop_dense_id_has_at_most_one_external_key(e in any::<u64>(), d in any::<u32>()) {
        let map = DenseExternalMap::try_from_pairs([(e, d)]).unwrap();
        prop_assert_eq!(map.external_for(d), Some(&e));
    }

    #[test]
    fn prop_dense_external_mapping_round_trips(e in any::<u64>(), d in any::<u32>()) {
        let map = DenseExternalMap::try_from_pairs([(e, d)]).unwrap();
        prop_assert_eq!(map.external_for(map.dense_for(&e).unwrap()), Some(&e));
    }
}
