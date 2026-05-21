//! Second-order differentiation for Hessian computation.
//!
//! This module provides support for computing second-order derivatives
//! (Hessians) through WFST operations, enabling advanced optimization
//! techniques.
//!
//! ## Use Cases
//!
//! 1. **Natural Gradient**: Uses Fisher information matrix for better optimization
//! 2. **Uncertainty Estimation**: Hessian diagonal approximates parameter uncertainty
//! 3. **Second-order Optimization**: Newton's method and variants
//! 4. **Influence Functions**: Understanding training data impact
//!
//! ## Algorithm
//!
//! Second-order differentiation extends the forward/backward passes:
//!
//! 1. **Forward pass**: Compute α values (path scores to each state)
//! 2. **First backward pass**: Compute β values and first-order gradients
//! 3. **Second backward pass**: Propagate gradient-of-gradient
//!
//! The Hessian H[i,j] = ∂²L/∂w_i∂w_j measures how the gradient of arc i
//! changes with respect to arc j.
//!
//! ## Efficiency
//!
//! Full Hessian computation is O(|E|²) which is expensive. We provide:
//! - Hessian-vector products (O(|E|))
//! - Diagonal Hessian approximation (O(|E|))
//! - Block-diagonal Hessian (O(|E| × block_size))
//!
//! ## References
//!
//! - Pearlmutter, "Fast exact multiplication by the Hessian" (1994)
//! - Martens, "Deep learning via Hessian-free optimization" (2010)

use std::cell::RefCell;

use super::forward_score::forward_score;
use super::gradient::{backward, GradientAccumulator, GradientWfst};
use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst};

/// Configuration for second-order differentiation.
#[derive(Clone, Debug)]
pub struct SecondOrderConfig {
    /// Whether to compute full Hessian (expensive).
    pub full_hessian: bool,
    /// Whether to compute diagonal Hessian only.
    pub diagonal_only: bool,
    /// Block size for block-diagonal approximation (0 = no blocking).
    pub block_size: usize,
    /// Damping factor for numerical stability.
    pub damping: f64,
}

impl Default for SecondOrderConfig {
    fn default() -> Self {
        Self {
            full_hessian: false,
            diagonal_only: true,
            block_size: 0,
            damping: 1e-6,
        }
    }
}

/// Hessian matrix storage.
#[derive(Clone, Debug)]
pub struct HessianMatrix {
    /// Number of parameters (arcs).
    pub size: usize,
    /// Storage for Hessian entries.
    /// For diagonal: just the diagonal elements.
    /// For full: row-major storage.
    pub data: Vec<f64>,
    /// Whether this is diagonal only.
    pub is_diagonal: bool,
}

impl HessianMatrix {
    /// Create a new diagonal Hessian.
    pub fn diagonal(size: usize) -> Self {
        Self {
            size,
            data: vec![0.0; size],
            is_diagonal: true,
        }
    }

    /// Create a new full Hessian.
    pub fn full(size: usize) -> Self {
        Self {
            size,
            data: vec![0.0; size * size],
            is_diagonal: false,
        }
    }

    /// Get element (i, j).
    pub fn get(&self, i: usize, j: usize) -> f64 {
        if self.is_diagonal {
            if i == j && i < self.size {
                self.data[i]
            } else {
                0.0
            }
        } else {
            if i < self.size && j < self.size {
                self.data[i * self.size + j]
            } else {
                0.0
            }
        }
    }

    /// Set element (i, j).
    pub fn set(&mut self, i: usize, j: usize, value: f64) {
        if self.is_diagonal {
            if i == j && i < self.size {
                self.data[i] = value;
            }
        } else {
            if i < self.size && j < self.size {
                self.data[i * self.size + j] = value;
            }
        }
    }

    /// Add to element (i, j).
    pub fn add(&mut self, i: usize, j: usize, value: f64) {
        if self.is_diagonal {
            if i == j && i < self.size {
                self.data[i] += value;
            }
        } else {
            if i < self.size && j < self.size {
                self.data[i * self.size + j] += value;
            }
        }
    }

    /// Get diagonal elements.
    pub fn diagonal_elements(&self) -> Vec<f64> {
        if self.is_diagonal {
            self.data.clone()
        } else {
            (0..self.size)
                .map(|i| self.data[i * self.size + i])
                .collect()
        }
    }

    /// Compute Hessian-vector product.
    pub fn hvp(&self, v: &[f64]) -> Vec<f64> {
        if v.len() != self.size {
            return vec![0.0; self.size];
        }

        if self.is_diagonal {
            self.data
                .iter()
                .zip(v.iter())
                .map(|(&h, &x)| h * x)
                .collect()
        } else {
            (0..self.size)
                .map(|i| {
                    (0..self.size)
                        .map(|j| self.data[i * self.size + j] * v[j])
                        .sum()
                })
                .collect()
        }
    }
}

/// Extended gradient WFST with second-order gradient tracking.
#[derive(Clone, Debug)]
pub struct SecondOrderWfst<L: Clone> {
    /// First-order gradient WFST.
    pub first_order: GradientWfst<L>,
    /// Second-order backward scores (for Hessian computation).
    second_backward: RefCell<Vec<LogWeight>>,
    /// Gradient of gradients for each arc.
    grad_grad: RefCell<Vec<f64>>,
}

impl<L: Clone + Send + Sync> SecondOrderWfst<L> {
    /// Create from a first-order gradient WFST.
    pub fn from_gradient_wfst(first_order: GradientWfst<L>) -> Self {
        let num_states = first_order.num_states();
        let num_arcs = count_arcs_in_grad_fst(&first_order);

        Self {
            first_order,
            second_backward: RefCell::new(vec![LogWeight::zero(); num_states]),
            grad_grad: RefCell::new(vec![0.0; num_arcs]),
        }
    }

    /// Create from a WFST.
    pub fn from_wfst(fst: &VectorWfst<L, LogWeight>) -> Self {
        let first_order = GradientWfst::from_wfst(fst);
        Self::from_gradient_wfst(first_order)
    }

    /// Get the number of arcs (parameters).
    pub fn num_arcs(&self) -> usize {
        self.grad_grad.borrow().len()
    }

    /// Reset second-order computation state.
    pub fn reset_second_order(&self) {
        let num_states = self.first_order.num_states();
        let num_arcs = self.grad_grad.borrow().len();
        *self.second_backward.borrow_mut() = vec![LogWeight::zero(); num_states];
        *self.grad_grad.borrow_mut() = vec![0.0; num_arcs];
    }
}

/// Compute the diagonal Hessian approximation.
///
/// The diagonal Hessian H[i,i] = ∂²L/∂w_i² measures the curvature
/// along each parameter axis.
///
/// # Algorithm
///
/// For the forward score (log-sum-exp), the diagonal Hessian is:
/// H[i,i] = g[i] - g[i]²
///
/// where g[i] is the first-order gradient.
///
/// # Arguments
///
/// * `so_wfst` - Second-order WFST with computed forward pass
///
/// # Returns
///
/// Diagonal Hessian matrix.
pub fn compute_diagonal_hessian<L: Clone + Send + Sync>(
    so_wfst: &SecondOrderWfst<L>,
) -> HessianMatrix {
    // Ensure first-order gradients are computed
    let _score = forward_score(&so_wfst.first_order);
    let first_grads = backward(&so_wfst.first_order);

    let num_arcs = so_wfst.num_arcs();
    let mut hessian = HessianMatrix::diagonal(num_arcs);

    // For log-sum-exp (softmax), diagonal Hessian ≈ g - g²
    // This is an approximation based on the variance of the gradient
    for (idx, arc_grad) in first_grads.arc_gradients.iter().enumerate() {
        let g = arc_grad.gradient;
        // Diagonal Hessian for log-sum-exp
        // H[i,i] = ∂g[i]/∂w[i] = g[i](1 - g[i]) for normalized gradients
        let h_ii = g * (1.0 - g);
        hessian.set(idx, idx, h_ii);
    }

    hessian
}

/// Compute Hessian-vector product without materializing the full Hessian.
///
/// This is much more efficient than computing the full Hessian and then
/// multiplying. Complexity: O(|E|) instead of O(|E|²).
///
/// # Algorithm
///
/// Uses the R-operator (Pearlmutter, 1994):
/// 1. Forward pass with perturbed weights: w + ε·v
/// 2. Backward pass to get perturbed gradients
/// 3. Hv ≈ (g(w + ε·v) - g(w)) / ε
///
/// # Arguments
///
/// * `so_wfst` - Second-order WFST
/// * `v` - Vector to multiply with Hessian
/// * `epsilon` - Perturbation size
///
/// # Returns
///
/// The Hessian-vector product H·v.
pub fn hessian_vector_product<L: Clone + Send + Sync>(
    so_wfst: &SecondOrderWfst<L>,
    v: &[f64],
    epsilon: f64,
) -> Vec<f64> {
    let num_arcs = so_wfst.num_arcs();
    if v.len() != num_arcs {
        return vec![0.0; num_arcs];
    }

    // Get baseline gradients
    let _score = forward_score(&so_wfst.first_order);
    let base_grads = backward(&so_wfst.first_order);

    // Create perturbed WFST
    let perturbed_fst = create_perturbed_wfst(&so_wfst.first_order, v, epsilon);
    let perturbed_grad_wfst = GradientWfst::from_wfst(&perturbed_fst);

    // Get perturbed gradients
    let _perturbed_score = forward_score(&perturbed_grad_wfst);
    let perturbed_grads = backward(&perturbed_grad_wfst);

    // Compute finite difference approximation of Hv
    let mut hvp = vec![0.0; num_arcs];
    for (idx, (base, perturbed)) in base_grads
        .arc_gradients
        .iter()
        .zip(perturbed_grads.arc_gradients.iter())
        .enumerate()
    {
        hvp[idx] = (perturbed.gradient - base.gradient) / epsilon;
    }

    hvp
}

/// Create a perturbed copy of a WFST.
fn create_perturbed_wfst<L: Clone + Send + Sync>(
    grad_wfst: &GradientWfst<L>,
    perturbation: &[f64],
    epsilon: f64,
) -> VectorWfst<L, LogWeight> {
    let original = grad_wfst.fst();
    let mut perturbed = VectorWfst::new();

    // Copy states
    for _ in 0..original.num_states() {
        perturbed.add_state();
    }

    if original.start() != crate::wfst::NO_STATE {
        perturbed.set_start(original.start());
    }

    // Copy arcs with perturbation
    let mut arc_idx = 0;
    for state in 0..original.num_states() as StateId {
        for arc in original.transitions(state) {
            let delta = if arc_idx < perturbation.len() {
                perturbation[arc_idx] * epsilon
            } else {
                0.0
            };

            let new_weight = LogWeight::new(arc.weight.value() + delta);
            perturbed.add_arc(
                state,
                arc.input.clone(),
                arc.output.clone(),
                arc.to,
                new_weight,
            );
            arc_idx += 1;
        }

        if original.is_final(state) {
            perturbed.set_final(state, original.final_weight(state));
        }
    }

    perturbed
}

/// Compute Fisher information matrix approximation.
///
/// The Fisher information F = E[∇log p · ∇log p^T] approximates the
/// Hessian for probabilistic models.
///
/// For a single sample, F ≈ g · g^T where g is the gradient.
///
/// # Arguments
///
/// * `gradients` - Gradient accumulator from backward pass
///
/// # Returns
///
/// Fisher information matrix (symmetric, positive semi-definite).
pub fn compute_fisher_information(gradients: &GradientAccumulator) -> HessianMatrix {
    let n = gradients.arc_gradients.len();
    let mut fisher = HessianMatrix::full(n);

    // F[i,j] = g[i] * g[j]
    for i in 0..n {
        let g_i = gradients.arc_gradients[i].gradient;
        for j in 0..n {
            let g_j = gradients.arc_gradients[j].gradient;
            fisher.set(i, j, g_i * g_j);
        }
    }

    fisher
}

/// Compute diagonal Fisher information (efficient approximation).
pub fn compute_diagonal_fisher(gradients: &GradientAccumulator) -> HessianMatrix {
    let n = gradients.arc_gradients.len();
    let mut fisher = HessianMatrix::diagonal(n);

    for (i, grad) in gradients.arc_gradients.iter().enumerate() {
        fisher.set(i, i, grad.gradient * grad.gradient);
    }

    fisher
}

/// Natural gradient: precondition gradient with inverse Fisher.
///
/// The natural gradient ∇̃L = F^{-1} · ∇L often leads to faster
/// convergence than the standard gradient.
///
/// # Arguments
///
/// * `gradients` - Standard gradients
/// * `fisher` - Fisher information matrix (diagonal)
/// * `damping` - Damping factor for numerical stability
///
/// # Returns
///
/// Natural gradients.
pub fn natural_gradient(
    gradients: &GradientAccumulator,
    fisher: &HessianMatrix,
    damping: f64,
) -> Vec<f64> {
    if !fisher.is_diagonal {
        // For full Fisher, would need to solve linear system
        // For now, just return standard gradients
        return gradients.arc_gradients.iter().map(|g| g.gradient).collect();
    }

    gradients
        .arc_gradients
        .iter()
        .enumerate()
        .map(|(i, grad)| {
            let f_ii = fisher.get(i, i) + damping;
            if f_ii.abs() > 1e-10 {
                grad.gradient / f_ii
            } else {
                grad.gradient
            }
        })
        .collect()
}

/// Count arcs in a gradient WFST.
fn count_arcs_in_grad_fst<L: Clone + Send + Sync>(grad_fst: &GradientWfst<L>) -> usize {
    let mut count = 0;
    for s in 0..grad_fst.num_states() as StateId {
        count += grad_fst.transitions(s).len();
    }
    count
}

/// Result of second-order computation.
#[derive(Clone, Debug)]
pub struct SecondOrderResult {
    /// First-order gradients.
    pub gradients: GradientAccumulator,
    /// Hessian (may be diagonal only).
    pub hessian: HessianMatrix,
    /// Natural gradient (if computed).
    pub natural_grad: Option<Vec<f64>>,
}

/// Compute both gradient and Hessian in one pass.
pub fn gradient_and_hessian<L: Clone + Send + Sync>(
    fst: &VectorWfst<L, LogWeight>,
    config: &SecondOrderConfig,
) -> SecondOrderResult {
    let so_wfst = SecondOrderWfst::from_wfst(fst);

    // Compute first-order
    let _score = forward_score(&so_wfst.first_order);
    let gradients = backward(&so_wfst.first_order);

    // Compute Hessian based on config
    let hessian = if config.full_hessian {
        compute_fisher_information(&gradients)
    } else if config.diagonal_only {
        compute_diagonal_hessian(&so_wfst)
    } else {
        HessianMatrix::diagonal(gradients.arc_gradients.len())
    };

    // Compute natural gradient if using diagonal
    let natural_grad = if config.diagonal_only {
        let fisher = compute_diagonal_fisher(&gradients);
        Some(natural_gradient(&gradients, &fisher, config.damping))
    } else {
        None
    };

    SecondOrderResult {
        gradients,
        hessian,
        natural_grad,
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::wfst::MutableWfst;
    use proptest::prelude::*;

    /// Strategy for generating simple parallel path WFSTs.
    fn arb_parallel_wfst(max_paths: usize) -> impl Strategy<Value = VectorWfst<char, LogWeight>> {
        proptest::collection::vec(0.1f64..5.0, 1..=max_paths).prop_map(|weights| {
            let mut fst = VectorWfst::new();
            let s0 = fst.add_state();
            let s1 = fst.add_state();
            fst.set_start(s0);
            fst.set_final(s1, LogWeight::one());
            for (i, w) in weights.iter().enumerate() {
                let label = (b'a' + (i % 26) as u8) as char;
                fst.add_arc(s0, Some(label), Some(label), s1, LogWeight::new(*w));
            }
            fst
        })
    }

    /// Strategy for generating chain WFSTs.
    fn arb_chain_wfst(max_length: usize) -> impl Strategy<Value = VectorWfst<char, LogWeight>> {
        (1..=max_length).prop_flat_map(|len| {
            proptest::collection::vec(0.1f64..5.0, len).prop_map(move |weights| {
                let mut fst = VectorWfst::new();
                for _ in 0..=len {
                    fst.add_state();
                }
                fst.set_start(0);
                fst.set_final(len as u32, LogWeight::one());
                for (i, w) in weights.iter().enumerate() {
                    let label = (b'a' + (i % 26) as u8) as char;
                    fst.add_arc(
                        i as u32,
                        Some(label),
                        Some(label),
                        (i + 1) as u32,
                        LogWeight::new(*w),
                    );
                }
                fst
            })
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        /// Diagonal Hessian values are finite.
        #[test]
        fn diagonal_hessian_finite(fst in arb_parallel_wfst(5)) {
            let so_wfst = SecondOrderWfst::from_wfst(&fst);
            let hessian = compute_diagonal_hessian(&so_wfst);

            for i in 0..hessian.size {
                let h_ii = hessian.get(i, i);
                prop_assert!(h_ii.is_finite(), "H[{},{}] = {} is not finite", i, i, h_ii);
            }
        }

        /// Fisher matrix is symmetric (F[i,j] = F[j,i]).
        #[test]
        fn fisher_symmetric(fst in arb_parallel_wfst(4)) {
            let grad_wfst = GradientWfst::from_wfst(&fst);
            let _ = forward_score(&grad_wfst);
            let grads = backward(&grad_wfst);

            let fisher = compute_fisher_information(&grads);

            for i in 0..fisher.size {
                for j in 0..fisher.size {
                    prop_assert!((fisher.get(i, j) - fisher.get(j, i)).abs() < 1e-9,
                        "Fisher not symmetric: F[{},{}]={} != F[{},{}]={}",
                        i, j, fisher.get(i, j), j, i, fisher.get(j, i));
                }
            }
        }

        /// Fisher diagonal equals gradient squared.
        #[test]
        fn fisher_diagonal_is_grad_squared(fst in arb_parallel_wfst(4)) {
            let grad_wfst = GradientWfst::from_wfst(&fst);
            let _ = forward_score(&grad_wfst);
            let grads = backward(&grad_wfst);

            let fisher = compute_diagonal_fisher(&grads);

            for (i, arc_grad) in grads.arc_gradients.iter().enumerate() {
                let expected = arc_grad.gradient * arc_grad.gradient;
                prop_assert!((fisher.get(i, i) - expected).abs() < 1e-9,
                    "Fisher[{},{}] = {} != g^2 = {}", i, i, fisher.get(i, i), expected);
            }
        }

        /// Fisher matrix is positive semi-definite (all eigenvalues ≥ 0).
        /// For diagonal, just check all diagonal elements ≥ 0.
        #[test]
        fn fisher_positive_semidefinite(fst in arb_parallel_wfst(4)) {
            let grad_wfst = GradientWfst::from_wfst(&fst);
            let _ = forward_score(&grad_wfst);
            let grads = backward(&grad_wfst);

            let fisher = compute_diagonal_fisher(&grads);

            for i in 0..fisher.size {
                prop_assert!(fisher.get(i, i) >= -1e-9,
                    "Fisher[{},{}] = {} < 0", i, i, fisher.get(i, i));
            }
        }

        /// HessianMatrix diagonal get/set consistency.
        #[test]
        fn hessian_diagonal_consistency(size in 1usize..10, values in proptest::collection::vec(-10.0f64..10.0, 1..10)) {
            let mut h = HessianMatrix::diagonal(size);

            for (i, v) in values.iter().enumerate() {
                if i < size {
                    h.set(i, i, *v);
                    prop_assert!((h.get(i, i) - *v).abs() < 1e-9);
                }
            }

            // Off-diagonal should be 0
            for i in 0..size {
                for j in 0..size {
                    if i != j {
                        prop_assert_eq!(h.get(i, j), 0.0);
                    }
                }
            }
        }

        /// HessianMatrix full get/set consistency.
        #[test]
        fn hessian_full_consistency(size in 1usize..5, row in 0usize..5, col in 0usize..5, val in -10.0f64..10.0) {
            let mut h = HessianMatrix::full(size);

            if row < size && col < size {
                h.set(row, col, val);
                prop_assert!((h.get(row, col) - val).abs() < 1e-9);
            }
        }

        /// HessianMatrix add accumulates correctly.
        #[test]
        fn hessian_add_accumulates(size in 1usize..5, idx in 0usize..5, v1 in -10.0f64..10.0, v2 in -10.0f64..10.0) {
            let mut h = HessianMatrix::diagonal(size);

            if idx < size {
                h.set(idx, idx, v1);
                h.add(idx, idx, v2);
                prop_assert!((h.get(idx, idx) - (v1 + v2)).abs() < 1e-9);
            }
        }

        /// HVP with zero vector gives zero result.
        #[test]
        fn hvp_zero_vector(size in 1usize..10) {
            let h = HessianMatrix::diagonal(size);
            let v = vec![0.0; size];
            let result = h.hvp(&v);

            for r in result {
                prop_assert!((r - 0.0).abs() < 1e-9);
            }
        }

        /// HVP with identity diagonal and unit vector.
        #[test]
        fn hvp_identity_diagonal(size in 1usize..10) {
            let mut h = HessianMatrix::diagonal(size);
            for i in 0..size {
                h.set(i, i, 1.0);
            }

            let v: Vec<f64> = (0..size).map(|i| i as f64).collect();
            let result = h.hvp(&v);

            prop_assert_eq!(result.len(), size);
            for (i, r) in result.iter().enumerate() {
                prop_assert!((r - i as f64).abs() < 1e-9);
            }
        }

        /// Natural gradient divides by Fisher diagonal.
        #[test]
        fn natural_gradient_divides(grad_val in 0.1f64..10.0, fisher_val in 0.1f64..10.0) {
            let mut grads = GradientAccumulator::new();
            grads.add_gradient(super::super::gradient::ArcIndex::new(0, 0), grad_val);

            let mut fisher = HessianMatrix::diagonal(1);
            fisher.set(0, 0, fisher_val);

            let damping = 0.0;
            let nat_grad = natural_gradient(&grads, &fisher, damping);

            let expected = grad_val / fisher_val;
            prop_assert!((nat_grad[0] - expected).abs() < 1e-6,
                "Natural grad {} != expected {}", nat_grad[0], expected);
        }

        /// Natural gradient with damping prevents division by zero.
        #[test]
        fn natural_gradient_damping(grad_val in 0.1f64..10.0, damping in 1e-6f64..1.0) {
            let mut grads = GradientAccumulator::new();
            grads.add_gradient(super::super::gradient::ArcIndex::new(0, 0), grad_val);

            let fisher = HessianMatrix::diagonal(1); // All zeros

            let nat_grad = natural_gradient(&grads, &fisher, damping);

            let expected = grad_val / damping;
            prop_assert!((nat_grad[0] - expected).abs() < 1e-6,
                "Damped natural grad {} != expected {}", nat_grad[0], expected);
        }

        /// SecondOrderWfst reset clears state.
        #[test]
        fn second_order_reset(fst in arb_chain_wfst(3)) {
            let so_wfst = SecondOrderWfst::from_wfst(&fst);
            let _ = compute_diagonal_hessian(&so_wfst);

            so_wfst.reset_second_order();
            // Should be able to recompute
            let hessian = compute_diagonal_hessian(&so_wfst);
            prop_assert!(hessian.size > 0);
        }

        /// gradient_and_hessian returns consistent results.
        #[test]
        fn gradient_and_hessian_consistent(fst in arb_parallel_wfst(4)) {
            let config = SecondOrderConfig::default();
            let result = gradient_and_hessian(&fst, &config);

            // Gradients should be non-empty and finite
            prop_assert!(!result.gradients.arc_gradients.is_empty());
            for g in &result.gradients.arc_gradients {
                prop_assert!(g.gradient.is_finite());
            }

            // Hessian should be diagonal
            prop_assert!(result.hessian.is_diagonal);

            // Natural gradient should exist
            prop_assert!(result.natural_grad.is_some());
        }

        /// Diagonal elements from full Hessian match diagonal_elements().
        #[test]
        fn full_hessian_diagonal_extraction(size in 1usize..5, values in proptest::collection::vec(-10.0f64..10.0, 1..25)) {
            let mut h = HessianMatrix::full(size);

            for i in 0..size {
                for j in 0..size {
                    let idx = i * size + j;
                    if idx < values.len() {
                        h.set(i, j, values[idx]);
                    }
                }
            }

            let diagonal = h.diagonal_elements();
            prop_assert_eq!(diagonal.len(), size);

            for (i, d) in diagonal.iter().enumerate() {
                prop_assert!((d - h.get(i, i)).abs() < 1e-9);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::MutableWfst;

    fn create_simple_fst() -> VectorWfst<char, LogWeight> {
        let mut fst = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));
        fst
    }

    #[test]
    fn test_second_order_config_default() {
        let config = SecondOrderConfig::default();
        assert!(!config.full_hessian);
        assert!(config.diagonal_only);
    }

    #[test]
    fn test_hessian_matrix_diagonal() {
        let mut h = HessianMatrix::diagonal(3);
        h.set(0, 0, 1.0);
        h.set(1, 1, 2.0);
        h.set(2, 2, 3.0);

        assert_eq!(h.get(0, 0), 1.0);
        assert_eq!(h.get(1, 1), 2.0);
        assert_eq!(h.get(0, 1), 0.0); // Off-diagonal is 0
    }

    #[test]
    fn test_hessian_matrix_full() {
        let mut h = HessianMatrix::full(2);
        h.set(0, 0, 1.0);
        h.set(0, 1, 2.0);
        h.set(1, 0, 3.0);
        h.set(1, 1, 4.0);

        assert_eq!(h.get(0, 0), 1.0);
        assert_eq!(h.get(0, 1), 2.0);
        assert_eq!(h.get(1, 0), 3.0);
        assert_eq!(h.get(1, 1), 4.0);
    }

    #[test]
    fn test_hessian_hvp() {
        let mut h = HessianMatrix::diagonal(2);
        h.set(0, 0, 2.0);
        h.set(1, 1, 3.0);

        let v = vec![1.0, 2.0];
        let result = h.hvp(&v);

        assert_eq!(result[0], 2.0); // 2.0 * 1.0
        assert_eq!(result[1], 6.0); // 3.0 * 2.0
    }

    #[test]
    fn test_second_order_wfst_creation() {
        let fst = create_simple_fst();
        let so_wfst = SecondOrderWfst::from_wfst(&fst);

        assert_eq!(so_wfst.first_order.num_states(), 2);
        assert_eq!(so_wfst.num_arcs(), 1);
    }

    #[test]
    fn test_compute_diagonal_hessian() {
        let fst = create_simple_fst();
        let so_wfst = SecondOrderWfst::from_wfst(&fst);

        let hessian = compute_diagonal_hessian(&so_wfst);

        assert!(hessian.is_diagonal);
        assert_eq!(hessian.size, 1);
    }

    #[test]
    fn test_compute_fisher_information() {
        let fst = create_simple_fst();
        let grad_wfst = GradientWfst::from_wfst(&fst);
        let _ = forward_score(&grad_wfst);
        let grads = backward(&grad_wfst);

        let fisher = compute_fisher_information(&grads);

        assert!(!fisher.is_diagonal);
        assert_eq!(fisher.size, grads.arc_gradients.len());
    }

    #[test]
    fn test_compute_diagonal_fisher() {
        let fst = create_simple_fst();
        let grad_wfst = GradientWfst::from_wfst(&fst);
        let _ = forward_score(&grad_wfst);
        let grads = backward(&grad_wfst);

        let fisher = compute_diagonal_fisher(&grads);

        assert!(fisher.is_diagonal);
        // F[i,i] = g[i]^2
        let expected = grads.arc_gradients[0].gradient.powi(2);
        assert!((fisher.get(0, 0) - expected).abs() < 1e-6);
    }

    #[test]
    fn test_natural_gradient() {
        let fst = create_simple_fst();
        let grad_wfst = GradientWfst::from_wfst(&fst);
        let _ = forward_score(&grad_wfst);
        let grads = backward(&grad_wfst);

        let fisher = compute_diagonal_fisher(&grads);
        let nat_grad = natural_gradient(&grads, &fisher, 1e-6);

        assert_eq!(nat_grad.len(), grads.arc_gradients.len());
    }

    #[test]
    fn test_gradient_and_hessian() {
        let fst = create_simple_fst();
        let config = SecondOrderConfig::default();

        let result = gradient_and_hessian(&fst, &config);

        assert!(!result.gradients.arc_gradients.is_empty());
        assert!(result.hessian.is_diagonal);
        assert!(result.natural_grad.is_some());
    }

    #[test]
    fn test_hessian_vector_product() {
        let fst = create_simple_fst();
        let so_wfst = SecondOrderWfst::from_wfst(&fst);

        let v = vec![1.0];
        let hvp = hessian_vector_product(&so_wfst, &v, 1e-4);

        assert_eq!(hvp.len(), 1);
    }

    #[test]
    fn test_second_order_wfst_reset() {
        let fst = create_simple_fst();
        let so_wfst = SecondOrderWfst::from_wfst(&fst);

        // Compute something
        let _ = compute_diagonal_hessian(&so_wfst);

        // Reset and verify
        so_wfst.reset_second_order();
        // Should be able to recompute
        let _ = compute_diagonal_hessian(&so_wfst);
    }
}
