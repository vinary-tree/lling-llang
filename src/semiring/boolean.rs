//! Boolean semiring for unweighted automata.
//!
//! The boolean semiring ({true, false}, OR, AND, false, true) represents
//! simple reachability without weights:
//!
//! - **⊕ = OR**: Path exists if either alternative exists
//! - **⊗ = AND**: Path exists if all transitions exist
//! - **0̄ = false**: Represents no path (unreachable)
//! - **1̄ = true**: Represents path exists (reachable)
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{Semiring, BoolWeight};
//!
//! let a = BoolWeight::from(true);
//! let b = BoolWeight::from(false);
//!
//! // OR: true OR false = true
//! assert_eq!(a.plus(&b), BoolWeight::from(true));
//!
//! // AND: true AND false = false
//! assert_eq!(a.times(&b), BoolWeight::from(false));
//! ```

use super::traits::{
    CommutativeTimesSemiring, IdempotentSemiring, KClosedSemiring, Semiring, StarSemiring,
    ZeroSumFreeSemiring,
};

/// Boolean semiring weight for unweighted automata.
///
/// Represents simple path existence without numeric weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct BoolWeight(pub bool);

impl BoolWeight {
    /// Create a new boolean weight.
    #[inline]
    pub const fn new(value: bool) -> Self {
        BoolWeight(value)
    }

    /// Get the underlying boolean value.
    #[inline]
    pub const fn value(self) -> bool {
        self.0
    }
}

impl From<bool> for BoolWeight {
    #[inline]
    fn from(value: bool) -> Self {
        BoolWeight::new(value)
    }
}

impl From<BoolWeight> for bool {
    #[inline]
    fn from(weight: BoolWeight) -> Self {
        weight.value()
    }
}

impl Semiring for BoolWeight {
    /// Additive identity: false (no path)
    #[inline]
    fn zero() -> Self {
        BoolWeight(false)
    }

    /// Multiplicative identity: true (path exists)
    #[inline]
    fn one() -> Self {
        BoolWeight(true)
    }

    /// Addition: OR (path exists if either exists)
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        BoolWeight(self.0 || other.0)
    }

    /// Multiplication: AND (path exists if both exist)
    #[inline]
    fn times(&self, other: &Self) -> Self {
        BoolWeight(self.0 && other.0)
    }

    #[inline]
    fn is_zero(&self) -> bool {
        !self.0
    }

    #[inline]
    fn is_one(&self) -> bool {
        self.0
    }

    fn approx_eq(&self, other: &Self, _epsilon: f64) -> bool {
        self.0 == other.0
    }

    /// Natural ordering: true is better (path exists).
    fn natural_less(&self, other: &Self) -> Option<bool> {
        // true > false in natural ordering (having a path is better)
        Some(self.0 > other.0)
    }

    fn to_bytes(&self) -> Vec<u8> {
        vec![self.0 as u8]
    }
}

impl StarSemiring for BoolWeight {
    /// Kleene closure for boolean semiring.
    ///
    /// The star of any boolean value is true:
    /// - false* = true ⊕ false ⊕ false² ⊕ ... = true (since true is identity for OR)
    /// - true* = true ⊕ true ⊕ true² ⊕ ... = true
    ///
    /// Always converges to true (the series always includes 1̄ = true).
    fn star(&self) -> Option<Self> {
        // In boolean semiring: a* = 1 ⊕ a ⊕ a² ⊕ ...
        // Since 1 = true, and true OR anything = true, star is always true
        Some(BoolWeight::one())
    }
}

// ============================================================================
// Algebraic Property Marker Trait Implementations
// ============================================================================

/// BoolWeight is idempotent: a OR a = a
impl IdempotentSemiring for BoolWeight {}

/// BoolWeight is k-closed with k=0.
///
/// The star operation always returns `true` immediately:
/// - `false* = true ⊕ false = true`
/// - `true* = true ⊕ true = true`
impl KClosedSemiring for BoolWeight {
    fn closure_bound() -> Option<usize> {
        // Star converges immediately at k=0
        Some(0)
    }
}

/// BoolWeight is zero-sum-free: a OR b = false only if both a = false and b = false
impl ZeroSumFreeSemiring for BoolWeight {}

// Note: BoolWeight is NOT WeaklyLeftDivisibleSemiring because boolean algebra has no division

/// BoolWeight has commutative multiplication: a AND b = b AND a
impl CommutativeTimesSemiring for BoolWeight {}

impl std::ops::BitOr for BoolWeight {
    type Output = Self;

    /// Operator `|` implements semiring ⊕ (OR).
    #[inline]
    fn bitor(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::BitAnd for BoolWeight {
    type Output = Self;

    /// Operator `&` implements semiring ⊗ (AND).
    #[inline]
    fn bitand(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::BitOrAssign for BoolWeight {
    #[inline]
    fn bitor_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::BitAndAssign for BoolWeight {
    #[inline]
    fn bitand_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

// Also implement Add and Mul for consistency with other semirings
impl std::ops::Add for BoolWeight {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Mul for BoolWeight {
    type Output = Self;

    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::AddAssign for BoolWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::MulAssign for BoolWeight {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for BoolWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for BoolWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        bool::deserialize(deserializer).map(BoolWeight::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::traits::tests::{
        verify_commutative_times_semiring, verify_idempotent_semiring, verify_k_closed_semiring,
        verify_semiring_axioms, verify_star_semiring, verify_zero_sum_free_semiring,
    };

    #[test]
    fn test_basic_operations() {
        let t = BoolWeight::from(true);
        let f = BoolWeight::from(false);

        // Plus is OR
        assert_eq!(t.plus(&t), BoolWeight::from(true));
        assert_eq!(t.plus(&f), BoolWeight::from(true));
        assert_eq!(f.plus(&t), BoolWeight::from(true));
        assert_eq!(f.plus(&f), BoolWeight::from(false));

        // Times is AND
        assert_eq!(t.times(&t), BoolWeight::from(true));
        assert_eq!(t.times(&f), BoolWeight::from(false));
        assert_eq!(f.times(&t), BoolWeight::from(false));
        assert_eq!(f.times(&f), BoolWeight::from(false));
    }

    #[test]
    fn test_identities() {
        let t = BoolWeight::from(true);
        let f = BoolWeight::from(false);

        // Zero is additive identity
        assert_eq!(t.plus(&BoolWeight::zero()), t);
        assert_eq!(f.plus(&BoolWeight::zero()), f);

        // One is multiplicative identity
        assert_eq!(t.times(&BoolWeight::one()), t);
        assert_eq!(f.times(&BoolWeight::one()), f);
    }

    #[test]
    fn test_annihilation() {
        let t = BoolWeight::from(true);

        // Zero annihilates
        assert_eq!(t.times(&BoolWeight::zero()), BoolWeight::zero());
        assert_eq!(BoolWeight::zero().times(&t), BoolWeight::zero());
    }

    #[test]
    fn test_star() {
        // Star of any boolean is true
        assert_eq!(BoolWeight::from(true).star(), Some(BoolWeight::from(true)));
        assert_eq!(BoolWeight::from(false).star(), Some(BoolWeight::from(true)));
    }

    #[test]
    fn test_operators() {
        let t = BoolWeight::from(true);
        let f = BoolWeight::from(false);

        // BitOr
        assert_eq!(t | f, BoolWeight::from(true));

        // BitAnd
        assert_eq!(t & f, BoolWeight::from(false));

        // Add (same as OR)
        assert_eq!(t + f, BoolWeight::from(true));

        // Mul (same as AND)
        assert_eq!(t * f, BoolWeight::from(false));
    }

    #[test]
    fn test_semiring_axioms() {
        // Test all combinations
        let values = [BoolWeight::from(true), BoolWeight::from(false)];

        for &a in &values {
            for &b in &values {
                for &c in &values {
                    verify_semiring_axioms(a, b, c, 0.0);
                }
            }
        }
    }

    #[test]
    fn test_star_semiring() {
        verify_star_semiring(BoolWeight::from(true), 0.0);
        verify_star_semiring(BoolWeight::from(false), 0.0);
    }

    #[test]
    fn test_idempotent_semiring() {
        verify_idempotent_semiring(BoolWeight::from(true), 0.0);
        verify_idempotent_semiring(BoolWeight::from(false), 0.0);
    }

    #[test]
    fn test_k_closed_semiring() {
        verify_k_closed_semiring(BoolWeight::from(true), 0.0);
        verify_k_closed_semiring(BoolWeight::from(false), 0.0);
    }

    #[test]
    fn test_zero_sum_free_semiring() {
        let t = BoolWeight::from(true);
        let f = BoolWeight::from(false);

        verify_zero_sum_free_semiring(t, t, 0.0);
        verify_zero_sum_free_semiring(t, f, 0.0);
        verify_zero_sum_free_semiring(f, t, 0.0);
        verify_zero_sum_free_semiring(f, f, 0.0);
    }

    #[test]
    fn test_commutative_times_semiring() {
        let t = BoolWeight::from(true);
        let f = BoolWeight::from(false);

        verify_commutative_times_semiring(t, t, 0.0);
        verify_commutative_times_semiring(t, f, 0.0);
        verify_commutative_times_semiring(f, t, 0.0);
        verify_commutative_times_semiring(f, f, 0.0);
    }
}
