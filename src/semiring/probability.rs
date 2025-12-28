//! Probability semiring for direct probability operations.
//!
//! The probability semiring (ℝ₊ ∪ {0}, +, ×, 0, 1) operates directly on
//! probability values:
//!
//! - **⊕ = +**: Sum probabilities for parallel paths
//! - **⊗ = ×**: Multiply probabilities for sequential transitions
//! - **0̄ = 0**: Represents impossible events
//! - **1̄ = 1**: Represents certain events
//!
//! # Comparison with Log Semiring
//!
//! Use the probability semiring when:
//! - Probabilities are small enough to avoid underflow
//! - Direct probability arithmetic is needed
//! - Converting between probability and log space frequently
//!
//! Use the log semiring when:
//! - Working with very small probabilities
//! - Numerical stability is critical
//! - Performing many multiplications (which become additions in log space)
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{Semiring, ProbabilityWeight};
//!
//! let a = ProbabilityWeight::new(0.3);
//! let b = ProbabilityWeight::new(0.5);
//!
//! // Sum probabilities: 0.3 + 0.5 = 0.8
//! assert!((a.plus(&b).value() - 0.8).abs() < 1e-10);
//!
//! // Multiply probabilities: 0.3 × 0.5 = 0.15
//! assert!((a.times(&b).value() - 0.15).abs() < 1e-10);
//! ```

use ordered_float::OrderedFloat;

use super::log::LogWeight;
use super::traits::{DivisibleSemiring, Semiring, StarSemiring};

/// Probability semiring weight.
///
/// Stores a non-negative probability value directly (not in log space).
/// Values are clamped to [0, ∞).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ProbabilityWeight(pub OrderedFloat<f64>);

impl ProbabilityWeight {
    /// Create a new probability weight.
    ///
    /// Negative values are clamped to 0.
    #[inline]
    pub fn new(value: f64) -> Self {
        ProbabilityWeight(OrderedFloat(value.max(0.0)))
    }

    /// Get the underlying probability value.
    #[inline]
    pub fn value(self) -> f64 {
        self.0.into_inner()
    }

    /// Convert from negative log probability.
    ///
    /// Computes `exp(-neg_log_prob)`.
    #[inline]
    pub fn from_log(neg_log_prob: f64) -> Self {
        if neg_log_prob.is_infinite() && neg_log_prob > 0.0 {
            Self::zero()
        } else {
            ProbabilityWeight::new((-neg_log_prob).exp())
        }
    }

    /// Convert to negative log probability.
    ///
    /// Computes `-log(self)`. Returns infinity for probability 0.
    #[inline]
    pub fn to_log(self) -> f64 {
        let v = self.0.into_inner();
        if v == 0.0 {
            f64::INFINITY
        } else {
            -v.ln()
        }
    }

    /// Convert to LogWeight.
    #[inline]
    pub fn to_log_weight(self) -> LogWeight {
        LogWeight::new(self.to_log())
    }

    /// Create from LogWeight.
    #[inline]
    pub fn from_log_weight(log_weight: LogWeight) -> Self {
        Self::from_log(log_weight.value())
    }
}

impl From<f64> for ProbabilityWeight {
    #[inline]
    fn from(value: f64) -> Self {
        ProbabilityWeight::new(value)
    }
}

impl From<ProbabilityWeight> for f64 {
    #[inline]
    fn from(weight: ProbabilityWeight) -> Self {
        weight.value()
    }
}

impl From<LogWeight> for ProbabilityWeight {
    #[inline]
    fn from(log_weight: LogWeight) -> Self {
        ProbabilityWeight::from_log_weight(log_weight)
    }
}

impl From<ProbabilityWeight> for LogWeight {
    #[inline]
    fn from(prob_weight: ProbabilityWeight) -> Self {
        prob_weight.to_log_weight()
    }
}

impl Default for ProbabilityWeight {
    /// Default is one (certain event).
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl Semiring for ProbabilityWeight {
    /// Additive identity: 0 (impossible event)
    #[inline]
    fn zero() -> Self {
        ProbabilityWeight::new(0.0)
    }

    /// Multiplicative identity: 1 (certain event)
    #[inline]
    fn one() -> Self {
        ProbabilityWeight::new(1.0)
    }

    /// Addition: sum of probabilities.
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        ProbabilityWeight::new(self.0.into_inner() + other.0.into_inner())
    }

    /// Multiplication: product of probabilities.
    #[inline]
    fn times(&self, other: &Self) -> Self {
        ProbabilityWeight::new(self.0.into_inner() * other.0.into_inner())
    }

    #[inline]
    fn is_zero(&self) -> bool {
        self.0.into_inner() == 0.0
    }

    #[inline]
    fn is_one(&self) -> bool {
        self.0.into_inner() == 1.0
    }

    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        (self.0.into_inner() - other.0.into_inner()).abs() <= epsilon
    }

    /// Natural ordering: larger probability is better (higher probability).
    fn natural_less(&self, other: &Self) -> Option<bool> {
        // Higher probability is "better", so self < other means self has lower prob
        Some(self.0 > other.0)
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.0.into_inner().to_le_bytes().to_vec()
    }
}

impl DivisibleSemiring for ProbabilityWeight {
    /// Division: quotient of probabilities.
    fn divide(&self, other: &Self) -> Option<Self> {
        if other.is_zero() {
            None
        } else {
            Some(ProbabilityWeight::new(
                self.0.into_inner() / other.0.into_inner(),
            ))
        }
    }
}

impl super::traits::NumericalWeight for ProbabilityWeight {
    #[inline]
    fn numerical_value(&self) -> f64 {
        self.value()
    }
}

impl StarSemiring for ProbabilityWeight {
    /// Kleene closure for probability semiring.
    ///
    /// For probability p:
    /// - p* = 1 + p + p² + p³ + ... = 1/(1-p) for |p| < 1
    /// - p = 1: series diverges
    /// - p > 1: series diverges
    fn star(&self) -> Option<Self> {
        let p = self.0.into_inner();
        if p >= 1.0 {
            // Series diverges
            None
        } else {
            // Geometric series: 1/(1-p)
            Some(ProbabilityWeight::new(1.0 / (1.0 - p)))
        }
    }
}

impl std::ops::Add for ProbabilityWeight {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Mul for ProbabilityWeight {
    type Output = Self;

    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::AddAssign for ProbabilityWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::MulAssign for ProbabilityWeight {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ProbabilityWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.into_inner().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ProbabilityWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        f64::deserialize(deserializer).map(ProbabilityWeight::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::traits::tests::{
        verify_divisible_semiring, verify_semiring_axioms, verify_star_semiring,
    };
    use proptest::prelude::*;

    #[test]
    fn test_basic_operations() {
        let a = ProbabilityWeight::new(0.3);
        let b = ProbabilityWeight::new(0.5);

        // Plus is addition
        let sum = a.plus(&b);
        assert!((sum.value() - 0.8).abs() < 1e-10);

        // Times is multiplication
        let prod = a.times(&b);
        assert!((prod.value() - 0.15).abs() < 1e-10);
    }

    #[test]
    fn test_identities() {
        let a = ProbabilityWeight::new(0.5);

        // Zero is additive identity
        assert!(a.plus(&ProbabilityWeight::zero()).approx_eq(&a, 1e-10));
        assert!(ProbabilityWeight::zero().plus(&a).approx_eq(&a, 1e-10));

        // One is multiplicative identity
        assert!(a.times(&ProbabilityWeight::one()).approx_eq(&a, 1e-10));
        assert!(ProbabilityWeight::one().times(&a).approx_eq(&a, 1e-10));
    }

    #[test]
    fn test_annihilation() {
        let a = ProbabilityWeight::new(0.5);

        // Zero annihilates
        assert!(a.times(&ProbabilityWeight::zero()).is_zero());
        assert!(ProbabilityWeight::zero().times(&a).is_zero());
    }

    #[test]
    fn test_division() {
        let a = ProbabilityWeight::new(0.3);
        let b = ProbabilityWeight::new(0.5);

        // (a * b) / b = a
        let product = a.times(&b);
        let quotient = product.divide(&b).expect("Division should succeed");
        assert!(
            a.approx_eq(&quotient, 1e-10),
            "Division inverse failed: {} * {} / {} = {}, expected {}",
            a.value(),
            b.value(),
            b.value(),
            quotient.value(),
            a.value()
        );

        // Division by zero returns None
        assert!(a.divide(&ProbabilityWeight::zero()).is_none());
    }

    #[test]
    fn test_star() {
        // For p = 0.5: star = 1/(1-0.5) = 2
        let half = ProbabilityWeight::new(0.5);
        let star = half.star().expect("Star should converge for p < 1");
        assert!(
            (star.value() - 2.0).abs() < 1e-10,
            "Star of 0.5 should be 2.0, got {}",
            star.value()
        );

        // For p = 0.25: star = 1/(1-0.25) = 4/3
        let quarter = ProbabilityWeight::new(0.25);
        let star_q = quarter.star().expect("Star should converge for p < 1");
        assert!(
            (star_q.value() - 4.0 / 3.0).abs() < 1e-10,
            "Star of 0.25 should be {}, got {}",
            4.0 / 3.0,
            star_q.value()
        );

        // p = 1 should not converge
        assert!(ProbabilityWeight::one().star().is_none());

        // p > 1 should not converge
        assert!(ProbabilityWeight::new(1.5).star().is_none());
    }

    #[test]
    fn test_log_conversion() {
        let probs = [0.1, 0.3, 0.5, 0.7, 0.9, 1.0];
        for &p in &probs {
            let prob_weight = ProbabilityWeight::new(p);
            let log_weight = prob_weight.to_log_weight();
            let recovered = ProbabilityWeight::from_log_weight(log_weight);
            assert!(
                (p - recovered.value()).abs() < 1e-10,
                "Log conversion failed: {} -> {:?} -> {}",
                p,
                log_weight.value(),
                recovered.value()
            );
        }

        // Test zero
        let zero_prob = ProbabilityWeight::zero();
        let zero_log = zero_prob.to_log_weight();
        assert!(zero_log.is_zero()); // infinity in log space
    }

    #[test]
    fn test_negative_clamping() {
        // Negative values should be clamped to 0
        let neg = ProbabilityWeight::new(-0.5);
        assert_eq!(neg.value(), 0.0);
        assert!(neg.is_zero());
    }

    proptest! {
        #[test]
        fn proptest_semiring_axioms(
            a in 0.0f64..10.0,
            b in 0.0f64..10.0,
            c in 0.0f64..10.0
        ) {
            let wa = ProbabilityWeight::new(a);
            let wb = ProbabilityWeight::new(b);
            let wc = ProbabilityWeight::new(c);
            verify_semiring_axioms(wa, wb, wc, 1e-8);
        }

        #[test]
        fn proptest_divisible_semiring(
            a in 0.0f64..10.0,
            b in 0.001f64..10.0 // Avoid near-zero
        ) {
            let wa = ProbabilityWeight::new(a);
            let wb = ProbabilityWeight::new(b);
            verify_divisible_semiring(wa, wb, 1e-8);
        }

        #[test]
        fn proptest_star_semiring(p in 0.001f64..0.999) {
            let wp = ProbabilityWeight::new(p);
            verify_star_semiring(wp, 1e-6);
        }

        #[test]
        fn proptest_log_conversion(p in 0.001f64..10.0) {
            let prob = ProbabilityWeight::new(p);
            let log = prob.to_log_weight();
            let recovered = ProbabilityWeight::from_log_weight(log);
            prop_assert!(prob.approx_eq(&recovered, 1e-10));
        }
    }
}
