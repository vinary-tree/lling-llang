use lling_llang::prelude::{ArcticWeight, Semiring, StarSemiring};

#[test]
fn maximum_path_score_and_unreachable_identity_are_exact() {
    let first = ArcticWeight::new(18.0).times(&ArcticWeight::new(-3.0));
    let second = ArcticWeight::new(12.0).times(&ArcticWeight::new(7.0));
    assert_eq!(first, ArcticWeight::new(15.0));
    assert_eq!(first.plus(&second), ArcticWeight::new(19.0));
    assert_eq!(first.plus(&ArcticWeight::zero()), first);
    assert_eq!(first.times(&ArcticWeight::zero()), ArcticWeight::zero());
}

#[test]
fn positive_cycle_refuses_finite_star() {
    assert_eq!(ArcticWeight::new(-1.0).star(), Some(ArcticWeight::one()));
    assert_eq!(ArcticWeight::new(1.0).star(), None);
}

#[test]
fn stable_bytes_preserve_negative_infinity_and_finite_scores() {
    assert_eq!(
        ArcticWeight::zero().to_bytes(),
        f64::NEG_INFINITY.to_le_bytes()
    );
    assert_eq!(ArcticWeight::new(42.0).to_bytes(), 42.0f64.to_le_bytes());
}

#[test]
fn extreme_products_are_total_and_saturating() {
    let positive = ArcticWeight::new(f64::MAX);
    let negative = ArcticWeight::new(-f64::MAX);
    assert_eq!(positive.times(&positive), positive);
    assert_eq!(negative.times(&negative), negative);
    assert_eq!(positive.times(&ArcticWeight::zero()), ArcticWeight::zero());
}
