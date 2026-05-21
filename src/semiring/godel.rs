//! Gödel fuzzy logic semiring for soft constraint satisfaction.
//!
//! The Gödel semiring ([0,1], max, min, 0, 1) is used in fuzzy logic and
//! soft constraint satisfaction problems:
//!
//! - **⊕ = max**: Selects the best (maximum) of parallel fuzzy memberships
//! - **⊗ = min**: Computes conjunction (AND) of fuzzy memberships
//! - **0̄ = 0**: Represents complete non-membership (false)
//! - **1̄ = 1**: Represents complete membership (true)
//!
//! # Use Cases
//!
//! - **Fuzzy string matching**: Path confidence is min of membership degrees
//! - **Soft constraint satisfaction**: Combining fuzzy constraint violations
//! - **Fuzzy language model combination**: Combining membership functions
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{Semiring, GodelWeight};
//!
//! let a = GodelWeight::new(0.7);
//! let b = GodelWeight::new(0.9);
//!
//! // max(0.7, 0.9) = 0.9
//! assert!((a.plus(&b).value() - 0.9).abs() < 1e-10);
//!
//! // min(0.7, 0.9) = 0.7
//! assert!((a.times(&b).value() - 0.7).abs() < 1e-10);
//! ```
//!
//! # Algebraic Properties
//!
//! - Idempotent: max(a, a) = a
//! - Zero-sum-free: max(a, b) = 0 implies a = b = 0
//! - Commutative: min(a, b) = min(b, a)
//! - Totally ordered: all values in [0, 1] are comparable
//! - Non-negative: domain is [0, 1]
//!
//! Note: GodelWeight does NOT implement StarSemiring or DivisibleSemiring
//! because min/max don't have well-defined closure or division operations.

use ordered_float::OrderedFloat;

use super::traits::{
    CommutativeTimesSemiring, IdempotentSemiring, NonnegativeSemiring, QuantizableSemiring,
    Semiring, StochasticSemiring, TotallyOrderedSemiring, ZeroSumFreeSemiring,
};

/// Gödel fuzzy logic semiring weight.
///
/// Represents a fuzzy membership degree in the range [0, 1].
/// Higher values indicate stronger membership.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct GodelWeight(pub OrderedFloat<f64>);

impl GodelWeight {
    /// Create a new Gödel weight from a raw f64.
    ///
    /// The value will be clamped to the [0, 1] range.
    #[inline]
    pub fn new(value: f64) -> Self {
        let clamped = value.clamp(0.0, 1.0);
        GodelWeight(OrderedFloat(clamped))
    }

    /// Create a new Gödel weight without clamping (for internal use).
    ///
    /// # Safety
    ///
    /// The caller must ensure the value is in [0, 1].
    #[inline]
    pub(crate) const fn new_unchecked(value: f64) -> Self {
        GodelWeight(OrderedFloat(value))
    }

    /// Get the underlying f64 value.
    #[inline]
    pub fn value(self) -> f64 {
        self.0.into_inner()
    }

    /// Check if this weight represents complete membership (1.0).
    #[inline]
    pub fn is_one_membership(self) -> bool {
        (self.0.into_inner() - 1.0).abs() < f64::EPSILON
    }

    /// Check if this weight represents complete non-membership (0.0).
    #[inline]
    pub fn is_zero_membership(self) -> bool {
        self.0.into_inner().abs() < f64::EPSILON
    }
}

impl From<f64> for GodelWeight {
    #[inline]
    fn from(value: f64) -> Self {
        GodelWeight::new(value)
    }
}

impl From<GodelWeight> for f64 {
    #[inline]
    fn from(weight: GodelWeight) -> Self {
        weight.value()
    }
}

impl Default for GodelWeight {
    /// Default is one (multiplicative identity), representing complete membership.
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl Semiring for GodelWeight {
    /// Additive identity: 0 (complete non-membership)
    ///
    /// max(a, 0) = a for all a in [0, 1]
    #[inline]
    fn zero() -> Self {
        GodelWeight::new_unchecked(0.0)
    }

    /// Multiplicative identity: 1 (complete membership)
    ///
    /// min(a, 1) = a for all a in [0, 1]
    #[inline]
    fn one() -> Self {
        GodelWeight::new_unchecked(1.0)
    }

    /// Addition: max(a, b)
    ///
    /// In fuzzy logic, this represents disjunction (OR).
    /// The result is the higher membership degree.
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        GodelWeight(self.0.max(other.0))
    }

    /// Multiplication: min(a, b)
    ///
    /// In fuzzy logic, this represents conjunction (AND).
    /// The result is limited by the weakest link.
    #[inline]
    fn times(&self, other: &Self) -> Self {
        GodelWeight(self.0.min(other.0))
    }

    #[inline]
    fn is_zero(&self) -> bool {
        self.is_zero_membership()
    }

    #[inline]
    fn is_one(&self) -> bool {
        self.is_one_membership()
    }

    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        (self.0.into_inner() - other.0.into_inner()).abs() <= epsilon
    }

    /// Natural ordering: higher is better (stronger membership).
    ///
    /// Unlike tropical semiring where lower is better, in Gödel semiring
    /// higher membership values are preferred.
    fn natural_less(&self, other: &Self) -> Option<bool> {
        // In Gödel semiring, higher values are "better"
        // So a is "less than" b (worse) if a.value < b.value
        Some(self.0 < other.0)
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.0.into_inner().to_le_bytes().to_vec()
    }
}

// ============================================================================
// Algebraic Property Marker Trait Implementations
// ============================================================================

/// GodelWeight is idempotent: max(a, a) = a
impl IdempotentSemiring for GodelWeight {}

/// GodelWeight is zero-sum-free: max(a, b) = 0 only if both a = 0 and b = 0
impl ZeroSumFreeSemiring for GodelWeight {}

/// GodelWeight has commutative multiplication: min(a, b) = min(b, a)
impl CommutativeTimesSemiring for GodelWeight {}

// ============================================================================
// Algorithm Requirement Trait Implementations
// ============================================================================

/// GodelWeight has a total order via OrderedFloat.
///
/// All values in [0, 1] are totally ordered.
impl TotallyOrderedSemiring for GodelWeight {}

/// GodelWeight is non-negative.
///
/// The domain is [0, 1], so all values are non-negative.
impl NonnegativeSemiring for GodelWeight {}

/// GodelWeight can be quantized for approximate comparison.
impl QuantizableSemiring for GodelWeight {
    fn quantize(&self, epsilon: f64) -> i64 {
        let v = self.value();
        if v.is_nan() {
            i64::MIN
        } else {
            (v / epsilon).round() as i64
        }
    }
}

/// GodelWeight can be converted to probability for sampling.
///
/// Since the domain is already [0, 1], the value can be used directly as a probability.
impl StochasticSemiring for GodelWeight {
    fn to_probability(&self) -> f64 {
        self.value()
    }
}

impl super::traits::NumericalWeight for GodelWeight {
    #[inline]
    fn numerical_value(&self) -> f64 {
        self.value()
    }
}

impl std::ops::Add for GodelWeight {
    type Output = Self;

    /// Operator `+` implements semiring ⊕ (max).
    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Mul for GodelWeight {
    type Output = Self;

    /// Operator `*` implements semiring ⊗ (min).
    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::AddAssign for GodelWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::MulAssign for GodelWeight {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for GodelWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.into_inner().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for GodelWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        f64::deserialize(deserializer).map(GodelWeight::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::traits::tests::{
        verify_commutative_times_semiring, verify_idempotent_semiring, verify_quantizable_semiring,
        verify_semiring_axioms, verify_stochastic_semiring, verify_totally_ordered_semiring,
        verify_zero_sum_free_semiring,
    };
    use proptest::prelude::*;

    #[test]
    fn test_basic_operations() {
        let a = GodelWeight::new(0.3);
        let b = GodelWeight::new(0.7);

        // Plus is max
        assert!((a.plus(&b).value() - 0.7).abs() < 1e-10);
        assert!((b.plus(&a).value() - 0.7).abs() < 1e-10);

        // Times is min
        assert!((a.times(&b).value() - 0.3).abs() < 1e-10);
        assert!((b.times(&a).value() - 0.3).abs() < 1e-10);
    }

    #[test]
    fn test_identities() {
        let a = GodelWeight::new(0.5);

        // Zero is additive identity: max(a, 0) = a
        assert!(a.plus(&GodelWeight::zero()).approx_eq(&a, 1e-10));
        assert!(GodelWeight::zero().plus(&a).approx_eq(&a, 1e-10));

        // One is multiplicative identity: min(a, 1) = a
        assert!(a.times(&GodelWeight::one()).approx_eq(&a, 1e-10));
        assert!(GodelWeight::one().times(&a).approx_eq(&a, 1e-10));
    }

    #[test]
    fn test_annihilation() {
        let a = GodelWeight::new(0.5);

        // Zero annihilates: min(a, 0) = 0
        assert!(a.times(&GodelWeight::zero()).is_zero());
        assert!(GodelWeight::zero().times(&a).is_zero());
    }

    #[test]
    fn test_clamping() {
        // Values outside [0, 1] should be clamped
        let neg = GodelWeight::new(-0.5);
        assert!((neg.value() - 0.0).abs() < 1e-10);

        let big = GodelWeight::new(1.5);
        assert!((big.value() - 1.0).abs() < 1e-10);

        // Values within range should be unchanged
        let mid = GodelWeight::new(0.5);
        assert!((mid.value() - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_idempotence() {
        let a = GodelWeight::new(0.5);
        // max(a, a) = a
        assert!(a.plus(&a).approx_eq(&a, 1e-10));
    }

    #[test]
    fn test_boundary_values() {
        let zero = GodelWeight::zero();
        let one = GodelWeight::one();

        // max(0, 1) = 1
        assert!(zero.plus(&one).approx_eq(&one, 1e-10));

        // min(0, 1) = 0
        assert!(zero.times(&one).approx_eq(&zero, 1e-10));

        // max(1, 1) = 1
        assert!(one.plus(&one).approx_eq(&one, 1e-10));

        // min(0, 0) = 0
        assert!(zero.times(&zero).approx_eq(&zero, 1e-10));
    }

    #[test]
    fn test_natural_ordering() {
        let low = GodelWeight::new(0.3);
        let high = GodelWeight::new(0.7);

        // Lower membership is "worse" (natural_less returns true)
        assert_eq!(low.natural_less(&high), Some(true));
        assert_eq!(high.natural_less(&low), Some(false));
        assert_eq!(low.natural_less(&low), Some(false));
    }

    #[test]
    fn test_fuzzy_conjunction() {
        // Classic fuzzy logic example: "tall AND heavy"
        let tall = GodelWeight::new(0.8); // 80% tall
        let heavy = GodelWeight::new(0.6); // 60% heavy

        // Conjunction is min: 60% "tall AND heavy"
        let conjunction = tall.times(&heavy);
        assert!((conjunction.value() - 0.6).abs() < 1e-10);
    }

    #[test]
    fn test_fuzzy_disjunction() {
        // Classic fuzzy logic example: "tall OR heavy"
        let tall = GodelWeight::new(0.8); // 80% tall
        let heavy = GodelWeight::new(0.6); // 60% heavy

        // Disjunction is max: 80% "tall OR heavy"
        let disjunction = tall.plus(&heavy);
        assert!((disjunction.value() - 0.8).abs() < 1e-10);
    }

    proptest! {
        #[test]
        fn proptest_semiring_axioms(
            a in 0.0f64..=1.0,
            b in 0.0f64..=1.0,
            c in 0.0f64..=1.0
        ) {
            let wa = GodelWeight::new(a);
            let wb = GodelWeight::new(b);
            let wc = GodelWeight::new(c);
            verify_semiring_axioms(wa, wb, wc, 1e-10);
        }

        #[test]
        fn proptest_idempotent_semiring(a in 0.0f64..=1.0) {
            let wa = GodelWeight::new(a);
            verify_idempotent_semiring(wa, 1e-10);
        }

        #[test]
        fn proptest_zero_sum_free_semiring(
            a in 0.0f64..=1.0,
            b in 0.0f64..=1.0
        ) {
            let wa = GodelWeight::new(a);
            let wb = GodelWeight::new(b);
            verify_zero_sum_free_semiring(wa, wb, 1e-10);
        }

        #[test]
        fn proptest_commutative_times_semiring(
            a in 0.0f64..=1.0,
            b in 0.0f64..=1.0
        ) {
            let wa = GodelWeight::new(a);
            let wb = GodelWeight::new(b);
            verify_commutative_times_semiring(wa, wb, 1e-10);
        }

        #[test]
        fn proptest_totally_ordered_semiring(
            a in 0.0f64..=1.0,
            b in 0.0f64..=1.0,
            c in 0.0f64..=1.0
        ) {
            let wa = GodelWeight::new(a);
            let wb = GodelWeight::new(b);
            let wc = GodelWeight::new(c);
            verify_totally_ordered_semiring(wa, wb, wc);
        }

        #[test]
        fn proptest_quantizable_semiring(a in 0.0f64..=1.0) {
            let wa = GodelWeight::new(a);
            verify_quantizable_semiring(wa, 1e-10);
        }

        #[test]
        fn proptest_stochastic_semiring(a in 0.0f64..=1.0) {
            let wa = GodelWeight::new(a);
            verify_stochastic_semiring(wa);
        }

        #[test]
        fn proptest_clamping(value in -10.0f64..10.0) {
            let w = GodelWeight::new(value);
            let v = w.value();
            prop_assert!(v >= 0.0 && v <= 1.0, "Value {} should be in [0, 1]", v);
        }

        #[test]
        fn proptest_min_max_consistency(
            a in 0.0f64..=1.0,
            b in 0.0f64..=1.0
        ) {
            let wa = GodelWeight::new(a);
            let wb = GodelWeight::new(b);

            // max(a, b) >= a and max(a, b) >= b
            let sum = wa.plus(&wb);
            prop_assert!(sum.value() >= a - 1e-10);
            prop_assert!(sum.value() >= b - 1e-10);

            // min(a, b) <= a and min(a, b) <= b
            let product = wa.times(&wb);
            prop_assert!(product.value() <= a + 1e-10);
            prop_assert!(product.value() <= b + 1e-10);
        }

        #[test]
        fn proptest_absorption_law(
            a in 0.0f64..=1.0,
            b in 0.0f64..=1.0
        ) {
            // Gödel semiring satisfies absorption: max(a, min(a, b)) = a
            let wa = GodelWeight::new(a);
            let wb = GodelWeight::new(b);

            let inner = wa.times(&wb);  // min(a, b)
            let result = wa.plus(&inner);  // max(a, min(a, b))
            prop_assert!(result.approx_eq(&wa, 1e-10), "Absorption law failed");
        }
    }
}
