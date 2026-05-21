//! Counting semiring for path enumeration.
//!
//! The counting semiring (ℕ, +, ×, 0, 1) counts the number of paths
//! or derivations in a weighted automaton:
//!
//! - **⊕ = +**: Sum counts for parallel paths
//! - **⊗ = ×**: Multiply counts for sequential transitions
//! - **0̄ = 0**: Zero paths (impossible)
//! - **1̄ = 1**: One path (single path)
//!
//! # Use Cases
//!
//! - **Path counting**: Count the number of accepting paths in an FST
//! - **Derivation counting**: Count ambiguous parses in CFG parsing
//! - **Feature counting**: Count the number of times a feature fires
//!
//! # Mathematical Properties
//!
//! - NOT idempotent: a + a = 2a ≠ a
//! - NOT k-closed: star operation diverges for n > 0
//! - Zero-sum-free: a + b = 0 implies a = b = 0
//! - Commutative multiplication: a × b = b × a
//! - Totally ordered by natural number ordering
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{Semiring, CountWeight};
//!
//! let a = CountWeight::new(3);
//! let b = CountWeight::new(5);
//!
//! // Sum counts: 3 + 5 = 8 paths
//! assert_eq!(a.plus(&b).value(), 8);
//!
//! // Multiply counts: 3 × 5 = 15 path combinations
//! assert_eq!(a.times(&b).value(), 15);
//! ```
//!
//! # Overflow Behavior
//!
//! Operations use saturating arithmetic to avoid panics:
//! - Addition saturates at `u64::MAX`
//! - Multiplication saturates at `u64::MAX`
//!
//! For very large path counts, consider using the log semiring instead.

use crate::semiring::traits::{
    CommutativeTimesSemiring, DivisibleSemiring, NonnegativeSemiring, QuantizableSemiring,
    Semiring, TotallyOrderedSemiring, ZeroSumFreeSemiring,
};

/// Counting semiring weight.
///
/// Stores a non-negative integer count of paths or derivations.
/// Uses saturating arithmetic to prevent overflow panics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct CountWeight(pub u64);

impl CountWeight {
    /// Create a new count weight.
    #[inline]
    pub const fn new(count: u64) -> Self {
        CountWeight(count)
    }

    /// Get the underlying count value.
    #[inline]
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Create from a usize count.
    #[inline]
    pub const fn from_usize(count: usize) -> Self {
        CountWeight(count as u64)
    }

    /// Convert to usize (saturates if count > usize::MAX).
    #[inline]
    pub fn to_usize(self) -> usize {
        self.0.min(usize::MAX as u64) as usize
    }

    /// Check if the count is saturated (at maximum value).
    #[inline]
    pub fn is_saturated(self) -> bool {
        self.0 == u64::MAX
    }
}

impl From<u64> for CountWeight {
    #[inline]
    fn from(value: u64) -> Self {
        CountWeight::new(value)
    }
}

impl From<CountWeight> for u64 {
    #[inline]
    fn from(weight: CountWeight) -> Self {
        weight.value()
    }
}

impl From<usize> for CountWeight {
    #[inline]
    fn from(value: usize) -> Self {
        CountWeight::from_usize(value)
    }
}

impl Semiring for CountWeight {
    /// Additive identity: 0 (no paths)
    #[inline]
    fn zero() -> Self {
        CountWeight::new(0)
    }

    /// Multiplicative identity: 1 (one path)
    #[inline]
    fn one() -> Self {
        CountWeight::new(1)
    }

    /// Addition: sum of counts (saturating).
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        CountWeight::new(self.0.saturating_add(other.0))
    }

    /// Multiplication: product of counts (saturating).
    #[inline]
    fn times(&self, other: &Self) -> Self {
        CountWeight::new(self.0.saturating_mul(other.0))
    }

    #[inline]
    fn is_zero(&self) -> bool {
        self.0 == 0
    }

    #[inline]
    fn is_one(&self) -> bool {
        self.0 == 1
    }

    /// Exact equality for integer counts.
    fn approx_eq(&self, other: &Self, _epsilon: f64) -> bool {
        self.0 == other.0
    }

    /// Natural ordering: smaller count is "better" (less ambiguity).
    ///
    /// This interpretation treats fewer paths as preferable (less ambiguity).
    /// For applications where more paths are better, use the inverse.
    fn natural_less(&self, other: &Self) -> Option<bool> {
        Some(self.0 < other.0)
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.0.to_le_bytes().to_vec()
    }
}

impl DivisibleSemiring for CountWeight {
    /// Integer division of counts.
    ///
    /// Returns `None` if divisor is zero.
    /// Uses integer division (truncating).
    fn divide(&self, other: &Self) -> Option<Self> {
        if other.is_zero() {
            None
        } else {
            Some(CountWeight::new(self.0 / other.0))
        }
    }
}

impl crate::semiring::traits::NumericalWeight for CountWeight {
    #[inline]
    fn numerical_value(&self) -> f64 {
        self.value() as f64
    }
}

// Note: CountWeight does NOT implement StarSemiring because:
// For n > 0: n* = 1 + n + n² + n³ + ... diverges to infinity
// For n = 0: 0* = 1 (identity), but this is a special case

// ============================================================================
// Algebraic Property Marker Trait Implementations
// ============================================================================

// Note: CountWeight is NOT IdempotentSemiring because a + a = 2a ≠ a

// Note: CountWeight is NOT KClosedSemiring because star diverges for n > 0

/// CountWeight is zero-sum-free: a + b = 0 only if both a = 0 and b = 0
impl ZeroSumFreeSemiring for CountWeight {}

// Note: CountWeight is NOT WeaklyLeftDivisibleSemiring because integer division
// doesn't satisfy (a / d) × d = a due to truncation. For example:
// a = 1, d = 2: (1 / 2) × 2 = 0 × 2 = 0 ≠ 1

/// CountWeight has commutative multiplication: a × b = b × a
impl CommutativeTimesSemiring for CountWeight {}

// ============================================================================
// Algorithm Requirement Trait Implementations
// ============================================================================

/// CountWeight has a total order via natural number ordering.
impl TotallyOrderedSemiring for CountWeight {}

/// CountWeight values are non-negative (unsigned integers).
impl NonnegativeSemiring for CountWeight {}

/// CountWeight can be quantized trivially (already integral).
impl QuantizableSemiring for CountWeight {
    fn quantize(&self, _epsilon: f64) -> i64 {
        self.0.min(i64::MAX as u64) as i64
    }
}

impl std::ops::Add for CountWeight {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Mul for CountWeight {
    type Output = Self;

    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::AddAssign for CountWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::MulAssign for CountWeight {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for CountWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for CountWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        u64::deserialize(deserializer).map(CountWeight::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::traits::tests::{
        verify_commutative_times_semiring, verify_divisible_semiring, verify_quantizable_semiring,
        verify_semiring_axioms, verify_totally_ordered_semiring, verify_zero_sum_free_semiring,
    };
    use proptest::prelude::*;

    #[test]
    fn test_basic_operations() {
        let a = CountWeight::new(3);
        let b = CountWeight::new(5);

        // Plus is addition
        let sum = a.plus(&b);
        assert_eq!(sum.value(), 8);

        // Times is multiplication
        let prod = a.times(&b);
        assert_eq!(prod.value(), 15);
    }

    #[test]
    fn test_identities() {
        let a = CountWeight::new(42);

        // Zero is additive identity
        assert_eq!(a.plus(&CountWeight::zero()), a);
        assert_eq!(CountWeight::zero().plus(&a), a);

        // One is multiplicative identity
        assert_eq!(a.times(&CountWeight::one()), a);
        assert_eq!(CountWeight::one().times(&a), a);
    }

    #[test]
    fn test_annihilation() {
        let a = CountWeight::new(42);

        // Zero annihilates
        assert!(a.times(&CountWeight::zero()).is_zero());
        assert!(CountWeight::zero().times(&a).is_zero());
    }

    #[test]
    fn test_division() {
        let a = CountWeight::new(15);
        let b = CountWeight::new(3);

        // 15 / 3 = 5
        let quotient = a.divide(&b).expect("Division should succeed");
        assert_eq!(quotient.value(), 5);

        // Division by zero returns None
        assert!(a.divide(&CountWeight::zero()).is_none());

        // Integer division truncates
        let c = CountWeight::new(10);
        let d = CountWeight::new(3);
        let trunc = c.divide(&d).expect("Division should succeed");
        assert_eq!(trunc.value(), 3); // 10 / 3 = 3 (truncated)
    }

    #[test]
    fn test_saturation() {
        let max = CountWeight::new(u64::MAX);
        let one = CountWeight::one();

        // Addition saturates
        let sum = max.plus(&one);
        assert_eq!(sum.value(), u64::MAX);
        assert!(sum.is_saturated());

        // Multiplication saturates
        let big = CountWeight::new(u64::MAX / 2 + 1);
        let prod = big.times(&CountWeight::new(3));
        assert_eq!(prod.value(), u64::MAX);
        assert!(prod.is_saturated());
    }

    #[test]
    fn test_conversions() {
        // From usize
        let from_usize = CountWeight::from_usize(42);
        assert_eq!(from_usize.value(), 42);

        // To usize
        let count = CountWeight::new(100);
        assert_eq!(count.to_usize(), 100);

        // From u64
        let from_u64: CountWeight = 123u64.into();
        assert_eq!(from_u64.value(), 123);

        // To u64
        let to_u64: u64 = CountWeight::new(456).into();
        assert_eq!(to_u64, 456);
    }

    #[test]
    fn test_natural_ordering() {
        let small = CountWeight::new(1);
        let large = CountWeight::new(10);

        // Smaller is "better" (less ambiguity)
        assert_eq!(small.natural_less(&large), Some(true));
        assert_eq!(large.natural_less(&small), Some(false));
        assert_eq!(small.natural_less(&small), Some(false));
    }

    proptest! {
        #[test]
        fn proptest_semiring_axioms(
            a in 0u64..1000,
            b in 0u64..1000,
            c in 0u64..1000
        ) {
            let wa = CountWeight::new(a);
            let wb = CountWeight::new(b);
            let wc = CountWeight::new(c);
            verify_semiring_axioms(wa, wb, wc, 0.0);
        }

        #[test]
        fn proptest_divisible_semiring(
            a in 0u64..1000,
            b in 1u64..1000 // Avoid zero divisor
        ) {
            let wa = CountWeight::new(a);
            let wb = CountWeight::new(b);
            verify_divisible_semiring(wa, wb, 0.0);
        }

        #[test]
        fn proptest_zero_sum_free_semiring(
            a in 0u64..1000,
            b in 0u64..1000
        ) {
            let wa = CountWeight::new(a);
            let wb = CountWeight::new(b);
            verify_zero_sum_free_semiring(wa, wb, 0.0);
        }

        #[test]
        fn proptest_commutative_times_semiring(
            a in 0u64..1000,
            b in 0u64..1000
        ) {
            let wa = CountWeight::new(a);
            let wb = CountWeight::new(b);
            verify_commutative_times_semiring(wa, wb, 0.0);
        }

        #[test]
        fn proptest_totally_ordered_semiring(
            a in 0u64..1000,
            b in 0u64..1000,
            c in 0u64..1000
        ) {
            let wa = CountWeight::new(a);
            let wb = CountWeight::new(b);
            let wc = CountWeight::new(c);
            verify_totally_ordered_semiring(wa, wb, wc);
        }

        #[test]
        fn proptest_quantizable_semiring(a in 0u64..1000) {
            let wa = CountWeight::new(a);
            verify_quantizable_semiring(wa, 1.0);
        }

        #[test]
        fn proptest_saturation_add(a in 0u64..u64::MAX, b in 0u64..u64::MAX) {
            let wa = CountWeight::new(a);
            let wb = CountWeight::new(b);
            let sum = wa.plus(&wb);
            // Either the sum is exact or saturated
            let expected = a.saturating_add(b);
            prop_assert_eq!(sum.value(), expected);
        }

        #[test]
        fn proptest_saturation_mul(a in 0u64..u64::MAX, b in 0u64..u64::MAX) {
            let wa = CountWeight::new(a);
            let wb = CountWeight::new(b);
            let prod = wa.times(&wb);
            // Either the product is exact or saturated
            let expected = a.saturating_mul(b);
            prop_assert_eq!(prod.value(), expected);
        }
    }
}
