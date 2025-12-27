//! Core semiring trait definitions.
//!
//! Semirings provide the algebraic structure for weighted automata operations.
//! The traits here form a hierarchy:
//!
//! - [`Semiring`]: Basic semiring operations (⊕, ⊗, 0̄, 1̄)
//! - [`DivisibleSemiring`]: Semirings with division (for weight pushing)
//! - [`StarSemiring`]: Semirings with Kleene closure (for epsilon removal)

use std::fmt::Debug;
use std::hash::Hash;

/// Algebraic semiring for WFST weight operations.
///
/// A semiring (K, ⊕, ⊗, 0̄, 1̄) satisfies the following axioms:
///
/// 1. (K, ⊕, 0̄) is a commutative monoid:
///    - Associativity: (a ⊕ b) ⊕ c = a ⊕ (b ⊕ c)
///    - Commutativity: a ⊕ b = b ⊕ a
///    - Identity: a ⊕ 0̄ = a
///
/// 2. (K, ⊗, 1̄) is a monoid:
///    - Associativity: (a ⊗ b) ⊗ c = a ⊗ (b ⊗ c)
///    - Identity: a ⊗ 1̄ = 1̄ ⊗ a = a
///
/// 3. ⊗ distributes over ⊕:
///    - Left: a ⊗ (b ⊕ c) = (a ⊗ b) ⊕ (a ⊗ c)
///    - Right: (a ⊕ b) ⊗ c = (a ⊗ c) ⊕ (b ⊗ c)
///
/// 4. 0̄ is an annihilator for ⊗:
///    - 0̄ ⊗ a = a ⊗ 0̄ = 0̄
///
/// # Semantic Interpretation
///
/// - **⊕ (plus)**: Combines weights of parallel paths (e.g., min for shortest path)
/// - **⊗ (times)**: Combines weights of sequential transitions (e.g., + for costs)
/// - **0̄ (zero)**: Identity for ⊕, annihilator for ⊗ (e.g., ∞ for tropical)
/// - **1̄ (one)**: Identity for ⊗ (e.g., 0 for tropical costs)
pub trait Semiring: Clone + Copy + Debug + PartialEq + Send + Sync + 'static {
    /// Additive identity (0̄).
    ///
    /// For any weight `a`: `a.plus(&Self::zero()) == a`
    fn zero() -> Self;

    /// Multiplicative identity (1̄).
    ///
    /// For any weight `a`: `a.times(&Self::one()) == a`
    fn one() -> Self;

    /// Addition (⊕): combines parallel path weights.
    ///
    /// In the tropical semiring, this is `min`.
    /// In the log semiring, this is `log(exp(a) + exp(b))`.
    fn plus(&self, other: &Self) -> Self;

    /// Multiplication (⊗): combines sequential transition weights.
    ///
    /// In both tropical and log semirings, this is `+`.
    fn times(&self, other: &Self) -> Self;

    /// Check if this weight is the additive identity.
    #[inline]
    fn is_zero(&self) -> bool {
        *self == Self::zero()
    }

    /// Check if this weight is the multiplicative identity.
    #[inline]
    fn is_one(&self) -> bool {
        *self == Self::one()
    }

    /// Approximate equality check for floating-point semirings.
    ///
    /// Returns `true` if the weights are within `epsilon` of each other
    /// according to the semiring's natural metric.
    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool;

    /// Natural ordering comparison.
    ///
    /// Returns `Some(true)` if `self` is "better" than `other` according to
    /// the semiring's natural ordering:
    /// - Tropical: smaller is better (shorter path)
    /// - Log: larger is better (higher probability)
    /// - Boolean: true is better
    ///
    /// Returns `None` if the semiring has no natural ordering.
    fn natural_less(&self, other: &Self) -> Option<bool>;

    /// Convert weight to bytes for hashing/merkleization.
    ///
    /// The byte representation must be stable (same weight = same bytes).
    fn to_bytes(&self) -> Vec<u8>;
}

/// Semiring with division operation.
///
/// Division is required for weight pushing algorithms that redistribute
/// weights along paths. Not all semirings support division (e.g., boolean).
///
/// # Requirements
///
/// For a ∈ K and b ∈ K where b ≠ 0̄:
/// - `(a.times(&b)).divide(&b) == Some(a)`
pub trait DivisibleSemiring: Semiring {
    /// Division operation.
    ///
    /// Returns `None` if division by the given weight is undefined
    /// (e.g., division by zero).
    fn divide(&self, other: &Self) -> Option<Self>;
}

/// Semiring with Kleene closure (star) operation.
///
/// The star operation computes the infinite sum:
/// `a* = 1̄ ⊕ a ⊕ (a ⊗ a) ⊕ (a ⊗ a ⊗ a) ⊕ ...`
///
/// This is required for epsilon removal and other WFST algorithms that
/// need to handle cycles.
///
/// # Convergence
///
/// The star operation may not converge for all weights. Implementations
/// should return `None` when the series does not converge.
pub trait StarSemiring: Semiring {
    /// Kleene closure (star) operation.
    ///
    /// Computes `a* = Σ_{n=0}^∞ aⁿ` where:
    /// - `a⁰ = 1̄`
    /// - `aⁿ = a ⊗ aⁿ⁻¹`
    ///
    /// Returns `None` if the series does not converge.
    fn star(&self) -> Option<Self>;
}

/// Marker trait for semirings that can be used as HashMap keys.
///
/// This requires the semiring to implement `Eq` and `Hash`, which means
/// it must have exact equality semantics. Floating-point semirings typically
/// cannot implement this trait directly.
pub trait HashableSemiring: Semiring + Eq + Hash {}

impl<S: Semiring + Eq + Hash> HashableSemiring for S {}

/// Test utilities for verifying semiring axioms.
#[cfg(test)]
pub mod tests {
    use super::*;

    /// Helper function to verify semiring axioms for a given implementation.
    pub fn verify_semiring_axioms<S: Semiring>(a: S, b: S, c: S, epsilon: f64) {
        // Additive identity
        assert!(
            a.plus(&S::zero()).approx_eq(&a, epsilon),
            "Additive identity failed: a ⊕ 0̄ ≠ a"
        );

        // Multiplicative identity
        assert!(
            a.times(&S::one()).approx_eq(&a, epsilon),
            "Multiplicative identity (right) failed: a ⊗ 1̄ ≠ a"
        );
        assert!(
            S::one().times(&a).approx_eq(&a, epsilon),
            "Multiplicative identity (left) failed: 1̄ ⊗ a ≠ a"
        );

        // Additive commutativity
        assert!(
            a.plus(&b).approx_eq(&b.plus(&a), epsilon),
            "Additive commutativity failed: a ⊕ b ≠ b ⊕ a"
        );

        // Additive associativity
        let left = a.plus(&b).plus(&c);
        let right = a.plus(&b.plus(&c));
        assert!(
            left.approx_eq(&right, epsilon),
            "Additive associativity failed: (a ⊕ b) ⊕ c ≠ a ⊕ (b ⊕ c)"
        );

        // Multiplicative associativity
        let left = a.times(&b).times(&c);
        let right = a.times(&b.times(&c));
        assert!(
            left.approx_eq(&right, epsilon),
            "Multiplicative associativity failed: (a ⊗ b) ⊗ c ≠ a ⊗ (b ⊗ c)"
        );

        // Left distributivity
        let left = a.times(&b.plus(&c));
        let right = a.times(&b).plus(&a.times(&c));
        assert!(
            left.approx_eq(&right, epsilon),
            "Left distributivity failed: a ⊗ (b ⊕ c) ≠ (a ⊗ b) ⊕ (a ⊗ c)"
        );

        // Right distributivity
        let left = a.plus(&b).times(&c);
        let right = a.times(&c).plus(&b.times(&c));
        assert!(
            left.approx_eq(&right, epsilon),
            "Right distributivity failed: (a ⊕ b) ⊗ c ≠ (a ⊗ c) ⊕ (b ⊗ c)"
        );

        // Zero annihilation
        assert!(
            S::zero().times(&a).approx_eq(&S::zero(), epsilon),
            "Zero annihilation (left) failed: 0̄ ⊗ a ≠ 0̄"
        );
        assert!(
            a.times(&S::zero()).approx_eq(&S::zero(), epsilon),
            "Zero annihilation (right) failed: a ⊗ 0̄ ≠ 0̄"
        );
    }

    /// Helper function to verify divisible semiring axioms.
    pub fn verify_divisible_semiring<S: DivisibleSemiring>(a: S, b: S, epsilon: f64) {
        if !b.is_zero() {
            let product = a.times(&b);
            if let Some(quotient) = product.divide(&b) {
                assert!(
                    quotient.approx_eq(&a, epsilon),
                    "Division inverse failed: (a ⊗ b) ÷ b ≠ a"
                );
            }
        }
    }

    /// Helper function to verify star semiring axioms.
    pub fn verify_star_semiring<S: StarSemiring>(a: S, epsilon: f64) {
        if let Some(star_a) = a.star() {
            // a* should satisfy: a* = 1 ⊕ (a ⊗ a*)
            let expected = S::one().plus(&a.times(&star_a));
            assert!(
                star_a.approx_eq(&expected, epsilon),
                "Star axiom failed: a* ≠ 1̄ ⊕ (a ⊗ a*)"
            );
        }
    }
}
