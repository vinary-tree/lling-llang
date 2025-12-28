//! Gradient data structures and backward pass implementation.
//!
//! This module provides the core infrastructure for automatic differentiation
//! through WFST operations, including gradient storage and backward propagation.

use std::cell::RefCell;
use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{StateId, VectorWfst, Wfst, WeightedTransition};

/// Index identifying an arc in a WFST.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArcIndex {
    /// Source state of the arc.
    pub from: StateId,
    /// Index of the arc in the source state's transition list.
    pub arc_idx: usize,
}

impl ArcIndex {
    /// Create a new arc index.
    pub fn new(from: StateId, arc_idx: usize) -> Self {
        Self { from, arc_idx }
    }
}

/// Gradient associated with a single arc.
#[derive(Clone, Debug)]
pub struct ArcGradient {
    /// Arc identifier.
    pub arc: ArcIndex,
    /// Gradient value (∂loss/∂arc_weight).
    pub gradient: f64,
}

/// Accumulated gradients for all arcs in a WFST.
#[derive(Clone, Debug)]
pub struct GradientAccumulator {
    /// Gradients indexed by arc.
    pub arc_gradients: Vec<ArcGradient>,
    /// Total number of arcs.
    pub num_arcs: usize,
}

impl GradientAccumulator {
    /// Create a new gradient accumulator.
    pub fn new() -> Self {
        Self {
            arc_gradients: Vec::new(),
            num_arcs: 0,
        }
    }

    /// Create with expected capacity.
    pub fn with_capacity(num_arcs: usize) -> Self {
        Self {
            arc_gradients: Vec::with_capacity(num_arcs),
            num_arcs,
        }
    }

    /// Add a gradient for an arc.
    pub fn add_gradient(&mut self, arc: ArcIndex, gradient: f64) {
        self.arc_gradients.push(ArcGradient { arc, gradient });
    }

    /// Get gradient for a specific arc, or 0 if not found.
    pub fn get_gradient(&self, arc: ArcIndex) -> f64 {
        self.arc_gradients
            .iter()
            .find(|g| g.arc == arc)
            .map(|g| g.gradient)
            .unwrap_or(0.0)
    }

    /// Merge another accumulator into this one (sum gradients).
    pub fn merge(&mut self, other: &GradientAccumulator) {
        for grad in &other.arc_gradients {
            self.add_gradient(grad.arc, grad.gradient);
        }
    }
}

impl Default for GradientAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// A WFST with gradient tracking for automatic differentiation.
///
/// This wraps a WFST and maintains the state necessary for computing
/// gradients through forward and backward passes.
#[derive(Clone, Debug)]
pub struct GradientWfst<L: Clone> {
    /// The underlying WFST (LogWeight for differentiable operations).
    fst: VectorWfst<L, LogWeight>,

    /// Forward scores for each state (α values).
    /// α[s] = total weight of all paths from start to s.
    forward_scores: RefCell<Vec<LogWeight>>,

    /// Backward scores for each state (β values).
    /// β[s] = total weight of all paths from s to final states.
    backward_scores: RefCell<Vec<LogWeight>>,

    /// Whether forward pass has been computed.
    forward_computed: RefCell<bool>,

    /// Whether backward pass has been computed.
    backward_computed: RefCell<bool>,

    /// Cached total score (for gradient computation).
    total_score: RefCell<Option<LogWeight>>,
}

impl<L: Clone + Send + Sync> GradientWfst<L> {
    /// Create a GradientWfst from an existing WFST with LogWeight.
    pub fn from_wfst(fst: &VectorWfst<L, LogWeight>) -> Self {
        let num_states = fst.num_states();
        Self {
            fst: fst.clone(),
            forward_scores: RefCell::new(vec![LogWeight::zero(); num_states]),
            backward_scores: RefCell::new(vec![LogWeight::zero(); num_states]),
            forward_computed: RefCell::new(false),
            backward_computed: RefCell::new(false),
            total_score: RefCell::new(None),
        }
    }

    /// Get a reference to the underlying WFST.
    pub fn fst(&self) -> &VectorWfst<L, LogWeight> {
        &self.fst
    }

    /// Get the number of states.
    pub fn num_states(&self) -> usize {
        self.fst.num_states()
    }

    /// Get the start state.
    pub fn start(&self) -> StateId {
        self.fst.start()
    }

    /// Check if a state is final.
    pub fn is_final(&self, state: StateId) -> bool {
        self.fst.is_final(state)
    }

    /// Get the final weight.
    pub fn final_weight(&self, state: StateId) -> LogWeight {
        self.fst.final_weight(state)
    }

    /// Get transitions from a state.
    pub fn transitions(&self, state: StateId) -> &[WeightedTransition<L, LogWeight>] {
        self.fst.transitions(state)
    }

    /// Get the forward score for a state.
    pub fn forward_score(&self, state: StateId) -> LogWeight {
        self.forward_scores.borrow()[state as usize]
    }

    /// Set the forward score for a state.
    pub fn set_forward_score(&self, state: StateId, score: LogWeight) {
        self.forward_scores.borrow_mut()[state as usize] = score;
    }

    /// Get the backward score for a state.
    pub fn backward_score(&self, state: StateId) -> LogWeight {
        self.backward_scores.borrow()[state as usize]
    }

    /// Set the backward score for a state.
    pub fn set_backward_score(&self, state: StateId, score: LogWeight) {
        self.backward_scores.borrow_mut()[state as usize] = score;
    }

    /// Check if forward pass is computed.
    pub fn is_forward_computed(&self) -> bool {
        *self.forward_computed.borrow()
    }

    /// Mark forward pass as computed.
    pub fn set_forward_computed(&self, computed: bool) {
        *self.forward_computed.borrow_mut() = computed;
    }

    /// Check if backward pass is computed.
    pub fn is_backward_computed(&self) -> bool {
        *self.backward_computed.borrow()
    }

    /// Mark backward pass as computed.
    pub fn set_backward_computed(&self, computed: bool) {
        *self.backward_computed.borrow_mut() = computed;
    }

    /// Get the cached total score.
    pub fn total_score(&self) -> Option<LogWeight> {
        *self.total_score.borrow()
    }

    /// Set the cached total score.
    pub fn set_total_score(&self, score: LogWeight) {
        *self.total_score.borrow_mut() = Some(score);
    }

    /// Reset all computed values.
    pub fn reset(&self) {
        let num_states = self.num_states();
        *self.forward_scores.borrow_mut() = vec![LogWeight::zero(); num_states];
        *self.backward_scores.borrow_mut() = vec![LogWeight::zero(); num_states];
        *self.forward_computed.borrow_mut() = false;
        *self.backward_computed.borrow_mut() = false;
        *self.total_score.borrow_mut() = None;
    }
}

/// Compute backward pass gradients through a WFST.
///
/// This implements reverse-mode automatic differentiation for WFST operations.
/// It assumes the forward pass has already been computed (via `forward_score`).
///
/// # Algorithm
///
/// 1. Initialize β[f] = final_weight for all final states
/// 2. Process states in reverse topological order
/// 3. For each arc (s, t, w): β[s] = β[s] ⊕ (w ⊗ β[t])
/// 4. Compute arc gradients: ∂Z/∂w = exp(α[s] + w + β[t] - Z)
///
/// # Returns
///
/// A `GradientAccumulator` containing the gradient for each arc weight.
pub fn backward<L: Clone + Send + Sync>(grad_fst: &GradientWfst<L>) -> GradientAccumulator {
    let num_states = grad_fst.num_states();

    // Ensure forward pass is done
    if !grad_fst.is_forward_computed() {
        // Force forward computation
        super::forward_score::forward_score(grad_fst);
    }

    // Get total score (normalization constant)
    let total_score = grad_fst.total_score().unwrap_or_else(LogWeight::zero);

    // Initialize backward scores (β values)
    // β[f] = final_weight for final states
    for s in 0..num_states as StateId {
        if grad_fst.is_final(s) {
            grad_fst.set_backward_score(s, grad_fst.final_weight(s));
        } else {
            grad_fst.set_backward_score(s, LogWeight::zero());
        }
    }

    // Compute topological order (reverse for backward pass)
    let topo_order = compute_topological_order(grad_fst);

    // Process states in reverse topological order
    for &state in topo_order.iter().rev() {
        let transitions = grad_fst.transitions(state);
        for trans in transitions {
            let to_state = trans.to;
            let arc_weight = trans.weight;
            let beta_to = grad_fst.backward_score(to_state);

            // β[from] = β[from] ⊕ (arc_weight ⊗ β[to])
            let contribution = arc_weight.times(&beta_to);
            let current = grad_fst.backward_score(state);
            grad_fst.set_backward_score(state, current.plus(&contribution));
        }
    }

    grad_fst.set_backward_computed(true);

    // Compute arc gradients
    let mut gradients = GradientAccumulator::with_capacity(count_arcs(grad_fst));

    for state in 0..num_states as StateId {
        let alpha_from = grad_fst.forward_score(state);
        let transitions = grad_fst.transitions(state);

        for (arc_idx, trans) in transitions.iter().enumerate() {
            let to_state = trans.to;
            let arc_weight = trans.weight;
            let beta_to = grad_fst.backward_score(to_state);

            // Gradient = exp(α[from] + w + β[to] - Z)
            // In log semiring: α[from].value() + w.value() + β[to].value() - Z.value()
            let log_gradient = alpha_from.value() + arc_weight.value() + beta_to.value()
                - total_score.value();
            let gradient = log_gradient.exp();

            gradients.add_gradient(ArcIndex::new(state, arc_idx), gradient);
        }
    }

    gradients
}

/// Compute topological order for a WFST.
fn compute_topological_order<L: Clone + Send + Sync>(grad_fst: &GradientWfst<L>) -> Vec<StateId> {
    let num_states = grad_fst.num_states();
    let mut in_degree = vec![0usize; num_states];
    let mut order = Vec::with_capacity(num_states);

    // Count in-degrees
    for s in 0..num_states as StateId {
        for trans in grad_fst.transitions(s) {
            in_degree[trans.to as usize] += 1;
        }
    }

    // Start with states having zero in-degree
    let mut queue: Vec<StateId> = (0..num_states as StateId)
        .filter(|&s| in_degree[s as usize] == 0)
        .collect();

    while let Some(state) = queue.pop() {
        order.push(state);
        for trans in grad_fst.transitions(state) {
            let to = trans.to as usize;
            in_degree[to] -= 1;
            if in_degree[to] == 0 {
                queue.push(trans.to);
            }
        }
    }

    // If not all states are in order, graph has cycles - use BFS order as fallback
    if order.len() < num_states {
        order = (0..num_states as StateId).collect();
    }

    order
}

/// Count total arcs in a WFST.
fn count_arcs<L: Clone + Send + Sync>(grad_fst: &GradientWfst<L>) -> usize {
    let mut count = 0;
    for s in 0..grad_fst.num_states() as StateId {
        count += grad_fst.transitions(s).len();
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::MutableWfst;

    #[test]
    fn test_arc_index() {
        let idx = ArcIndex::new(5, 3);
        assert_eq!(idx.from, 5);
        assert_eq!(idx.arc_idx, 3);
    }

    #[test]
    fn test_gradient_accumulator() {
        let mut acc = GradientAccumulator::new();
        acc.add_gradient(ArcIndex::new(0, 0), 0.5);
        acc.add_gradient(ArcIndex::new(1, 0), 0.3);

        assert_eq!(acc.get_gradient(ArcIndex::new(0, 0)), 0.5);
        assert_eq!(acc.get_gradient(ArcIndex::new(1, 0)), 0.3);
        assert_eq!(acc.get_gradient(ArcIndex::new(2, 0)), 0.0);
    }

    #[test]
    fn test_gradient_wfst_creation() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        assert_eq!(grad_fst.num_states(), 2);
        assert_eq!(grad_fst.start(), 0);
        assert!(grad_fst.is_final(1));
        assert!(!grad_fst.is_forward_computed());
    }

    #[test]
    fn test_gradient_wfst_reset() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s0, LogWeight::one());

        let grad_fst = GradientWfst::from_wfst(&fst);
        grad_fst.set_forward_score(0, LogWeight::new(-1.0));
        grad_fst.set_forward_computed(true);

        grad_fst.reset();

        assert!(!grad_fst.is_forward_computed());
        assert!(grad_fst.forward_score(0).is_zero());
    }
}
