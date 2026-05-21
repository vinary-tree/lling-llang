//! Comprehensive property-based tests for semiring trait verification.
//!
//! This module systematically verifies all semiring algebraic properties
//! across all concrete semiring implementations using the verification
//! helpers from `src/semiring/traits.rs`.

use lling_llang::semiring::{
    BoolWeight, CountWeight, ExpectationWeight, LexicographicWeight, LogWeight, PowerWeight,
    ProbabilityWeight, ProductWeight, Semiring, SignedTropicalWeight, TropicalWeight,
};
use proptest::prelude::*;

// =============================================================================
// Weight Generation Strategies
// =============================================================================

/// Strategy for tropical weights (non-negative costs)
fn arb_tropical() -> impl Strategy<Value = TropicalWeight> {
    prop_oneof![
        9 => (0.0f64..1000.0).prop_map(TropicalWeight::new),
        1 => Just(TropicalWeight::one()),
    ]
}

/// Strategy for tropical weights including infinity
fn arb_tropical_with_infinity() -> impl Strategy<Value = TropicalWeight> {
    prop_oneof![
        8 => (0.0f64..1000.0).prop_map(TropicalWeight::new),
        1 => Just(TropicalWeight::one()),
        1 => Just(TropicalWeight::zero()), // infinity
    ]
}

/// Strategy for log weights
fn arb_log() -> impl Strategy<Value = LogWeight> {
    prop_oneof![
        9 => (0.0f64..20.0).prop_map(LogWeight::new),
        1 => Just(LogWeight::one()),
    ]
}

/// Strategy for log weights including infinity
fn arb_log_with_infinity() -> impl Strategy<Value = LogWeight> {
    prop_oneof![
        8 => (0.0f64..20.0).prop_map(LogWeight::new),
        1 => Just(LogWeight::one()),
        1 => Just(LogWeight::zero()), // infinity
    ]
}

/// Strategy for probability weights [0, 1]
fn arb_probability() -> impl Strategy<Value = ProbabilityWeight> {
    prop_oneof![
        9 => (0.001f64..=1.0).prop_map(ProbabilityWeight::new),
        1 => Just(ProbabilityWeight::one()),
    ]
}

/// Strategy for probability weights including zero
fn arb_probability_with_zero() -> impl Strategy<Value = ProbabilityWeight> {
    prop_oneof![
        8 => (0.001f64..=1.0).prop_map(ProbabilityWeight::new),
        1 => Just(ProbabilityWeight::one()),
        1 => Just(ProbabilityWeight::zero()),
    ]
}

/// Strategy for boolean weights
fn arb_bool() -> impl Strategy<Value = BoolWeight> {
    any::<bool>().prop_map(BoolWeight::new)
}

/// Strategy for signed tropical weights (including negatives)
fn arb_signed_tropical() -> impl Strategy<Value = SignedTropicalWeight> {
    prop_oneof![
        8 => (-1000.0f64..1000.0).prop_map(SignedTropicalWeight::new),
        1 => Just(SignedTropicalWeight::one()),
        1 => Just(SignedTropicalWeight::zero()), // +infinity
    ]
}

/// Strategy for non-negative signed tropical weights (for star tests)
fn arb_signed_tropical_nonneg() -> impl Strategy<Value = SignedTropicalWeight> {
    prop_oneof![
        9 => (0.0f64..1000.0).prop_map(SignedTropicalWeight::new),
        1 => Just(SignedTropicalWeight::one()),
    ]
}

/// Strategy for power weights with eta parameter
fn arb_power() -> impl Strategy<Value = PowerWeight> {
    (0.001f64..1000.0, 0.1f64..10.0).prop_map(|(v, eta)| PowerWeight::new(v, eta))
}

/// Strategy for count weights
fn arb_count() -> impl Strategy<Value = CountWeight> {
    prop_oneof![
        9 => (0u64..1000).prop_map(CountWeight::new),
        1 => Just(CountWeight::one()),
    ]
}

/// Strategy for expectation weights
fn arb_expectation() -> impl Strategy<Value = ExpectationWeight> {
    (0.001f64..100.0, -100.0f64..100.0).prop_map(|(v, e)| ExpectationWeight::new(v, e))
}

/// Strategy for product weights (tropical × log)
fn arb_product_tropical_log() -> impl Strategy<Value = ProductWeight<TropicalWeight, LogWeight>> {
    (arb_tropical(), arb_log()).prop_map(|(w1, w2)| ProductWeight::new(w1, w2))
}

/// Strategy for lexicographic weights
fn arb_lexicographic() -> impl Strategy<Value = LexicographicWeight<TropicalWeight, TropicalWeight>>
{
    (arb_tropical(), arb_tropical()).prop_map(|(w1, w2)| LexicographicWeight::new(w1, w2))
}

// =============================================================================
// Module for importing verification functions
// =============================================================================

mod verification {
    use lling_llang::semiring::*;

    /// Verify basic semiring axioms.
    pub fn verify_semiring_axioms<S: Semiring>(a: S, b: S, c: S, epsilon: f64) {
        // Additive identity: a ⊕ 0 = a
        assert!(
            a.plus(&S::zero()).approx_eq(&a, epsilon),
            "Additive identity failed: a ⊕ 0̄ ≠ a"
        );

        // Multiplicative identity: a ⊗ 1 = a
        assert!(
            a.times(&S::one()).approx_eq(&a, epsilon),
            "Multiplicative identity (right) failed: a ⊗ 1̄ ≠ a"
        );
        assert!(
            S::one().times(&a).approx_eq(&a, epsilon),
            "Multiplicative identity (left) failed: 1̄ ⊗ a ≠ a"
        );

        // Additive commutativity: a ⊕ b = b ⊕ a
        assert!(
            a.plus(&b).approx_eq(&b.plus(&a), epsilon),
            "Additive commutativity failed: a ⊕ b ≠ b ⊕ a"
        );

        // Additive associativity: (a ⊕ b) ⊕ c = a ⊕ (b ⊕ c)
        let left = a.plus(&b).plus(&c);
        let right = a.plus(&b.plus(&c));
        assert!(
            left.approx_eq(&right, epsilon),
            "Additive associativity failed: (a ⊕ b) ⊕ c ≠ a ⊕ (b ⊕ c)"
        );

        // Multiplicative associativity: (a ⊗ b) ⊗ c = a ⊗ (b ⊗ c)
        let left = a.times(&b).times(&c);
        let right = a.times(&b.times(&c));
        assert!(
            left.approx_eq(&right, epsilon),
            "Multiplicative associativity failed: (a ⊗ b) ⊗ c ≠ a ⊗ (b ⊗ c)"
        );

        // Left distributivity: a ⊗ (b ⊕ c) = (a ⊗ b) ⊕ (a ⊗ c)
        let left = a.times(&b.plus(&c));
        let right = a.times(&b).plus(&a.times(&c));
        assert!(
            left.approx_eq(&right, epsilon),
            "Left distributivity failed: a ⊗ (b ⊕ c) ≠ (a ⊗ b) ⊕ (a ⊗ c)"
        );

        // Right distributivity: (a ⊕ b) ⊗ c = (a ⊗ c) ⊕ (b ⊗ c)
        let left = a.plus(&b).times(&c);
        let right = a.times(&c).plus(&b.times(&c));
        assert!(
            left.approx_eq(&right, epsilon),
            "Right distributivity failed: (a ⊕ b) ⊗ c ≠ (a ⊗ c) ⊕ (b ⊗ c)"
        );

        // Zero annihilation: 0 ⊗ a = a ⊗ 0 = 0
        assert!(
            S::zero().times(&a).approx_eq(&S::zero(), epsilon),
            "Zero annihilation (left) failed: 0̄ ⊗ a ≠ 0̄"
        );
        assert!(
            a.times(&S::zero()).approx_eq(&S::zero(), epsilon),
            "Zero annihilation (right) failed: a ⊗ 0̄ ≠ 0̄"
        );
    }

    /// Verify idempotent semiring: a ⊕ a = a
    pub fn verify_idempotent<S: IdempotentSemiring>(a: S, epsilon: f64) {
        assert!(
            a.plus(&a).approx_eq(&a, epsilon),
            "Idempotency failed: a ⊕ a ≠ a"
        );
    }

    /// Verify zero-sum-free: a ⊕ b = 0 implies a = b = 0
    pub fn verify_zero_sum_free<S: ZeroSumFreeSemiring>(a: S, b: S, epsilon: f64) {
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

    /// Verify commutative times: a ⊗ b = b ⊗ a
    pub fn verify_commutative_times<S: CommutativeTimesSemiring>(a: S, b: S, epsilon: f64) {
        assert!(
            a.times(&b).approx_eq(&b.times(&a), epsilon),
            "Multiplicative commutativity failed: a ⊗ b ≠ b ⊗ a"
        );
    }

    /// Verify divisible semiring: (a ⊗ b) ÷ b = a
    pub fn verify_divisible<S: DivisibleSemiring>(a: S, b: S, epsilon: f64) {
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

    /// Verify star semiring: a* = 1 ⊕ (a ⊗ a*)
    pub fn verify_star<S: StarSemiring>(a: S, epsilon: f64) {
        if let Some(star_a) = a.star() {
            let expected = S::one().plus(&a.times(&star_a));
            assert!(
                star_a.approx_eq(&expected, epsilon),
                "Star axiom failed: a* ≠ 1̄ ⊕ (a ⊗ a*)"
            );
        }
    }

    /// Verify weakly left divisible: left_divide(a, d) ⊗ d = a
    pub fn verify_weakly_left_divisible<S: WeaklyLeftDivisibleSemiring>(
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

    /// Verify k-closed semiring: star converges in bounded iterations
    pub fn verify_k_closed<S: KClosedSemiring + StarSemiring>(a: S, epsilon: f64) {
        if let Some(k) = S::closure_bound() {
            // Compute partial sum up to k iterations
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

            // Star should equal partial sum
            if let Some(star_a) = a.star() {
                assert!(
                    star_a.approx_eq(&partial_sum, epsilon),
                    "k-closedness failed: a* ≠ partial sum at k={k}"
                );
            }
        }
    }

    /// Verify totally ordered semiring: total order properties
    pub fn verify_totally_ordered<S: TotallyOrderedSemiring>(a: S, b: S, c: S) {
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

        // Transitivity for Less
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

    /// Verify quantizable semiring: deterministic quantization
    pub fn verify_quantizable<S: QuantizableSemiring>(a: S, epsilon: f64) {
        let q1 = a.quantize(epsilon);
        let q2 = a.quantize(epsilon);
        assert_eq!(q1, q2, "Quantization should be deterministic");
    }

    /// Verify stochastic semiring: non-negative probability
    pub fn verify_stochastic<S: StochasticSemiring>(a: S) {
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

// =============================================================================
// TropicalWeight Tests
// =============================================================================

mod tropical_tests {
    use super::*;
    use verification::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn semiring_axioms(a in arb_tropical(), b in arb_tropical(), c in arb_tropical()) {
            verify_semiring_axioms(a, b, c, 1e-10);
        }

        #[test]
        fn idempotent(a in arb_tropical_with_infinity()) {
            verify_idempotent(a, 1e-10);
        }

        #[test]
        fn zero_sum_free(a in arb_tropical_with_infinity(), b in arb_tropical_with_infinity()) {
            verify_zero_sum_free(a, b, 1e-10);
        }

        #[test]
        fn commutative_times(a in arb_tropical(), b in arb_tropical()) {
            verify_commutative_times(a, b, 1e-10);
        }

        #[test]
        fn divisible(a in arb_tropical(), b in (0.001f64..1000.0).prop_map(TropicalWeight::new)) {
            verify_divisible(a, b, 1e-10);
        }

        #[test]
        fn star(a in arb_tropical()) {
            verify_star(a, 1e-10);
        }

        #[test]
        fn weakly_left_divisible(
            a in arb_tropical(),
            b in (0.001f64..1000.0).prop_map(TropicalWeight::new)
        ) {
            let divisor = a.plus(&b);
            verify_weakly_left_divisible(a, divisor, 1e-10);
        }

        #[test]
        fn k_closed(a in arb_tropical()) {
            verify_k_closed(a, 1e-10);
        }

        #[test]
        fn totally_ordered(a in arb_tropical(), b in arb_tropical(), c in arb_tropical()) {
            verify_totally_ordered(a, b, c);
        }

        #[test]
        fn quantizable(a in arb_tropical()) {
            verify_quantizable(a, 1e-10);
        }

        #[test]
        fn stochastic(a in (0.0f64..100.0).prop_map(TropicalWeight::new)) {
            verify_stochastic(a);
        }
    }
}

// =============================================================================
// LogWeight Tests
// =============================================================================

mod log_tests {
    use super::*;
    use verification::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn semiring_axioms(a in arb_log(), b in arb_log(), c in arb_log()) {
            // Log-sum-exp has numerical precision limitations with large value differences
            verify_semiring_axioms(a, b, c, 1e-6);
        }

        #[test]
        fn zero_sum_free(a in arb_log_with_infinity(), b in arb_log_with_infinity()) {
            verify_zero_sum_free(a, b, 1e-9);
        }

        #[test]
        fn commutative_times(a in arb_log(), b in arb_log()) {
            verify_commutative_times(a, b, 1e-9);
        }

        #[test]
        fn divisible(a in arb_log(), b in (0.001f64..20.0).prop_map(LogWeight::new)) {
            verify_divisible(a, b, 1e-9);
        }

        #[test]
        fn star(a in (1.0f64..20.0).prop_map(LogWeight::new)) {
            // Star converges for weights >= 1 in log semiring
            verify_star(a, 1e-6);
        }

        #[test]
        fn weakly_left_divisible(
            a in arb_log(),
            b in (0.001f64..20.0).prop_map(LogWeight::new)
        ) {
            let divisor = a.plus(&b);
            verify_weakly_left_divisible(a, divisor, 1e-9);
        }

        #[test]
        fn totally_ordered(a in arb_log(), b in arb_log(), c in arb_log()) {
            verify_totally_ordered(a, b, c);
        }

        #[test]
        fn quantizable(a in arb_log()) {
            verify_quantizable(a, 1e-10);
        }

        #[test]
        fn stochastic(a in (0.0f64..20.0).prop_map(LogWeight::new)) {
            verify_stochastic(a);
        }
    }
}

// =============================================================================
// ProbabilityWeight Tests
// =============================================================================

mod probability_tests {
    use super::*;
    use verification::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn semiring_axioms(
            a in arb_probability(),
            b in arb_probability(),
            c in arb_probability()
        ) {
            verify_semiring_axioms(a, b, c, 1e-10);
        }

        #[test]
        fn zero_sum_free(
            a in arb_probability_with_zero(),
            b in arb_probability_with_zero()
        ) {
            verify_zero_sum_free(a, b, 1e-10);
        }

        #[test]
        fn commutative_times(a in arb_probability(), b in arb_probability()) {
            verify_commutative_times(a, b, 1e-10);
        }

        #[test]
        fn divisible(
            a in arb_probability(),
            b in (0.01f64..=1.0).prop_map(ProbabilityWeight::new)
        ) {
            verify_divisible(a, b, 1e-10);
        }

        #[test]
        fn weakly_left_divisible(
            a in arb_probability(),
            b in (0.01f64..=1.0).prop_map(ProbabilityWeight::new)
        ) {
            let divisor = a.plus(&b);
            verify_weakly_left_divisible(a, divisor, 1e-10);
        }

        #[test]
        fn stochastic(a in arb_probability()) {
            verify_stochastic(a);
        }
    }
}

// =============================================================================
// BoolWeight Tests
// =============================================================================

mod bool_tests {
    use super::*;
    use verification::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn semiring_axioms(a in arb_bool(), b in arb_bool(), c in arb_bool()) {
            verify_semiring_axioms(a, b, c, 0.0);
        }

        #[test]
        fn idempotent(a in arb_bool()) {
            verify_idempotent(a, 0.0);
        }

        #[test]
        fn zero_sum_free(a in arb_bool(), b in arb_bool()) {
            verify_zero_sum_free(a, b, 0.0);
        }

        #[test]
        fn commutative_times(a in arb_bool(), b in arb_bool()) {
            verify_commutative_times(a, b, 0.0);
        }

        #[test]
        fn star(a in arb_bool()) {
            verify_star(a, 0.0);
        }

        #[test]
        fn k_closed(a in arb_bool()) {
            verify_k_closed(a, 0.0);
        }
    }

    /// Exhaustive test of all boolean combinations
    #[test]
    fn exhaustive_semiring_axioms() {
        let values = [BoolWeight::new(true), BoolWeight::new(false)];
        for &a in &values {
            for &b in &values {
                for &c in &values {
                    verify_semiring_axioms(a, b, c, 0.0);
                    verify_idempotent(a, 0.0);
                    verify_zero_sum_free(a, b, 0.0);
                    verify_commutative_times(a, b, 0.0);
                    verify_star(a, 0.0);
                    verify_k_closed(a, 0.0);
                }
            }
        }
    }
}

// =============================================================================
// SignedTropicalWeight Tests
// =============================================================================

mod signed_tropical_tests {
    use super::*;
    use verification::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn semiring_axioms(
            a in arb_signed_tropical(),
            b in arb_signed_tropical(),
            c in arb_signed_tropical()
        ) {
            verify_semiring_axioms(a, b, c, 1e-10);
        }

        #[test]
        fn idempotent(a in arb_signed_tropical()) {
            verify_idempotent(a, 1e-10);
        }

        #[test]
        fn commutative_times(a in arb_signed_tropical(), b in arb_signed_tropical()) {
            verify_commutative_times(a, b, 1e-10);
        }

        #[test]
        fn divisible(
            a in arb_signed_tropical(),
            b in (-1000.0f64..1000.0)
                .prop_filter("non-infinite", |x| x.is_finite())
                .prop_map(SignedTropicalWeight::new)
        ) {
            verify_divisible(a, b, 1e-10);
        }

        #[test]
        fn weakly_left_divisible(
            a in arb_signed_tropical(),
            b in arb_signed_tropical()
        ) {
            let divisor = a.plus(&b);
            verify_weakly_left_divisible(a, divisor, 1e-10);
        }

        #[test]
        fn totally_ordered(
            a in arb_signed_tropical(),
            b in arb_signed_tropical(),
            c in arb_signed_tropical()
        ) {
            verify_totally_ordered(a, b, c);
        }

        #[test]
        fn quantizable(a in arb_signed_tropical()) {
            verify_quantizable(a, 1e-10);
        }

        #[test]
        fn star_nonnegative(a in arb_signed_tropical_nonneg()) {
            // Star is only defined for non-negative weights
            if let Some(star_a) = a.star_checked() {
                let expected = SignedTropicalWeight::one().plus(&a.times(&star_a));
                assert!(
                    star_a.approx_eq(&expected, 1e-10),
                    "Star axiom failed for non-negative signed tropical"
                );
            }
        }

        #[test]
        fn star_negative_diverges(
            a in (-1000.0f64..-0.001).prop_map(SignedTropicalWeight::new)
        ) {
            // Star should return None for negative weights
            assert!(
                a.star_checked().is_none(),
                "Star should diverge for negative weights"
            );
        }
    }
}

// =============================================================================
// PowerWeight Tests
// =============================================================================

mod power_tests {
    use super::*;
    use verification::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn semiring_axioms(
            a in arb_power(),
            b in arb_power(),
            c in arb_power()
        ) {
            // Use eta=1.0 to match S::zero() and S::one() which use default eta
            let eta = 1.0;
            let a = PowerWeight::new(a.value(), eta);
            let b = PowerWeight::new(b.value(), eta);
            let c = PowerWeight::new(c.value(), eta);
            verify_semiring_axioms(a, b, c, 1e-6);
        }

        #[test]
        fn zero_sum_free(a in arb_power(), b in arb_power()) {
            let eta = 1.0;
            let a = PowerWeight::new(a.value(), eta);
            let b = PowerWeight::new(b.value(), eta);
            verify_zero_sum_free(a, b, 1e-6);
        }

        #[test]
        fn commutative_times(a in arb_power(), b in arb_power()) {
            let eta = 1.0;
            let a = PowerWeight::new(a.value(), eta);
            let b = PowerWeight::new(b.value(), eta);
            verify_commutative_times(a, b, 1e-6);
        }

        #[test]
        fn stochastic(a in arb_power()) {
            verify_stochastic(a);
        }
    }
}

// =============================================================================
// CountWeight Tests
// =============================================================================

mod count_tests {
    use super::*;
    use verification::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn semiring_axioms(a in arb_count(), b in arb_count(), c in arb_count()) {
            verify_semiring_axioms(a, b, c, 0.0);
        }

        #[test]
        fn zero_sum_free(a in arb_count(), b in arb_count()) {
            verify_zero_sum_free(a, b, 0.0);
        }

        #[test]
        fn commutative_times(a in arb_count(), b in arb_count()) {
            verify_commutative_times(a, b, 0.0);
        }
    }

    #[test]
    fn basic_operations() {
        let a = CountWeight::new(3);
        let b = CountWeight::new(5);

        // Plus is addition
        assert_eq!(a.plus(&b).value(), 8);

        // Times is multiplication
        assert_eq!(a.times(&b).value(), 15);

        // Zero is additive identity (0)
        assert_eq!(CountWeight::zero().value(), 0);

        // One is multiplicative identity (1)
        assert_eq!(CountWeight::one().value(), 1);
    }
}

// =============================================================================
// ExpectationWeight Tests
// =============================================================================

mod expectation_tests {
    use super::*;
    use verification::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn semiring_axioms(
            a in arb_expectation(),
            b in arb_expectation(),
            c in arb_expectation()
        ) {
            verify_semiring_axioms(a, b, c, 1e-6);
        }

        #[test]
        fn zero_sum_free(a in arb_expectation(), b in arb_expectation()) {
            verify_zero_sum_free(a, b, 1e-6);
        }

        #[test]
        fn commutative_times(a in arb_expectation(), b in arb_expectation()) {
            verify_commutative_times(a, b, 1e-6);
        }
    }

    #[test]
    fn expectation_semantics() {
        // Test that expectation weight correctly computes expected values
        let a = ExpectationWeight::new(0.5, 10.0); // 50% prob, value 10
        let b = ExpectationWeight::new(0.5, 20.0); // 50% prob, value 20

        // Sum should give total prob and weighted sum
        let sum = a.plus(&b);
        assert!((sum.value() - 1.0).abs() < 1e-10); // Total prob = 1.0
        assert!((sum.expectation() - 30.0).abs() < 1e-10); // Weighted sum = 10 + 20 = 30
    }
}

// =============================================================================
// ProductWeight Tests
// =============================================================================

mod product_tests {
    use super::*;
    use verification::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn semiring_axioms(
            a in arb_product_tropical_log(),
            b in arb_product_tropical_log(),
            c in arb_product_tropical_log()
        ) {
            // ProductWeight with LogWeight inherits log-sum-exp precision limitations
            verify_semiring_axioms(a, b, c, 1e-6);
        }

        #[test]
        fn commutative_times(
            a in arb_product_tropical_log(),
            b in arb_product_tropical_log()
        ) {
            verify_commutative_times(a, b, 1e-9);
        }
    }

    #[test]
    fn product_component_operations() {
        let w1 = TropicalWeight::new(2.0);
        let w2 = LogWeight::new(3.0);
        let a = ProductWeight::new(w1, w2);

        let v1 = TropicalWeight::new(4.0);
        let v2 = LogWeight::new(5.0);
        let b = ProductWeight::new(v1, v2);

        // Plus operates component-wise
        let sum = a.plus(&b);
        assert!(sum.first().approx_eq(&w1.plus(&v1), 1e-10));
        assert!(sum.second().approx_eq(&w2.plus(&v2), 1e-10));

        // Times operates component-wise
        let prod = a.times(&b);
        assert!(prod.first().approx_eq(&w1.times(&v1), 1e-10));
        assert!(prod.second().approx_eq(&w2.times(&v2), 1e-10));
    }
}

// =============================================================================
// LexicographicWeight Tests
// =============================================================================

mod lexicographic_tests {
    use super::*;
    use verification::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn semiring_axioms(
            a in arb_lexicographic(),
            b in arb_lexicographic(),
            c in arb_lexicographic()
        ) {
            verify_semiring_axioms(a, b, c, 1e-10);
        }

        #[test]
        fn idempotent(a in arb_lexicographic()) {
            verify_idempotent(a, 1e-10);
        }

        #[test]
        fn commutative_times(a in arb_lexicographic(), b in arb_lexicographic()) {
            verify_commutative_times(a, b, 1e-10);
        }

        #[test]
        fn totally_ordered(
            a in arb_lexicographic(),
            b in arb_lexicographic(),
            c in arb_lexicographic()
        ) {
            verify_totally_ordered(a, b, c);
        }
    }

    #[test]
    fn lexicographic_ordering() {
        // Test that plus uses lexicographic minimum
        let a = LexicographicWeight::new(TropicalWeight::new(1.0), TropicalWeight::new(5.0));
        let b = LexicographicWeight::new(TropicalWeight::new(1.0), TropicalWeight::new(3.0));
        let c = LexicographicWeight::new(TropicalWeight::new(2.0), TropicalWeight::new(1.0));

        // (1,5) vs (1,3) -> (1,3) wins on second component
        let sum_ab = a.plus(&b);
        assert_eq!(sum_ab.first().value(), 1.0);
        assert_eq!(sum_ab.second().value(), 3.0);

        // (1,3) vs (2,1) -> (1,3) wins on first component
        let sum_bc = b.plus(&c);
        assert_eq!(sum_bc.first().value(), 1.0);
        assert_eq!(sum_bc.second().value(), 3.0);
    }
}

// =============================================================================
// Cross-Semiring Compatibility Tests
// =============================================================================

mod cross_semiring_tests {
    use super::*;

    #[test]
    fn tropical_to_signed_conversion() {
        let t = TropicalWeight::new(5.0);
        let s: SignedTropicalWeight = t.into();
        assert_eq!(s.value(), 5.0);
    }

    #[test]
    fn signed_to_tropical_conversion_success() {
        let s = SignedTropicalWeight::new(5.0);
        let result: Result<TropicalWeight, _> = s.try_into();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().value(), 5.0);
    }

    #[test]
    fn signed_to_tropical_conversion_fail() {
        let s = SignedTropicalWeight::new(-1.0);
        let result: Result<TropicalWeight, _> = s.try_into();
        assert!(result.is_err());
    }

    #[test]
    fn product_weight_variants() {
        // Test different product weight combinations
        type TropicalLog = ProductWeight<TropicalWeight, LogWeight>;
        type TropicalTropical = ProductWeight<TropicalWeight, TropicalWeight>;
        type LogLog = ProductWeight<LogWeight, LogWeight>;

        // Verify each satisfies semiring axioms
        let tl = TropicalLog::new(TropicalWeight::new(1.0), LogWeight::new(2.0));
        let tt = TropicalTropical::new(TropicalWeight::new(1.0), TropicalWeight::new(2.0));
        let ll = LogLog::new(LogWeight::new(1.0), LogWeight::new(2.0));

        assert!(tl.plus(&TropicalLog::zero()).approx_eq(&tl, 1e-10));
        assert!(tt.plus(&TropicalTropical::zero()).approx_eq(&tt, 1e-10));
        assert!(ll.plus(&LogLog::zero()).approx_eq(&ll, 1e-10));
    }

    #[test]
    fn lexicographic_variants() {
        use lling_llang::semiring::{lexicographic3, lexicographic4};

        // 3-tuple lexicographic weight
        let l3 = lexicographic3(
            TropicalWeight::new(1.0),
            TropicalWeight::new(2.0),
            TropicalWeight::new(3.0),
        );
        let l3_zero = lexicographic3(
            TropicalWeight::zero(),
            TropicalWeight::zero(),
            TropicalWeight::zero(),
        );
        assert!(l3.plus(&l3_zero).approx_eq(&l3, 1e-10));

        // 4-tuple lexicographic weight
        let l4 = lexicographic4(
            TropicalWeight::new(1.0),
            TropicalWeight::new(2.0),
            TropicalWeight::new(3.0),
            TropicalWeight::new(4.0),
        );
        let l4_zero = lexicographic4(
            TropicalWeight::zero(),
            TropicalWeight::zero(),
            TropicalWeight::zero(),
            TropicalWeight::zero(),
        );
        assert!(l4.plus(&l4_zero).approx_eq(&l4, 1e-10));
    }
}

// =============================================================================
// Edge Case and Boundary Tests
// =============================================================================

mod edge_case_tests {
    use super::*;

    #[test]
    fn tropical_infinity_operations() {
        let inf = TropicalWeight::zero(); // infinity
        let finite = TropicalWeight::new(5.0);

        // inf ⊕ x = x (infinity is additive identity)
        assert_eq!(inf.plus(&finite), finite);

        // x ⊗ inf = inf (infinity annihilates)
        assert!(finite.times(&inf).is_zero());

        // inf ⊕ inf = inf
        assert!(inf.plus(&inf).is_zero());
    }

    #[test]
    fn log_infinity_operations() {
        let inf = LogWeight::zero(); // infinity
        let finite = LogWeight::new(5.0);

        // inf ⊕ x = x
        assert_eq!(inf.plus(&finite), finite);

        // x ⊗ inf = inf
        assert!(finite.times(&inf).is_zero());
    }

    #[test]
    fn probability_boundary_values() {
        let zero = ProbabilityWeight::zero();
        let one = ProbabilityWeight::one();
        let half = ProbabilityWeight::new(0.5);

        // 0 is additive identity
        assert_eq!(half.plus(&zero), half);

        // 1 is multiplicative identity
        assert_eq!(half.times(&one), half);

        // 0 annihilates
        assert_eq!(half.times(&zero), zero);
    }

    #[test]
    fn signed_tropical_boundary_values() {
        let pos_inf = SignedTropicalWeight::infinity();
        let neg_inf = SignedTropicalWeight::neg_infinity();
        let zero = SignedTropicalWeight::one(); // multiplicative identity = 0.0

        // pos_inf is additive identity
        let finite = SignedTropicalWeight::new(5.0);
        assert_eq!(pos_inf.plus(&finite), finite);

        // neg_inf dominates all finite values in plus (min)
        assert_eq!(finite.plus(&neg_inf), neg_inf);

        // zero is multiplicative identity
        assert_eq!(finite.times(&zero), finite);
    }

    #[test]
    fn count_overflow_safety() {
        let large = CountWeight::new(u64::MAX / 2);
        let small = CountWeight::new(10);

        // Ensure overflow doesn't panic (may wrap or saturate)
        let _ = large.times(&small);
        let _ = large.plus(&large);
    }

    #[test]
    fn expectation_degenerate_cases() {
        let zero_prob = ExpectationWeight::zero();
        let one = ExpectationWeight::one();

        // Zero is additive identity
        let a = ExpectationWeight::new(0.5, 10.0);
        assert!(a.plus(&zero_prob).approx_eq(&a, 1e-10));

        // One is multiplicative identity
        assert!(a.times(&one).approx_eq(&a, 1e-10));
    }
}
