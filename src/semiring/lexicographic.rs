//! Lexicographic semiring for multi-level priority optimization.
//!
//! The lexicographic semiring combines multiple semirings with strict priority ordering:
//! the first component takes absolute precedence, the second only matters when first components
//! are equal, and so on.
//!
//! **Definition:**
//! ```text
//! S_lex = (W₁ × W₂ × ... × Wₖ, ⊕_lex, ⊗_lex, 0̄_lex, 1̄_lex)
//!
//! (a₁,...,aₖ) ⊕_lex (b₁,...,bₖ):
//!   Compare lexicographically; return the smaller tuple
//!
//! (a₁,...,aₖ) ⊗_lex (b₁,...,bₖ) = (a₁⊗b₁, ..., aₖ⊗bₖ)
//! ```
//!
//! Unlike [`ProductWeight`](super::ProductWeight) which applies ⊕ component-wise,
//! `LexicographicWeight` uses lexicographic comparison: the first component is
//! compared first, and subsequent components are only considered if earlier
//! components are equal.
//!
//! # Use Cases
//!
//! - **Multi-objective optimization with priorities**: "minimize errors first, then cost"
//! - **Tiered scoring**: "exact match > fuzzy match > no match"
//! - **Error correction**: "prefer fewer edits, then shorter length, then higher probability"
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{Semiring, TropicalWeight, LexicographicWeight};
//!
//! // Priority: minimize edit distance, then minimize length
//! let a = LexicographicWeight::new(TropicalWeight::new(2.0), TropicalWeight::new(10.0));
//! let b = LexicographicWeight::new(TropicalWeight::new(1.0), TropicalWeight::new(100.0));
//!
//! // b wins because its first component (1.0) is smaller than a's (2.0)
//! // The second component is irrelevant since first components differ
//! let best = a.plus(&b);
//! assert_eq!(best.first().value(), 1.0);
//! assert_eq!(best.second().value(), 100.0);
//! ```
//!
//! # Comparison with ProductWeight
//!
//! | Operation | ProductWeight | LexicographicWeight |
//! |-----------|--------------|---------------------|
//! | ⊕ (plus) | Component-wise | Lexicographic min |
//! | ⊗ (times) | Component-wise | Component-wise |
//! | 0̄ (zero) | (0̄₁, 0̄₂) | (0̄₁, 0̄₂) |
//! | 1̄ (one) | (1̄₁, 1̄₂) | (1̄₁, 1̄₂) |
//!
//! # Algebraic Properties
//!
//! - **Idempotent**: If both components are idempotent (lex-min(a, a) = a)
//! - **K-closed**: If the first component is k-closed
//! - **Zero-sum-free**: If the first component is zero-sum-free
//! - **Commutative ⊗**: If both components have commutative ⊗

use super::traits::{
    CommutativeTimesSemiring, DivisibleSemiring, IdempotentSemiring, KClosedSemiring,
    NonnegativeSemiring, QuantizableSemiring, Semiring, StarSemiring, StochasticSemiring,
    TotallyOrderedSemiring, WeaklyLeftDivisibleSemiring, ZeroSumFreeSemiring,
};

/// Lexicographic semiring combining two semirings with priority ordering.
///
/// The first component has absolute priority: ⊕ returns the tuple whose first
/// component is smaller. The second component only matters when first components
/// are equal. Multiplication (⊗) is applied component-wise.
///
/// # Type Parameters
///
/// * `S1` - First (primary) semiring type
/// * `S2` - Second (tiebreaker) semiring type
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LexicographicWeight<S1, S2>(pub S1, pub S2)
where
    S1: Semiring + Ord,
    S2: Semiring + Ord;

impl<S1, S2> LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    /// Create a new lexicographic weight from two components.
    ///
    /// The first component has higher priority in comparisons.
    #[inline]
    pub const fn new(first: S1, second: S2) -> Self {
        LexicographicWeight(first, second)
    }

    /// Get the first (primary) component.
    #[inline]
    pub fn first(&self) -> S1 {
        self.0
    }

    /// Get the second (tiebreaker) component.
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
        LexicographicWeight(f(self.0), self.1)
    }

    /// Map the second component.
    #[inline]
    pub fn map_second<F>(self, f: F) -> Self
    where
        F: FnOnce(S2) -> S2,
    {
        LexicographicWeight(self.0, f(self.1))
    }
}

impl<S1, S2> Default for LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    /// Default is (one, one).
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl<S1, S2> From<(S1, S2)> for LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    #[inline]
    fn from((first, second): (S1, S2)) -> Self {
        LexicographicWeight::new(first, second)
    }
}

impl<S1, S2> From<LexicographicWeight<S1, S2>> for (S1, S2)
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    #[inline]
    fn from(weight: LexicographicWeight<S1, S2>) -> Self {
        (weight.0, weight.1)
    }
}

impl<S1, S2> Semiring for LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    /// Component-wise zeros: (0̄₁, 0̄₂).
    #[inline]
    fn zero() -> Self {
        LexicographicWeight(S1::zero(), S2::zero())
    }

    /// Component-wise ones: (1̄₁, 1̄₂).
    #[inline]
    fn one() -> Self {
        LexicographicWeight(S1::one(), S2::one())
    }

    /// Lexicographic addition: returns the lexicographically smaller tuple.
    ///
    /// Compares first components; if equal, compares second components.
    /// This implements a strict priority system where the first component
    /// takes absolute precedence.
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        use std::cmp::Ordering;

        match self.0.cmp(&other.0) {
            Ordering::Less => *self,
            Ordering::Greater => *other,
            Ordering::Equal => {
                // First components equal, compare second
                if self.1 <= other.1 {
                    *self
                } else {
                    *other
                }
            }
        }
    }

    /// Component-wise multiplication: (a₁⊗b₁, a₂⊗b₂).
    #[inline]
    fn times(&self, other: &Self) -> Self {
        LexicographicWeight(self.0.times(&other.0), self.1.times(&other.1))
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

    /// Natural ordering: lexicographic comparison.
    ///
    /// Returns `Some(true)` if self is lexicographically smaller than other.
    fn natural_less(&self, other: &Self) -> Option<bool> {
        match self.0.cmp(&other.0) {
            std::cmp::Ordering::Less => Some(true),
            std::cmp::Ordering::Greater => Some(false),
            std::cmp::Ordering::Equal => Some(self.1 < other.1),
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = self.0.to_bytes();
        bytes.extend(self.1.to_bytes());
        bytes
    }
}

impl<S1, S2> DivisibleSemiring for LexicographicWeight<S1, S2>
where
    S1: DivisibleSemiring + Ord,
    S2: DivisibleSemiring + Ord,
{
    /// Component-wise division.
    fn divide(&self, other: &Self) -> Option<Self> {
        match (self.0.divide(&other.0), self.1.divide(&other.1)) {
            (Some(first), Some(second)) => Some(LexicographicWeight(first, second)),
            _ => None,
        }
    }
}

impl<S1, S2> StarSemiring for LexicographicWeight<S1, S2>
where
    S1: StarSemiring + Ord,
    S2: StarSemiring + Ord,
{
    /// Kleene closure for lexicographic semiring.
    ///
    /// The star operation computes the infinite sum of powers. For lexicographic
    /// semiring, this converges if the first component's star converges (since
    /// it dominates the comparison).
    fn star(&self) -> Option<Self> {
        match (self.0.star(), self.1.star()) {
            (Some(first), Some(second)) => Some(LexicographicWeight(first, second)),
            _ => None,
        }
    }
}

// ============================================================================
// Algebraic Property Marker Trait Implementations
// ============================================================================

/// LexicographicWeight is idempotent if both components are idempotent.
///
/// lex_min(a, a) = a for all a.
impl<S1, S2> IdempotentSemiring for LexicographicWeight<S1, S2>
where
    S1: IdempotentSemiring + Ord,
    S2: IdempotentSemiring + Ord,
{
}

/// LexicographicWeight is k-closed if both components are k-closed.
///
/// Since ⊕ is lexicographic min, the star operation stabilizes when the
/// first component stabilizes.
impl<S1, S2> KClosedSemiring for LexicographicWeight<S1, S2>
where
    S1: KClosedSemiring + Ord,
    S2: KClosedSemiring + Ord,
{
    fn closure_bound() -> Option<usize> {
        // The bound is dominated by the first component since it has priority
        // but we need both to stabilize
        match (S1::closure_bound(), S2::closure_bound()) {
            (Some(k1), Some(k2)) => Some(k1.max(k2)),
            _ => None,
        }
    }
}

/// LexicographicWeight is zero-sum-free if both components are zero-sum-free.
///
/// lex_min(a, b) = (0̄₁, 0̄₂) only if both a and b are (0̄₁, 0̄₂).
impl<S1, S2> ZeroSumFreeSemiring for LexicographicWeight<S1, S2>
where
    S1: ZeroSumFreeSemiring + Ord,
    S2: ZeroSumFreeSemiring + Ord,
{
}

/// LexicographicWeight is weakly left-divisible if both components are.
///
/// Note: The semantics are more complex than for ProductWeight because ⊕
/// is not component-wise. We implement left division as component-wise,
/// which works when the divisor is the result of a plus operation involving
/// the dividend.
impl<S1, S2> WeaklyLeftDivisibleSemiring for LexicographicWeight<S1, S2>
where
    S1: WeaklyLeftDivisibleSemiring + Ord,
    S2: WeaklyLeftDivisibleSemiring + Ord,
{
    fn left_divide(&self, divisor: &Self) -> Option<Self> {
        // For lexicographic semiring, if divisor = self ⊕ other,
        // then divisor is either self or other (whichever is lex-smaller).
        // If divisor == self, we need c such that c ⊗ self = self, so c = one.
        // If divisor == other (and self != other), this is undefined in general.
        //
        // We implement component-wise division which works for the common case
        // where divisor is derived from self.
        match (
            self.0.left_divide(&divisor.0),
            self.1.left_divide(&divisor.1),
        ) {
            (Some(first), Some(second)) => Some(LexicographicWeight(first, second)),
            _ => None,
        }
    }
}

/// LexicographicWeight has commutative multiplication if both components do.
///
/// (a₁, a₂) ⊗ (b₁, b₂) = (a₁ ⊗ b₁, a₂ ⊗ b₂) = (b₁ ⊗ a₁, b₂ ⊗ a₂) = (b₁, b₂) ⊗ (a₁, a₂)
impl<S1, S2> CommutativeTimesSemiring for LexicographicWeight<S1, S2>
where
    S1: CommutativeTimesSemiring + Ord,
    S2: CommutativeTimesSemiring + Ord,
{
}

// ============================================================================
// Algorithm Requirement Trait Implementations
// ============================================================================

/// LexicographicWeight has a total order via lexicographic comparison.
impl<S1, S2> TotallyOrderedSemiring for LexicographicWeight<S1, S2>
where
    S1: TotallyOrderedSemiring,
    S2: TotallyOrderedSemiring,
{
}

/// LexicographicWeight is non-negative if both components are non-negative.
impl<S1, S2> NonnegativeSemiring for LexicographicWeight<S1, S2>
where
    S1: NonnegativeSemiring + Ord,
    S2: NonnegativeSemiring + Ord,
{
}

/// LexicographicWeight can be quantized if both components can be quantized.
impl<S1, S2> QuantizableSemiring for LexicographicWeight<S1, S2>
where
    S1: QuantizableSemiring + Ord,
    S2: QuantizableSemiring + Ord,
{
    fn quantize(&self, epsilon: f64) -> i64 {
        let q1 = self.0.quantize(epsilon);
        let q2 = self.1.quantize(epsilon);

        // Combine using a hash-like operation that preserves lexicographic info
        // First component gets high bits (more significant)
        (q1.wrapping_shl(32)) ^ (q2 & 0xFFFFFFFF)
    }
}

/// LexicographicWeight can be converted to probability for sampling.
///
/// Uses the first component's probability, since it dominates the comparison.
/// The second component is used as a tiebreaker in a multiplicative sense.
impl<S1, S2> StochasticSemiring for LexicographicWeight<S1, S2>
where
    S1: StochasticSemiring + Ord,
    S2: StochasticSemiring + Ord,
{
    fn to_probability(&self) -> f64 {
        // The first component dominates, but we incorporate the second
        // component with a small weight for tiebreaking
        let p1 = self.0.to_probability();
        let p2 = self.1.to_probability();

        // Use multiplicative combination with heavy weight on first
        // This preserves the priority relationship while allowing
        // second component to influence sampling when first is similar
        p1 * (1.0 + 1e-10 * p2)
    }
}

impl<S1, S2> std::ops::Add for LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl<S1, S2> std::ops::Mul for LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    type Output = Self;

    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl<S1, S2> std::ops::AddAssign for LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl<S1, S2> std::ops::MulAssign for LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

// Implement PartialOrd and Ord based on lexicographic ordering
impl<S1, S2> PartialOrd for LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<S1, S2> Ord for LexicographicWeight<S1, S2>
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
impl<S1, S2> serde::Serialize for LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord + serde::Serialize,
    S2: Semiring + Ord + serde::Serialize,
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
impl<'de, S1, S2> serde::Deserialize<'de> for LexicographicWeight<S1, S2>
where
    S1: Semiring + Ord + serde::Deserialize<'de>,
    S2: Semiring + Ord + serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (first, second) = <(S1, S2)>::deserialize(deserializer)?;
        Ok(LexicographicWeight::new(first, second))
    }
}

// ============================================================================
// N-ary Lexicographic Weight
// ============================================================================

/// Three-component lexicographic weight for triple-priority optimization.
///
/// First component has highest priority, second is tiebreaker for first,
/// third is tiebreaker for second.
pub type Lexicographic3<S1, S2, S3> = LexicographicWeight<S1, LexicographicWeight<S2, S3>>;

/// Four-component lexicographic weight.
pub type Lexicographic4<S1, S2, S3, S4> =
    LexicographicWeight<S1, LexicographicWeight<S2, LexicographicWeight<S3, S4>>>;

/// Create a 3-component lexicographic weight.
///
/// # Example
///
/// ```
/// use lling_llang::semiring::{TropicalWeight, lexicographic3};
///
/// // Priority: errors > cost > length
/// let w = lexicographic3(
///     TropicalWeight::new(0.0),   // 0 errors
///     TropicalWeight::new(5.0),   // cost 5
///     TropicalWeight::new(10.0),  // length 10
/// );
/// ```
pub fn lexicographic3<S1, S2, S3>(first: S1, second: S2, third: S3) -> Lexicographic3<S1, S2, S3>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
    S3: Semiring + Ord,
{
    LexicographicWeight::new(first, LexicographicWeight::new(second, third))
}

/// Create a 4-component lexicographic weight.
pub fn lexicographic4<S1, S2, S3, S4>(
    first: S1,
    second: S2,
    third: S3,
    fourth: S4,
) -> Lexicographic4<S1, S2, S3, S4>
where
    S1: Semiring + Ord,
    S2: Semiring + Ord,
    S3: Semiring + Ord,
    S4: Semiring + Ord,
{
    LexicographicWeight::new(
        first,
        LexicographicWeight::new(second, LexicographicWeight::new(third, fourth)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::traits::tests::{
        verify_commutative_times_semiring, verify_divisible_semiring, verify_idempotent_semiring,
        verify_k_closed_semiring, verify_quantizable_semiring, verify_semiring_axioms,
        verify_star_semiring, verify_stochastic_semiring, verify_totally_ordered_semiring,
        verify_weakly_left_divisible_semiring, verify_zero_sum_free_semiring,
    };
    use crate::semiring::TropicalWeight;
    use proptest::prelude::*;

    type LexTrop = LexicographicWeight<TropicalWeight, TropicalWeight>;

    #[test]
    fn test_lexicographic_plus() {
        // First component decides
        let a = LexTrop::new(TropicalWeight::new(2.0), TropicalWeight::new(100.0));
        let b = LexTrop::new(TropicalWeight::new(1.0), TropicalWeight::new(1000.0));

        let result = a.plus(&b);
        assert_eq!(result.first().value(), 1.0); // b wins on first
        assert_eq!(result.second().value(), 1000.0); // b's second comes along

        // Tie on first, second decides
        let c = LexTrop::new(TropicalWeight::new(5.0), TropicalWeight::new(10.0));
        let d = LexTrop::new(TropicalWeight::new(5.0), TropicalWeight::new(20.0));

        let result = c.plus(&d);
        assert_eq!(result.first().value(), 5.0);
        assert_eq!(result.second().value(), 10.0); // c wins on second
    }

    #[test]
    fn test_lexicographic_times() {
        // Component-wise multiplication
        let a = LexTrop::new(TropicalWeight::new(2.0), TropicalWeight::new(3.0));
        let b = LexTrop::new(TropicalWeight::new(4.0), TropicalWeight::new(1.0));

        let result = a.times(&b);
        assert_eq!(result.first().value(), 6.0); // 2 + 4
        assert_eq!(result.second().value(), 4.0); // 3 + 1
    }

    #[test]
    fn test_identities() {
        let a = LexTrop::new(TropicalWeight::new(5.0), TropicalWeight::new(3.0));

        // Zero is additive identity
        let sum = a.plus(&LexTrop::zero());
        assert!(a.approx_eq(&sum, 1e-10));

        // One is multiplicative identity
        let prod = a.times(&LexTrop::one());
        assert!(a.approx_eq(&prod, 1e-10));
    }

    #[test]
    fn test_annihilation() {
        let a = LexTrop::new(TropicalWeight::new(5.0), TropicalWeight::new(3.0));

        // Zero annihilates
        let prod = a.times(&LexTrop::zero());
        assert!(prod.is_zero());
    }

    #[test]
    fn test_division() {
        let a = LexTrop::new(TropicalWeight::new(5.0), TropicalWeight::new(3.0));
        let b = LexTrop::new(TropicalWeight::new(2.0), TropicalWeight::new(1.0));

        // (a * b) / b = a
        let product = a.times(&b);
        let quotient = product.divide(&b).expect("Division should succeed");
        assert!(a.approx_eq(&quotient, 1e-10));
    }

    #[test]
    fn test_star() {
        // Positive weights should converge
        let positive = LexTrop::new(TropicalWeight::new(1.0), TropicalWeight::new(2.0));
        let star = positive.star().expect("Star should converge");
        assert!(star.is_one());

        // Negative weight in first position should not converge
        let negative = LexTrop::new(TropicalWeight::new(-1.0), TropicalWeight::new(2.0));
        assert!(negative.star().is_none());
    }

    #[test]
    fn test_three_level_priority() {
        type Lex3 = Lexicographic3<TropicalWeight, TropicalWeight, TropicalWeight>;

        // Priority: errors > cost > length
        let a = lexicographic3(
            TropicalWeight::new(1.0),  // 1 error
            TropicalWeight::new(5.0),  // cost 5
            TropicalWeight::new(10.0), // length 10
        );
        let b = lexicographic3(
            TropicalWeight::new(0.0),   // 0 errors
            TropicalWeight::new(100.0), // cost 100
            TropicalWeight::new(200.0), // length 200
        );

        // b wins because it has fewer errors (0 < 1)
        let best: Lex3 = a.plus(&b);
        assert_eq!(best.first().value(), 0.0);
    }

    #[test]
    fn test_error_correction_scenario() {
        // Scenario: "minimize edit distance, then minimize acoustic cost, then maximize LM score"
        // Using tropical weights (lower is better)

        type CorrectionWeight = LexicographicWeight<TropicalWeight, TropicalWeight>;

        // Candidate A: 1 edit, good acoustic (2.0)
        let candidate_a = CorrectionWeight::new(TropicalWeight::new(1.0), TropicalWeight::new(2.0));

        // Candidate B: 2 edits, excellent acoustic (0.5)
        let candidate_b = CorrectionWeight::new(TropicalWeight::new(2.0), TropicalWeight::new(0.5));

        // A should win because it has fewer edits
        let best = candidate_a.plus(&candidate_b);
        assert_eq!(best.first().value(), 1.0);
        assert_eq!(best.second().value(), 2.0);

        // Candidate C: 1 edit, worse acoustic (3.0)
        let candidate_c = CorrectionWeight::new(TropicalWeight::new(1.0), TropicalWeight::new(3.0));

        // Between A and C (both 1 edit), A wins on acoustic
        let best = candidate_a.plus(&candidate_c);
        assert_eq!(best.first().value(), 1.0);
        assert_eq!(best.second().value(), 2.0);
    }

    #[test]
    fn test_ordering() {
        let a = LexTrop::new(TropicalWeight::new(1.0), TropicalWeight::new(5.0));
        let b = LexTrop::new(TropicalWeight::new(2.0), TropicalWeight::new(1.0));
        let c = LexTrop::new(TropicalWeight::new(1.0), TropicalWeight::new(3.0));

        // a < b (first component)
        assert!(a < b);

        // c < a (same first, smaller second)
        assert!(c < a);

        // c < b (transitivity)
        assert!(c < b);
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
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = LexTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            let wc = LexTrop::new(TropicalWeight::new(c1), TropicalWeight::new(c2));
            verify_semiring_axioms(wa, wb, wc, 1e-10);
        }

        #[test]
        fn proptest_divisible_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0,
            b1 in 0.001f64..100.0,
            b2 in 0.001f64..100.0
        ) {
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = LexTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            verify_divisible_semiring(wa, wb, 1e-10);
        }

        #[test]
        fn proptest_star_semiring(
            a1 in 0.001f64..100.0,
            a2 in 0.001f64..100.0
        ) {
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            verify_star_semiring(wa, 1e-10);
        }

        #[test]
        fn proptest_idempotent_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0
        ) {
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            verify_idempotent_semiring(wa, 1e-10);
        }

        #[test]
        fn proptest_k_closed_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0
        ) {
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            verify_k_closed_semiring(wa, 1e-10);
        }

        #[test]
        fn proptest_zero_sum_free_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0,
            b1 in 0.0f64..100.0,
            b2 in 0.0f64..100.0
        ) {
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = LexTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            verify_zero_sum_free_semiring(wa, wb, 1e-10);
        }

        #[test]
        fn proptest_weakly_left_divisible_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0,
            b1 in 0.0f64..100.0,
            b2 in 0.0f64..100.0
        ) {
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = LexTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            // The divisor should be the result of wa ⊕ wb for weak left divisibility
            let divisor = wa.plus(&wb);
            verify_weakly_left_divisible_semiring(wa, divisor, 1e-10);
        }

        #[test]
        fn proptest_commutative_times_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0,
            b1 in 0.0f64..100.0,
            b2 in 0.0f64..100.0
        ) {
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = LexTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
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
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            let wb = LexTrop::new(TropicalWeight::new(b1), TropicalWeight::new(b2));
            let wc = LexTrop::new(TropicalWeight::new(c1), TropicalWeight::new(c2));
            verify_totally_ordered_semiring(wa, wb, wc);
        }

        #[test]
        fn proptest_quantizable_semiring(
            a1 in 0.0f64..100.0,
            a2 in 0.0f64..100.0
        ) {
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            verify_quantizable_semiring(wa, 1e-10);
        }

        #[test]
        fn proptest_stochastic_semiring(
            a1 in 0.0f64..50.0,
            a2 in 0.0f64..50.0
        ) {
            let wa = LexTrop::new(TropicalWeight::new(a1), TropicalWeight::new(a2));
            verify_stochastic_semiring(wa);
        }
    }

    #[test]
    fn test_k_closed_bound() {
        // LexTrop should have k=0 since both TropicalWeight components have k=0
        assert_eq!(LexTrop::closure_bound(), Some(0));
    }
}
