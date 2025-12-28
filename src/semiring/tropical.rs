//! Tropical semiring for shortest-path optimization.
//!
//! The tropical semiring (ℝ ∪ {∞}, min, +, ∞, 0) is the standard choice
//! for shortest-path problems in WFSTs:
//!
//! - **⊕ = min**: Selects the best (minimum cost) of parallel paths
//! - **⊗ = +**: Accumulates costs along sequential transitions
//! - **0̄ = ∞**: Represents unreachable states
//! - **1̄ = 0**: Represents zero cost (free transitions)
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{Semiring, TropicalWeight};
//!
//! let a = TropicalWeight::new(2.0);
//! let b = TropicalWeight::new(3.0);
//!
//! // min(2, 3) = 2
//! assert_eq!(a.plus(&b), TropicalWeight::new(2.0));
//!
//! // 2 + 3 = 5
//! assert_eq!(a.times(&b), TropicalWeight::new(5.0));
//! ```

use ordered_float::OrderedFloat;

use super::traits::{DivisibleSemiring, Semiring, StarSemiring};

/// Tropical semiring weight.
///
/// Internally stores an `f64` representing cost. Lower values are better.
/// Infinity represents unreachable/impossible states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct TropicalWeight(pub OrderedFloat<f64>);

impl TropicalWeight {
    /// Create a new tropical weight from a raw f64.
    #[inline]
    pub const fn new(value: f64) -> Self {
        TropicalWeight(OrderedFloat(value))
    }

    /// Get the underlying f64 value.
    #[inline]
    pub fn value(self) -> f64 {
        self.0.into_inner()
    }

    /// Create a tropical weight representing infinity (unreachable).
    #[inline]
    pub const fn infinity() -> Self {
        TropicalWeight(OrderedFloat(f64::INFINITY))
    }

    /// Check if this weight represents infinity.
    #[inline]
    pub fn is_infinite(self) -> bool {
        self.0.is_infinite()
    }
}

impl From<f64> for TropicalWeight {
    #[inline]
    fn from(value: f64) -> Self {
        TropicalWeight::new(value)
    }
}

impl From<TropicalWeight> for f64 {
    #[inline]
    fn from(weight: TropicalWeight) -> Self {
        weight.value()
    }
}

impl Default for TropicalWeight {
    /// Default is zero (multiplicative identity), not infinity.
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl Semiring for TropicalWeight {
    /// Additive identity: ∞ (unreachable)
    #[inline]
    fn zero() -> Self {
        TropicalWeight::infinity()
    }

    /// Multiplicative identity: 0 (zero cost)
    #[inline]
    fn one() -> Self {
        TropicalWeight::new(0.0)
    }

    /// Addition: min(a, b)
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        TropicalWeight(self.0.min(other.0))
    }

    /// Multiplication: a + b
    #[inline]
    fn times(&self, other: &Self) -> Self {
        TropicalWeight(OrderedFloat(self.0.into_inner() + other.0.into_inner()))
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

    /// Natural ordering: smaller is better (shorter path).
    fn natural_less(&self, other: &Self) -> Option<bool> {
        Some(self.0 < other.0)
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.0.into_inner().to_le_bytes().to_vec()
    }
}

impl DivisibleSemiring for TropicalWeight {
    /// Division: a - b
    fn divide(&self, other: &Self) -> Option<Self> {
        if other.is_zero() {
            // Division by infinity is undefined
            None
        } else {
            Some(TropicalWeight(OrderedFloat(
                self.0.into_inner() - other.0.into_inner(),
            )))
        }
    }
}

impl super::traits::NumericalWeight for TropicalWeight {
    #[inline]
    fn numerical_value(&self) -> f64 {
        self.value()
    }
}

impl StarSemiring for TropicalWeight {
    /// Kleene closure for tropical semiring.
    ///
    /// For tropical semiring:
    /// - If weight > 0: star = 0 (taking zero copies is optimal)
    /// - If weight = 0: star = 0 (any number of copies has cost 0)
    /// - If weight < 0: series diverges to -∞ (no finite star)
    fn star(&self) -> Option<Self> {
        let v = self.0.into_inner();
        if v >= 0.0 {
            // min(0, v, 2v, 3v, ...) = 0 for v >= 0
            Some(TropicalWeight::one())
        } else {
            // Negative costs: series diverges to -∞
            None
        }
    }
}

impl std::ops::Add for TropicalWeight {
    type Output = Self;

    /// Operator `+` implements semiring ⊕ (min).
    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Mul for TropicalWeight {
    type Output = Self;

    /// Operator `*` implements semiring ⊗ (+).
    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::AddAssign for TropicalWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::MulAssign for TropicalWeight {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for TropicalWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.into_inner().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for TropicalWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        f64::deserialize(deserializer).map(TropicalWeight::new)
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
        let a = TropicalWeight::new(2.0);
        let b = TropicalWeight::new(3.0);

        // Plus is min
        assert_eq!(a.plus(&b), TropicalWeight::new(2.0));
        assert_eq!(b.plus(&a), TropicalWeight::new(2.0));

        // Times is add
        assert_eq!(a.times(&b), TropicalWeight::new(5.0));
        assert_eq!(b.times(&a), TropicalWeight::new(5.0));
    }

    #[test]
    fn test_identities() {
        let a = TropicalWeight::new(5.0);

        // Zero is additive identity
        assert_eq!(a.plus(&TropicalWeight::zero()), a);
        assert_eq!(TropicalWeight::zero().plus(&a), a);

        // One is multiplicative identity
        assert_eq!(a.times(&TropicalWeight::one()), a);
        assert_eq!(TropicalWeight::one().times(&a), a);
    }

    #[test]
    fn test_annihilation() {
        let a = TropicalWeight::new(5.0);

        // Zero annihilates
        assert_eq!(a.times(&TropicalWeight::zero()), TropicalWeight::zero());
        assert_eq!(TropicalWeight::zero().times(&a), TropicalWeight::zero());
    }

    #[test]
    fn test_division() {
        let a = TropicalWeight::new(5.0);
        let b = TropicalWeight::new(3.0);

        // (a * b) / b = a
        let product = a.times(&b);
        assert_eq!(product.divide(&b), Some(a));

        // Division by zero returns None
        assert_eq!(a.divide(&TropicalWeight::zero()), None);
    }

    #[test]
    fn test_star() {
        // Positive weight: star = 0
        let pos = TropicalWeight::new(5.0);
        assert_eq!(pos.star(), Some(TropicalWeight::one()));

        // Zero weight: star = 0
        let zero = TropicalWeight::one();
        assert_eq!(zero.star(), Some(TropicalWeight::one()));

        // Negative weight: star diverges
        let neg = TropicalWeight::new(-1.0);
        assert_eq!(neg.star(), None);
    }

    proptest! {
        #[test]
        fn proptest_semiring_axioms(
            a in 0.0f64..1000.0,
            b in 0.0f64..1000.0,
            c in 0.0f64..1000.0
        ) {
            let wa = TropicalWeight::new(a);
            let wb = TropicalWeight::new(b);
            let wc = TropicalWeight::new(c);
            verify_semiring_axioms(wa, wb, wc, 1e-10);
        }

        #[test]
        fn proptest_divisible_semiring(
            a in 0.0f64..1000.0,
            b in 0.001f64..1000.0 // Avoid near-zero
        ) {
            let wa = TropicalWeight::new(a);
            let wb = TropicalWeight::new(b);
            verify_divisible_semiring(wa, wb, 1e-10);
        }

        #[test]
        fn proptest_star_semiring(a in 0.0f64..1000.0) {
            let wa = TropicalWeight::new(a);
            verify_star_semiring(wa, 1e-10);
        }
    }
}
