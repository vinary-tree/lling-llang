//! Property-based tests for semiring algebraic laws.

use lling_llang::semiring::{
    Semiring, TropicalWeight, LogWeight, ProbabilityWeight,
};
use proptest::prelude::*;

fn arb_tropical() -> impl Strategy<Value = TropicalWeight> {
    (0.0f64..1000.0).prop_map(TropicalWeight::new)
}

fn arb_log() -> impl Strategy<Value = LogWeight> {
    (-100.0f64..100.0).prop_map(LogWeight::new)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn tropical_plus_commutative(a in arb_tropical(), b in arb_tropical()) {
        let ab = a.plus(&b);
        let ba = b.plus(&a);
        prop_assert!((ab.value() - ba.value()).abs() < 1e-10);
    }

    #[test]
    fn tropical_times_associative(
        a in arb_tropical(),
        b in arb_tropical(),
        c in arb_tropical()
    ) {
        let ab_c = a.times(&b).times(&c);
        let a_bc = a.times(&b.times(&c));
        prop_assert!((ab_c.value() - a_bc.value()).abs() < 1e-10);
    }

    #[test]
    fn tropical_one_identity(a in arb_tropical()) {
        let one = TropicalWeight::one();
        let a_one = a.times(&one);
        prop_assert!((a_one.value() - a.value()).abs() < 1e-10);
    }

    #[test]
    fn log_plus_commutative(a in arb_log(), b in arb_log()) {
        let ab = a.plus(&b);
        let ba = b.plus(&a);
        prop_assert!((ab.value() - ba.value()).abs() < 1e-10);
    }

    #[test]
    fn log_one_identity(a in arb_log()) {
        let one = LogWeight::one();
        let a_one = a.times(&one);
        prop_assert!((a_one.value() - a.value()).abs() < 1e-10);
    }
}
