//! Core semiring trait definitions.
//!
//! Semirings provide the algebraic structure for weighted automata operations.
//! The traits here form a hierarchy:
//!
//! - [`Semiring`]: Basic semiring operations ($`\oplus`$, $`\otimes`$,
//!   $`\bar{0}`$, $`\bar{1}`$)
//! - [`DivisibleSemiring`]: Semirings with division (for weight pushing)
//! - [`StarSemiring`]: Semirings with Kleene closure (for epsilon removal)
//!
//! # Algebraic Property Markers
//!
//! Additional marker traits encode algebraic properties that enable compile-time
//! verification of algorithm requirements:
//!
//! - [`IdempotentSemiring`]: $`\oplus`$ is idempotent
//!   ($`a \oplus a = a`$)
//! - [`KClosedSemiring`]: Star operation converges in bounded iterations
//! - [`ZeroSumFreeSemiring`]: $`a \oplus b = \bar{0}`$ implies
//!   $`a = b = \bar{0}`$
//! - [`WeaklyLeftDivisibleSemiring`]: Left quotient exists for sums
//! - [`CommutativeTimesSemiring`]: $`\otimes`$ is commutative
//!   ($`a \otimes b = b \otimes a`$)

use std::fmt::Debug;
use std::hash::Hash;

/// Algebraic semiring for WFST weight operations.
///
/// A semiring $`(K, \oplus, \otimes, \bar{0}, \bar{1})`$ satisfies the
/// following axioms:
///
/// 1. $`(K, \oplus, \bar{0})`$ is a commutative monoid:
///    - Associativity: $`(a \oplus b) \oplus c = a \oplus (b \oplus c)`$
///    - Commutativity: $`a \oplus b = b \oplus a`$
///    - Identity: $`a \oplus \bar{0} = a`$
///
/// 2. $`(K, \otimes, \bar{1})`$ is a monoid:
///    - Associativity: $`(a \otimes b) \otimes c = a \otimes (b \otimes c)`$
///    - Identity: $`a \otimes \bar{1} = \bar{1} \otimes a = a`$
///
/// 3. $`\otimes`$ distributes over $`\oplus`$:
///    - Left: $`a \otimes (b \oplus c) = (a \otimes b) \oplus (a \otimes c)`$
///    - Right: $`(a \oplus b) \otimes c = (a \otimes c) \oplus (b \otimes c)`$
///
/// 4. $`\bar{0}`$ is an annihilator for $`\otimes`$:
///    - $`\bar{0} \otimes a = a \otimes \bar{0} = \bar{0}`$
///
/// # Semantic Interpretation
///
/// - **$`\oplus`$ (plus)**: Combines weights of parallel paths (for example,
///   minimum for a shortest path).
/// - **$`\otimes`$ (times)**: Combines weights of sequential transitions
///   (for example, addition for costs).
/// - **$`\bar{0}`$ (zero)**: Identity for $`\oplus`$ and annihilator for
///   $`\otimes`$ (for example, $`\infty`$ for the tropical semiring).
/// - **$`\bar{1}`$ (one)**: Identity for $`\otimes`$ (for example,
///   zero for tropical costs).
pub trait Semiring: Clone + Copy + Debug + PartialEq + Send + Sync + 'static {
    /// Additive identity ($`\bar{0}`$).
    ///
    /// For any weight `a`: `a.plus(&Self::zero()) == a`
    fn zero() -> Self;

    /// Multiplicative identity ($`\bar{1}`$).
    ///
    /// For any weight `a`: `a.times(&Self::one()) == a`
    fn one() -> Self;

    /// Addition ($`\oplus`$): combines parallel path weights.
    ///
    /// In the tropical semiring, this is `min`.
    /// In the log semiring, this is `log(exp(a) + exp(b))`.
    fn plus(&self, other: &Self) -> Self;

    /// Multiplication ($`\otimes`$): combines sequential transition weights.
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
/// For $`a \in K`$ and $`b \in K`$ where $`b \ne \bar{0}`$:
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
///
/// ```math
/// a^{*}=\bar{1}\oplus a\oplus(a\otimes a)
/// \oplus(a\otimes a\otimes a)\oplus\cdots
/// ```
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
    /// Computes $`a^{*} = \sum_{n=0}^{\infty} a^n`$ where:
    /// - $`a^0 = \bar{1}`$
    /// - $`a^n = a \otimes a^{n-1}`$
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

/// Trait for semirings that have an underlying numerical value.
///
/// This is used for algorithms that need to extract the raw numerical
/// value from a weight, such as quantization for approximate comparison.
///
/// Implemented for numerical semirings like Tropical, Log, and Probability.
/// Not applicable to non-numerical semirings like Boolean or String.
pub trait NumericalWeight: Semiring {
    /// Get the underlying numerical value of this weight.
    ///
    /// For Tropical and Log semirings, this returns the raw f64 value.
    /// For Probability semiring, this returns the probability value.
    fn numerical_value(&self) -> f64;
}

// ============================================================================
// Algebraic Property Marker Traits
// ============================================================================

/// Marker trait for semirings where $`\oplus`$ is idempotent.
///
/// # Property
///
/// For every $`a \in K`$, $`a \oplus a = a`$.
///
/// # Implications
///
/// - The semiring forms a join-semilattice under $`\oplus`$.
/// - Shortest-path algorithms (e.g., Dijkstra) work correctly
/// - Epsilon removal can safely revisit states
///
/// # Implementations
///
/// - [`TropicalWeight`](crate::semiring::TropicalWeight):
///   $`\min(a, a) = a`$
/// - [`BoolWeight`](crate::semiring::BoolWeight): $`a \lor a = a`$
pub trait IdempotentSemiring: Semiring {}

/// Trait for k-closed semirings where the star operation converges in bounded iterations.
///
/// # Property
///
/// For every $`a \in K`$, there exists $`k \ge 0`$ such that:
/// ```math
/// \bigoplus_{n=0}^{k+1} a^n = \bigoplus_{n=0}^{k} a^n
/// ```
///
/// That is, the infinite sum
/// $`a^{*} = \bar{1} \oplus a \oplus a^2 \oplus \cdots`$ stabilizes
/// after $`k`$ iterations.
///
/// # Implications
///
/// - FIFO queue shortest-distance algorithms terminate
/// - Epsilon removal on cyclic graphs converges
/// - The closure bound can be used to optimize star computation
///
/// # Implementations
///
/// - [`TropicalWeight`](crate::semiring::TropicalWeight): $`k=0`$ for
///   non-negative weights (minimum stabilizes immediately).
/// - [`LogWeight`](crate::semiring::LogWeight): $`k=0`$ for weights
///   $`\ge 1`$ (log-sum-exp stabilizes).
/// - `BoolWeight`: $`k=0`$ (`true.star() == true` and
///   `false.star() == true`).
pub trait KClosedSemiring: Semiring {
    /// Returns the closure bound k such that star converges in at most k+1 iterations.
    ///
    /// Returns `None` if the semiring is not uniformly k-closed (i.e., k depends
    /// on the specific weight value).
    ///
    /// # Examples
    ///
    /// - Tropical with non-negative weights: `Some(0)`
    /// - Boolean: `Some(0)`
    /// - Log with arbitrary weights: `None` (depends on value)
    fn closure_bound() -> Option<usize>;
}

/// Marker trait for zero-sum-free semirings.
///
/// # Property
///
/// For all $`a,b \in K`$, $`a \oplus b = \bar{0}`$ implies
/// $`a = \bar{0}`$ and $`b = \bar{0}`$.
///
/// # Implications
///
/// - Weighted determinization is well-defined (subset weights are non-zero)
/// - Stochastic sampling is possible (weights sum to non-zero)
/// - The sum operation never "cancels out" to zero
///
/// # Implementations
///
/// All numerical semirings with non-negative weights:
/// - [`TropicalWeight`](crate::semiring::TropicalWeight):
///   $`\min(a,b) = \infty`$ only if both are $`\infty`$.
/// - [`LogWeight`](crate::semiring::LogWeight):
///   $`\operatorname{logadd}(a,b) = \infty`$ only if both are
///   $`\infty`$.
/// - [`ProbabilityWeight`](crate::semiring::ProbabilityWeight):
///   $`a+b=0`$ only if both are zero.
/// - [`ExpectationWeight`](crate::semiring::ExpectationWeight): Component-wise zero-sum-free
/// - [`PowerWeight`](crate::semiring::PowerWeight):
///   $`\left(a^{1/\eta}+b^{1/\eta}\right)^{\eta}=0`$ only if both are
///   zero.
pub trait ZeroSumFreeSemiring: Semiring {}

/// Trait for weakly left-divisible semirings.
///
/// # Property
///
/// For all $`a,b \in K`$ where $`a \oplus b \ne \bar{0}`$, there
/// exists $`c \in K`$ such that:
/// ```math
/// c \otimes (a \oplus b) = a
/// ```
///
/// This is weaker than full divisibility because it only requires left quotients
/// to exist for sums, not for arbitrary products.
///
/// # Implications
///
/// - Weight normalization in determinization is possible
/// - Weights can be "factored out" during powerset construction
/// - Enables canonical subset representation
///
/// # Difference from [`DivisibleSemiring`]
///
/// - `DivisibleSemiring`: $`(a \otimes b)/b = a`$ (product inverse).
/// - `WeaklyLeftDivisibleSemiring`: $`c \otimes (a \oplus b)=a`$
///   (left quotient for sums).
///
/// All divisible semirings are weakly left divisible, but not vice versa.
///
/// # Implementations
///
/// - [`TropicalWeight`](crate::semiring::TropicalWeight): `left_divide(a, min(a,b)) = 0` or `a - min(a,b)`
/// - [`LogWeight`](crate::semiring::LogWeight): `left_divide(a, log-add(a,b)) = a - log-add(a,b)`
/// - [`ProbabilityWeight`](crate::semiring::ProbabilityWeight): `left_divide(a, a+b) = a / (a+b)`
/// - [`ExpectationWeight`](crate::semiring::ExpectationWeight): Component-wise left division
pub trait WeaklyLeftDivisibleSemiring: Semiring {
    /// Computes the left quotient $`c`$ such that
    /// $`c \otimes \mathrm{divisor}=\mathrm{self}`$.
    ///
    /// Returns `None` if:
    /// - The divisor is zero
    /// - No such quotient exists
    /// - The quotient would be undefined (e.g., 0/0)
    ///
    /// # Arguments
    ///
    /// * `divisor` - The sum $`a \oplus b`$ to divide by.
    ///
    /// # Returns
    ///
    /// `Some(c)` where $`c \otimes \mathrm{divisor}=\mathrm{self}`$, or
    /// `None` if undefined.
    fn left_divide(&self, divisor: &Self) -> Option<Self>;
}

/// Marker trait for semirings where $`\otimes`$ is commutative.
///
/// # Property
///
/// For all $`a,b \in K`$, $`a \otimes b=b \otimes a`$.
///
/// Note: The base [`Semiring`] trait already requires $`\oplus`$ to be
/// commutative. This trait additionally requires $`\otimes`$ to be
/// commutative.
///
/// # Implications
///
/// - Order of sequential transitions doesn't affect the weight
/// - Some determinization variants can be optimized
/// - Enables symmetric algorithm formulations
///
/// # Implementations
///
/// Most numerical semirings:
/// - [`TropicalWeight`](crate::semiring::TropicalWeight): `a + b = b + a`
/// - [`LogWeight`](crate::semiring::LogWeight): `a + b = b + a`
/// - [`ProbabilityWeight`](crate::semiring::ProbabilityWeight): `a × b = b × a`
/// - [`BoolWeight`](crate::semiring::BoolWeight):
///   $`a \land b=b \land a`$
/// - [`ExpectationWeight`](crate::semiring::ExpectationWeight): Component-wise commutative
/// - [`PowerWeight`](crate::semiring::PowerWeight): `a × b = b × a`
///
/// Not implemented for string semirings (concatenation is not commutative).
pub trait CommutativeTimesSemiring: Semiring {}

// ============================================================================
// Algorithm Requirement Traits
// ============================================================================

/// Marker trait for semirings with a total order on weights.
///
/// # Property
///
/// For all $`a,b \in K`$, exactly one of these holds:
/// - `a < b`
/// - `a = b`
/// - `a > b`
///
/// This is stronger than `PartialOrd`, which allows incomparable elements.
///
/// # Implications
///
/// - Determinization can safely compute minimum weights without fallback
/// - Sorting operations are well-defined
/// - Priority queue comparisons are always valid
///
/// # Algorithm Requirements
///
/// Required by:
/// - `determinize`: For computing minimum weights in weighted subsets
///
/// # Implementations
///
/// All numerical semirings with `OrderedFloat`:
/// - [`TropicalWeight`](crate::semiring::TropicalWeight): Real numbers with infinity have total order
/// - [`LogWeight`](crate::semiring::LogWeight): Negative log probabilities have total order
/// - [`ProbabilityWeight`](crate::semiring::ProbabilityWeight): Non-negative reals have total order
/// - [`PowerWeight`](crate::semiring::PowerWeight): Non-negative reals with eta have total order
/// - [`ExpectationWeight`](crate::semiring::ExpectationWeight): Lexicographic order on (value, expectation)
pub trait TotallyOrderedSemiring: Semiring + Ord {
    /// Total comparison, guaranteed to never return None.
    ///
    /// Unlike `PartialOrd::partial_cmp`, this always produces a valid ordering.
    #[inline]
    fn total_cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp(other)
    }
}

/// Marker trait for semirings where all weights are non-negative.
///
/// # Property
///
/// For every $`a \in K`$, the weight represents a non-negative quantity in its
/// natural interpretation (costs, probabilities, distances).
///
/// # Implications
///
/// - Dijkstra's algorithm produces correct results
/// - ShortestFirstQueue can safely use a min-heap
/// - No negative cycles exist that could cause infinite loops
///
/// # Algorithm Requirements
///
/// Required by:
/// - `ShortestFirstQueue`: Dijkstra-style priority queue
///
/// # Note on Interpretation
///
/// This trait asserts that the semiring is *used* in a context where weights
/// are non-negative. The tropical semiring can represent negative costs, but
/// when used for shortest-path problems with non-negative edge weights, it
/// satisfies this property.
///
/// # Implementations
///
/// - [`TropicalWeight`](crate::semiring::TropicalWeight): When used with non-negative costs
/// - [`LogWeight`](crate::semiring::LogWeight): Negative log probabilities are always non-negative
/// - [`ProbabilityWeight`](crate::semiring::ProbabilityWeight): Probabilities are in [0, 1]
/// - [`PowerWeight`](crate::semiring::PowerWeight): Values are clamped to non-negative
pub trait NonnegativeSemiring: Semiring {}

/// Trait for semirings whose weights can be quantized for approximate comparison.
///
/// # Property
///
/// Weights can be mapped to integers such that "close" weights map to the
/// same integer, enabling HashMap-based equivalence testing.
///
/// # Implications
///
/// - Minimization can use HashMap for partition refinement
/// - Approximate equality testing is efficient
/// - Floating-point artifacts from weight pushing are handled gracefully
///
/// # Algorithm Requirements
///
/// Required by:
/// - `minimize`: For HashMap-based partition refinement
///
/// # Implementations
///
/// All [`NumericalWeight`] types that represent floating-point values.
pub trait QuantizableSemiring: Semiring {
    /// Quantize the weight to an integer for hashing.
    ///
    /// Two weights that are approximately equal (within `epsilon`) should
    /// produce the same quantized value.
    ///
    /// # Arguments
    ///
    /// * `epsilon` - The quantization precision. Weights within `epsilon`
    ///   of each other should produce the same quantized value.
    ///
    /// # Special Values
    ///
    /// - NaN: Returns `i64::MIN`
    /// - +Infinity: Returns `i64::MAX`
    /// - -Infinity: Returns `i64::MIN + 1`
    fn quantize(&self, epsilon: f64) -> i64;
}

/// Trait for semirings whose weights can be interpreted as probabilities for sampling.
///
/// # Property
///
/// Weights can be converted to non-negative real numbers suitable for
/// probability-proportional sampling. The returned value represents an
/// unnormalized probability (higher = more likely to be sampled).
///
/// # Implications
///
/// - Proportional path sampling is well-defined
/// - Monte Carlo estimation over paths is possible
/// - RRWM and similar algorithms can sample paths
///
/// # Algorithm Requirements
///
/// Required by:
/// - `sample_path`: For proportional sampling strategy
///
/// # Implementations
///
/// - [`TropicalWeight`](crate::semiring::TropicalWeight): `exp(-x)` converts cost to probability-like value
/// - [`LogWeight`](crate::semiring::LogWeight): `exp(-x)` recovers probability from negative log space
/// - [`ProbabilityWeight`](crate::semiring::ProbabilityWeight): Direct probability value
/// - [`PowerWeight`](crate::semiring::PowerWeight): Via power-to-probability isomorphism
pub trait StochasticSemiring: Semiring {
    /// Convert weight to a non-negative value suitable for probability sampling.
    ///
    /// The returned value should be in $`[0,\infty)`$. Higher values indicate
    /// higher probability of selection. Values will be normalized by the
    /// sampling algorithm.
    ///
    /// # Interpretation
    ///
    /// - For probability-like semirings: return the probability directly
    /// - For cost-like semirings: return `exp(-cost)` to convert to likelihood
    ///
    /// # Returns
    ///
    /// A non-negative f64 suitable for proportional sampling.
    fn to_probability(&self) -> f64;
}

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

    /// Helper function to verify idempotent semiring axioms.
    ///
    /// Verifies that $`a \oplus a=a`$ for the given weight.
    pub fn verify_idempotent_semiring<S: IdempotentSemiring>(a: S, epsilon: f64) {
        assert!(
            a.plus(&a).approx_eq(&a, epsilon),
            "Idempotency failed: a ⊕ a ≠ a"
        );
    }

    /// Helper function to verify zero-sum-free semiring axioms.
    ///
    /// Verifies that $`a \oplus b=\bar{0}`$ implies
    /// $`a=\bar{0}`$ and $`b=\bar{0}`$.
    pub fn verify_zero_sum_free_semiring<S: ZeroSumFreeSemiring>(a: S, b: S, epsilon: f64) {
        let sum = a.plus(&b);
        if sum.approx_eq(&S::zero(), epsilon) {
            assert!(
                a.approx_eq(&S::zero(), epsilon),
                "Zero-sum-free failed: a ⊕ b = 0̄ but a ≠ 0̄"
            );
            assert!(
                b.approx_eq(&S::zero(), epsilon),
                "Zero-sum-free failed: a ⊕ b = 0̄ but b ≠ 0̄"
            );
        }
    }

    /// Helper function to verify weakly left-divisible semiring axioms.
    ///
    /// Verifies that for non-zero divisor, `left_divide(a, divisor)` returns
    /// $`c`$ such that $`c \otimes \mathrm{divisor}=a`$.
    pub fn verify_weakly_left_divisible_semiring<S: WeaklyLeftDivisibleSemiring>(
        a: S,
        divisor: S,
        epsilon: f64,
    ) {
        if !divisor.is_zero() {
            if let Some(quotient) = a.left_divide(&divisor) {
                let product = quotient.times(&divisor);
                assert!(
                    product.approx_eq(&a, epsilon),
                    "Weak left-divisibility failed: (a / d) ⊗ d ≠ a"
                );
            }
        }
    }

    /// Helper function to verify commutative times semiring axioms.
    ///
    /// Verifies that $`a \otimes b=b \otimes a`$.
    pub fn verify_commutative_times_semiring<S: CommutativeTimesSemiring>(
        a: S,
        b: S,
        epsilon: f64,
    ) {
        assert!(
            a.times(&b).approx_eq(&b.times(&a), epsilon),
            "Multiplicative commutativity failed: a ⊗ b ≠ b ⊗ a"
        );
    }

    /// Helper function to verify k-closed semiring properties.
    ///
    /// For semirings with a finite closure bound, verifies that the star
    /// operation stabilizes within the bound.
    pub fn verify_k_closed_semiring<S: KClosedSemiring + StarSemiring>(a: S, epsilon: f64) {
        if let Some(k) = S::closure_bound() {
            // Compute the partial sum up to k iterations
            let mut partial_sum = S::one();
            let mut power = S::one();

            for _ in 0..=k {
                partial_sum = partial_sum.plus(&power);
                power = power.times(&a);
            }

            // Adding one more term should not change the sum
            let next_sum = partial_sum.plus(&power);

            assert!(
                partial_sum.approx_eq(&next_sum, epsilon),
                "k-closedness failed: sum did not stabilize at k={k}"
            );

            // If star is defined, it should equal the partial sum
            if let Some(star_a) = a.star() {
                assert!(
                    star_a.approx_eq(&partial_sum, epsilon),
                    "k-closedness failed: a* ≠ partial sum at k={k}"
                );
            }
        }
    }

    /// Helper function to verify totally ordered semiring properties.
    ///
    /// Verifies trichotomy and transitivity of the total order.
    pub fn verify_totally_ordered_semiring<S: TotallyOrderedSemiring>(a: S, b: S, c: S) {
        use std::cmp::Ordering;

        // Antisymmetry: cmp(a, b) is the reverse of cmp(b, a)
        let cmp_ab = a.total_cmp(&b);
        let cmp_ba = b.total_cmp(&a);
        assert_eq!(
            cmp_ab.reverse(),
            cmp_ba,
            "Total order antisymmetry failed: cmp(a,b) ≠ reverse(cmp(b,a))"
        );

        // Reflexivity: cmp(a, a) == Equal
        assert_eq!(
            a.total_cmp(&a),
            Ordering::Equal,
            "Total order reflexivity failed: cmp(a,a) ≠ Equal"
        );

        // Transitivity (for Less case)
        let cmp_bc = b.total_cmp(&c);
        let cmp_ac = a.total_cmp(&c);
        if cmp_ab == Ordering::Less && cmp_bc == Ordering::Less {
            assert_eq!(
                cmp_ac,
                Ordering::Less,
                "Total order transitivity failed: a < b < c but a ≮ c"
            );
        }
    }

    /// Helper function to verify quantizable semiring properties.
    ///
    /// Verifies that quantization is deterministic.
    pub fn verify_quantizable_semiring<S: QuantizableSemiring>(a: S, epsilon: f64) {
        // Quantization should be deterministic
        let q1 = a.quantize(epsilon);
        let q2 = a.quantize(epsilon);
        assert_eq!(q1, q2, "Quantization should be deterministic");

        // Quantization should handle epsilon scaling
        let q_fine = a.quantize(epsilon / 10.0);
        // Fine quantization may differ but should be valid
        let _ = q_fine; // Just verify it doesn't panic
    }

    /// Helper function to verify stochastic semiring properties.
    ///
    /// Verifies that to_probability returns non-negative values.
    pub fn verify_stochastic_semiring<S: StochasticSemiring>(a: S) {
        let prob = a.to_probability();
        assert!(
            prob >= 0.0,
            "Stochastic semiring failed: probability must be non-negative, got {}",
            prob
        );
        assert!(
            !prob.is_nan(),
            "Stochastic semiring failed: probability must not be NaN"
        );
    }
}
