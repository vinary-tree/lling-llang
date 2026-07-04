//! Expectation semiring for computing expected values over paths.
//!
//! The expectation semiring (ℝ × ℝ, ⊕, ⊗, (0,0), (1,0)) combines
//! probabilities with expected value computation:
//!
//! - **(x₁, y₁) ⊕ (x₂, y₂) = (x₁ + x₂, y₁ + y₂)**: Sum probabilities and expectations
//! - **(x₁, y₁) ⊗ (x₂, y₂) = (x₁·x₂, x₁·y₂ + x₂·y₁)**: Product rule for expectations
//! - **0̄ = (0, 0)**: Zero probability, zero expectation
//! - **1̄ = (1, 0)**: Certain event, zero cost
//!
//! # Components
//!
//! - `value`: The probability component (first element)
//! - `expectation`: The expected value accumulator (second element)
//!
//! # Use Cases
//!
//! - Computing expected path lengths/costs
//! - Relative entropy (KL divergence) between automata
//! - Gradient computation in differentiable WFSTs
//! - Risk-based optimization
//!
//! # Interpretation
//!
//! For a path with probability `p` and cost `c`:
//! - Initial weight: `(p, p·c)`
//! - After summing paths: `(Σp_i, Σp_i·c_i)`
//! - Expected cost: `expectation / value = (Σp_i·c_i) / (Σp_i)`
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{Semiring, ExpectationWeight};
//!
//! // Two paths with different probabilities and costs
//! let path1 = ExpectationWeight::from_probability_and_cost(0.3, 2.0);
//! let path2 = ExpectationWeight::from_probability_and_cost(0.5, 1.0);
//!
//! // Sum paths: total prob = 0.8, weighted cost sum = 0.3*2 + 0.5*1 = 1.1
//! let total = path1.plus(&path2);
//! assert!((total.value() - 0.8).abs() < 1e-10);
//!
//! // Expected cost = 1.1 / 0.8 = 1.375
//! let expected = total.expected_value().expect("semiring/expectation.rs: required value was None/Err");
//! assert!((expected - 1.375).abs() < 1e-10);
//! ```

use ordered_float::OrderedFloat;

use super::super::traits::{
    CommutativeTimesSemiring, DivisibleSemiring, KClosedSemiring, QuantizableSemiring, Semiring,
    StarSemiring, TotallyOrderedSemiring, WeaklyLeftDivisibleSemiring, ZeroSumFreeSemiring,
};

/// Expectation semiring weight.
///
/// Stores a (value, expectation) pair for computing expected values
/// over weighted paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExpectationWeight {
    /// Probability/weight component.
    value: OrderedFloat<f64>,
    /// Expected value accumulator (probability × cost).
    expectation: OrderedFloat<f64>,
}

impl ExpectationWeight {
    /// Create a new expectation weight from components.
    #[inline]
    pub fn new(value: f64, expectation: f64) -> Self {
        ExpectationWeight {
            value: OrderedFloat(value),
            expectation: OrderedFloat(expectation),
        }
    }

    /// Create from a probability (with zero cost).
    #[inline]
    pub fn from_probability(prob: f64) -> Self {
        Self::new(prob, 0.0)
    }

    /// Create from probability and cost.
    ///
    /// The expectation is set to `prob * cost`.
    #[inline]
    pub fn from_probability_and_cost(prob: f64, cost: f64) -> Self {
        Self::new(prob, prob * cost)
    }

    /// Create from negative log probability and cost.
    ///
    /// Converts from log space: `prob = exp(-neg_log_prob)`.
    #[inline]
    pub fn from_log_probability_and_cost(neg_log_prob: f64, cost: f64) -> Self {
        let prob = (-neg_log_prob).exp();
        Self::from_probability_and_cost(prob, cost)
    }

    /// Get the value (probability) component.
    #[inline]
    pub fn value(&self) -> f64 {
        self.value.into_inner()
    }

    /// Get the expectation component.
    #[inline]
    pub fn expectation(&self) -> f64 {
        self.expectation.into_inner()
    }

    /// Compute the expected value: expectation / value.
    ///
    /// Returns `None` if value is zero (no paths).
    #[inline]
    pub fn expected_value(&self) -> Option<f64> {
        let v = self.value.into_inner();
        if v == 0.0 {
            None
        } else {
            Some(self.expectation.into_inner() / v)
        }
    }

    /// Get both components as a tuple.
    #[inline]
    pub fn components(&self) -> (f64, f64) {
        (self.value.into_inner(), self.expectation.into_inner())
    }
}

impl Default for ExpectationWeight {
    /// Default is one (1, 0).
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl From<(f64, f64)> for ExpectationWeight {
    fn from((value, expectation): (f64, f64)) -> Self {
        ExpectationWeight::new(value, expectation)
    }
}

impl From<ExpectationWeight> for (f64, f64) {
    fn from(w: ExpectationWeight) -> Self {
        w.components()
    }
}

impl Semiring for ExpectationWeight {
    /// Additive identity: (0, 0)
    #[inline]
    fn zero() -> Self {
        ExpectationWeight::new(0.0, 0.0)
    }

    /// Multiplicative identity: (1, 0)
    #[inline]
    fn one() -> Self {
        ExpectationWeight::new(1.0, 0.0)
    }

    /// Addition: component-wise sum.
    ///
    /// (x₁, y₁) ⊕ (x₂, y₂) = (x₁ + x₂, y₁ + y₂)
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        ExpectationWeight::new(
            self.value.into_inner() + other.value.into_inner(),
            self.expectation.into_inner() + other.expectation.into_inner(),
        )
    }

    /// Multiplication: product rule for expectations.
    ///
    /// (x₁, y₁) ⊗ (x₂, y₂) = (x₁·x₂, x₁·y₂ + x₂·y₁)
    ///
    /// This follows from: E[c₁ + c₂] = E[c₁] + E[c₂]
    /// When costs are additive along paths.
    #[inline]
    fn times(&self, other: &Self) -> Self {
        let x1 = self.value.into_inner();
        let y1 = self.expectation.into_inner();
        let x2 = other.value.into_inner();
        let y2 = other.expectation.into_inner();

        ExpectationWeight::new(x1 * x2, x1 * y2 + x2 * y1)
    }

    #[inline]
    fn is_zero(&self) -> bool {
        self.value.into_inner() == 0.0 && self.expectation.into_inner() == 0.0
    }

    #[inline]
    fn is_one(&self) -> bool {
        self.value.into_inner() == 1.0 && self.expectation.into_inner() == 0.0
    }

    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        (self.value.into_inner() - other.value.into_inner()).abs() <= epsilon
            && (self.expectation.into_inner() - other.expectation.into_inner()).abs() <= epsilon
    }

    /// Natural ordering: higher value (probability) is "better".
    fn natural_less(&self, other: &Self) -> Option<bool> {
        // Compare by value first, then by expectation
        if (self.value.into_inner() - other.value.into_inner()).abs() < 1e-10 {
            // Equal values: lower expectation is "better" (lower cost)
            Some(self.expectation < other.expectation)
        } else {
            // Higher value (probability) is "better"
            Some(self.value > other.value)
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend(self.value.into_inner().to_le_bytes());
        bytes.extend(self.expectation.into_inner().to_le_bytes());
        bytes
    }
}

impl DivisibleSemiring for ExpectationWeight {
    /// Division: inverse of multiplication.
    ///
    /// For (x₁, y₁) ÷ (x₂, y₂), we need (a, b) such that:
    /// (a, b) ⊗ (x₂, y₂) = (x₁, y₁)
    /// (a·x₂, a·y₂ + x₂·b) = (x₁, y₁)
    ///
    /// So: a = x₁/x₂
    ///     b = (y₁ - a·y₂) / x₂ = (y₁ - x₁·y₂/x₂) / x₂ = (y₁·x₂ - x₁·y₂) / x₂²
    fn divide(&self, other: &Self) -> Option<Self> {
        let x1 = self.value.into_inner();
        let y1 = self.expectation.into_inner();
        let x2 = other.value.into_inner();
        let y2 = other.expectation.into_inner();

        if x2 == 0.0 {
            return None;
        }

        let a = x1 / x2;
        let b = (y1 * x2 - x1 * y2) / (x2 * x2);

        Some(ExpectationWeight::new(a, b))
    }
}

impl StarSemiring for ExpectationWeight {
    /// Kleene closure for expectation semiring.
    ///
    /// For (x, y), we need (x, y)* = Σ_{n=0}^∞ (x, y)ⁿ
    ///
    /// This converges when |x| < 1:
    /// - Value component: x* = 1/(1-x) (geometric series)
    /// - Expectation component: Derived from d/dx of geometric series
    ///
    /// The n-th power: (x, y)ⁿ = (xⁿ, n·xⁿ⁻¹·y)
    /// Sum: (Σxⁿ, y·Σn·xⁿ⁻¹) = (1/(1-x), y·d/dx(1/(1-x))) = (1/(1-x), y/(1-x)²)
    fn star(&self) -> Option<Self> {
        let x = self.value.into_inner();
        let y = self.expectation.into_inner();

        if x >= 1.0 {
            return None;
        }

        let one_minus_x = 1.0 - x;
        let star_value = 1.0 / one_minus_x;
        let star_expectation = y / (one_minus_x * one_minus_x);

        Some(ExpectationWeight::new(star_value, star_expectation))
    }
}

// ============================================================================
// Algebraic Property Marker Trait Implementations
// ============================================================================

// Note: ExpectationWeight is NOT idempotent.
// (a, b) ⊕ (a, b) = (2a, 2b) ≠ (a, b) for non-zero values.

/// ExpectationWeight is k-closed with no uniform bound.
///
/// The star operation converges when value < 1, but there's no fixed k
/// that works for all values.
impl KClosedSemiring for ExpectationWeight {
    fn closure_bound() -> Option<usize> {
        // No uniform bound - depends on the specific value
        None
    }
}

/// ExpectationWeight is zero-sum-free.
///
/// (x₁, y₁) ⊕ (x₂, y₂) = (0, 0) implies x₁ + x₂ = 0 and y₁ + y₂ = 0.
/// For non-negative values, this means x₁ = x₂ = 0 and y₁ = y₂ = 0.
impl ZeroSumFreeSemiring for ExpectationWeight {}

/// ExpectationWeight is weakly left divisible.
///
/// For non-zero divisor (x₂, y₂), we can compute the left quotient.
impl WeaklyLeftDivisibleSemiring for ExpectationWeight {
    fn left_divide(&self, divisor: &Self) -> Option<Self> {
        // left_divide computes c such that c ⊗ divisor = self
        // This is the same as regular division for this semiring
        self.divide(divisor)
    }
}

/// ExpectationWeight has commutative multiplication.
///
/// (x₁, y₁) ⊗ (x₂, y₂) = (x₁·x₂, x₁·y₂ + x₂·y₁)
/// (x₂, y₂) ⊗ (x₁, y₁) = (x₂·x₁, x₂·y₁ + x₁·y₂) = (x₁·x₂, x₁·y₂ + x₂·y₁) ✓
impl CommutativeTimesSemiring for ExpectationWeight {}

// ============================================================================
// Algorithm Requirement Trait Implementations
// ============================================================================

/// ExpectationWeight has a total order (lexicographic on (value, expectation)).
impl TotallyOrderedSemiring for ExpectationWeight {}

// Note: ExpectationWeight does NOT implement NonnegativeSemiring because
// the expectation component can be negative (costs can be negative).

/// ExpectationWeight can be quantized for approximate comparison.
///
/// Quantizes both components and combines them into a single i64 hash.
impl QuantizableSemiring for ExpectationWeight {
    fn quantize(&self, epsilon: f64) -> i64 {
        let v = self.value();
        let e = self.expectation();

        // Quantize both components
        let qv = if v.is_nan() {
            i64::MIN
        } else if v.is_infinite() {
            if v > 0.0 {
                i64::MAX / 2
            } else {
                i64::MIN / 2
            }
        } else {
            (v / epsilon).round() as i64
        };

        let qe = if e.is_nan() {
            0
        } else if e.is_infinite() {
            if e > 0.0 {
                i32::MAX as i64
            } else {
                i32::MIN as i64
            }
        } else {
            ((e / epsilon).round() as i32) as i64
        };

        // Combine: use upper bits for value, lower bits for expectation
        // This preserves the lexicographic ordering
        (qv.wrapping_shl(32)) ^ (qe & 0xFFFFFFFF)
    }
}

// Note: ExpectationWeight does NOT implement StochasticSemiring because
// the expectation semiring doesn't have a direct probability interpretation.
// The value component is a probability, but the expected value computation
// is more complex than simple probability sampling.

impl std::ops::Add for ExpectationWeight {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Mul for ExpectationWeight {
    type Output = Self;

    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::AddAssign for ExpectationWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::MulAssign for ExpectationWeight {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

impl PartialOrd for ExpectationWeight {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ExpectationWeight {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.value.cmp(&other.value) {
            std::cmp::Ordering::Equal => self.expectation.cmp(&other.expectation),
            ord => ord,
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ExpectationWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        (self.value.into_inner(), self.expectation.into_inner()).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ExpectationWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (value, expectation) = <(f64, f64)>::deserialize(deserializer)?;
        Ok(ExpectationWeight::new(value, expectation))
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::traits::tests::{
        verify_commutative_times_semiring, verify_divisible_semiring, verify_k_closed_semiring,
        verify_quantizable_semiring, verify_semiring_axioms, verify_star_semiring,
        verify_totally_ordered_semiring, verify_weakly_left_divisible_semiring,
        verify_zero_sum_free_semiring,
    };
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_basic_operations() {
        let a = ExpectationWeight::new(0.3, 0.6);
        let b = ExpectationWeight::new(0.5, 0.5);

        // Plus: (0.3 + 0.5, 0.6 + 0.5) = (0.8, 1.1)
        let sum = a.plus(&b);
        assert!((sum.value() - 0.8).abs() < 1e-10);
        assert!((sum.expectation() - 1.1).abs() < 1e-10);

        // Times: (0.3 * 0.5, 0.3 * 0.5 + 0.5 * 0.6) = (0.15, 0.15 + 0.3) = (0.15, 0.45)
        let prod = a.times(&b);
        assert!((prod.value() - 0.15).abs() < 1e-10);
        assert!((prod.expectation() - 0.45).abs() < 1e-10);
    }

    #[test]
    fn test_identities() {
        let a = ExpectationWeight::new(0.5, 0.3);

        // Zero is additive identity
        let sum = a.plus(&ExpectationWeight::zero());
        assert!(a.approx_eq(&sum, 1e-10));

        // One is multiplicative identity
        let prod = a.times(&ExpectationWeight::one());
        assert!(a.approx_eq(&prod, 1e-10));

        let prod2 = ExpectationWeight::one().times(&a);
        assert!(a.approx_eq(&prod2, 1e-10));
    }

    #[test]
    fn test_annihilation() {
        let a = ExpectationWeight::new(0.5, 0.3);

        // Zero annihilates
        let prod = a.times(&ExpectationWeight::zero());
        assert!(prod.is_zero());

        let prod2 = ExpectationWeight::zero().times(&a);
        assert!(prod2.is_zero());
    }

    #[test]
    fn test_expected_value() {
        // Path with prob=0.3, cost=2: contribution = (0.3, 0.6)
        let path1 = ExpectationWeight::from_probability_and_cost(0.3, 2.0);
        assert!((path1.value() - 0.3).abs() < 1e-10);
        assert!((path1.expectation() - 0.6).abs() < 1e-10);

        // Path with prob=0.5, cost=1: contribution = (0.5, 0.5)
        let path2 = ExpectationWeight::from_probability_and_cost(0.5, 1.0);

        // Sum: (0.8, 1.1)
        let total = path1.plus(&path2);

        // Expected cost = 1.1 / 0.8 = 1.375
        let expected = total.expected_value().expect("Non-zero total");
        assert!(
            (expected - 1.375).abs() < 1e-10,
            "Expected 1.375, got {}",
            expected
        );
    }

    #[test]
    fn test_division() {
        let a = ExpectationWeight::new(0.3, 0.6);
        let b = ExpectationWeight::new(0.5, 0.4);

        // (a * b) / b = a
        let product = a.times(&b);
        let quotient = product.divide(&b).expect("Division should succeed");
        assert!(
            a.approx_eq(&quotient, 1e-10),
            "Division inverse failed: ({}, {}) * ({}, {}) / ({}, {}) = ({}, {}), expected ({}, {})",
            a.value(),
            a.expectation(),
            b.value(),
            b.expectation(),
            b.value(),
            b.expectation(),
            quotient.value(),
            quotient.expectation(),
            a.value(),
            a.expectation()
        );

        // Division by zero returns None
        assert!(a.divide(&ExpectationWeight::zero()).is_none());
    }

    #[test]
    fn test_star() {
        // For (x, y) with x < 1: star = (1/(1-x), y/(1-x)²)
        let half = ExpectationWeight::new(0.5, 0.2);
        let star = half.star().expect("Star should converge for x < 1");

        // star_value = 1/(1-0.5) = 2
        assert!((star.value() - 2.0).abs() < 1e-10);

        // star_expectation = 0.2/(1-0.5)² = 0.2/0.25 = 0.8
        assert!((star.expectation() - 0.8).abs() < 1e-10);

        // Verify semiring property: star = 1 ⊕ (w ⊗ star)
        let one_plus_w_star = ExpectationWeight::one().plus(&half.times(&star));
        assert!(
            star.approx_eq(&one_plus_w_star, 1e-6),
            "Star axiom failed: ({}, {}) ≠ 1 ⊕ (w ⊗ star) = ({}, {})",
            star.value(),
            star.expectation(),
            one_plus_w_star.value(),
            one_plus_w_star.expectation()
        );

        // x = 1 should not converge
        assert!(ExpectationWeight::one().star().is_none());

        // x > 1 should not converge
        assert!(ExpectationWeight::new(1.5, 0.1).star().is_none());
    }

    #[test]
    fn test_multiplicative_identity_property() {
        // (1, 0) is the multiplicative identity
        let one = ExpectationWeight::one();
        let a = ExpectationWeight::new(0.3, 0.5);

        // one * a = a
        let prod1 = one.times(&a);
        assert!(prod1.approx_eq(&a, 1e-10));

        // a * one = a
        let prod2 = a.times(&one);
        assert!(prod2.approx_eq(&a, 1e-10));
    }

    #[test]
    fn test_sequential_costs() {
        // Sequential composition: if we traverse edge 1 then edge 2,
        // total prob = p1 * p2, total cost = c1 + c2

        // Edge 1: prob=0.5, cost=2
        let e1 = ExpectationWeight::from_probability_and_cost(0.5, 2.0);
        // Edge 2: prob=0.4, cost=3
        let e2 = ExpectationWeight::from_probability_and_cost(0.4, 3.0);

        let path = e1.times(&e2);

        // Total prob = 0.5 * 0.4 = 0.2
        assert!((path.value() - 0.2).abs() < 1e-10);

        // Expected cost = (c1 + c2) = 5, so expectation = 0.2 * 5 = 1.0
        // Using the formula: x1*y2 + x2*y1 = 0.5*1.2 + 0.4*1.0 = 0.6 + 0.4 = 1.0
        assert!((path.expectation() - 1.0).abs() < 1e-10);

        // Verify expected value
        let expected = path.expected_value().expect("Non-zero path");
        assert!(
            (expected - 5.0).abs() < 1e-10,
            "Expected cost 5, got {}",
            expected
        );
    }

    proptest! {
        #[test]
        fn proptest_semiring_axioms(
            v1 in 0.0f64..10.0,
            e1 in -10.0f64..10.0,
            v2 in 0.0f64..10.0,
            e2 in -10.0f64..10.0,
            v3 in 0.0f64..10.0,
            e3 in -10.0f64..10.0
        ) {
            let wa = ExpectationWeight::new(v1, e1);
            let wb = ExpectationWeight::new(v2, e2);
            let wc = ExpectationWeight::new(v3, e3);
            verify_semiring_axioms(wa, wb, wc, 1e-6);
        }

        #[test]
        fn proptest_divisible_semiring(
            v1 in 0.0f64..10.0,
            e1 in -10.0f64..10.0,
            v2 in 0.001f64..10.0, // Avoid near-zero
            e2 in -10.0f64..10.0
        ) {
            let wa = ExpectationWeight::new(v1, e1);
            let wb = ExpectationWeight::new(v2, e2);
            verify_divisible_semiring(wa, wb, 1e-6);
        }

        #[test]
        fn proptest_star_semiring(
            v in 0.001f64..0.999,
            e in -10.0f64..10.0
        ) {
            let w = ExpectationWeight::new(v, e);
            verify_star_semiring(w, 1e-4);
        }

        #[test]
        fn proptest_k_closed_semiring(
            v in 0.0f64..10.0,
            e in -10.0f64..10.0
        ) {
            let w = ExpectationWeight::new(v, e);
            verify_k_closed_semiring(w, 1e-6);
        }

        #[test]
        fn proptest_zero_sum_free_semiring(
            v1 in 0.0f64..10.0,
            e1 in 0.0f64..10.0, // Use non-negative expectations for zero-sum-free verification
            v2 in 0.0f64..10.0,
            e2 in 0.0f64..10.0
        ) {
            let wa = ExpectationWeight::new(v1, e1);
            let wb = ExpectationWeight::new(v2, e2);
            verify_zero_sum_free_semiring(wa, wb, 1e-6);
        }

        #[test]
        fn proptest_weakly_left_divisible_semiring(
            v1 in 0.0f64..10.0,
            e1 in -10.0f64..10.0,
            v2 in 0.0f64..10.0,
            e2 in -10.0f64..10.0
        ) {
            let wa = ExpectationWeight::new(v1, e1);
            let wb = ExpectationWeight::new(v2, e2);
            verify_weakly_left_divisible_semiring(wa, wb, 1e-6);
        }

        #[test]
        fn proptest_commutative_times_semiring(
            v1 in 0.0f64..10.0,
            e1 in -10.0f64..10.0,
            v2 in 0.0f64..10.0,
            e2 in -10.0f64..10.0
        ) {
            let wa = ExpectationWeight::new(v1, e1);
            let wb = ExpectationWeight::new(v2, e2);
            verify_commutative_times_semiring(wa, wb, 1e-6);
        }

        #[test]
        fn proptest_totally_ordered_semiring(
            v1 in 0.0f64..10.0,
            e1 in -10.0f64..10.0,
            v2 in 0.0f64..10.0,
            e2 in -10.0f64..10.0,
            v3 in 0.0f64..10.0,
            e3 in -10.0f64..10.0
        ) {
            let wa = ExpectationWeight::new(v1, e1);
            let wb = ExpectationWeight::new(v2, e2);
            let wc = ExpectationWeight::new(v3, e3);
            verify_totally_ordered_semiring(wa, wb, wc);
        }

        #[test]
        fn proptest_quantizable_semiring(
            v in 0.0f64..10.0,
            e in -10.0f64..10.0
        ) {
            let wa = ExpectationWeight::new(v, e);
            verify_quantizable_semiring(wa, 1e-10);
        }
    }

    #[test]
    fn test_k_closed_bound() {
        // ExpectationWeight has no uniform closure bound
        assert_eq!(ExpectationWeight::closure_bound(), None);
    }
}
