//! Signed tropical semiring for bidirectional scoring with rewards.
//!
//! The signed tropical semiring extends the standard tropical semiring to allow
//! negative weights, enabling representation of rewards (negative costs) alongside
//! penalties (positive costs).
//!
//! # Mathematical Definition
//!
//! ```text
//! S = (ℝ ∪ {+∞}, min, +, +∞, 0)
//! ```
//!
//! Unlike the standard tropical semiring which assumes non-negative weights,
//! the signed tropical semiring allows the full real number line:
//!
//! - **Positive weights**: Represent costs/penalties
//! - **Negative weights**: Represent rewards/bonuses
//! - **Zero**: Neutral (no cost or reward)
//!
//! # Star Operation
//!
//! The star operation w* = 1 ⊕ w ⊕ w² ⊕ ... diverges for negative weights:
//!
//! - If w ≥ 0: w* = 0 (the multiplicative identity)
//! - If w < 0: w* diverges (returns None or error)
//!
//! This is because repeatedly adding a negative value produces an unbounded
//! sequence approaching -∞.
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{Semiring, SignedTropicalWeight};
//!
//! // Positive weights (costs)
//! let cost = SignedTropicalWeight::new(2.0);
//!
//! // Negative weights (rewards)
//! let reward = SignedTropicalWeight::new(-1.5);
//!
//! // Combined: 2.0 + (-1.5) = 0.5 net cost
//! assert_eq!(cost.times(&reward), SignedTropicalWeight::new(0.5));
//!
//! // Min selects best (lowest) value
//! assert_eq!(cost.plus(&reward), reward); // -1.5 < 2.0
//! ```
//!
//! # Use Cases
//!
//! - **Language model scoring**: Bonuses for fluent phrases
//! - **Preference modeling**: Rewards for preferred outputs
//! - **Bidirectional optimization**: Balance costs and rewards
//! - **Game-theoretic applications**: Minimax-style scoring

use ordered_float::OrderedFloat;
use std::fmt::{self, Display};

use crate::semiring::traits::{
    CommutativeTimesSemiring, DivisibleSemiring, IdempotentSemiring, QuantizableSemiring, Semiring,
    TotallyOrderedSemiring, WeaklyLeftDivisibleSemiring,
};

/// Signed tropical semiring weight.
///
/// Allows both positive (costs) and negative (rewards) values.
/// The star operation is only defined for non-negative weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct SignedTropicalWeight(pub OrderedFloat<f64>);

impl SignedTropicalWeight {
    /// Create a new signed tropical weight.
    #[inline]
    pub const fn new(value: f64) -> Self {
        SignedTropicalWeight(OrderedFloat(value))
    }

    /// Get the underlying f64 value.
    #[inline]
    pub fn value(self) -> f64 {
        self.0.into_inner()
    }

    /// Create a weight representing positive infinity (unreachable).
    #[inline]
    pub const fn infinity() -> Self {
        SignedTropicalWeight(OrderedFloat(f64::INFINITY))
    }

    /// Create a weight representing negative infinity.
    ///
    /// This represents an "infinitely good" reward, which typically
    /// indicates an error or special case in algorithms.
    #[inline]
    pub const fn neg_infinity() -> Self {
        SignedTropicalWeight(OrderedFloat(f64::NEG_INFINITY))
    }

    /// Check if this weight is positive infinity.
    #[inline]
    pub fn is_pos_infinite(self) -> bool {
        self.0.is_infinite() && self.0.into_inner() > 0.0
    }

    /// Check if this weight is negative infinity.
    #[inline]
    pub fn is_neg_infinite(self) -> bool {
        self.0.is_infinite() && self.0.into_inner() < 0.0
    }

    /// Check if this weight is any kind of infinity.
    #[inline]
    pub fn is_infinite(self) -> bool {
        self.0.is_infinite()
    }

    /// Check if this weight is finite.
    #[inline]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    /// Check if this weight is negative (a reward).
    #[inline]
    pub fn is_negative(self) -> bool {
        self.0.into_inner() < 0.0
    }

    /// Check if this weight is non-negative (a cost or neutral).
    #[inline]
    pub fn is_nonnegative(self) -> bool {
        self.0.into_inner() >= 0.0
    }

    /// Check if the star operation is defined for this weight.
    ///
    /// Star is only defined for non-negative weights.
    #[inline]
    pub fn star_defined(self) -> bool {
        self.is_nonnegative()
    }

    /// Compute the star operation, returning None if undefined.
    ///
    /// For non-negative weights, star(w) = 0 (the multiplicative identity).
    /// For negative weights, the operation diverges and returns None.
    #[inline]
    pub fn star_checked(self) -> Option<Self> {
        if self.star_defined() {
            Some(Self::one())
        } else {
            None
        }
    }

    /// Negate the weight (flip cost to reward and vice versa).
    #[inline]
    pub fn negate(self) -> Self {
        SignedTropicalWeight::new(-self.value())
    }

    /// Get the absolute value of the weight.
    #[inline]
    pub fn abs(self) -> Self {
        SignedTropicalWeight::new(self.value().abs())
    }

    /// Clamp the weight to a range.
    #[inline]
    pub fn clamp(self, min: f64, max: f64) -> Self {
        SignedTropicalWeight::new(self.value().clamp(min, max))
    }
}

impl From<f64> for SignedTropicalWeight {
    #[inline]
    fn from(value: f64) -> Self {
        SignedTropicalWeight::new(value)
    }
}

impl From<SignedTropicalWeight> for f64 {
    #[inline]
    fn from(weight: SignedTropicalWeight) -> Self {
        weight.value()
    }
}

impl Default for SignedTropicalWeight {
    /// Default is one (multiplicative identity = 0.0).
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl Display for SignedTropicalWeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_pos_infinite() {
            write!(f, "+∞")
        } else if self.is_neg_infinite() {
            write!(f, "-∞")
        } else {
            write!(f, "{}", self.value())
        }
    }
}

impl Semiring for SignedTropicalWeight {
    /// Additive identity: +∞ (unreachable).
    #[inline]
    fn zero() -> Self {
        SignedTropicalWeight::infinity()
    }

    /// Multiplicative identity: 0 (neutral).
    #[inline]
    fn one() -> Self {
        SignedTropicalWeight::new(0.0)
    }

    /// Addition: min(a, b).
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        SignedTropicalWeight(self.0.min(other.0))
    }

    /// Multiplication: a + b.
    #[inline]
    fn times(&self, other: &Self) -> Self {
        SignedTropicalWeight::new(self.value() + other.value())
    }

    /// Check if this is zero (additive identity).
    #[inline]
    fn is_zero(&self) -> bool {
        self.is_pos_infinite()
    }

    /// Check if this is one (multiplicative identity).
    #[inline]
    fn is_one(&self) -> bool {
        self.value() == 0.0
    }

    /// Approximate equality for floating-point comparison.
    #[inline]
    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        if self.is_pos_infinite() && other.is_pos_infinite() {
            return true;
        }
        if self.is_neg_infinite() && other.is_neg_infinite() {
            return true;
        }
        if self.is_infinite() || other.is_infinite() {
            return false;
        }
        (self.value() - other.value()).abs() <= epsilon
    }

    /// Natural less: smaller is better (like costs).
    #[inline]
    fn natural_less(&self, other: &Self) -> Option<bool> {
        Some(self.0 < other.0)
    }

    /// Convert to bytes for hashing.
    fn to_bytes(&self) -> Vec<u8> {
        self.value().to_le_bytes().to_vec()
    }
}

impl IdempotentSemiring for SignedTropicalWeight {}

impl CommutativeTimesSemiring for SignedTropicalWeight {}

impl TotallyOrderedSemiring for SignedTropicalWeight {}

impl WeaklyLeftDivisibleSemiring for SignedTropicalWeight {
    fn left_divide(&self, divisor: &Self) -> Option<Self> {
        if divisor.is_pos_infinite() {
            None
        } else {
            Some(SignedTropicalWeight::new(self.value() - divisor.value()))
        }
    }
}

impl DivisibleSemiring for SignedTropicalWeight {
    fn divide(&self, divisor: &Self) -> Option<Self> {
        self.left_divide(divisor)
    }
}

impl QuantizableSemiring for SignedTropicalWeight {
    fn quantize(&self, epsilon: f64) -> i64 {
        let value = self.value();
        if value.is_nan() {
            i64::MIN
        } else if value.is_infinite() && value > 0.0 {
            i64::MAX
        } else if value.is_infinite() && value < 0.0 {
            i64::MIN + 1
        } else {
            (value / epsilon).round() as i64
        }
    }
}

/// Error type for star operation on negative weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarDivergenceError;

impl fmt::Display for StarDivergenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "star operation diverges for negative weights")
    }
}

impl std::error::Error for StarDivergenceError {}

/// Trait for semirings where star may fail.
pub trait FallibleStarSemiring: Semiring {
    /// Error type for failed star operation.
    type Error;

    /// Attempt to compute the star operation.
    fn try_star(&self) -> Result<Self, Self::Error>;
}

impl FallibleStarSemiring for SignedTropicalWeight {
    type Error = StarDivergenceError;

    fn try_star(&self) -> Result<Self, Self::Error> {
        self.star_checked().ok_or(StarDivergenceError)
    }
}

// ============================================================================
// Arithmetic operations
// ============================================================================

impl std::ops::Add for SignedTropicalWeight {
    type Output = Self;

    /// Semiring multiplication (value addition).
    #[inline]
    fn add(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::AddAssign for SignedTropicalWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

impl std::ops::Neg for SignedTropicalWeight {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        self.negate()
    }
}

impl std::ops::Sub for SignedTropicalWeight {
    type Output = Self;

    /// Semiring division (value subtraction).
    #[inline]
    fn sub(self, other: Self) -> Self {
        SignedTropicalWeight::new(self.value() - other.value())
    }
}

impl std::ops::SubAssign for SignedTropicalWeight {
    #[inline]
    fn sub_assign(&mut self, other: Self) {
        *self = SignedTropicalWeight::new(self.value() - other.value());
    }
}

// ============================================================================
// Conversions
// ============================================================================

use crate::semiring::basic::TropicalWeight;

impl From<TropicalWeight> for SignedTropicalWeight {
    /// Convert from standard tropical weight.
    ///
    /// This is always valid since TropicalWeight values are non-negative.
    #[inline]
    fn from(w: TropicalWeight) -> Self {
        SignedTropicalWeight::new(w.value())
    }
}

impl TryFrom<SignedTropicalWeight> for TropicalWeight {
    type Error = &'static str;

    /// Convert to standard tropical weight.
    ///
    /// Fails if the weight is negative.
    fn try_from(w: SignedTropicalWeight) -> Result<Self, Self::Error> {
        if w.is_negative() {
            Err("cannot convert negative signed tropical weight to tropical weight")
        } else {
            Ok(TropicalWeight::new(w.value()))
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creation() {
        let w = SignedTropicalWeight::new(2.5);
        assert_eq!(w.value(), 2.5);

        let neg = SignedTropicalWeight::new(-1.5);
        assert_eq!(neg.value(), -1.5);
    }

    #[test]
    fn test_infinity() {
        let pos_inf = SignedTropicalWeight::infinity();
        assert!(pos_inf.is_pos_infinite());
        assert!(!pos_inf.is_neg_infinite());
        assert!(pos_inf.is_infinite());

        let neg_inf = SignedTropicalWeight::neg_infinity();
        assert!(!neg_inf.is_pos_infinite());
        assert!(neg_inf.is_neg_infinite());
        assert!(neg_inf.is_infinite());
    }

    #[test]
    fn test_zero_one() {
        let zero = SignedTropicalWeight::zero();
        let one = SignedTropicalWeight::one();

        assert!(zero.is_zero());
        assert!(zero.is_pos_infinite());
        assert!(one.is_one());
        assert_eq!(one.value(), 0.0);
    }

    #[test]
    fn test_plus_is_min() {
        let a = SignedTropicalWeight::new(2.0);
        let b = SignedTropicalWeight::new(3.0);
        let c = SignedTropicalWeight::new(-1.0);

        assert_eq!(a.plus(&b), a); // min(2, 3) = 2
        assert_eq!(a.plus(&c), c); // min(2, -1) = -1
        assert_eq!(b.plus(&c), c); // min(3, -1) = -1
    }

    #[test]
    fn test_times_is_add() {
        let a = SignedTropicalWeight::new(2.0);
        let b = SignedTropicalWeight::new(3.0);
        let c = SignedTropicalWeight::new(-1.0);

        assert_eq!(a.times(&b), SignedTropicalWeight::new(5.0));
        assert_eq!(a.times(&c), SignedTropicalWeight::new(1.0));
        assert_eq!(b.times(&c), SignedTropicalWeight::new(2.0));
    }

    #[test]
    fn test_semiring_identity() {
        let w = SignedTropicalWeight::new(2.5);
        let zero = SignedTropicalWeight::zero();
        let one = SignedTropicalWeight::one();

        // Zero is additive identity: w ⊕ 0 = w
        assert_eq!(w.plus(&zero), w);
        assert_eq!(zero.plus(&w), w);

        // One is multiplicative identity: w ⊗ 1 = w
        assert_eq!(w.times(&one), w);
        assert_eq!(one.times(&w), w);
    }

    #[test]
    fn test_negative_detection() {
        let pos = SignedTropicalWeight::new(1.0);
        let zero = SignedTropicalWeight::new(0.0);
        let neg = SignedTropicalWeight::new(-1.0);

        assert!(!pos.is_negative());
        assert!(pos.is_nonnegative());

        assert!(!zero.is_negative());
        assert!(zero.is_nonnegative());

        assert!(neg.is_negative());
        assert!(!neg.is_nonnegative());
    }

    #[test]
    fn test_star_defined() {
        let pos = SignedTropicalWeight::new(1.0);
        let zero = SignedTropicalWeight::new(0.0);
        let neg = SignedTropicalWeight::new(-1.0);

        assert!(pos.star_defined());
        assert!(zero.star_defined());
        assert!(!neg.star_defined());
    }

    #[test]
    fn test_star_checked() {
        let pos = SignedTropicalWeight::new(1.0);
        let neg = SignedTropicalWeight::new(-1.0);

        assert_eq!(pos.star_checked(), Some(SignedTropicalWeight::one()));
        assert_eq!(neg.star_checked(), None);
    }

    #[test]
    fn test_try_star() {
        let pos = SignedTropicalWeight::new(1.0);
        let neg = SignedTropicalWeight::new(-1.0);

        assert!(pos.try_star().is_ok());
        assert!(neg.try_star().is_err());
    }

    #[test]
    fn test_negate() {
        let w = SignedTropicalWeight::new(2.5);
        assert_eq!(w.negate(), SignedTropicalWeight::new(-2.5));
        assert_eq!(w.negate().negate(), w);
    }

    #[test]
    fn test_abs() {
        let pos = SignedTropicalWeight::new(2.5);
        let neg = SignedTropicalWeight::new(-2.5);

        assert_eq!(pos.abs(), pos);
        assert_eq!(neg.abs(), pos);
    }

    #[test]
    fn test_clamp() {
        let w = SignedTropicalWeight::new(5.0);

        assert_eq!(w.clamp(-10.0, 10.0), w);
        assert_eq!(w.clamp(0.0, 3.0), SignedTropicalWeight::new(3.0));
        assert_eq!(w.clamp(7.0, 10.0), SignedTropicalWeight::new(7.0));
    }

    #[test]
    fn test_left_divide() {
        let a = SignedTropicalWeight::new(5.0);
        let b = SignedTropicalWeight::new(3.0);

        // 5 - 3 = 2
        assert_eq!(a.left_divide(&b), Some(SignedTropicalWeight::new(2.0)));

        // Works with negatives too
        let c = SignedTropicalWeight::new(-2.0);
        assert_eq!(a.left_divide(&c), Some(SignedTropicalWeight::new(7.0)));
    }

    #[test]
    fn test_quantize() {
        let w = SignedTropicalWeight::new(2.7);

        // 2.7 / 1.0 = 2.7, rounded = 3
        assert_eq!(w.quantize(1.0), 3);
        // 2.7 / 0.5 = 5.4, rounded = 5
        assert_eq!(w.quantize(0.5), 5);

        // Test infinity
        assert_eq!(SignedTropicalWeight::infinity().quantize(1.0), i64::MAX);
        assert_eq!(
            SignedTropicalWeight::neg_infinity().quantize(1.0),
            i64::MIN + 1
        );
    }

    #[test]
    fn test_from_tropical() {
        let tropical = TropicalWeight::new(2.5);
        let signed: SignedTropicalWeight = tropical.into();

        assert_eq!(signed.value(), 2.5);
    }

    #[test]
    fn test_try_into_tropical() {
        let pos = SignedTropicalWeight::new(2.5);
        let neg = SignedTropicalWeight::new(-1.0);

        let result: Result<TropicalWeight, _> = pos.try_into();
        assert!(result.is_ok());

        let result: Result<TropicalWeight, _> = neg.try_into();
        assert!(result.is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", SignedTropicalWeight::new(2.5)), "2.5");
        assert_eq!(format!("{}", SignedTropicalWeight::new(-1.0)), "-1");
        assert_eq!(format!("{}", SignedTropicalWeight::infinity()), "+∞");
        assert_eq!(format!("{}", SignedTropicalWeight::neg_infinity()), "-∞");
    }

    #[test]
    fn test_arithmetic_ops() {
        let a = SignedTropicalWeight::new(2.0);
        let b = SignedTropicalWeight::new(3.0);

        // Add is times (value addition)
        assert_eq!(a + b, SignedTropicalWeight::new(5.0));

        // Sub is division (value subtraction)
        assert_eq!(a - b, SignedTropicalWeight::new(-1.0));

        // Neg is negate
        assert_eq!(-a, SignedTropicalWeight::new(-2.0));
    }

    #[test]
    fn test_idempotent() {
        let w = SignedTropicalWeight::new(2.0);

        // min(w, w) = w
        assert_eq!(w.plus(&w), w);
    }

    #[test]
    fn test_totally_ordered() {
        use std::cmp::Ordering;

        let a = SignedTropicalWeight::new(-1.0);
        let b = SignedTropicalWeight::new(0.0);
        let c = SignedTropicalWeight::new(1.0);

        assert_eq!(a.total_cmp(&b), Ordering::Less);
        assert_eq!(b.total_cmp(&c), Ordering::Less);
        assert_eq!(a.total_cmp(&a), Ordering::Equal);
    }

    #[test]
    fn test_reward_cost_scenario() {
        // Scenario: Path with costs and rewards
        let cost1 = SignedTropicalWeight::new(2.0); // First edge costs 2
        let reward = SignedTropicalWeight::new(-1.0); // Second edge gives -1 reward
        let cost2 = SignedTropicalWeight::new(1.5); // Third edge costs 1.5

        // Total path cost: 2 + (-1) + 1.5 = 2.5
        let total = cost1.times(&reward).times(&cost2);
        assert_eq!(total, SignedTropicalWeight::new(2.5));

        // Alternative path with higher cost
        let alt = SignedTropicalWeight::new(3.0);

        // Best path is the one with lower total
        let best = total.plus(&alt);
        assert_eq!(best, total); // 2.5 < 3.0
    }

    #[test]
    fn test_commutative() {
        let a = SignedTropicalWeight::new(2.0);
        let b = SignedTropicalWeight::new(-1.0);

        assert_eq!(a.times(&b), b.times(&a));
        assert_eq!(a.plus(&b), b.plus(&a));
    }

    #[test]
    fn test_associative() {
        let a = SignedTropicalWeight::new(1.0);
        let b = SignedTropicalWeight::new(-2.0);
        let c = SignedTropicalWeight::new(3.0);

        // Times is associative
        assert_eq!(a.times(&b).times(&c), a.times(&b.times(&c)));

        // Plus is associative
        assert_eq!(a.plus(&b).plus(&c), a.plus(&b.plus(&c)));
    }

    #[test]
    fn test_distributive() {
        let a = SignedTropicalWeight::new(1.0);
        let b = SignedTropicalWeight::new(-2.0);
        let c = SignedTropicalWeight::new(3.0);

        // a ⊗ (b ⊕ c) = (a ⊗ b) ⊕ (a ⊗ c)
        let lhs = a.times(&b.plus(&c));
        let rhs = a.times(&b).plus(&a.times(&c));
        assert_eq!(lhs, rhs);
    }
}
