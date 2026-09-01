use libdictenstein::union_zipper::ValueMergeStrategy;
use libdictenstein_llattice::LatticeJoin;
use llattice::{JoinSemilattice, MeetSemilattice};
use lling_llang::lattice_bridge::SemiringJoinWrapper;
use lling_llang::semiring::{Semiring, TropicalWeight};
use proptest::prelude::*;
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(Vec<u8>: JoinSemilattice);
assert_not_impl_any!(f64: JoinSemilattice);
assert_not_impl_any!(SemiringJoinWrapper<TropicalWeight>: MeetSemilattice);

proptest! {
    #[test]
    fn prop_dictionary_join_merge_is_idempotent(a in any::<u32>()) {
        prop_assert_eq!(LatticeJoin.merge(a, a), a);
    }
    #[test]
    fn prop_dictionary_join_merge_is_commutative(a in any::<u32>(), b in any::<u32>()) {
        prop_assert_eq!(LatticeJoin.merge(a, b), LatticeJoin.merge(b, a));
    }
    #[test]
    fn prop_dictionary_join_merge_is_associative(a in any::<u32>(), b in any::<u32>(), c in any::<u32>()) {
        prop_assert_eq!(LatticeJoin.merge(LatticeJoin.merge(a,b),c), LatticeJoin.merge(a,LatticeJoin.merge(b,c)));
    }
    #[test]
    fn prop_tropical_times_is_not_idempotent(v in 1.0f64..1.0e6) {
        let w = TropicalWeight::from(v);
        prop_assert_ne!(w.times(&w), w);
    }
    #[test]
    fn prop_semiring_times_is_never_inferred_as_meet(v in 1.0f64..1.0e6) {
        let w = SemiringJoinWrapper(TropicalWeight::from(v));
        prop_assert_eq!(w.join(&w), w);
    }
    #[test]
    fn prop_tropical_times_breaks_meet_absorption(v in 1.0f64..1.0e6) {
        let w = TropicalWeight::from(v);
        prop_assert_ne!(w.times(&w.plus(&TropicalWeight::from(v + 1.0))), w);
    }
    #[test]
    fn prop_left_biased_sequence_is_not_a_join(a in any::<u8>(), b in any::<u8>()) {
        prop_assume!(a != b); prop_assert_ne!(vec![a,b], vec![b,a]);
    }
    #[test]
    fn prop_nonfinite_values_are_rejected(i in 0usize..3) {
        prop_assert!(![f64::NAN,f64::INFINITY,f64::NEG_INFINITY][i].is_finite());
    }
    #[test]
    fn prop_raw_float_domain_is_not_lawful_by_default(v in any::<f64>()) {
        prop_assert_eq!(v.is_finite(), !v.is_nan() && !v.is_infinite());
    }
}
