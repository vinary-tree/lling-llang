//! Log semiring for probabilistic operations.
//!
//! The log semiring (ℝ ∪ {-∞}, ⊕_log, +, -∞, 0) operates in negative log
//! probability space, which is numerically more stable than raw probabilities:
//!
//! - **⊕ = log-add**: `log(exp(-a) + exp(-b))` (probabilistic sum)
//! - **⊗ = +**: Multiplication of probabilities (addition in log space)
//! - **0̄ = ∞**: Represents probability 0 (impossible)
//! - **1̄ = 0**: Represents probability 1 (certain)
//!
//! # Negative Log Probabilities
//!
//! We use *negative* log probabilities so that:
//! - Lower values = higher probability (consistent with costs)
//! - 0 = probability 1 (certain event)
//! - ∞ = probability 0 (impossible event)
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{Semiring, LogWeight};
//!
//! let a = LogWeight::from_probability(0.3);
//! let b = LogWeight::from_probability(0.5);
//!
//! // Sum probabilities: P(a) + P(b) = 0.3 + 0.5 = 0.8
//! let sum = a.plus(&b);
//! assert!((sum.to_probability() - 0.8).abs() < 1e-10);
//!
//! // Product probabilities: P(a) * P(b) = 0.3 * 0.5 = 0.15
//! let prod = a.times(&b);
//! assert!((prod.to_probability() - 0.15).abs() < 1e-10);
//! ```

use ordered_float::OrderedFloat;

use super::traits::{DivisibleSemiring, Semiring, StarSemiring};

/// Log semiring weight (negative log probability).
///
/// Stores `-log(p)` where `p` is a probability in [0, 1].
/// Lower values indicate higher probability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct LogWeight(pub OrderedFloat<f64>);

impl LogWeight {
    /// Create a new log weight from a raw negative log probability.
    #[inline]
    pub const fn new(neg_log_prob: f64) -> Self {
        LogWeight(OrderedFloat(neg_log_prob))
    }

    /// Create a log weight from a probability in [0, 1].
    #[inline]
    pub fn from_probability(prob: f64) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&prob),
            "Probability must be in [0, 1]"
        );
        if prob == 0.0 {
            Self::zero()
        } else {
            LogWeight::new(-prob.ln())
        }
    }

    /// Convert to probability in [0, 1].
    #[inline]
    pub fn to_probability(self) -> f64 {
        (-self.0.into_inner()).exp()
    }

    /// Get the underlying negative log probability.
    #[inline]
    pub fn value(self) -> f64 {
        self.0.into_inner()
    }

    /// Create a log weight representing zero probability (impossible).
    #[inline]
    pub const fn infinity() -> Self {
        LogWeight(OrderedFloat(f64::INFINITY))
    }

    /// Check if this weight represents zero probability.
    #[inline]
    pub fn is_infinite(self) -> bool {
        self.0.is_infinite()
    }

    /// Numerically stable log-sum-exp: log(exp(-a) + exp(-b))
    ///
    /// Uses the identity: log(exp(a) + exp(b)) = max(a,b) + log(1 + exp(-|a-b|))
    #[inline]
    fn log_sum_exp(a: f64, b: f64) -> f64 {
        if a.is_infinite() {
            return b;
        }
        if b.is_infinite() {
            return a;
        }

        // We want log(exp(-a) + exp(-b)) = -log(exp(a)^-1 + exp(b)^-1)
        // = -log((exp(-a) + exp(-b)))
        // Using: log(exp(-a) + exp(-b)) = -max(-a, -b) + log(1 + exp(-|a - b|))
        //      = min(a, b) + log(1 + exp(-|a - b|))
        let min = a.min(b);
        let diff = (a - b).abs();

        // Fast path: when diff > 20, exp(-diff) ≈ 2e-9 underflows to effectively 0
        // So ln(1 + exp(-diff)) ≈ ln(1) = 0, and result is just min
        if diff > 20.0 {
            return min;
        }

        min - (1.0 + (-diff).exp()).ln()
    }
}

impl From<f64> for LogWeight {
    /// Create from raw negative log probability.
    #[inline]
    fn from(neg_log_prob: f64) -> Self {
        LogWeight::new(neg_log_prob)
    }
}

impl From<LogWeight> for f64 {
    #[inline]
    fn from(weight: LogWeight) -> Self {
        weight.value()
    }
}

impl Default for LogWeight {
    /// Default is one (probability 1, neg log prob 0).
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl Semiring for LogWeight {
    /// Additive identity: ∞ (probability 0)
    #[inline]
    fn zero() -> Self {
        LogWeight::infinity()
    }

    /// Multiplicative identity: 0 (probability 1)
    #[inline]
    fn one() -> Self {
        LogWeight::new(0.0)
    }

    /// Addition: log-sum-exp (probabilistic sum).
    ///
    /// Computes `-log(exp(-a) + exp(-b))` which corresponds to
    /// `P(a) + P(b)` in probability space.
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        LogWeight::new(Self::log_sum_exp(self.0.into_inner(), other.0.into_inner()))
    }

    /// Multiplication: addition in log space.
    ///
    /// Computes `a + b` which corresponds to `P(a) * P(b)` in probability space.
    #[inline]
    fn times(&self, other: &Self) -> Self {
        LogWeight(OrderedFloat(self.0.into_inner() + other.0.into_inner()))
    }

    #[inline]
    fn is_zero(&self) -> bool {
        self.is_infinite()
    }

    #[inline]
    fn is_one(&self) -> bool {
        self.0.into_inner() == 0.0
    }

    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        if self.is_zero() && other.is_zero() {
            return true;
        }
        if self.is_zero() || other.is_zero() {
            return false;
        }
        (self.0.into_inner() - other.0.into_inner()).abs() <= epsilon
    }

    /// Natural ordering: smaller negative log prob = higher probability = better.
    fn natural_less(&self, other: &Self) -> Option<bool> {
        Some(self.0 < other.0)
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.0.into_inner().to_le_bytes().to_vec()
    }
}

impl DivisibleSemiring for LogWeight {
    /// Division: subtraction in log space.
    ///
    /// Computes `a - b` which corresponds to `P(a) / P(b)` in probability space.
    fn divide(&self, other: &Self) -> Option<Self> {
        if other.is_zero() {
            // Division by zero probability is undefined
            None
        } else {
            Some(LogWeight::new(self.0.into_inner() - other.0.into_inner()))
        }
    }
}

impl super::traits::NumericalWeight for LogWeight {
    #[inline]
    fn numerical_value(&self) -> f64 {
        self.value()
    }
}

impl StarSemiring for LogWeight {
    /// Kleene closure for log semiring.
    ///
    /// The star of a weight w is: 1 ⊕ w ⊕ w² ⊕ w³ ⊕ ...
    ///
    /// In the log semiring:
    /// - a* = -log(Σ_{n=0}^∞ exp(-n·w))
    /// - For w > 0: exp(-w) < 1, so the geometric series converges
    /// - a* = -log(1/(1-exp(-w))) = log(1-exp(-w))
    ///
    /// Note: The result can be negative (representing accumulated weight > 1
    /// in probability space), which is mathematically valid for the closure.
    ///
    /// Converges only when w > 0 (probability p < 1).
    fn star(&self) -> Option<Self> {
        let w = self.0.into_inner();

        if w <= 0.0 {
            // p >= 1, series diverges
            return None;
        }

        // Compute log(1 - exp(-w))
        let exp_neg_w = (-w).exp();
        if exp_neg_w >= 1.0 {
            // Shouldn't happen for w > 0, but guard anyway
            return None;
        }

        // For numerical stability when w is large, exp(-w) ≈ 0, so result ≈ log(1) = 0
        let result = (1.0 - exp_neg_w).ln();
        Some(LogWeight::new(result))
    }
}

impl std::ops::Add for LogWeight {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Mul for LogWeight {
    type Output = Self;

    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::AddAssign for LogWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::MulAssign for LogWeight {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for LogWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.into_inner().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for LogWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        f64::deserialize(deserializer).map(LogWeight::new)
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
    fn test_probability_conversion() {
        let probs = [0.1, 0.3, 0.5, 0.7, 0.9, 1.0];
        for &p in &probs {
            let w = LogWeight::from_probability(p);
            let recovered = w.to_probability();
            assert!(
                (p - recovered).abs() < 1e-10,
                "Probability conversion failed: {} -> {} -> {}",
                p,
                w.value(),
                recovered
            );
        }
    }

    #[test]
    fn test_probability_zero() {
        let w = LogWeight::from_probability(0.0);
        assert!(w.is_zero());
        assert_eq!(w.to_probability(), 0.0);
    }

    #[test]
    fn test_basic_operations() {
        let a = LogWeight::from_probability(0.3);
        let b = LogWeight::from_probability(0.5);

        // Plus is probability addition
        let sum = a.plus(&b);
        let expected_prob = 0.3 + 0.5;
        assert!(
            (sum.to_probability() - expected_prob).abs() < 1e-10,
            "Plus failed: expected {}, got {}",
            expected_prob,
            sum.to_probability()
        );

        // Times is probability multiplication
        let prod = a.times(&b);
        let expected_prob = 0.3 * 0.5;
        assert!(
            (prod.to_probability() - expected_prob).abs() < 1e-10,
            "Times failed: expected {}, got {}",
            expected_prob,
            prod.to_probability()
        );
    }

    #[test]
    fn test_identities() {
        let a = LogWeight::from_probability(0.5);

        // Zero is additive identity (adding probability 0)
        let sum = a.plus(&LogWeight::zero());
        assert!(
            a.approx_eq(&sum, 1e-10),
            "Additive identity failed: {:?} + zero = {:?}",
            a,
            sum
        );

        // One is multiplicative identity (multiplying by probability 1)
        let prod = a.times(&LogWeight::one());
        assert!(
            a.approx_eq(&prod, 1e-10),
            "Multiplicative identity failed: {:?} * one = {:?}",
            a,
            prod
        );
    }

    #[test]
    fn test_division() {
        let a = LogWeight::from_probability(0.3);
        let b = LogWeight::from_probability(0.5);

        // (a * b) / b = a
        let product = a.times(&b);
        let quotient = product.divide(&b).expect("Division should succeed");
        assert!(
            a.approx_eq(&quotient, 1e-10),
            "Division inverse failed: {:?} * {:?} / {:?} = {:?}, expected {:?}",
            a,
            b,
            b,
            quotient,
            a
        );
    }

    #[test]
    fn test_star() {
        // For probability p = 0.5, star = 1/(1-0.5) = 2
        // In negative log space: star = log(1 - exp(-w)) = log(1 - 0.5) = log(0.5) ≈ -0.693
        let half = LogWeight::from_probability(0.5);
        let star = half.star().expect("Star should converge for p < 1");

        // The star result is negative (log(0.5) ≈ -0.693), representing
        // an accumulated sum > 1 in probability space (which is 2)
        assert!(
            star.value() < 0.0,
            "Star should be negative for p = 0.5, got {}",
            star.value()
        );

        // Verify the semiring property: star = 1 ⊕ (w ⊗ star)
        let one_plus_w_star = LogWeight::one().plus(&half.times(&star));
        assert!(
            star.approx_eq(&one_plus_w_star, 1e-6),
            "Star axiom failed: {:?} ≠ 1 ⊕ (w ⊗ star) = {:?}",
            star,
            one_plus_w_star
        );

        // Probability 1 (weight 0) should not converge
        let one = LogWeight::one();
        assert!(
            one.star().is_none(),
            "Star should not converge for probability 1"
        );
    }

    proptest! {
        #[test]
        fn proptest_semiring_axioms(
            a_prob in 0.001f64..0.999,
            b_prob in 0.001f64..0.999,
            c_prob in 0.001f64..0.999
        ) {
            // Use smaller probabilities to avoid overflow in times
            let wa = LogWeight::from_probability(a_prob * 0.1);
            let wb = LogWeight::from_probability(b_prob * 0.1);
            let wc = LogWeight::from_probability(c_prob * 0.1);
            verify_semiring_axioms(wa, wb, wc, 1e-8);
        }

        #[test]
        fn proptest_divisible_semiring(
            a_prob in 0.001f64..0.999,
            b_prob in 0.001f64..0.999
        ) {
            let wa = LogWeight::from_probability(a_prob);
            let wb = LogWeight::from_probability(b_prob);
            verify_divisible_semiring(wa, wb, 1e-8);
        }

        #[test]
        fn proptest_star_semiring(prob in 0.001f64..0.999) {
            let w = LogWeight::from_probability(prob);
            verify_star_semiring(w, 1e-6);
        }
    }
}
