//! η-Power semiring for soft path selection.
//!
//! The η-power semiring `S_η = (R+ ∪ {+∞}, ⊕_η, ×, 0, 1)` provides a parameterized
//! family of semirings that interpolate between different optimization objectives:
//!
//! - **⊕_η**: `x ⊕_η y = (x^(1/η) + y^(1/η))^η` (generalized addition)
//! - **⊗**: `x ⊗ y = x × y` (standard multiplication)
//! - **0̄**: `0` (additive identity)
//! - **1̄**: `1` (multiplicative identity)
//!
//! # Properties
//!
//! The η parameter controls the "softness" of the addition operation:
//!
//! - As `η → 0`: approaches max semiring (winner-take-all)
//! - At `η = 1`: equivalent to probability semiring (standard addition)
//! - As `η → ∞`: approaches min semiring on inverse scale
//!
//! # Isomorphism
//!
//! The η-power semiring is isomorphic to the probability semiring via:
//!
//! - `Ψ_η(x) = x^η` maps `(R+, +, ×, 0, 1) → S_η`
//! - `Ψ_η^{-1}(x) = x^{1/η}` maps `S_η → (R+, +, ×, 0, 1)`
//!
//! This isomorphism preserves semiring operations:
//! - `Ψ_η(x + y) = Ψ_η(x) ⊕_η Ψ_η(y)`
//! - `Ψ_η(x × y) = Ψ_η(x) × Ψ_η(y)`
//!
//! # Use Cases
//!
//! - **Softmax-like path selection**: Interpolate between hard and soft argmax
//! - **Differentiable WFST operations**: Smooth approximations for gradient descent
//! - **Rational loss functions**: As described in Cortes et al. (2015)
//! - **RRWM algorithm**: Online learning with rational losses
//!
//! # References
//!
//! - Cortes, C., Kuznetsov, V., Mohri, M., & Warmuth, M. K. (2015).
//!   "On-Line Learning Algorithms for Path Experts with Non-Additive Losses"
//!   JMLR 16, 2015.

use ordered_float::OrderedFloat;

use super::traits::{
    CommutativeTimesSemiring, DivisibleSemiring, KClosedSemiring, NonnegativeSemiring,
    NumericalWeight, QuantizableSemiring, Semiring, StarSemiring, StochasticSemiring,
    TotallyOrderedSemiring, ZeroSumFreeSemiring,
};

/// Default η value (equivalent to probability semiring).
pub const DEFAULT_ETA: f64 = 1.0;

/// η-Power semiring weight.
///
/// Stores a value in the η-power semiring along with the η parameter.
/// The η parameter determines the "softness" of the addition operation.
#[derive(Clone, Copy, Debug)]
pub struct PowerWeight {
    /// The weight value in the power semiring.
    value: OrderedFloat<f64>,
    /// The η parameter controlling the semiring behavior.
    eta: OrderedFloat<f64>,
}

impl PowerWeight {
    /// Create a new power weight with the given value and η parameter.
    ///
    /// # Arguments
    ///
    /// * `value` - The weight value (must be non-negative)
    /// * `eta` - The η parameter (must be positive)
    ///
    /// # Panics
    ///
    /// Panics if η ≤ 0.
    #[inline]
    pub fn new(value: f64, eta: f64) -> Self {
        debug_assert!(eta > 0.0, "η must be positive, got {}", eta);
        Self {
            value: OrderedFloat(value.max(0.0)),
            eta: OrderedFloat(eta),
        }
    }

    /// Create a new power weight with the default η = 1.0.
    #[inline]
    pub fn with_default_eta(value: f64) -> Self {
        Self::new(value, DEFAULT_ETA)
    }

    /// Get the underlying value.
    #[inline]
    pub fn value(&self) -> f64 {
        self.value.into_inner()
    }

    /// Get the η parameter.
    #[inline]
    pub fn eta(&self) -> f64 {
        self.eta.into_inner()
    }

    /// Create a zero weight (additive identity).
    #[inline]
    pub fn zero_with_eta(eta: f64) -> Self {
        Self::new(0.0, eta)
    }

    /// Create a one weight (multiplicative identity).
    #[inline]
    pub fn one_with_eta(eta: f64) -> Self {
        Self::new(1.0, eta)
    }

    /// Create an infinity weight (for unreachable states).
    #[inline]
    pub fn infinity(eta: f64) -> Self {
        Self::new(f64::INFINITY, eta)
    }

    /// Check if this weight is zero.
    #[inline]
    pub fn is_zero_value(&self) -> bool {
        self.value.into_inner() == 0.0
    }

    /// Check if this weight is one.
    #[inline]
    pub fn is_one_value(&self) -> bool {
        (self.value.into_inner() - 1.0).abs() < f64::EPSILON
    }

    /// Check if this weight is infinite.
    #[inline]
    pub fn is_infinite(&self) -> bool {
        self.value.is_infinite()
    }

    /// Convert from the probability semiring using the isomorphism Ψ_η(x) = x^η.
    ///
    /// This maps a probability value `p` to its representation in the η-power semiring.
    ///
    /// # Arguments
    ///
    /// * `prob` - A probability value (typically in [0, 1] but can be any non-negative value)
    /// * `eta` - The η parameter for the target power semiring
    #[inline]
    pub fn from_probability(prob: f64, eta: f64) -> Self {
        Self::new(prob.powf(eta), eta)
    }

    /// Convert to the probability semiring using the inverse isomorphism Ψ_η^{-1}(x) = x^{1/η}.
    ///
    /// This recovers the original probability value from the power semiring representation.
    #[inline]
    pub fn to_probability(&self) -> f64 {
        let eta = self.eta.into_inner();
        if eta == 0.0 {
            // Special case: η = 0 would require computing x^∞
            // In the limit, this should return 0 for x < 1, 1 for x = 1, ∞ for x > 1
            let v = self.value.into_inner();
            if v < 1.0 {
                0.0
            } else if v == 1.0 {
                1.0
            } else {
                f64::INFINITY
            }
        } else {
            self.value.powf(1.0 / eta)
        }
    }

    /// Compute the power semiring addition: x ⊕_η y = (x^{1/η} + y^{1/η})^η
    #[inline]
    fn power_plus(&self, other: &Self) -> Self {
        let eta = self.eta.into_inner();

        // Handle special cases
        if self.is_zero_value() {
            return Self::new(other.value.into_inner(), eta);
        }
        if other.is_zero_value() {
            return Self::new(self.value.into_inner(), eta);
        }
        if self.is_infinite() || other.is_infinite() {
            return Self::infinity(eta);
        }

        // General case: (x^{1/η} + y^{1/η})^η
        let x_root = self.value.powf(1.0 / eta);
        let y_root = other.value.powf(1.0 / eta);
        let sum = x_root + y_root;
        Self::new(sum.powf(eta), eta)
    }

    /// Ensure both weights have compatible η values.
    ///
    /// For now, we require exact η match. In a more sophisticated implementation,
    /// we could convert between different η values using the isomorphism.
    #[inline]
    fn check_eta_compatibility(&self, other: &Self) {
        debug_assert!(
            (self.eta.into_inner() - other.eta.into_inner()).abs() < 1e-10,
            "η values must match: {} vs {}",
            self.eta,
            other.eta
        );
    }
}

impl PartialEq for PowerWeight {
    fn eq(&self, other: &Self) -> bool {
        // Two weights are equal if they have the same value and compatible η
        (self.value.into_inner() - other.value.into_inner()).abs() < f64::EPSILON
            && (self.eta.into_inner() - other.eta.into_inner()).abs() < f64::EPSILON
    }
}

impl Eq for PowerWeight {}

impl std::hash::Hash for PowerWeight {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash the bits of the value and eta for exact matching
        self.value.to_bits().hash(state);
        self.eta.to_bits().hash(state);
    }
}

impl PartialOrd for PowerWeight {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PowerWeight {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Order by value (larger is "better" in probability interpretation)
        self.value.cmp(&other.value)
    }
}

impl Default for PowerWeight {
    /// Default is multiplicative identity (one) with default η.
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl From<f64> for PowerWeight {
    fn from(value: f64) -> Self {
        Self::with_default_eta(value)
    }
}

impl From<PowerWeight> for f64 {
    fn from(weight: PowerWeight) -> Self {
        weight.value()
    }
}

impl Semiring for PowerWeight {
    /// Additive identity: 0
    #[inline]
    fn zero() -> Self {
        Self::zero_with_eta(DEFAULT_ETA)
    }

    /// Multiplicative identity: 1
    #[inline]
    fn one() -> Self {
        Self::one_with_eta(DEFAULT_ETA)
    }

    /// Addition: x ⊕_η y = (x^{1/η} + y^{1/η})^η
    #[inline]
    fn plus(&self, other: &Self) -> Self {
        self.check_eta_compatibility(other);
        self.power_plus(other)
    }

    /// Multiplication: x ⊗ y = x × y
    #[inline]
    fn times(&self, other: &Self) -> Self {
        self.check_eta_compatibility(other);
        Self::new(
            self.value.into_inner() * other.value.into_inner(),
            self.eta.into_inner(),
        )
    }

    #[inline]
    fn is_zero(&self) -> bool {
        self.is_zero_value()
    }

    #[inline]
    fn is_one(&self) -> bool {
        self.is_one_value()
    }

    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        if self.is_zero() && other.is_zero() {
            return true;
        }
        if self.is_infinite() && other.is_infinite() {
            return true;
        }
        if self.is_zero() || other.is_zero() || self.is_infinite() || other.is_infinite() {
            return false;
        }
        (self.value.into_inner() - other.value.into_inner()).abs() <= epsilon
            && (self.eta.into_inner() - other.eta.into_inner()).abs() <= epsilon
    }

    /// Natural ordering: larger value is "better" (higher probability).
    fn natural_less(&self, other: &Self) -> Option<bool> {
        // In probability interpretation, larger values are better
        // So self is "better" if self.value > other.value
        // natural_less returns true if self is less preferred than other
        Some(self.value < other.value)
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&self.value.into_inner().to_le_bytes());
        bytes.extend_from_slice(&self.eta.into_inner().to_le_bytes());
        bytes
    }
}

impl DivisibleSemiring for PowerWeight {
    /// Division: x / y (standard division since multiplication is standard).
    fn divide(&self, other: &Self) -> Option<Self> {
        self.check_eta_compatibility(other);
        if other.is_zero() {
            None
        } else {
            Some(Self::new(
                self.value.into_inner() / other.value.into_inner(),
                self.eta.into_inner(),
            ))
        }
    }
}

impl StarSemiring for PowerWeight {
    /// Kleene closure for the power semiring.
    ///
    /// The power semiring is isomorphic to the probability semiring via Ψ_η(x) = x^η.
    /// In the probability semiring, a* = 1/(1-a) for a < 1.
    ///
    /// Since we store values in the power semiring representation (y = a^η where a is
    /// the probability value), the star must be computed as:
    ///
    /// - Convert to probability: a = y^{1/η}
    /// - Compute probability star: a* = 1/(1-a)
    /// - Convert back to power: (a*)^η
    ///
    /// This converges when the underlying probability a = y^{1/η} < 1.
    fn star(&self) -> Option<Self> {
        let v = self.value.into_inner();
        let eta = self.eta.into_inner();

        // Convert to probability space
        let prob = v.powf(1.0 / eta);

        if prob < 1.0 {
            // Geometric series converges in probability space: 1/(1-prob)
            let prob_star = 1.0 / (1.0 - prob);
            // Convert back to power semiring
            Some(Self::new(prob_star.powf(eta), eta))
        } else if (prob - 1.0).abs() < f64::EPSILON {
            // prob = 1: series diverges
            None
        } else {
            // prob > 1: series diverges
            None
        }
    }
}

impl NumericalWeight for PowerWeight {
    #[inline]
    fn numerical_value(&self) -> f64 {
        self.value()
    }
}

// ============================================================================
// Algebraic Property Marker Trait Implementations
// ============================================================================

// Note: PowerWeight is NOT idempotent.
// a ⊕_η a = (a^{1/η} + a^{1/η})^η = (2·a^{1/η})^η = 2^η · a ≠ a for η ≠ 0

/// PowerWeight is k-closed with no uniform bound.
///
/// The star operation converges when the underlying probability is < 1,
/// but there's no fixed k that works for all values.
impl KClosedSemiring for PowerWeight {
    fn closure_bound() -> Option<usize> {
        // No uniform bound - depends on the specific value
        None
    }
}

/// PowerWeight is zero-sum-free.
///
/// Since all values are non-negative (enforced by constructor),
/// x ⊕_η y = 0 only when both x = 0 and y = 0.
impl ZeroSumFreeSemiring for PowerWeight {}

/// PowerWeight has commutative multiplication.
///
/// x ⊗ y = x × y = y × x = y ⊗ x
impl CommutativeTimesSemiring for PowerWeight {}

// ============================================================================
// Algorithm Requirement Trait Implementations
// ============================================================================

/// PowerWeight has a total order.
impl TotallyOrderedSemiring for PowerWeight {}

/// PowerWeight values are non-negative (clamped to 0 in constructor).
impl NonnegativeSemiring for PowerWeight {}

/// PowerWeight can be quantized for approximate comparison.
impl QuantizableSemiring for PowerWeight {
    fn quantize(&self, epsilon: f64) -> i64 {
        let v = self.value();
        if v.is_nan() {
            i64::MIN
        } else if v.is_infinite() {
            i64::MAX
        } else {
            (v / epsilon).round() as i64
        }
    }
}

/// PowerWeight can be converted to probability for sampling.
///
/// Uses the existing `to_probability()` method which computes value^{1/η}.
impl StochasticSemiring for PowerWeight {
    fn to_probability(&self) -> f64 {
        PowerWeight::to_probability(self)
    }
}

impl std::ops::Add for PowerWeight {
    type Output = Self;

    /// Operator `+` implements semiring ⊕_η.
    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Mul for PowerWeight {
    type Output = Self;

    /// Operator `*` implements semiring ⊗.
    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::AddAssign for PowerWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::MulAssign for PowerWeight {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

impl std::fmt::Display for PowerWeight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PowerWeight({}, η={})", self.value, self.eta)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for PowerWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("PowerWeight", 2)?;
        state.serialize_field("value", &self.value.into_inner())?;
        state.serialize_field("eta", &self.eta.into_inner())?;
        state.end()
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PowerWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct PowerWeightData {
            value: f64,
            eta: f64,
        }
        let data = PowerWeightData::deserialize(deserializer)?;
        Ok(PowerWeight::new(data.value, data.eta))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::traits::tests::{
        verify_commutative_times_semiring, verify_k_closed_semiring, verify_quantizable_semiring,
        verify_stochastic_semiring, verify_totally_ordered_semiring, verify_zero_sum_free_semiring,
    };
    use proptest::prelude::*;

    #[test]
    fn test_basic_operations() {
        let eta = 2.0;
        let a = PowerWeight::new(4.0, eta);
        let b = PowerWeight::new(9.0, eta);

        // Plus: (4^{1/2} + 9^{1/2})^2 = (2 + 3)^2 = 25
        let sum = a.plus(&b);
        assert!(
            (sum.value() - 25.0).abs() < 1e-10,
            "Expected 25.0, got {}",
            sum.value()
        );

        // Times: 4 × 9 = 36
        let product = a.times(&b);
        assert!(
            (product.value() - 36.0).abs() < 1e-10,
            "Expected 36.0, got {}",
            product.value()
        );
    }

    #[test]
    fn test_eta_one_is_probability_semiring() {
        // With η = 1, the power semiring is equivalent to the probability semiring
        let a = PowerWeight::new(0.3, 1.0);
        let b = PowerWeight::new(0.5, 1.0);

        // Plus: (0.3^1 + 0.5^1)^1 = 0.8
        let sum = a.plus(&b);
        assert!(
            (sum.value() - 0.8).abs() < 1e-10,
            "Expected 0.8, got {}",
            sum.value()
        );

        // Times: 0.3 × 0.5 = 0.15
        let product = a.times(&b);
        assert!(
            (product.value() - 0.15).abs() < 1e-10,
            "Expected 0.15, got {}",
            product.value()
        );
    }

    #[test]
    fn test_identities() {
        let eta = 2.0;
        let a = PowerWeight::new(5.0, eta);
        let zero = PowerWeight::zero_with_eta(eta);
        let one = PowerWeight::one_with_eta(eta);

        // Zero is additive identity
        let sum = a.plus(&zero);
        assert!(
            (sum.value() - 5.0).abs() < 1e-10,
            "Additive identity failed"
        );

        // One is multiplicative identity
        let product = a.times(&one);
        assert!(
            (product.value() - 5.0).abs() < 1e-10,
            "Multiplicative identity failed"
        );
    }

    #[test]
    fn test_zero_annihilation() {
        let eta = 2.0;
        let a = PowerWeight::new(5.0, eta);
        let zero = PowerWeight::zero_with_eta(eta);

        // Zero annihilates
        let product = a.times(&zero);
        assert!(product.is_zero(), "Zero annihilation failed");
    }

    #[test]
    fn test_division() {
        let eta = 2.0;
        let a = PowerWeight::new(10.0, eta);
        let b = PowerWeight::new(2.0, eta);

        // (a * b) / b = a
        let product = a.times(&b);
        let quotient = product.divide(&b).expect("Division should succeed");
        assert!(
            (quotient.value() - 10.0).abs() < 1e-10,
            "Division failed: expected 10.0, got {}",
            quotient.value()
        );

        // Division by zero returns None
        let zero = PowerWeight::zero_with_eta(eta);
        assert!(a.divide(&zero).is_none(), "Division by zero should fail");
    }

    #[test]
    fn test_star() {
        let eta = 1.0;

        // x < 1: star converges
        let a = PowerWeight::new(0.5, eta);
        let star_a = a.star().expect("Star should converge for x < 1");
        // 1 / (1 - 0.5) = 2.0
        assert!(
            (star_a.value() - 2.0).abs() < 1e-10,
            "Star failed: expected 2.0, got {}",
            star_a.value()
        );

        // x = 1: star diverges
        let one = PowerWeight::new(1.0, eta);
        assert!(one.star().is_none(), "Star should diverge for x = 1");

        // x > 1: star diverges
        let big = PowerWeight::new(2.0, eta);
        assert!(big.star().is_none(), "Star should diverge for x > 1");
    }

    #[test]
    fn test_probability_conversion() {
        let eta = 3.0;
        let prob = 0.7;

        // Convert to power semiring and back
        let pw = PowerWeight::from_probability(prob, eta);
        let recovered = pw.to_probability();

        assert!(
            (recovered - prob).abs() < 1e-10,
            "Probability roundtrip failed: {} -> {} -> {}",
            prob,
            pw.value(),
            recovered
        );
    }

    #[test]
    fn test_isomorphism_preserves_plus() {
        // Verify: Ψ_η(x + y) = Ψ_η(x) ⊕_η Ψ_η(y)
        let eta = 2.0;
        let x = 0.3;
        let y = 0.5;

        // Left side: Ψ_η(x + y)
        let left = PowerWeight::from_probability(x + y, eta);

        // Right side: Ψ_η(x) ⊕_η Ψ_η(y)
        let px = PowerWeight::from_probability(x, eta);
        let py = PowerWeight::from_probability(y, eta);
        let right = px.plus(&py);

        assert!(
            (left.value() - right.value()).abs() < 1e-10,
            "Isomorphism failed for plus: {} vs {}",
            left.value(),
            right.value()
        );
    }

    #[test]
    fn test_isomorphism_preserves_times() {
        // Verify: Ψ_η(x × y) = Ψ_η(x) × Ψ_η(y)
        let eta = 2.0;
        let x = 0.3;
        let y = 0.5;

        // Left side: Ψ_η(x × y)
        let left = PowerWeight::from_probability(x * y, eta);

        // Right side: Ψ_η(x) × Ψ_η(y)
        let px = PowerWeight::from_probability(x, eta);
        let py = PowerWeight::from_probability(y, eta);
        let right = px.times(&py);

        assert!(
            (left.value() - right.value()).abs() < 1e-10,
            "Isomorphism failed for times: {} vs {}",
            left.value(),
            right.value()
        );
    }

    #[test]
    fn test_large_eta_behavior() {
        // With large η, the semiring should approach min-like behavior
        // (on the inverse/log scale)
        let eta = 100.0;
        let a = PowerWeight::new(0.1, eta);
        let b = PowerWeight::new(0.9, eta);

        // For large η, plus should be dominated by the larger value
        let sum = a.plus(&b);
        // The result should be close to (0.1^{1/100} + 0.9^{1/100})^100
        // ≈ (0.977 + 0.999)^100 ≈ very large, but dominated by the larger input's root

        // Just verify it's larger than either input (due to the sum operation)
        assert!(
            sum.value() > a.value() && sum.value() > b.value(),
            "Large η plus should produce larger value"
        );
    }

    proptest! {
        #[test]
        fn proptest_semiring_axioms(
            a in 0.001f64..100.0,
            b in 0.001f64..100.0,
            c in 0.001f64..100.0,
            eta in 0.5f64..5.0
        ) {
            // Custom verification since generic helper uses Semiring::zero()/one()
            // which have fixed η=1, incompatible with our parametrized semiring
            let wa = PowerWeight::new(a, eta);
            let wb = PowerWeight::new(b, eta);
            let wc = PowerWeight::new(c, eta);
            let zero = PowerWeight::zero_with_eta(eta);
            let one = PowerWeight::one_with_eta(eta);
            let epsilon = 1e-6;

            // Additive identity
            prop_assert!(wa.plus(&zero).approx_eq(&wa, epsilon),
                "Additive identity failed: a ⊕ 0̄ ≠ a");

            // Multiplicative identity
            prop_assert!(wa.times(&one).approx_eq(&wa, epsilon),
                "Multiplicative identity (right) failed: a ⊗ 1̄ ≠ a");
            prop_assert!(one.times(&wa).approx_eq(&wa, epsilon),
                "Multiplicative identity (left) failed: 1̄ ⊗ a ≠ a");

            // Additive commutativity
            prop_assert!(wa.plus(&wb).approx_eq(&wb.plus(&wa), epsilon),
                "Additive commutativity failed: a ⊕ b ≠ b ⊕ a");

            // Additive associativity
            let left = wa.plus(&wb).plus(&wc);
            let right = wa.plus(&wb.plus(&wc));
            prop_assert!(left.approx_eq(&right, epsilon),
                "Additive associativity failed: (a ⊕ b) ⊕ c ≠ a ⊕ (b ⊕ c)");

            // Multiplicative associativity
            let left = wa.times(&wb).times(&wc);
            let right = wa.times(&wb.times(&wc));
            prop_assert!(left.approx_eq(&right, epsilon),
                "Multiplicative associativity failed: (a ⊗ b) ⊗ c ≠ a ⊗ (b ⊗ c)");

            // Left distributivity
            let left = wa.times(&wb.plus(&wc));
            let right = wa.times(&wb).plus(&wa.times(&wc));
            prop_assert!(left.approx_eq(&right, epsilon),
                "Left distributivity failed: a ⊗ (b ⊕ c) ≠ (a ⊗ b) ⊕ (a ⊗ c)");

            // Right distributivity
            let left = wa.plus(&wb).times(&wc);
            let right = wa.times(&wc).plus(&wb.times(&wc));
            prop_assert!(left.approx_eq(&right, epsilon),
                "Right distributivity failed: (a ⊕ b) ⊗ c ≠ (a ⊗ c) ⊕ (b ⊗ c)");

            // Zero annihilation
            prop_assert!(zero.times(&wa).approx_eq(&zero, epsilon),
                "Zero annihilation (left) failed: 0̄ ⊗ a ≠ 0̄");
            prop_assert!(wa.times(&zero).approx_eq(&zero, epsilon),
                "Zero annihilation (right) failed: a ⊗ 0̄ ≠ 0̄");
        }

        #[test]
        fn proptest_divisible_semiring(
            a in 0.001f64..100.0,
            b in 0.001f64..100.0,
            eta in 0.5f64..5.0
        ) {
            let wa = PowerWeight::new(a, eta);
            let wb = PowerWeight::new(b, eta);
            let epsilon = 1e-6;

            // (a × b) / b = a
            if !wb.is_zero() {
                let product = wa.times(&wb);
                if let Some(quotient) = product.divide(&wb) {
                    prop_assert!(quotient.approx_eq(&wa, epsilon),
                        "Division inverse failed: (a ⊗ b) ÷ b ≠ a");
                }
            }
        }

        #[test]
        fn proptest_star_semiring(
            // Use probability space values bounded well below 1 to avoid
            // numerical instability when prob ≈ 1 causes huge star values
            prob in 0.01f64..0.8,
            eta in 0.5f64..5.0
        ) {
            // Create weight from probability to ensure we're in stable range
            let wa = PowerWeight::from_probability(prob, eta);
            let one = PowerWeight::one_with_eta(eta);
            // Use relative tolerance for large values
            let star_a = match wa.star() {
                Some(s) => s,
                None => return Ok(()),  // Skip if star doesn't converge
            };

            // a* should satisfy: a* = 1 ⊕ (a ⊗ a*)
            let expected = one.plus(&wa.times(&star_a));

            // Use relative error check for numerical stability
            let rel_error = if expected.value() > 1e-10 {
                (star_a.value() - expected.value()).abs() / expected.value()
            } else {
                (star_a.value() - expected.value()).abs()
            };

            prop_assert!(rel_error < 1e-6,
                "Star axiom failed: a* ≠ 1̄ ⊕ (a ⊗ a*), rel_error = {}", rel_error);
        }

        #[test]
        fn proptest_probability_roundtrip(
            prob in 0.001f64..1.0,
            eta in 0.5f64..5.0
        ) {
            let pw = PowerWeight::from_probability(prob, eta);
            let recovered = pw.to_probability();
            prop_assert!((recovered - prob).abs() < 1e-10,
                "Roundtrip failed: {} -> {} -> {}", prob, pw.value(), recovered);
        }

        #[test]
        fn proptest_k_closed_semiring(
            a in 0.001f64..100.0,
            eta in 0.5f64..5.0
        ) {
            let wa = PowerWeight::new(a, eta);
            verify_k_closed_semiring(wa, 1e-6);
        }

        #[test]
        fn proptest_zero_sum_free_semiring(
            a in 0.0f64..100.0,
            b in 0.0f64..100.0,
            eta in 0.5f64..5.0
        ) {
            let wa = PowerWeight::new(a, eta);
            let wb = PowerWeight::new(b, eta);
            verify_zero_sum_free_semiring(wa, wb, 1e-6);
        }

        #[test]
        fn proptest_commutative_times_semiring(
            a in 0.001f64..100.0,
            b in 0.001f64..100.0,
            eta in 0.5f64..5.0
        ) {
            let wa = PowerWeight::new(a, eta);
            let wb = PowerWeight::new(b, eta);
            verify_commutative_times_semiring(wa, wb, 1e-6);
        }

        #[test]
        fn proptest_totally_ordered_semiring(
            a in 0.001f64..100.0,
            b in 0.001f64..100.0,
            c in 0.001f64..100.0,
            eta in 0.5f64..5.0
        ) {
            let wa = PowerWeight::new(a, eta);
            let wb = PowerWeight::new(b, eta);
            let wc = PowerWeight::new(c, eta);
            verify_totally_ordered_semiring(wa, wb, wc);
        }

        #[test]
        fn proptest_quantizable_semiring(
            a in 0.001f64..100.0,
            eta in 0.5f64..5.0
        ) {
            let wa = PowerWeight::new(a, eta);
            verify_quantizable_semiring(wa, 1e-10);
        }

        #[test]
        fn proptest_stochastic_semiring(
            prob in 0.001f64..1.0,
            eta in 0.5f64..5.0
        ) {
            let wa = PowerWeight::from_probability(prob, eta);
            verify_stochastic_semiring(wa);
        }
    }

    #[test]
    fn test_k_closed_bound() {
        // PowerWeight has no uniform closure bound
        assert_eq!(PowerWeight::closure_bound(), None);
    }
}
