//! Product semiring for multi-objective optimization.
//!
//! The product semiring (K₁ × K₂, ⊕, ⊗, (0̄₁, 0̄₂), (1̄₁, 1̄₂)) combines
//! two semirings component-wise:
//!
//! - **⊕ = (⊕₁, ⊕₂)**: Component-wise addition
//! - **⊗ = (⊗₁, ⊗₂)**: Component-wise multiplication
//! - **0̄ = (0̄₁, 0̄₂)**: Component-wise zeros
//! - **1̄ = (1̄₁, 1̄₂)**: Component-wise ones
//!
//! This enables optimizing multiple objectives simultaneously, such as
//! finding the shortest path that also maximizes probability.
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{Semiring, TropicalWeight, LogWeight, ProductWeight};
//!
//! // Combine distance (tropical) with probability (log)
//! type DistanceProb = ProductWeight<TropicalWeight, LogWeight>;
//!
//! let a = DistanceProb::new(TropicalWeight::new(2.0), LogWeight::new(0.5));
//! let b = DistanceProb::new(TropicalWeight::new(3.0), LogWeight::new(0.3));
//!
//! // Component-wise min for tropical, log-add for log
//! let sum = a.plus(&b);
//!
//! // Component-wise addition for both
//! let prod = a.times(&b);
//! assert_eq!(prod.first().value(), 5.0);  // 2 + 3
//! ```

use super::super::traits::{
    CommutativeTimesSemiring, DivisibleSemiring, IdempotentSemiring, KClosedSemiring,
    NonnegativeSemiring, QuantizableSemiring, Semiring, StarSemiring, TotallyOrderedSemiring,
    WeaklyLeftDivisibleSemiring, ZeroSumFreeSemiring,
};

/// Product semiring combining two semirings component-wise.
///
/// Each operation is applied independently to each component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProductWeight<S1, S2>(pub S1, pub S2)
where
    S1: Semiring,
    S2: Semiring;

impl<S1, S2> ProductWeight<S1, S2>
where
    S1: Semiring,
    S2: Semiring,
{
    /// Create a new product weight from two components.
    #[inline]
    pub const fn new(first: S1, second: S2) -> Self {
        ProductWeight(first, second)
    }

    /// Get the first component.
    #[inline]
    pub fn first(&self) -> S1 {
        self.0
    }

    /// Get the second component.
    #[inline]
    pub fn second(&self) -> S2 {
        self.1
    }

    /// Map the first component.
    #[inline]
    pub fn map_first<F>(self, f: F) -> Self
    where
        F: FnOnce(S1) -> S1,
    {
        ProductWeight(f(self.0), self.1)
    }

    /// Map the second component.
    #[inline]
    pub fn map_second<F>(self, f: F) -> Self
    where
        F: FnOnce(S2) -> S2,
    {
        ProductWeight(self.0, f(self.1))
    }
}

impl<S1, S2> Default for ProductWeight<S1, S2>
where
    S1: Semiring,
    S2: Semiring,
{
    /// Default is (one, one).
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl<S1, S2> From<(S1, S2)> for ProductWeight<S1, S2>
where
    S1: Semiring,
    S2: Semiring,
{
    #[inline]
    fn from((first, second): (S1, S2)) -> Self {
        ProductWeight::new(first, second)
    }
}

impl<S1, S2> From<ProductWeight<S1, S2>> for (S1, S2)
where
    S1: Semiring,
    S2: Semiring,
{
    #[inline]
    fn from(weight: ProductWeight<S1, S2>) -> Self {
        (weight.0, weight.1)
    }
}

impl<S1, S2> Semiring for ProductWeight<S1, S2>
where
    S1: Semiring,
    S2: Semiring,
{
    /// Component-wise zeros.
    #[inline]
    fn zero() -> Self {
        ProductWeight(S1::zero(), S2::zero())
    }

    /// Component-wise ones.
    #[inline]
    fn one() -> Self {
        ProductWeight(S1::one(), S2::one())
    }

    /// Component-wise addition.
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        ProductWeight(self.0.plus(&other.0), self.1.plus(&other.1))
    }

    /// Component-wise multiplication.
    #[inline]
    fn times(&self, other: &Self) -> Self {
        ProductWeight(self.0.times(&other.0), self.1.times(&other.1))
    }

    #[inline]
    fn is_zero(&self) -> bool {
        // Zero if both components are zero
        self.0.is_zero() && self.1.is_zero()
    }

    #[inline]
    fn is_one(&self) -> bool {
        self.0.is_one() && self.1.is_one()
    }

    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        self.0.approx_eq(&other.0, epsilon) && self.1.approx_eq(&other.1, epsilon)
    }

    /// Natural ordering: compare first component, then second if tied.
    ///
    /// Returns `Some(true)` if self is strictly better on the first component,
    /// or equal on first and strictly better on second.
    fn natural_less(&self, other: &Self) -> Option<bool> {
        match (self.0.natural_less(&other.0), self.1.natural_less(&other.1)) {
            (Some(true), _) => Some(true),
            (Some(false), _) => Some(false),
            (None, second) => second,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = self.0.to_bytes();
        bytes.extend(self.1.to_bytes());
        bytes
    }
}

impl<S1, S2> DivisibleSemiring for ProductWeight<S1, S2>
where
    S1: DivisibleSemiring,
    S2: DivisibleSemiring,
{
    /// Component-wise division.
    fn divide(&self, other: &Self) -> Option<Self> {
        match (self.0.divide(&other.0), self.1.divide(&other.1)) {
            (Some(first), Some(second)) => Some(ProductWeight(first, second)),
            _ => None,
        }
    }
}

impl<S1, S2> StarSemiring for ProductWeight<S1, S2>
where
    S1: StarSemiring,
    S2: StarSemiring,
{
    /// Component-wise Kleene closure.
    fn star(&self) -> Option<Self> {
        match (self.0.star(), self.1.star()) {
            (Some(first), Some(second)) => Some(ProductWeight(first, second)),
            _ => None,
        }
    }
}

// ============================================================================
// Algebraic Property Marker Trait Implementations
// ============================================================================

/// ProductWeight is idempotent if both components are idempotent.
///
/// (a₁, a₂) ⊕ (a₁, a₂) = (a₁ ⊕ a₁, a₂ ⊕ a₂) = (a₁, a₂) when both components are idempotent.
impl<S1, S2> IdempotentSemiring for ProductWeight<S1, S2>
where
    S1: IdempotentSemiring,
    S2: IdempotentSemiring,
{
}

/// ProductWeight is k-closed if both components are k-closed.
///
/// The closure bound is the maximum of the component bounds.
impl<S1, S2> KClosedSemiring for ProductWeight<S1, S2>
where
    S1: KClosedSemiring,
    S2: KClosedSemiring,
{
    fn closure_bound() -> Option<usize> {
        match (S1::closure_bound(), S2::closure_bound()) {
            (Some(k1), Some(k2)) => Some(k1.max(k2)),
            _ => None,
        }
    }
}

/// ProductWeight is zero-sum-free if both components are zero-sum-free.
///
/// (a₁, a₂) ⊕ (b₁, b₂) = (0̄₁, 0̄₂) implies a₁ ⊕ b₁ = 0̄₁ and a₂ ⊕ b₂ = 0̄₂,
/// which implies a₁ = b₁ = 0̄₁ and a₂ = b₂ = 0̄₂ when both components are zero-sum-free.
impl<S1, S2> ZeroSumFreeSemiring for ProductWeight<S1, S2>
where
    S1: ZeroSumFreeSemiring,
    S2: ZeroSumFreeSemiring,
{
}

/// ProductWeight is weakly left divisible if both components are weakly left divisible.
///
/// The left quotient is computed component-wise.
impl<S1, S2> WeaklyLeftDivisibleSemiring for ProductWeight<S1, S2>
where
    S1: WeaklyLeftDivisibleSemiring,
    S2: WeaklyLeftDivisibleSemiring,
{
    fn left_divide(&self, divisor: &Self) -> Option<Self> {
        match (
            self.0.left_divide(&divisor.0),
            self.1.left_divide(&divisor.1),
        ) {
            (Some(first), Some(second)) => Some(ProductWeight(first, second)),
            _ => None,
        }
    }
}

/// ProductWeight has commutative multiplication if both components do.
///
/// (a₁, a₂) ⊗ (b₁, b₂) = (a₁ ⊗ b₁, a₂ ⊗ b₂) = (b₁ ⊗ a₁, b₂ ⊗ a₂) = (b₁, b₂) ⊗ (a₁, a₂)
impl<S1, S2> CommutativeTimesSemiring for ProductWeight<S1, S2>
where
    S1: CommutativeTimesSemiring,
    S2: CommutativeTimesSemiring,
{
}

// ============================================================================
// Algorithm Requirement Trait Implementations (Conditional)
// ============================================================================

/// ProductWeight has a total order if both components have total orders.
///
/// Uses lexicographic ordering: compare first component, then second.
impl<S1, S2> TotallyOrderedSemiring for ProductWeight<S1, S2>
where
    S1: TotallyOrderedSemiring,
    S2: TotallyOrderedSemiring,
{
}

/// ProductWeight is non-negative if both components are non-negative.
impl<S1, S2> NonnegativeSemiring for ProductWeight<S1, S2>
where
    S1: NonnegativeSemiring,
    S2: NonnegativeSemiring,
{
}

/// ProductWeight can be quantized if both components can be quantized.
///
/// Combines the quantized values of both components into a single hash.
impl<S1, S2> QuantizableSemiring for ProductWeight<S1, S2>
where
    S1: QuantizableSemiring,
    S2: QuantizableSemiring,
{
    fn quantize(&self, epsilon: f64) -> i64 {
        let q1 = self.0.quantize(epsilon);
        let q2 = self.1.quantize(epsilon);

        // Combine using a hash-like operation that preserves order information
        // Use XOR with shifted first component to combine both values
        (q1.wrapping_shl(32)) ^ (q2 & 0xFFFFFFFF)
    }
}

// Note: ProductWeight does NOT implement StochasticSemiring because
// a product of two semirings doesn't have a natural probability interpretation.
// The first or second component might individually be probabilities, but
// the product weight as a whole isn't suitable for probability-proportional sampling.

impl<S1, S2> std::ops::Add for ProductWeight<S1, S2>
where
    S1: Semiring,
    S2: Semiring,
{
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl<S1, S2> std::ops::Mul for ProductWeight<S1, S2>
where
    S1: Semiring,
    S2: Semiring,
{
    type Output = Self;

    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl<S1, S2> std::ops::AddAssign for ProductWeight<S1, S2>
where
    S1: Semiring,
    S2: Semiring,
{
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl<S1, S2> std::ops::MulAssign for ProductWeight<S1, S2>
where
    S1: Semiring,
    S2: Semiring,
{
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

// Implement PartialOrd and Ord based on lexicographic ordering
impl<S1, S2> PartialOrd for ProductWeight<S1, S2>
where
    S1: Semiring + PartialOrd,
    S2: Semiring + PartialOrd,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.0.partial_cmp(&other.0) {
            Some(std::cmp::Ordering::Equal) => self.1.partial_cmp(&other.1),
            other_cmp => other_cmp,
        }
    }
}

impl<S1, S2> Ord for ProductWeight<S1, S2>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.0.cmp(&other.0) {
            std::cmp::Ordering::Equal => self.1.cmp(&other.1),
            other_cmp => other_cmp,
        }
    }
}

#[cfg(feature = "serde")]
impl<S1, S2> serde::Serialize for ProductWeight<S1, S2>
where
    S1: Semiring + serde::Serialize,
    S2: Semiring + serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeTuple;
        let mut tuple = serializer.serialize_tuple(2)?;
        tuple.serialize_element(&self.0)?;
        tuple.serialize_element(&self.1)?;
        tuple.end()
    }
}

#[cfg(feature = "serde")]
impl<'de, S1, S2> serde::Deserialize<'de> for ProductWeight<S1, S2>
where
    S1: Semiring + serde::Deserialize<'de>,
    S2: Semiring + serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (first, second) = <(S1, S2)>::deserialize(deserializer)?;
        Ok(ProductWeight::new(first, second))
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::basic::{BoolWeight, LogWeight, TropicalWeight};
    use super::super::super::traits::tests::{
        verify_commutative_times_semiring, verify_divisible_semiring, verify_idempotent_semiring,
        verify_k_closed_semiring, verify_quantizable_semiring, verify_semiring_axioms,
        verify_star_semiring, verify_totally_ordered_semiring,
        verify_weakly_left_divisible_semiring, verify_zero_sum_free_semiring,
    };
    use super::*;
    use proptest::prelude::*;

    type TropTrop = ProductWeight<TropicalWeight, TropicalWeight>;
    type TropLog = ProductWeight<TropicalWeight, LogWeight>;
    type TropBool = ProductWeight<TropicalWeight, BoolWeight>;

    #[test]
    fn test_basic_operations() {
        let a = TropTrop::new(TropicalWeight::new(2.0), TropicalWeight::new(3.0));
        let b = TropTrop::new(TropicalWeight::new(4.0), TropicalWeight::new(1.0));

        // Plus: (min(2, 4), min(3, 1)) = (2, 1)
        let sum = a.plus(&b);
        assert_eq!(sum.first().value(), 2.0);
        assert_eq!(sum.second().value(), 1.0);

        // Times: (2 + 4, 3 + 1) = (6, 4)
        let prod = a.times(&b);
        assert_eq!(prod.first().value(), 6.0);
        assert_eq!(prod.second().value(), 4.0);
    }

    #[test]
    fn test_identities() {
        let a = TropTrop::new(TropicalWeight::new(5.0), TropicalWeight::new(3.0));

        // Zero is additive identity
        let sum = a.plus(&TropTrop::zero());
        assert!(a.approx_eq(&sum, 1e-10));

        // One is multiplicative identity
        let prod = a.times(&TropTrop::one());
        assert!(a.approx_eq(&prod, 1e-10));
    }

    #[test]
    fn test_annihilation() {
        let a = TropTrop::new(TropicalWeight::new(5.0), TropicalWeight::new(3.0));

        // Zero annihilates
        let prod = a.times(&TropTrop::zero());
        assert!(prod.is_zero());
    }

    #[test]
    fn test_division() {
        let a = TropTrop::new(TropicalWeight::new(5.0), TropicalWeight::new(3.0));
        let b = TropTrop::new(TropicalWeight::new(2.0), TropicalWeight::new(1.0));

        // (a * b) / b = a
        let product = a.times(&b);
        let quotient = product.divide(&b).expect("Division should succeed");
        assert!(a.approx_eq(&quotient, 1e-10));
    }

    #[test]
    fn test_star() {
        // Star for tropical semiring requires non-negative weights
        let positive = TropTrop::new(TropicalWeight::new(1.0), TropicalWeight::new(2.0));
        let star = positive
            .star()
            .expect("Star should converge for positive weights");

        // For tropical, star of positive weight = one
        assert!(star.is_one());

        // Negative weight should not converge
        let negative = TropTrop::new(TropicalWeight::new(-1.0), TropicalWeight::new(2.0));
        assert!(negative.star().is_none());
    }

    #[test]
    fn test_mixed_semirings() {
        // Tropical × Log
        let a = TropLog::new(TropicalWeight::new(2.0), LogWeight::from_probability(0.5));
        let b = TropLog::new(TropicalWeight::new(3.0), LogWeight::from_probability(0.3));

        // Times: tropical adds, log adds (prob multiplies)
        let prod = a.times(&b);
        assert_eq!(prod.first().value(), 5.0); // 2 + 3
        let expected_prob = 0.5 * 0.3;
        assert!((prod.second().to_probability() - expected_prob).abs() < 1e-10);
    }

    #[test]
    fn test_tropical_bool() {
        // Tropical × Boolean
        let a = TropBool::new(TropicalWeight::new(2.0), BoolWeight::from(true));
        let b = TropBool::new(TropicalWeight::new(3.0), BoolWeight::from(false));

        // Plus: (min(2, 3), true OR false) = (2, true)
        let sum = a.plus(&b);
        assert_eq!(sum.first().value(), 2.0);
        assert!(sum.second().value());

        // Times: (2 + 3, true AND false) = (5, false)
        let prod = a.times(&b);
        assert_eq!(prod.first().value(), 5.0);
        assert!(!prod.second().value());
    }

    proptest! {
        #[test]
        fn proptest_semiring_axioms(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0,
            b1 in 0.0f64..100.0,
            b2 in 0.0f64..100.0,
            c1 in 0.0f64..100.0,
            c2 in 0.0f64..100.0
        ) {
            let wa = TropTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = TropTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            let wc = TropTrop::new(TropicalWeight::new(c1), TropicalWeight::new(c2));
            verify_semiring_axioms(wa, wb, wc, 1e-10);
        }

        #[test]
        fn proptest_divisible_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0,
            b1 in 0.001f64..100.0,
            b2 in 0.001f64..100.0
        ) {
            let wa = TropTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = TropTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            verify_divisible_semiring(wa, wb, 1e-10);
        }

        #[test]
        fn proptest_star_semiring(
            a1 in 0.001f64..100.0,
            a2 in 0.001f64..100.0
        ) {
            let wa = TropTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            verify_star_semiring(wa, 1e-10);
        }

        #[test]
        fn proptest_idempotent_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0
        ) {
            // TropTrop is idempotent since both TropicalWeight components are idempotent
            let wa = TropTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            verify_idempotent_semiring(wa, 1e-10);
        }

        #[test]
        fn proptest_k_closed_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0
        ) {
            let wa = TropTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            verify_k_closed_semiring(wa, 1e-10);
        }

        #[test]
        fn proptest_zero_sum_free_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0,
            b1 in 0.0f64..100.0,
            b2 in 0.0f64..100.0
        ) {
            let wa = TropTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = TropTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            verify_zero_sum_free_semiring(wa, wb, 1e-10);
        }

        #[test]
        fn proptest_weakly_left_divisible_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0,
            b1 in 0.0f64..100.0,
            b2 in 0.0f64..100.0
        ) {
            let wa = TropTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = TropTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            verify_weakly_left_divisible_semiring(wa, wb, 1e-10);
        }

        #[test]
        fn proptest_commutative_times_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0,
            b1 in 0.0f64..100.0,
            b2 in 0.0f64..100.0
        ) {
            let wa = TropTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = TropTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            verify_commutative_times_semiring(wa, wb, 1e-10);
        }

        #[test]
        fn proptest_totally_ordered_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0,
            b1 in 0.0f64..100.0,
            b2 in 0.0f64..100.0,
            c1 in 0.0f64..100.0,
            c2 in 0.0f64..100.0
        ) {
            let wa = TropTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = TropTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            let wc = TropTrop::new(TropicalWeight::new(c1), TropicalWeight::new(c2));
            verify_totally_ordered_semiring(wa, wb, wc);
        }

        #[test]
        fn proptest_quantizable_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0
        ) {
            let wa = TropTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            verify_quantizable_semiring(wa, 1e-10);
        }
    }

    #[test]
    fn test_k_closed_bound() {
        // TropTrop should have k=0 since both TropicalWeight components have k=0
        assert_eq!(TropTrop::closure_bound(), Some(0));
    }
}
