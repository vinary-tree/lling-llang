//! Arctic (max-plus) semiring for maximum-score path optimization.
//!
//! The arctic semiring is `(R union {-infinity}, max, +, -infinity, 0)`:
//!
//! - `plus` selects the larger score among parallel paths;
//! - `times` accumulates scores along a path;
//! - negative infinity represents an unreachable path; and
//! - zero is the score of the empty path.
//!
//! Unlike a non-negative tropical cost, an arctic transition may be a gain or
//! a penalty. Consequently [`ArcticWeight`] deliberately does **not** implement
//! `NonnegativeSemiring` or `KClosedSemiring`. Algorithms that require Dijkstra
//! monotonicity or a uniform closure bound must reject this type at compile
//! time.

use ordered_float::OrderedFloat;

use super::super::traits::{
    CommutativeTimesSemiring, IdempotentSemiring, NumericalWeight, QuantizableSemiring, Semiring,
    StarSemiring, TotallyOrderedSemiring, ZeroSumFreeSemiring,
};

/// A max-plus score.
///
/// Finite values are reachable scores and negative infinity is the additive
/// identity. `NaN` and positive infinity are outside the verified carrier.
/// Sequential overflow clamps to the corresponding largest finite value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ArcticWeight(pub OrderedFloat<f64>);

impl ArcticWeight {
    /// Whether `value` belongs to `R union {-infinity}`.
    #[inline]
    pub fn is_valid_raw(value: f64) -> bool {
        value.is_finite() || (value.is_infinite() && value.is_sign_negative())
    }

    /// Construct a verified-domain score.
    #[inline]
    pub fn new(value: f64) -> Self {
        Self::try_new(value).expect("arctic weight must be finite or -infinity")
    }

    /// Try to construct a verified-domain score.
    #[inline]
    pub fn try_new(value: f64) -> Option<Self> {
        Self::is_valid_raw(value).then_some(Self(OrderedFloat(value)))
    }

    /// Construct without validating the mathematical carrier.
    ///
    /// This exists for byte-preserving interoperation. Semiring algorithms
    /// should use [`Self::new`] or [`Self::try_new`].
    #[inline]
    pub const fn new_unchecked(value: f64) -> Self {
        Self(OrderedFloat(value))
    }

    /// Return the raw score.
    #[inline]
    pub fn value(self) -> f64 {
        self.0.into_inner()
    }

    /// Return the unreachable score.
    #[inline]
    pub const fn neg_infinity() -> Self {
        Self::new_unchecked(f64::NEG_INFINITY)
    }

    /// Whether this score is the unreachable additive identity.
    #[inline]
    pub fn is_neg_infinite(self) -> bool {
        self.value() == f64::NEG_INFINITY
    }

    /// Add two reachable scores without leaving the checked carrier.
    ///
    /// IEEE-754 overflow is clamped to the largest finite value with the same
    /// sign. The unreachable value is handled by [`Semiring::times`] before
    /// this helper is called, so finite inputs can never produce `NaN`.
    #[inline]
    fn saturating_score_add(left: f64, right: f64) -> f64 {
        let sum = left + right;
        if sum == f64::INFINITY {
            f64::MAX
        } else if sum == f64::NEG_INFINITY {
            -f64::MAX
        } else {
            sum
        }
    }
}

impl From<f64> for ArcticWeight {
    #[inline]
    fn from(value: f64) -> Self {
        Self::new(value)
    }
}

impl From<ArcticWeight> for f64 {
    #[inline]
    fn from(weight: ArcticWeight) -> Self {
        weight.value()
    }
}

impl Default for ArcticWeight {
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl Semiring for ArcticWeight {
    #[inline]
    fn zero() -> Self {
        Self::neg_infinity()
    }

    #[inline]
    fn one() -> Self {
        Self::new(0.0)
    }

    #[inline]
    fn plus(&self, other: &Self) -> Self {
        Self(self.0.max(other.0))
    }

    #[inline]
    fn times(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            Self::zero()
        } else {
            Self::new(Self::saturating_score_add(self.value(), other.value()))
        }
    }

    #[inline]
    fn is_zero(&self) -> bool {
        self.is_neg_infinite()
    }

    #[inline]
    fn is_one(&self) -> bool {
        self.value() == 0.0
    }

    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        match (self.is_zero(), other.is_zero()) {
            (true, true) => true,
            (true, false) | (false, true) => false,
            (false, false) => (self.value() - other.value()).abs() <= epsilon,
        }
    }

    /// Larger scores are better in the max-plus natural order.
    #[inline]
    fn natural_less(&self, other: &Self) -> Option<bool> {
        Some(self.0 > other.0)
    }

    #[inline]
    fn to_bytes(&self) -> Vec<u8> {
        self.value().to_le_bytes().to_vec()
    }
}

impl NumericalWeight for ArcticWeight {
    #[inline]
    fn numerical_value(&self) -> f64 {
        self.value()
    }
}

impl StarSemiring for ArcticWeight {
    /// `max(0, a, 2a, ...)` converges to zero exactly when `a <= 0`.
    fn star(&self) -> Option<Self> {
        (self.value() <= 0.0).then(Self::one)
    }
}

impl IdempotentSemiring for ArcticWeight {}
impl ZeroSumFreeSemiring for ArcticWeight {}
impl CommutativeTimesSemiring for ArcticWeight {}
impl TotallyOrderedSemiring for ArcticWeight {}

impl QuantizableSemiring for ArcticWeight {
    fn quantize(&self, epsilon: f64) -> i64 {
        let value = self.value();
        if value == f64::NEG_INFINITY {
            i64::MIN
        } else {
            (value / epsilon).round() as i64
        }
    }
}

impl std::ops::Add for ArcticWeight {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Mul for ArcticWeight {
    type Output = Self;

    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::AddAssign for ArcticWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::MulAssign for ArcticWeight {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ArcticWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.value().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ArcticWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let value = f64::deserialize(deserializer)?;
        Self::try_new(value)
            .ok_or_else(|| D::Error::custom("arctic weight must be finite or -infinity"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::traits::tests::{
        verify_commutative_times_semiring, verify_idempotent_semiring, verify_quantizable_semiring,
        verify_semiring_axioms, verify_totally_ordered_semiring, verify_zero_sum_free_semiring,
    };
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn max_plus_operations_and_identities() {
        let low = ArcticWeight::new(-3.0);
        let high = ArcticWeight::new(5.0);
        assert_eq!(low.plus(&high), high);
        assert_eq!(low.times(&high), ArcticWeight::new(2.0));
        assert_eq!(high.plus(&ArcticWeight::zero()), high);
        assert_eq!(high.times(&ArcticWeight::zero()), ArcticWeight::zero());
    }

    #[test]
    fn constructor_rejects_non_carrier_values() {
        assert_eq!(
            ArcticWeight::try_new(f64::NEG_INFINITY),
            Some(ArcticWeight::zero())
        );
        assert!(ArcticWeight::try_new(f64::INFINITY).is_none());
        assert!(ArcticWeight::try_new(f64::NAN).is_none());
    }

    #[test]
    fn star_distinguishes_penalty_from_positive_cycle() {
        assert_eq!(ArcticWeight::new(-1.0).star(), Some(ArcticWeight::one()));
        assert_eq!(ArcticWeight::one().star(), Some(ArcticWeight::one()));
        assert_eq!(ArcticWeight::new(1.0).star(), None);
    }

    #[test]
    fn natural_order_prefers_larger_scores() {
        assert_eq!(
            ArcticWeight::new(5.0).natural_less(&ArcticWeight::new(2.0)),
            Some(true)
        );
    }

    #[test]
    fn multiplication_is_closed_at_both_ieee_overflow_boundaries() {
        let maximum = ArcticWeight::new(f64::MAX);
        let minimum = ArcticWeight::new(-f64::MAX);
        assert_eq!(maximum.times(&maximum), maximum);
        assert_eq!(minimum.times(&minimum), minimum);
        assert!(ArcticWeight::is_valid_raw(maximum.times(&maximum).value()));
        assert!(ArcticWeight::is_valid_raw(minimum.times(&minimum).value()));
        assert_eq!(maximum.times(&ArcticWeight::zero()), ArcticWeight::zero());
    }

    #[test]
    fn overflow_policy_is_commutative_but_not_divisible() {
        let maximum = ArcticWeight::new(f64::MAX);
        let half = ArcticWeight::new(f64::MAX / 2.0);
        assert_eq!(maximum.times(&half), half.times(&maximum));
        assert_ne!(
            maximum.times(&maximum).value() - maximum.value(),
            maximum.value()
        );
    }

    proptest! {
        #[test]
        fn exact_integer_domain_satisfies_bitwise_associativity(
            a in -1_000_000i32..1_000_000,
            b in -1_000_000i32..1_000_000,
            c in -1_000_000i32..1_000_000,
        ) {
            let a = ArcticWeight::new(f64::from(a));
            let b = ArcticWeight::new(f64::from(b));
            let c = ArcticWeight::new(f64::from(c));
            prop_assert_eq!(a.times(&b).times(&c), a.times(&b.times(&c)));
            prop_assert_eq!(a.times(&b.plus(&c)), a.times(&b).plus(&a.times(&c)));
        }

        #[test]
        fn algebraic_laws(
            a in -1_000.0f64..1_000.0,
            b in -1_000.0f64..1_000.0,
            c in -1_000.0f64..1_000.0,
        ) {
            let a = ArcticWeight::new(a);
            let b = ArcticWeight::new(b);
            let c = ArcticWeight::new(c);
            verify_semiring_axioms(a, b, c, 1e-9);
            verify_idempotent_semiring(a, 1e-9);
            verify_zero_sum_free_semiring(a, b, 1e-9);
            verify_commutative_times_semiring(a, b, 1e-9);
            verify_totally_ordered_semiring(a, b, c);
            verify_quantizable_semiring(a, 1e-9);
        }

        #[test]
        fn extreme_finite_multiplication_remains_in_carrier_and_commutative(
            a in prop_oneof![Just(f64::MAX), Just(-f64::MAX), any::<f64>().prop_filter("finite", |x| x.is_finite())],
            b in prop_oneof![Just(f64::MAX), Just(-f64::MAX), any::<f64>().prop_filter("finite", |x| x.is_finite())],
        ) {
            let a = ArcticWeight::new(a);
            let b = ArcticWeight::new(b);
            let forward = a.times(&b);
            let reverse = b.times(&a);
            prop_assert!(ArcticWeight::is_valid_raw(forward.value()));
            prop_assert_eq!(forward, reverse);
        }
    }
}
