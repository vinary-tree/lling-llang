//! k2-style top-down automatic differentiation for WFSTs.
//!
//! This module implements efficient gradient computation following the k2
//! framework's approach, which differs from traditional bottom-up autodiff:
//!
//! ## Top-Down vs Bottom-Up Differentiation
//!
//! **Bottom-up (traditional)**: Each primitive operation records gradients.
//! - More general, works for arbitrary computation graphs
//! - Can have redundant computation for WFST pipelines
//! - Memory-intensive for large lattices
//!
//! **Top-down (k2-style)**: Differentiation at the algorithm level.
//! - More efficient for WFST-specific operations
//! - Better numerical stability for log-domain computations
//! - Enables gradients through pruned operations
//!
//! ## Key Components
//!
//! 1. **Composed WFST Backward**: Efficiently propagate gradients through
//!    composition without materializing intermediate products.
//!
//! 2. **Pruned Search Backward**: Compute gradients only for paths that
//!    survived pruning, avoiding wasted computation.
//!
//! 3. **Sparse Gradients**: Represent gradients compactly when most arcs
//!    have zero gradient (common after pruning).
//!
//! ## Example
//!
//! ```rust,ignore
//! use lling_llang::differentiable::topdown::{composed_backward, SparseGradient};
//!
//! // After composing two WFSTs and computing forward score
//! let (grad1, grad2) = composed_backward(&composed, &output_grad);
//!
//! // Gradients are sparse - only active arcs have non-zero values
//! for (arc_id, grad) in grad1.iter() {
//!     println!("Arc {}: gradient = {}", arc_id, grad);
//! }
//! ```
//!
//! ## References
//!
//! - k2-fsa/k2: <https://github.com/k2-fsa/k2>
//! - "k2: A Framework for Speech Recognition" (2020)

use std::collections::HashMap;

use crate::semiring::Semiring;
use crate::wfst::{StateId, VectorWfst, Wfst};

/// Sparse gradient representation for efficient storage.
///
/// Most arcs in a pruned WFST have zero gradient, so we only store
/// non-zero values.
#[derive(Debug, Clone, Default)]
pub struct SparseGradient {
    /// Arc index -> gradient value mapping.
    gradients: HashMap<usize, f64>,
    /// Total number of arcs (for dense conversion).
    num_arcs: usize,
}

impl SparseGradient {
    /// Create a new sparse gradient structure.
    pub fn new(num_arcs: usize) -> Self {
        Self {
            gradients: HashMap::new(),
            num_arcs,
        }
    }

    /// Set gradient for an arc.
    #[inline]
    pub fn set(&mut self, arc_id: usize, value: f64) {
        if arc_id >= self.num_arcs {
            return;
        }

        if value.abs() > 1e-10 {
            self.gradients.insert(arc_id, value);
        } else {
            self.gradients.remove(&arc_id);
        }
    }

    /// Get gradient for an arc (returns 0 if not stored).
    #[inline]
    pub fn get(&self, arc_id: usize) -> f64 {
        self.gradients.get(&arc_id).copied().unwrap_or(0.0)
    }

    /// Add to gradient for an arc.
    #[inline]
    pub fn add(&mut self, arc_id: usize, value: f64) {
        if arc_id >= self.num_arcs || value.abs() <= 1e-10 {
            return;
        }

        let entry = self.gradients.entry(arc_id).or_insert(0.0);
        *entry += value;
        if entry.abs() <= 1e-10 {
            self.gradients.remove(&arc_id);
        }
    }

    /// Number of non-zero gradients.
    pub fn nnz(&self) -> usize {
        self.gradients.len()
    }

    /// Total number of arcs.
    pub fn num_arcs(&self) -> usize {
        self.num_arcs
    }

    /// Sparsity ratio (0 = all non-zero, 1 = all zero).
    pub fn sparsity(&self) -> f64 {
        if self.num_arcs == 0 {
            0.0
        } else {
            1.0 - (self.gradients.len() as f64 / self.num_arcs as f64)
        }
    }

    /// Convert to dense gradient vector.
    pub fn to_dense(&self) -> Vec<f64> {
        let mut dense = vec![0.0; self.num_arcs];
        for (&arc_id, &value) in &self.gradients {
            if arc_id < self.num_arcs {
                dense[arc_id] = value;
            }
        }
        dense
    }

    /// Iterate over non-zero gradients.
    pub fn iter(&self) -> impl Iterator<Item = (usize, f64)> + '_ {
        self.gradients.iter().map(|(&k, &v)| (k, v))
    }

    /// Scale all gradients by a factor.
    pub fn scale(&mut self, factor: f64) {
        if factor.abs() <= 1e-10 {
            self.gradients.clear();
            return;
        }

        for value in self.gradients.values_mut() {
            *value *= factor;
        }
        self.gradients.retain(|_, value| value.abs() > 1e-10);
    }

    /// Add another sparse gradient.
    pub fn add_sparse(&mut self, other: &SparseGradient) {
        for (&arc_id, &value) in &other.gradients {
            self.add(arc_id, value);
        }
    }
}

/// Result of composed WFST backward pass.
#[derive(Debug)]
pub struct ComposedBackwardResult {
    /// Gradients for the first input WFST.
    pub grad1: SparseGradient,
    /// Gradients for the second input WFST.
    pub grad2: SparseGradient,
    /// Statistics about the backward pass.
    pub stats: BackwardStats,
}

/// Statistics from backward pass.
#[derive(Debug, Clone, Default)]
pub struct BackwardStats {
    /// Number of composed states visited.
    pub states_visited: usize,
    /// Number of arcs with non-zero gradient.
    pub nonzero_arcs: usize,
    /// Total gradient mass (sum of absolute gradients).
    pub total_gradient_mass: f64,
}

/// Forward-backward scores at each state.
#[derive(Debug, Clone)]
pub struct ForwardBackwardScores {
    /// Forward log-probabilities: α[s] = log P(reach s from start).
    pub alpha: Vec<f64>,
    /// Backward log-probabilities: β[s] = log P(reach final from s).
    pub beta: Vec<f64>,
    /// Total log-probability (α at final states + final weights).
    pub total_log_prob: f64,
}

impl ForwardBackwardScores {
    /// Create new forward-backward scores.
    pub fn new(num_states: usize) -> Self {
        Self {
            alpha: vec![f64::NEG_INFINITY; num_states],
            beta: vec![f64::NEG_INFINITY; num_states],
            total_log_prob: f64::NEG_INFINITY,
        }
    }

    /// Compute arc posterior: P(arc | observation) = exp(α + w + β - Z).
    #[inline]
    pub fn arc_posterior(&self, from_alpha: f64, arc_weight: f64, to_beta: f64) -> f64 {
        let log_posterior = from_alpha + arc_weight + to_beta - self.total_log_prob;
        if log_posterior > f64::NEG_INFINITY {
            log_posterior.exp()
        } else {
            0.0
        }
    }
}

/// Compute forward-backward scores for a WFST.
///
/// This is the foundation for top-down gradient computation.
/// Uses a single-pass algorithm that processes each arc exactly once,
/// suitable for acyclic graphs. For cyclic graphs, additional iterations
/// would be needed.
///
/// # Arguments
/// * `fst` - The WFST to analyze
///
/// # Returns
/// Forward and backward scores at each state.
pub fn forward_backward<L, W>(fst: &VectorWfst<L, W>) -> ForwardBackwardScores
where
    L: Clone + Eq + std::hash::Hash + Send + Sync,
    W: Semiring + Into<f64> + Clone,
{
    use std::collections::VecDeque;

    let num_states = fst.num_states();
    let mut scores = ForwardBackwardScores::new(num_states);

    if num_states == 0 {
        return scores;
    }

    let start = fst.start();
    if !fst.is_valid_state(start) {
        return scores;
    }

    // Forward pass: compute α[s] using BFS-style single pass
    // Each arc is processed exactly once
    scores.alpha[start as usize] = 0.0;

    // Track which states have been processed (all incoming arcs handled)
    let mut in_degree = vec![0usize; num_states];
    for state in 0..num_states as StateId {
        for tr in fst.transitions(state) {
            if !fst.is_valid_state(tr.to) {
                continue;
            }
            in_degree[tr.to as usize] += 1;
        }
    }

    // Queue states with no incoming edges (or start state)
    let mut queue: VecDeque<StateId> = VecDeque::new();
    let mut processed = vec![false; num_states];

    // Start from states with in_degree 0, but ensure start is processed
    queue.push_back(start);
    processed[start as usize] = true;

    // Also add any other states with in_degree 0
    for state in 0..num_states as StateId {
        if in_degree[state as usize] == 0 && state != start {
            queue.push_back(state);
            processed[state as usize] = true;
        }
    }

    // Process states, propagating alpha values
    let mut remaining_in = in_degree.clone();
    while let Some(state) = queue.pop_front() {
        if scores.alpha[state as usize] <= f64::NEG_INFINITY {
            // State not reachable from start
            for tr in fst.transitions(state) {
                let Some(to_remaining) = remaining_in.get_mut(tr.to as usize) else {
                    continue;
                };

                *to_remaining = to_remaining.saturating_sub(1);
                if *to_remaining == 0 && !processed[tr.to as usize] {
                    queue.push_back(tr.to);
                    processed[tr.to as usize] = true;
                }
            }
            continue;
        }

        for tr in fst.transitions(state) {
            let to_idx = tr.to as usize;
            if to_idx >= num_states {
                continue;
            }

            let arc_weight: f64 = tr.weight.clone().into();
            let new_alpha = scores.alpha[state as usize] + arc_weight;
            scores.alpha[to_idx] = log_add(scores.alpha[to_idx], new_alpha);

            remaining_in[to_idx] = remaining_in[to_idx].saturating_sub(1);
            if remaining_in[to_idx] == 0 && !processed[to_idx] {
                queue.push_back(tr.to);
                processed[to_idx] = true;
            }
        }
    }

    // Backward pass: compute β[s] using reverse topological order
    // Initialize final states
    for state in 0..num_states as StateId {
        if fst.is_final(state) {
            let final_weight: f64 = fst.final_weight(state).into();
            scores.beta[state as usize] = final_weight;
        }
    }

    // Compute out-degree for reverse traversal
    let mut out_degree = vec![0usize; num_states];
    for state in 0..num_states as StateId {
        out_degree[state as usize] = fst
            .transitions(state)
            .iter()
            .filter(|tr| fst.is_valid_state(tr.to))
            .count();
    }

    // Process in reverse topological order
    let mut reverse_queue: VecDeque<StateId> = VecDeque::new();
    let mut reverse_processed = vec![false; num_states];
    let mut remaining_out = out_degree.clone();

    // Start from states with no outgoing edges (sinks/finals)
    for state in 0..num_states as StateId {
        if out_degree[state as usize] == 0 {
            reverse_queue.push_back(state);
            reverse_processed[state as usize] = true;
        }
    }

    // Build reverse adjacency for backward propagation
    let mut reverse_adj: Vec<Vec<(StateId, f64)>> = vec![Vec::new(); num_states];
    for state in 0..num_states as StateId {
        for tr in fst.transitions(state) {
            if !fst.is_valid_state(tr.to) {
                continue;
            }

            let arc_weight: f64 = tr.weight.clone().into();
            reverse_adj[tr.to as usize].push((state, arc_weight));
        }
    }

    while let Some(state) = reverse_queue.pop_front() {
        // Propagate beta backwards
        for &(from_state, arc_weight) in &reverse_adj[state as usize] {
            if scores.beta[state as usize] > f64::NEG_INFINITY {
                let new_beta = arc_weight + scores.beta[state as usize];
                scores.beta[from_state as usize] =
                    log_add(scores.beta[from_state as usize], new_beta);
            }

            remaining_out[from_state as usize] =
                remaining_out[from_state as usize].saturating_sub(1);
            if remaining_out[from_state as usize] == 0 && !reverse_processed[from_state as usize] {
                reverse_queue.push_back(from_state);
                reverse_processed[from_state as usize] = true;
            }
        }
    }

    // Total log-probability
    scores.total_log_prob = scores.alpha[start as usize] + scores.beta[start as usize];

    scores
}

/// Compute gradients using top-down (k2-style) approach.
///
/// Given forward-backward scores, computes gradient for each arc weight.
/// The gradient of arc (s, t, w) is:
///   ∂Loss/∂w = -posterior(arc) = -exp(α[s] + w + β[t] - Z)
///
/// # Arguments
/// * `fst` - The WFST
/// * `fb_scores` - Forward-backward scores from `forward_backward()`
///
/// # Returns
/// Sparse gradients for arc weights.
pub fn topdown_backward<L, W>(
    fst: &VectorWfst<L, W>,
    fb_scores: &ForwardBackwardScores,
) -> SparseGradient
where
    L: Clone + Eq + std::hash::Hash + Send + Sync,
    W: Semiring + Into<f64> + Clone,
{
    let num_states = fst.num_states();
    let mut total_arcs = 0;
    for state in 0..num_states as StateId {
        total_arcs += fst.transitions(state).len();
    }

    let mut gradients = SparseGradient::new(total_arcs);
    let mut arc_id = 0;

    for state in 0..num_states as StateId {
        let alpha_s = fb_scores
            .alpha
            .get(state as usize)
            .copied()
            .unwrap_or(f64::NEG_INFINITY);
        if alpha_s <= f64::NEG_INFINITY {
            arc_id += fst.transitions(state).len();
            continue;
        }

        for tr in fst.transitions(state) {
            let beta_t = if fst.is_valid_state(tr.to) {
                fb_scores
                    .beta
                    .get(tr.to as usize)
                    .copied()
                    .unwrap_or(f64::NEG_INFINITY)
            } else {
                f64::NEG_INFINITY
            };
            if beta_t <= f64::NEG_INFINITY {
                arc_id += 1;
                continue;
            }

            let arc_weight: f64 = tr.weight.clone().into();
            let posterior = fb_scores.arc_posterior(alpha_s, arc_weight, beta_t);

            // Gradient is negative posterior for NLL loss
            if posterior > 1e-10 {
                gradients.set(arc_id, -posterior);
            }

            arc_id += 1;
        }
    }

    gradients
}

/// Configuration for pruned composition backward.
#[derive(Debug, Clone)]
pub struct PrunedBackwardConfig {
    /// Pruning beam used in forward pass.
    pub beam: f64,
    /// Whether to normalize gradients.
    pub normalize: bool,
    /// Minimum gradient magnitude to keep.
    pub min_gradient: f64,
}

impl Default for PrunedBackwardConfig {
    fn default() -> Self {
        Self {
            beam: 10.0,
            normalize: true,
            min_gradient: 1e-10,
        }
    }
}

/// Result of a pruned search operation (for backward pass).
#[derive(Debug)]
pub struct PrunedSearchResult<W: Semiring> {
    /// States that survived pruning (original state ID -> pruned state ID).
    pub surviving_states: HashMap<StateId, StateId>,
    /// Arcs that survived pruning (original arc ID -> pruned arc ID).
    pub surviving_arcs: HashMap<usize, usize>,
    /// Forward scores at surviving states.
    pub forward_scores: Vec<f64>,
    /// Best path score.
    pub best_score: f64,
    /// Pruning beam used.
    pub beam: f64,
    /// Phantom for weight type.
    _phantom: std::marker::PhantomData<W>,
}

impl<W: Semiring> PrunedSearchResult<W> {
    /// Create a new pruned search result.
    pub fn new(beam: f64) -> Self {
        Self {
            surviving_states: HashMap::new(),
            surviving_arcs: HashMap::new(),
            forward_scores: Vec::new(),
            best_score: f64::NEG_INFINITY,
            beam,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add a surviving state.
    pub fn add_state(&mut self, original: StateId, forward_score: f64) {
        let pruned_id = self.surviving_states.len() as StateId;
        self.surviving_states.insert(original, pruned_id);
        self.forward_scores.push(forward_score);
        if forward_score > self.best_score {
            self.best_score = forward_score;
        }
    }

    /// Add a surviving arc.
    pub fn add_arc(&mut self, original: usize) {
        let pruned_id = self.surviving_arcs.len();
        self.surviving_arcs.insert(original, pruned_id);
    }

    /// Check if state survived pruning.
    pub fn state_survived(&self, state: StateId) -> bool {
        self.surviving_states.contains_key(&state)
    }

    /// Check if arc survived pruning.
    pub fn arc_survived(&self, arc_id: usize) -> bool {
        self.surviving_arcs.contains_key(&arc_id)
    }
}

/// Compute gradients through pruned search.
///
/// Only computes gradients for arcs that survived pruning, avoiding
/// wasted computation on paths that were discarded.
///
/// # Arguments
/// * `fst` - Original (unpruned) WFST
/// * `search_result` - Result from pruned forward search
/// * `output_grad` - Gradient from loss with respect to output score
/// * `config` - Configuration options
///
/// # Returns
/// Sparse gradients for surviving arcs only.
pub fn pruned_search_backward<L, W>(
    fst: &VectorWfst<L, W>,
    search_result: &PrunedSearchResult<W>,
    output_grad: f64,
    config: &PrunedBackwardConfig,
) -> SparseGradient
where
    L: Clone + Eq + std::hash::Hash + Send + Sync,
    W: Semiring + Into<f64> + Clone,
{
    // Only compute for surviving arcs
    let num_surviving = search_result.surviving_arcs.len();
    let mut gradients = SparseGradient::new(num_surviving);

    // Compute backward scores only for surviving states
    let num_surviving_states = search_result.surviving_states.len();
    let mut beta = vec![f64::NEG_INFINITY; num_surviving_states];

    // Initialize final states
    for (&orig_state, &pruned_id) in &search_result.surviving_states {
        if fst.is_valid_state(orig_state) && fst.is_final(orig_state) {
            let final_weight: f64 = fst.final_weight(orig_state).into();
            if let Some(slot) = beta.get_mut(pruned_id as usize) {
                *slot = final_weight;
            }
        }
    }

    // Backward pass through surviving structure
    // (We need to track arc connectivity in the pruned graph)
    // For simplicity, we re-scan and only process surviving arcs
    let num_states = fst.num_states();
    for _ in 0..num_surviving_states + 1 {
        let mut changed = false;
        let mut arc_id = 0;

        for state in 0..num_states as StateId {
            let pruned_from = match search_result.surviving_states.get(&state) {
                Some(&id) => id,
                None => {
                    arc_id += fst.transitions(state).len();
                    continue;
                }
            };
            let Some(pruned_from_idx) =
                ((pruned_from as usize) < beta.len()).then_some(pruned_from as usize)
            else {
                arc_id += fst.transitions(state).len();
                continue;
            };

            for tr in fst.transitions(state) {
                if search_result.arc_survived(arc_id) {
                    if fst.is_valid_state(tr.to) {
                        if let Some(&pruned_to) = search_result.surviving_states.get(&tr.to) {
                            let Some(beta_to) = beta.get(pruned_to as usize).copied() else {
                                continue;
                            };

                            if beta_to > f64::NEG_INFINITY {
                                let old_beta = beta[pruned_from_idx];
                                let arc_weight: f64 = tr.weight.clone().into();
                                let new_beta = arc_weight + beta_to;
                                let updated = log_add(old_beta, new_beta);

                                if (updated - old_beta).abs() > 1e-10 {
                                    beta[pruned_from_idx] = updated;
                                    changed = true;
                                }
                            }
                        }
                    }
                }
                arc_id += 1;
            }
        }

        if !changed {
            break;
        }
    }

    // Compute total log-prob in pruned graph
    let start = fst.start();
    let total_log_prob = if let Some(&pruned_start) = search_result.surviving_states.get(&start) {
        let pruned_start_idx = pruned_start as usize;
        let alpha = search_result
            .forward_scores
            .get(pruned_start_idx)
            .copied()
            .unwrap_or(f64::NEG_INFINITY);
        let beta_start = beta
            .get(pruned_start_idx)
            .copied()
            .unwrap_or(f64::NEG_INFINITY);
        alpha + beta_start
    } else {
        f64::NEG_INFINITY
    };

    // Compute gradients for surviving arcs
    let mut arc_id = 0;
    for state in 0..num_states as StateId {
        let pruned_from = match search_result.surviving_states.get(&state) {
            Some(&id) => id,
            None => {
                arc_id += fst.transitions(state).len();
                continue;
            }
        };

        let alpha_s = search_result
            .forward_scores
            .get(pruned_from as usize)
            .copied()
            .unwrap_or(f64::NEG_INFINITY);

        for tr in fst.transitions(state) {
            if let Some(&pruned_arc_id) = search_result.surviving_arcs.get(&arc_id) {
                if pruned_arc_id >= num_surviving {
                    arc_id += 1;
                    continue;
                }

                if fst.is_valid_state(tr.to) {
                    if let Some(&pruned_to) = search_result.surviving_states.get(&tr.to) {
                        if let Some(beta_t) = beta.get(pruned_to as usize).copied() {
                            let arc_weight: f64 = tr.weight.clone().into();
                            let log_posterior = alpha_s + arc_weight + beta_t - total_log_prob;
                            let posterior = if log_posterior > f64::NEG_INFINITY {
                                log_posterior.exp()
                            } else {
                                0.0
                            };

                            if posterior > config.min_gradient {
                                // Gradient is posterior * output_grad (chain rule)
                                gradients.set(pruned_arc_id, -posterior * output_grad);
                            }
                        }
                    }
                }
            }
            arc_id += 1;
        }
    }

    if config.normalize && gradients.nnz() > 0 {
        let sum: f64 = gradients.iter().map(|(_, g)| g.abs()).sum();
        if sum > 1e-10 {
            gradients.scale(1.0 / sum);
        }
    }

    gradients
}

/// Composed state for backward pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComposedState {
    /// State in first WFST.
    pub s1: StateId,
    /// State in second WFST.
    pub s2: StateId,
}

/// Information about a composed arc needed for gradient computation.
#[derive(Debug, Clone, Copy)]
pub struct ComposedArcInfo {
    /// Source state in the composed FST.
    pub source: StateId,
    /// Destination state in the composed FST.
    pub dest: StateId,
    /// Log-weight of the composed arc (sum of weights in log domain).
    pub log_weight: f64,
    /// Arc index in first FST (None for epsilon).
    pub arc1: Option<usize>,
    /// Arc index in second FST (None for epsilon).
    pub arc2: Option<usize>,
}

/// Track arc mapping from composed WFST back to originals.
#[derive(Debug, Clone)]
pub struct ComposedArcMap {
    /// Map from composed arc to (arc_in_fst1, arc_in_fst2).
    /// One or both can be None for epsilon transitions.
    arc_origins: HashMap<usize, (Option<usize>, Option<usize>)>,
    /// Extended arc info for posterior computation.
    arc_info: Vec<ComposedArcInfo>,
}

impl ComposedArcMap {
    /// Create new arc map.
    pub fn new() -> Self {
        Self {
            arc_origins: HashMap::new(),
            arc_info: Vec::new(),
        }
    }

    /// Record origin of a composed arc (legacy method for compatibility).
    pub fn add(&mut self, composed_arc: usize, arc1: Option<usize>, arc2: Option<usize>) {
        self.arc_origins.insert(composed_arc, (arc1, arc2));
    }

    /// Record full information about a composed arc.
    pub fn add_with_info(
        &mut self,
        source: StateId,
        dest: StateId,
        log_weight: f64,
        arc1: Option<usize>,
        arc2: Option<usize>,
    ) {
        let idx = self.arc_info.len();
        self.arc_origins.insert(idx, (arc1, arc2));
        self.arc_info.push(ComposedArcInfo {
            source,
            dest,
            log_weight,
            arc1,
            arc2,
        });
    }

    /// Get origin arcs.
    pub fn get(&self, composed_arc: usize) -> Option<(Option<usize>, Option<usize>)> {
        self.arc_origins.get(&composed_arc).copied()
    }

    /// Get full arc info iterator.
    pub fn arc_infos(&self) -> impl Iterator<Item = &ComposedArcInfo> {
        self.arc_info.iter()
    }

    /// Check if extended arc info is available.
    pub fn has_arc_info(&self) -> bool {
        !self.arc_info.is_empty()
    }
}

impl Default for ComposedArcMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Backward pass through WFST composition.
///
/// Efficiently propagates gradients from composed WFST back to both
/// input WFSTs without materializing the full composition.
///
/// The gradient for arc a₁ in fst1 is:
///   ∂L/∂w(a₁) = Σ_{a₂} posterior(a₁ ∘ a₂) * output_grad
///
/// where posterior(arc) = exp(α[src] + w(arc) + β[dst] - Z)
///
/// Similarly for arcs in fst2.
///
/// # Arguments
/// * `fst1` - First input WFST
/// * `fst2` - Second input WFST
/// * `composed_fb` - Forward-backward scores in composed space
/// * `arc_map` - Mapping from composed arcs to original arcs
/// * `output_grad` - Gradient from loss
///
/// # Returns
/// Gradients for both input WFSTs.
pub fn composed_backward<L, W>(
    fst1: &VectorWfst<L, W>,
    fst2: &VectorWfst<L, W>,
    composed_fb: &ForwardBackwardScores,
    arc_map: &ComposedArcMap,
    output_grad: f64,
) -> ComposedBackwardResult
where
    L: Clone + Eq + std::hash::Hash + Send + Sync,
    W: Semiring + Into<f64> + Clone,
{
    // Count arcs in each input
    let mut num_arcs1 = 0;
    for state in 0..fst1.num_states() as StateId {
        num_arcs1 += fst1.transitions(state).len();
    }
    let mut num_arcs2 = 0;
    for state in 0..fst2.num_states() as StateId {
        num_arcs2 += fst2.transitions(state).len();
    }

    let mut grad1 = SparseGradient::new(num_arcs1);
    let mut grad2 = SparseGradient::new(num_arcs2);
    let mut stats = BackwardStats::default();

    // Use proper posterior computation if arc info is available
    if arc_map.has_arc_info() {
        // Compute posteriors for each composed arc and distribute gradients
        for arc_info in arc_map.arc_infos() {
            // Compute arc posterior: P(arc | observation) = exp(α[src] + w + β[dst] - Z)
            let src = arc_info.source as usize;
            let dst = arc_info.dest as usize;

            // Check bounds
            if src >= composed_fb.alpha.len() || dst >= composed_fb.beta.len() {
                continue;
            }

            let posterior = composed_fb.arc_posterior(
                composed_fb.alpha[src],
                arc_info.log_weight,
                composed_fb.beta[dst],
            );

            // Skip arcs with zero posterior (unreachable paths)
            if posterior <= 0.0 {
                continue;
            }

            // Gradient is proportional to posterior
            // For log-likelihood loss, grad = -posterior * output_grad
            let grad_value = -posterior * output_grad;
            let mut emitted = false;

            if let Some(arc1) = arc_info.arc1 {
                if arc1 < num_arcs1 {
                    grad1.add(arc1, grad_value);
                    stats.nonzero_arcs += 1;
                    emitted = true;
                }
            }
            if let Some(arc2) = arc_info.arc2 {
                if arc2 < num_arcs2 {
                    grad2.add(arc2, grad_value);
                    stats.nonzero_arcs += 1;
                    emitted = true;
                }
            }

            if emitted {
                stats.total_gradient_mass += grad_value.abs();
            }
        }
    } else {
        // Legacy fallback: uniform contribution when arc info not available
        // This is less accurate but maintains backward compatibility
        let num_arcs = arc_map.arc_origins.len();
        let uniform_weight = if num_arcs > 0 {
            1.0 / num_arcs as f64
        } else {
            0.0
        };

        for &(arc1_opt, arc2_opt) in arc_map.arc_origins.values() {
            let grad_value = -output_grad * uniform_weight;
            let mut emitted = false;

            if let Some(arc1) = arc1_opt {
                if arc1 < num_arcs1 {
                    grad1.add(arc1, grad_value);
                    stats.nonzero_arcs += 1;
                    emitted = true;
                }
            }
            if let Some(arc2) = arc2_opt {
                if arc2 < num_arcs2 {
                    grad2.add(arc2, grad_value);
                    stats.nonzero_arcs += 1;
                    emitted = true;
                }
            }

            if emitted {
                stats.total_gradient_mass += grad_value.abs();
            }
        }
    }

    stats.states_visited = composed_fb.alpha.len();

    ComposedBackwardResult {
        grad1,
        grad2,
        stats,
    }
}

/// Log-add operation: log(exp(a) + exp(b)).
#[inline]
fn log_add(a: f64, b: f64) -> f64 {
    if a == f64::NEG_INFINITY {
        b
    } else if b == f64::NEG_INFINITY {
        a
    } else if a > b {
        a + (1.0 + (b - a).exp()).ln()
    } else {
        b + (1.0 + (a - b).exp()).ln()
    }
}

/// Helper to count total arcs in a WFST.
pub fn count_arcs<L, W>(fst: &VectorWfst<L, W>) -> usize
where
    L: Clone + Eq + std::hash::Hash + Send + Sync,
    W: Semiring,
{
    let mut total = 0;
    for state in 0..fst.num_states() as StateId {
        total += fst.transitions(state).len();
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::LogWeight;
    use crate::wfst::MutableWfst;

    #[test]
    fn test_sparse_gradient_basic() {
        let mut grad = SparseGradient::new(10);
        grad.set(0, 0.5);
        grad.set(5, -0.3);

        assert_eq!(grad.nnz(), 2);
        assert!((grad.get(0) - 0.5).abs() < 1e-10);
        assert!((grad.get(5) - (-0.3)).abs() < 1e-10);
        assert!((grad.get(3) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_sparse_gradient_add() {
        let mut grad = SparseGradient::new(10);
        grad.add(0, 0.5);
        grad.add(0, 0.3);

        assert!((grad.get(0) - 0.8).abs() < 1e-10);
    }

    #[test]
    fn test_sparse_gradient_ignores_out_of_range_arcs() {
        let mut grad = SparseGradient::new(2);
        grad.set(5, 1.0);
        grad.add(6, 1.0);

        assert_eq!(grad.nnz(), 0);
        assert_eq!(grad.to_dense(), vec![0.0, 0.0]);
    }

    #[test]
    fn test_sparse_gradient_add_removes_zero_entries() {
        let mut grad = SparseGradient::new(2);
        grad.add(0, 0.5);
        grad.add(0, -0.5);

        assert_eq!(grad.nnz(), 0);
        assert_eq!(grad.get(0), 0.0);
    }

    #[test]
    fn test_sparse_gradient_set_zero_clears_entry() {
        let mut grad = SparseGradient::new(2);
        grad.set(0, 0.5);
        grad.set(0, 0.0);

        assert_eq!(grad.nnz(), 0);
        assert_eq!(grad.get(0), 0.0);
    }

    #[test]
    fn test_sparse_gradient_scale_zero_clears_entries() {
        let mut grad = SparseGradient::new(3);
        grad.set(0, 0.5);
        grad.set(2, -0.5);
        grad.scale(0.0);

        assert_eq!(grad.nnz(), 0);
        assert_eq!(grad.to_dense(), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_sparse_gradient_sparsity() {
        let mut grad = SparseGradient::new(100);
        grad.set(0, 0.5);
        grad.set(50, 0.3);

        assert!((grad.sparsity() - 0.98).abs() < 1e-10);
    }

    #[test]
    fn test_sparse_gradient_to_dense() {
        let mut grad = SparseGradient::new(5);
        grad.set(1, 0.5);
        grad.set(3, -0.3);

        let dense = grad.to_dense();
        assert_eq!(dense.len(), 5);
        assert!((dense[0] - 0.0).abs() < 1e-10);
        assert!((dense[1] - 0.5).abs() < 1e-10);
        assert!((dense[3] - (-0.3)).abs() < 1e-10);
    }

    #[test]
    fn test_forward_backward_single_path() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));

        let fb = forward_backward(&fst);

        // Forward: α[0] = 0, α[1] = 1.0 (arc weight)
        assert!((fb.alpha[0] - 0.0).abs() < 1e-6);
        assert!((fb.alpha[1] - 1.0).abs() < 1e-6);

        // Backward: β[1] = 0 (final), β[0] = 1.0
        assert!((fb.beta[1] - 0.0).abs() < 1e-6);
        assert!((fb.beta[0] - 1.0).abs() < 1e-6);

        // Total = α[0] + β[0] = 1.0
        assert!((fb.total_log_prob - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_forward_backward_no_start_returns_unreachable_scores() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        fst.add_states(2);
        fst.set_final(1, LogWeight::one());

        let fb = forward_backward(&fst);

        assert_eq!(fb.alpha, vec![f64::NEG_INFINITY; 2]);
        assert_eq!(fb.beta, vec![f64::NEG_INFINITY; 2]);
        assert_eq!(fb.total_log_prob, f64::NEG_INFINITY);
    }

    #[test]
    fn test_forward_backward_skips_malformed_transition_targets() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s0, Some('x'), Some('x'), 99, LogWeight::new(0.5));

        let fb = forward_backward(&fst);

        assert!((fb.alpha[0] - 0.0).abs() < 1e-6);
        assert!((fb.alpha[1] - 1.0).abs() < 1e-6);
        assert!((fb.beta[0] - 1.0).abs() < 1e-6);
        assert!((fb.beta[1] - 0.0).abs() < 1e-6);
        assert!((fb.total_log_prob - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_forward_backward_two_paths() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(2.0));

        let fb = forward_backward(&fst);

        // Total = log(exp(-1) + exp(-2)) in negative log domain
        // But LogWeight stores negative log, so we get log-sum-exp
        let expected_alpha1 = log_add(1.0, 2.0);
        assert!((fb.alpha[1] - expected_alpha1).abs() < 1e-6);
    }

    #[test]
    fn test_topdown_backward_single_path() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));

        let fb = forward_backward(&fst);
        let grads = topdown_backward(&fst, &fb);

        // Single path: posterior = 1.0, gradient = -1.0
        assert_eq!(grads.nnz(), 1);
        assert!((grads.get(0) - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_topdown_backward_skips_malformed_targets() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s0, Some('x'), Some('x'), 99, LogWeight::new(0.5));

        let fb = forward_backward(&fst);
        let grads = topdown_backward(&fst, &fb);

        assert_eq!(grads.num_arcs(), 2);
        assert_eq!(grads.nnz(), 1);
        assert!((grads.get(0) - (-1.0)).abs() < 1e-6);
        assert_eq!(grads.get(1), 0.0);
    }

    #[test]
    fn test_topdown_backward_tolerates_short_score_vectors() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));

        let fb = ForwardBackwardScores::new(1);
        let grads = topdown_backward(&fst, &fb);

        assert_eq!(grads.num_arcs(), 1);
        assert_eq!(grads.nnz(), 0);
    }

    #[test]
    fn test_pruned_search_result() {
        let mut result = PrunedSearchResult::<LogWeight>::new(10.0);
        result.add_state(0, -5.0);
        result.add_state(1, -3.0);
        result.add_arc(0);

        assert!(result.state_survived(0));
        assert!(result.state_survived(1));
        assert!(!result.state_survived(2));
        assert!(result.arc_survived(0));
        assert!(!result.arc_survived(1));
        assert!((result.best_score - (-3.0)).abs() < 1e-10);
    }

    #[test]
    fn test_pruned_search_backward_tolerates_inconsistent_maps() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        fst.add_state();
        fst.set_start(s0);
        fst.add_arc(s0, Some('x'), Some('x'), 99, LogWeight::new(0.5));

        let mut result = PrunedSearchResult::<LogWeight>::new(10.0);
        result.add_state(s0, 0.0);
        result.add_state(99, 0.5);
        result.forward_scores.truncate(1);
        result.add_arc(0);
        result.surviving_arcs.insert(0, 99);

        let grads = pruned_search_backward(&fst, &result, 1.0, &PrunedBackwardConfig::default());

        assert_eq!(grads.num_arcs(), 1);
        assert_eq!(grads.nnz(), 0);
    }

    #[test]
    fn test_composed_arc_map() {
        let mut map = ComposedArcMap::new();
        map.add(0, Some(0), Some(1));
        map.add(1, Some(2), None);
        map.add(2, None, Some(3));

        assert_eq!(map.get(0), Some((Some(0), Some(1))));
        assert_eq!(map.get(1), Some((Some(2), None)));
        assert_eq!(map.get(2), Some((None, Some(3))));
        assert_eq!(map.get(3), None);
    }

    #[test]
    fn test_log_add() {
        assert!((log_add(0.0, 0.0) - 0.693).abs() < 0.01); // ln(2)
        assert!((log_add(f64::NEG_INFINITY, 0.0) - 0.0).abs() < 0.001);
        assert!((log_add(0.0, f64::NEG_INFINITY) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_count_arcs() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(2.0));

        assert_eq!(count_arcs(&fst), 2);
    }

    #[test]
    fn test_composed_arc_map_with_info() {
        let mut map = ComposedArcMap::new();

        // Add arc info with source, dest, weight
        map.add_with_info(0, 1, 1.5, Some(0), Some(0));
        map.add_with_info(1, 2, 2.0, Some(1), None);

        assert!(map.has_arc_info());

        let infos: Vec<_> = map.arc_infos().collect();
        assert_eq!(infos.len(), 2);

        assert_eq!(infos[0].source, 0);
        assert_eq!(infos[0].dest, 1);
        assert!((infos[0].log_weight - 1.5).abs() < 1e-10);
        assert_eq!(infos[0].arc1, Some(0));
        assert_eq!(infos[0].arc2, Some(0));

        assert_eq!(infos[1].source, 1);
        assert_eq!(infos[1].dest, 2);
        assert!((infos[1].log_weight - 2.0).abs() < 1e-10);
        assert_eq!(infos[1].arc1, Some(1));
        assert_eq!(infos[1].arc2, None);
    }

    #[test]
    fn test_composed_backward_with_posteriors() {
        // Create two simple FSTs
        let mut fst1 = VectorWfst::<char, LogWeight>::new();
        let s0 = fst1.add_state();
        let s1 = fst1.add_state();
        fst1.set_start(s0);
        fst1.set_final(s1, LogWeight::one());
        fst1.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));

        let mut fst2 = VectorWfst::<char, LogWeight>::new();
        let t0 = fst2.add_state();
        let t1 = fst2.add_state();
        fst2.set_start(t0);
        fst2.set_final(t1, LogWeight::one());
        fst2.add_arc(t0, Some('a'), Some('a'), t1, LogWeight::new(0.5));

        // Create forward-backward scores for a composed FST with 2 states
        // Simulating composition: (s0,t0) -> (s1,t1) with combined weight 1.5
        let mut fb = ForwardBackwardScores::new(2);
        fb.alpha[0] = 0.0; // Start state
        fb.alpha[1] = 1.5; // After arc (1.0 + 0.5)
        fb.beta[1] = 0.0; // Final state
        fb.beta[0] = 1.5; // Before arc
        fb.total_log_prob = 1.5; // Total path weight

        // Create arc map with info
        let mut arc_map = ComposedArcMap::new();
        arc_map.add_with_info(0, 1, 1.5, Some(0), Some(0));

        // Compute backward pass
        let result = composed_backward(&fst1, &fst2, &fb, &arc_map, 1.0);

        // With single path, posterior = exp(0 + 1.5 + 0 - 1.5) = 1.0
        // Gradient should be -1.0 * 1.0 = -1.0 for both arcs
        assert!(
            (result.grad1.get(0) - (-1.0)).abs() < 1e-6,
            "grad1[0] = {}, expected -1.0",
            result.grad1.get(0)
        );
        assert!(
            (result.grad2.get(0) - (-1.0)).abs() < 1e-6,
            "grad2[0] = {}, expected -1.0",
            result.grad2.get(0)
        );
    }

    #[test]
    fn test_composed_backward_two_paths() {
        // Create FSTs with two paths
        let mut fst1 = VectorWfst::<char, LogWeight>::new();
        let s0 = fst1.add_state();
        let s1 = fst1.add_state();
        fst1.set_start(s0);
        fst1.set_final(s1, LogWeight::one());
        fst1.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0)); // arc 0
        fst1.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(2.0)); // arc 1

        let mut fst2 = VectorWfst::<char, LogWeight>::new();
        let t0 = fst2.add_state();
        let t1 = fst2.add_state();
        fst2.set_start(t0);
        fst2.set_final(t1, LogWeight::one());
        fst2.add_arc(t0, Some('a'), Some('a'), t1, LogWeight::new(0.0)); // arc 0
        fst2.add_arc(t0, Some('b'), Some('b'), t1, LogWeight::new(0.0)); // arc 1

        // Composed FST has two paths: a->a (weight 1.0) and b->b (weight 2.0)
        // Total = log(exp(-1) + exp(-2)) in log domain
        let total = log_add(1.0, 2.0);

        let mut fb = ForwardBackwardScores::new(2);
        fb.alpha[0] = 0.0;
        fb.alpha[1] = total;
        fb.beta[1] = 0.0;
        fb.beta[0] = total;
        fb.total_log_prob = total;

        // Arc map: two composed arcs
        let mut arc_map = ComposedArcMap::new();
        arc_map.add_with_info(0, 1, 1.0, Some(0), Some(0)); // a path
        arc_map.add_with_info(0, 1, 2.0, Some(1), Some(1)); // b path

        let result = composed_backward(&fst1, &fst2, &fb, &arc_map, 1.0);

        // Posteriors should sum to ~1.0
        let posterior_a = fb.arc_posterior(0.0, 1.0, 0.0);
        let posterior_b = fb.arc_posterior(0.0, 2.0, 0.0);
        let sum = posterior_a + posterior_b;
        assert!(
            (sum - 1.0).abs() < 0.01,
            "Posteriors sum to {}, expected ~1.0",
            sum
        );

        // Gradients should be negative posteriors
        assert!(
            (result.grad1.get(0) - (-posterior_a)).abs() < 1e-6,
            "grad1[0] = {}, expected {}",
            result.grad1.get(0),
            -posterior_a
        );
        assert!(
            (result.grad1.get(1) - (-posterior_b)).abs() < 1e-6,
            "grad1[1] = {}, expected {}",
            result.grad1.get(1),
            -posterior_b
        );
    }

    #[test]
    fn test_composed_backward_skips_out_of_range_origin_arcs() {
        let mut fst1 = VectorWfst::<char, LogWeight>::new();
        let s0 = fst1.add_state();
        let s1 = fst1.add_state();
        fst1.set_start(s0);
        fst1.set_final(s1, LogWeight::one());
        fst1.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));

        let mut fst2 = VectorWfst::<char, LogWeight>::new();
        let t0 = fst2.add_state();
        let t1 = fst2.add_state();
        fst2.set_start(t0);
        fst2.set_final(t1, LogWeight::one());
        fst2.add_arc(t0, Some('a'), Some('a'), t1, LogWeight::new(0.5));

        let mut fb = ForwardBackwardScores::new(2);
        fb.alpha[0] = 0.0;
        fb.beta[1] = 0.0;
        fb.total_log_prob = 1.5;

        let mut arc_map = ComposedArcMap::new();
        arc_map.add_with_info(0, 1, 1.5, Some(99), Some(99));

        let result = composed_backward(&fst1, &fst2, &fb, &arc_map, 1.0);

        assert_eq!(result.grad1.nnz(), 0);
        assert_eq!(result.grad2.nnz(), 0);
        assert_eq!(result.stats.nonzero_arcs, 0);
        assert_eq!(result.stats.total_gradient_mass, 0.0);
    }

    #[test]
    fn test_composed_backward_legacy_fallback() {
        // Test that legacy mode works when no arc info is provided
        let mut fst1 = VectorWfst::<char, LogWeight>::new();
        let s0 = fst1.add_state();
        let s1 = fst1.add_state();
        fst1.set_start(s0);
        fst1.set_final(s1, LogWeight::one());
        fst1.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));

        let fst2 = fst1.clone();

        let fb = ForwardBackwardScores::new(2);

        // Legacy arc map without info
        let mut arc_map = ComposedArcMap::new();
        arc_map.add(0, Some(0), Some(0));

        let result = composed_backward(&fst1, &fst2, &fb, &arc_map, 1.0);

        // Should not panic and should produce some gradient
        assert!(result.grad1.nnz() > 0 || result.grad2.nnz() > 0);
    }

    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(64))]

            /// `set(i, x)` followed by `get(i)` must round-trip for any
            /// non-tiny float (values below the 1e-10 sparsity threshold are
            /// elided by design).
            #[test]
            fn sparse_gradient_set_get_roundtrip(
                arc_id in 0usize..128,
                value in (-1.0e3f64..1.0e3).prop_filter("non-tiny", |v| v.abs() > 1e-6)
            ) {
                let mut grad = SparseGradient::new(128);
                grad.set(arc_id, value);
                prop_assert!((grad.get(arc_id) - value).abs() < 1e-9);
            }

            /// `to_dense()` always yields a vector of length `num_arcs`,
            /// regardless of how many non-zero values were stored.
            #[test]
            fn sparse_gradient_dense_len_equals_num_arcs(
                num_arcs in 0usize..256,
                writes in proptest::collection::vec(
                    (0usize..256, -10.0f64..10.0),
                    0..16,
                ),
            ) {
                let mut grad = SparseGradient::new(num_arcs);
                for (idx, val) in writes {
                    if idx < num_arcs {
                        grad.set(idx, val);
                    }
                }
                prop_assert_eq!(grad.to_dense().len(), num_arcs);
            }

            /// `scale(0.0)` reduces all stored gradients to zero and keeps the
            /// sparse representation empty rather than storing explicit zeros.
            #[test]
            fn sparse_gradient_scale_zero_zeros(
                writes in proptest::collection::vec(
                    (0usize..64, -100.0f64..100.0),
                    1..16,
                ),
            ) {
                let mut grad = SparseGradient::new(64);
                for (idx, val) in &writes {
                    grad.set(*idx, *val);
                }
                grad.scale(0.0);
                for (idx, _) in &writes {
                    prop_assert!(grad.get(*idx).abs() < 1e-12);
                }
                prop_assert_eq!(grad.nnz(), 0);
            }
        }
    }
}
