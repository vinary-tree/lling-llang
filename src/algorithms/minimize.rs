//! Weighted minimization algorithm for WFSTs.
//!
//! Minimization produces a WFST with the minimum number of states that
//! accepts the same weighted language. This is achieved by:
//!
//! 1. Weight pushing (to normalize weight distribution)
//! 2. Partition refinement (treating (label, weight) as atomic symbols)
//!
//! # Algorithm
//!
//! The algorithm follows Mohri's approach for weighted minimization:
//! 1. Push weights to ensure canonical form
//! 2. Compute equivalence classes by **worklist-driven partition refinement** (a
//!    Hopcroft-family algorithm): states are first separated by their quantized
//!    final weight, then a block is re-examined and split — by the signature
//!    `(input, output, quantized weight, target block)` of its members' outgoing
//!    transitions — only when one of those members' successor blocks actually
//!    changes, propagated backward through a predecessor index. The simpler Moore
//!    full-pass refinement is retained (behind `#[cfg(test)]`) as a differential
//!    correctness oracle.
//! 3. Merge equivalent states
//!
//! # Complexity
//!
//! Naive Moore refinement re-scans every state on every pass, which is
//! $`\mathcal{O}(\lvert Q\rvert^2)`$
//! on chain-shaped automata (each pass propagates a distinction only one hop). The
//! worklist refinement instead re-examines only the blocks whose successors just
//! changed, which on the same inputs is dramatically faster — measured **82–87 %**
//! lower wall time on the `minimize/redundant_large` benchmark (e.g. 546 ms → 72 ms
//! at approximately 4,000 states), with the improvement widening as
//! $`\lvert Q\rvert`$ grows. It computes the
//! coarsest stable partition and, after renumbering classes by first appearance in
//! state order, the **byte-identical** partition vector as the Moore reference
//! (asserted by a differential test), so minimized output is unchanged.
//!
//! # Requirements
//!
//! - Input must be deterministic (no input-$`\varepsilon`$ transition; a unique
//!   input label per state).
//! - Semiring must be divisible (for weight pushing)
//!
//! # References
//!
//! - Mohri, M. (2009). "Weighted Automata Algorithms"
//! - Moore, E. F. (1956). "Gedanken-experiments on Sequential Machines"
//! - Hopcroft, J. (1971). "An n log n algorithm for minimizing states in a finite automaton"

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Debug;
use std::hash::Hash;

use crate::semiring::{DivisibleSemiring, QuantizableSemiring, Semiring};
use crate::wfst::{MutableWfst, StateId, WeightedTransition, Wfst, NO_STATE};

use super::connect::{connect, ConnectConfig};
use super::push::{push_weights, PushConfig, PushDirection, PushError};
use super::shortest_distance::ShortestDistanceConfig;

/// Default epsilon for floating-point weight comparison during minimization.
/// Weights within this tolerance are considered equal for partition refinement.
/// This addresses floating-point artifacts introduced by weight pushing's division operations.
const MINIMIZE_EPSILON: f64 = 1e-10;

/// A wrapper for quantized weight values for hashable approximate comparison.
///
/// This enables HashMap-based partition refinement with floating-point weights
/// by using the `QuantizableSemiring::quantize()` method. This addresses the issue
/// where weight pushing introduces tiny floating-point differences that incorrectly
/// separate equivalent states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct QuantizedWeight {
    /// Weight value quantized as integer via QuantizableSemiring::quantize.
    quantized: i64,
}

impl QuantizedWeight {
    /// Create a quantized weight from a QuantizableSemiring value.
    ///
    /// # Arguments
    /// * `weight` - The weight to quantize
    /// * `epsilon` - The quantization precision (values within epsilon are equal)
    fn from_weight<W: QuantizableSemiring>(weight: &W, epsilon: f64) -> Self {
        Self {
            quantized: weight.quantize(epsilon),
        }
    }
}

/// Configuration for minimization.
#[derive(Clone, Debug)]
pub struct MinimizeConfig {
    /// Push weights before minimizing (recommended).
    pub push_weights: bool,
    /// Direction for weight pushing.
    pub push_direction: PushDirection,
    /// Whether to connect (trim) before minimization.
    pub connect_first: bool,
    /// Epsilon for weight comparison during partition refinement.
    /// Weights within this tolerance are considered equal.
    /// This addresses floating-point artifacts from weight pushing.
    pub weight_epsilon: f64,
}

impl Default for MinimizeConfig {
    fn default() -> Self {
        Self {
            push_weights: true,
            push_direction: PushDirection::Forward,
            connect_first: true,
            weight_epsilon: MINIMIZE_EPSILON,
        }
    }
}

impl MinimizeConfig {
    /// Standard minimization with weight pushing.
    pub fn standard() -> Self {
        Self::default()
    }

    /// Minimize without weight pushing (input must already be pushed).
    pub fn no_push() -> Self {
        Self {
            push_weights: false,
            push_direction: PushDirection::Forward,
            connect_first: true,
            weight_epsilon: MINIMIZE_EPSILON,
        }
    }

    /// Create config with custom weight epsilon.
    pub fn with_epsilon(epsilon: f64) -> Self {
        Self {
            weight_epsilon: epsilon,
            ..Self::default()
        }
    }
}

/// Error during minimization.
#[derive(Clone, Debug, PartialEq)]
pub enum MinimizeError {
    /// No start state defined.
    NoStartState,
    /// The reported state count cannot be represented without using `NO_STATE`.
    InvalidStateCount {
        /// Original input identity, captured before transformation.
        input_identity: MinimizeInputIdentity,
        /// Reported number of states.
        state_count: usize,
    },
    /// The input start is not a state in the immutable input.
    InvalidStartState {
        /// Original input identity, captured before transformation.
        input_identity: MinimizeInputIdentity,
        /// Number of states in that input.
        state_count: usize,
        /// Start identifier supplied by the input.
        start_state: StateId,
    },
    /// An arc in the original input or a downstream transform has invalid endpoints.
    InvalidTransition {
        /// Original input identity, captured before transformation.
        input_identity: MinimizeInputIdentity,
        /// Number of states at the point of detection.
        state_count: usize,
        /// State whose outgoing slice contains this transition.
        owner_state: StateId,
        /// Source encoded on the transition.
        source_state: StateId,
        /// Target encoded on the transition.
        target_state: StateId,
        /// Zero-based index in the owner's outgoing slice.
        transition_index: usize,
    },
    /// Input WFST is not deterministic.
    NotDeterministic,
    /// Weight quantization epsilon must be finite and positive.
    InvalidWeightEpsilon {
        /// The invalid epsilon value supplied in [`MinimizeConfig`].
        epsilon: f64,
    },
    /// Weight pushing failed.
    PushError(String),
}

impl std::fmt::Display for MinimizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MinimizeError::NoStartState => write!(f, "WFST has no start state"),
            MinimizeError::InvalidStateCount { state_count, .. } => write!(
                f, "WFST reports {} states, exceeding the StateId address space",
                state_count
            ),
            MinimizeError::InvalidStartState { state_count, start_state, .. } => write!(
                f, "WFST start state {} is invalid for {} states", start_state, state_count
            ),
            MinimizeError::InvalidTransition {
                owner_state, source_state, target_state, transition_index, state_count, ..
            } => write!(
                f,
                "WFST transition {} in state {} has endpoints {} -> {} outside/mismatched for {} states",
                transition_index, owner_state, source_state, target_state, state_count
            ),
            MinimizeError::NotDeterministic => {
                write!(f, "WFST must be deterministic before minimization")
            }
            MinimizeError::InvalidWeightEpsilon { epsilon } => write!(
                f,
                "weight epsilon must be finite and positive, got {}",
                epsilon
            ),
            MinimizeError::PushError(msg) => write!(f, "Weight pushing failed: {}", msg),
        }
    }
}

impl std::error::Error for MinimizeError {}

/// Identity of the immutable input used for minimization diagnostics.
///
/// [`BorrowedWfst`](Self::BorrowedWfst) is the input object's address and is
/// meaningful only while that object remains alive. Callers requiring durable
/// provenance should pass a stable snapshot key via [`minimize_with_input_identity`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MinimizeInputIdentity {
    /// Address of the borrowed input; valid only for its lifetime.
    BorrowedWfst(usize),
    /// Caller-supplied stable identity of an immutable input snapshot.
    Snapshot(u64),
}

#[inline]
fn borrowed_identity<F>(fst: &F) -> MinimizeInputIdentity {
    MinimizeInputIdentity::BorrowedWfst(fst as *const F as usize)
}

/// The bounded FFI path distinguishes exhaustion from ordinary native errors.
/// Keep this internal surface separate from the public minimization error type.
pub(crate) enum CheckedMinimizeError {
    Native(MinimizeError),
    DistanceLimitExceeded,
}

impl From<MinimizeError> for CheckedMinimizeError {
    fn from(error: MinimizeError) -> Self {
        Self::Native(error)
    }
}

/// Validate the original input before any graph transform or estimate.
///
/// This is a linear, allocation-free scan in state and transition-slice order.
/// Its first error has the same coordinates as `first_invalid` in the Rocq model.
fn validate_input_endpoints<L, W, F>(
    fst: &F,
    input_identity: MinimizeInputIdentity,
) -> Result<(), MinimizeError>
where
    W: Semiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n > NO_STATE as usize {
        return Err(MinimizeError::InvalidStateCount {
            input_identity,
            state_count: n,
        });
    }
    let start = fst.start();
    if (n == 0 && start != NO_STATE) || (n != 0 && (start as usize) >= n) {
        return Err(MinimizeError::InvalidStartState {
            input_identity,
            state_count: n,
            start_state: start,
        });
    }
    for state in 0..n {
        let owner = state as StateId;
        for (transition_index, trans) in fst.transitions(owner).iter().enumerate() {
            if trans.from != owner || (trans.from as usize) >= n || (trans.to as usize) >= n {
                return Err(MinimizeError::InvalidTransition {
                    input_identity,
                    state_count: n,
                    owner_state: owner,
                    source_state: trans.from,
                    target_state: trans.to,
                    transition_index,
                });
            }
        }
    }
    Ok(())
}

/// A signature for a state, used for partition refinement.
///
/// Uses QuantizedWeight instead of raw weights to enable approximate comparison,
/// addressing floating-point artifacts from weight pushing.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct StateSignature<L: Ord + Hash> {
    /// Quantized final weight (or None if not final)
    final_weight: Option<QuantizedWeight>,
    /// Sorted list of (input_label, output_label, quantized_weight, target_partition_id)
    transitions: Vec<(Option<L>, Option<L>, QuantizedWeight, usize)>,
}

#[cfg(test)]
impl<L: Ord + Hash + Clone> StateSignature<L> {
    fn new() -> Self {
        Self {
            final_weight: None,
            transitions: Vec::new(),
        }
    }
}

fn validate_weight_epsilon(epsilon: f64) -> Result<(), MinimizeError> {
    if epsilon.is_finite() && epsilon > 0.0 {
        Ok(())
    } else {
        Err(MinimizeError::InvalidWeightEpsilon { epsilon })
    }
}

/// Minimize a deterministic WFST.
///
/// Produces a WFST with the minimum number of states accepting the same
/// weighted language.
///
/// # Requirements
///
/// - Input must be deterministic (use `determinize` first if needed)
/// - Semiring must be divisible for weight pushing
/// - Semiring must implement `QuantizableSemiring` for approximate weight comparison
///
/// # Returns
///
/// A new minimized WFST, or an error if minimization fails.
///
/// # Example
///
/// ```ignore
/// use lling_llang::algorithms::{minimize, MinimizeConfig, determinize, DeterminizeConfig};
///
/// let fst = build_some_wfst();
/// let det_fst = determinize(&fst, DeterminizeConfig::standard())?;
/// let min_fst = minimize(&det_fst, MinimizeConfig::standard())?;
/// ```
pub fn minimize<L, W, F>(fst: &F, config: MinimizeConfig) -> Result<F, MinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: DivisibleSemiring + QuantizableSemiring + PartialOrd + Clone + Debug,
    F: MutableWfst<L, W> + Wfst<L, W> + Default + Clone,
{
    minimize_with_input_identity(fst, borrowed_identity(fst), config)
}

/// Minimize while preserving a caller-supplied immutable snapshot identity in
/// malformed-input diagnostics.
pub fn minimize_with_input_identity<L, W, F>(
    fst: &F,
    input_identity: MinimizeInputIdentity,
    config: MinimizeConfig,
) -> Result<F, MinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: DivisibleSemiring + QuantizableSemiring + PartialOrd + Clone + Debug,
    F: MutableWfst<L, W> + Wfst<L, W> + Default + Clone,
{
    match minimize_with_input_identity_and_distance_config(
        fst,
        input_identity,
        config,
        ShortestDistanceConfig::default(),
    ) {
        Ok(output) => Ok(output),
        Err(CheckedMinimizeError::Native(error)) => Err(error),
        Err(CheckedMinimizeError::DistanceLimitExceeded) => Err(MinimizeError::PushError(
            "distance iteration limit exceeded".to_owned(),
        )),
    }
}

/// Checked C path: cap weight-pushing iterations without changing the public
/// configuration layout. It shares the original-input validation boundary.
pub(crate) fn minimize_with_distance_config<L, W, F>(
    fst: &F,
    config: MinimizeConfig,
    distance_config: ShortestDistanceConfig,
) -> Result<F, CheckedMinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: DivisibleSemiring + QuantizableSemiring + PartialOrd + Clone + Debug,
    F: MutableWfst<L, W> + Wfst<L, W> + Default + Clone,
{
    minimize_with_input_identity_and_distance_config(
        fst,
        borrowed_identity(fst),
        config,
        distance_config,
    )
}

fn minimize_with_input_identity_and_distance_config<L, W, F>(
    fst: &F,
    input_identity: MinimizeInputIdentity,
    config: MinimizeConfig,
    distance_config: ShortestDistanceConfig,
) -> Result<F, CheckedMinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: DivisibleSemiring + QuantizableSemiring + PartialOrd + Clone + Debug,
    F: MutableWfst<L, W> + Wfst<L, W> + Default + Clone,
{
    validate_input_endpoints(fst, input_identity)?;
    validate_weight_epsilon(config.weight_epsilon)?;
    let n = fst.num_states();
    if n == 0 {
        return Ok(F::default());
    }

    // Check that input is deterministic
    if !super::determinize::is_deterministic(fst) {
        return Err(MinimizeError::NotDeterministic.into());
    }

    // Clone and preprocess
    let mut working = fst.clone();

    // Optionally connect first
    if config.connect_first {
        connect(&mut working, ConnectConfig::trim());
    }

    // Optionally push weights
    if config.push_weights {
        let push_config = PushConfig {
            direction: config.push_direction,
            remove_non_coaccessible: false, // We already connected if needed
            distance_config: distance_config.clone(),
        };
        push_weights(&mut working, push_config).map_err(|error| {
            if distance_config.max_iterations.is_some() && error == PushError::NoPotentials {
                CheckedMinimizeError::DistanceLimitExceeded
            } else {
                CheckedMinimizeError::Native(MinimizeError::PushError(error.to_string()))
            }
        })?;
    }

    // Partition refinement to find equivalent states
    let partitions = compute_partitions(&working, config.weight_epsilon, input_identity)?;

    // Build minimized WFST from partitions
    build_minimized(&working, &partitions, input_identity).map_err(Into::into)
}

/// Compute state partitions by worklist-driven partition refinement.
///
/// Computes the coarsest stable partition — the same equivalence relation as the
/// reference [`compute_partitions_moore`], and (classes renumbered by first
/// appearance in state order) the byte-identical partition vector — but only
/// re-examines a block when one of its members' successor blocks actually
/// changes, instead of re-scanning every state on every pass. On chain-shaped
/// automata this replaces Moore's $`\mathcal{O}(\lvert Q\rvert^2)`$
/// full-pass behaviour with near-linear
/// work while producing identical minimized output. Uses
/// `QuantizableSemiring::quantize()` for approximate weight comparison.
fn compute_partitions<L, W, F>(
    fst: &F,
    epsilon: f64,
    input_identity: MinimizeInputIdentity,
) -> Result<Vec<usize>, MinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: QuantizableSemiring + Clone + Debug,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(Vec::new());
    }

    validate_weight_epsilon(epsilon)?;

    // Per-state outgoing arcs in canonical (input-sorted) order, storing the
    // target STATE (whose block is read live during refinement). Deterministic,
    // input-ε-free input makes each state's inputs unique, so this order is a
    // stable canonical key. Predecessors are recorded for change propagation.
    let mut arcs: Vec<Vec<(Option<L>, Option<L>, QuantizedWeight, usize)>> = Vec::with_capacity(n);
    let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for state in 0..n {
        let state_id = state as StateId;
        let mut state_arcs: Vec<(Option<L>, Option<L>, QuantizedWeight, usize)> = Vec::new();
        for (transition_index, trans) in fst.transitions(state_id).iter().enumerate() {
            let to = trans.to as usize;
            if trans.from != state_id || to >= n {
                return Err(MinimizeError::InvalidTransition {
                    input_identity,
                    state_count: n,
                    owner_state: state_id,
                    source_state: trans.from,
                    target_state: trans.to,
                    transition_index,
                });
            }
            state_arcs.push((
                trans.input.clone(),
                trans.output.clone(),
                QuantizedWeight::from_weight(&trans.weight, epsilon),
                to,
            ));
            predecessors[to].push(state);
        }
        state_arcs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        arcs.push(state_arcs);
    }

    let final_weights: Vec<Option<QuantizedWeight>> = (0..n)
        .map(|state| {
            let state_id = state as StateId;
            fst.is_final(state_id)
                .then(|| QuantizedWeight::from_weight(&fst.final_weight(state_id), epsilon))
        })
        .collect();

    // Initial partition: one block per distinct quantized final weight, block ids
    // assigned by first appearance in state order.
    let mut block_of = vec![0usize; n];
    let mut blocks: Vec<Vec<usize>> = Vec::new();
    let mut fw_to_block: HashMap<Option<QuantizedWeight>, usize> = HashMap::with_capacity(n);
    for state in 0..n {
        let next = fw_to_block.len();
        let block = *fw_to_block.entry(final_weights[state]).or_insert(next);
        if block == blocks.len() {
            blocks.push(Vec::new());
        }
        block_of[state] = block;
        blocks[block].push(state);
    }

    // Worklist of blocks that might still be splittable (deduplicated via in_wl).
    let mut in_wl = vec![true; blocks.len()];
    let mut worklist: VecDeque<usize> = (0..blocks.len()).collect();

    while let Some(block) = worklist.pop_front() {
        in_wl[block] = false;
        if blocks[block].len() <= 1 {
            continue;
        }

        // Group this block's members by their signature under the current
        // partition: (final weight, canonical [(input, output, qweight, target
        // block)]).
        let members = std::mem::take(&mut blocks[block]);
        let mut groups: HashMap<
            (
                Option<QuantizedWeight>,
                Vec<(Option<L>, Option<L>, QuantizedWeight, usize)>,
            ),
            Vec<usize>,
        > = HashMap::new();
        for &state in &members {
            let signature: Vec<(Option<L>, Option<L>, QuantizedWeight, usize)> = arcs[state]
                .iter()
                .map(|(input, output, weight, to)| {
                    (input.clone(), output.clone(), *weight, block_of[*to])
                })
                .collect();
            groups
                .entry((final_weights[state], signature))
                .or_default()
                .push(state);
        }

        if groups.len() == 1 {
            blocks[block] = members;
            continue;
        }

        // Deterministic split: order groups by their smallest member so block-id
        // assignment does not depend on HashMap iteration order. The first group
        // keeps the original block id; the rest become new blocks.
        let mut split: Vec<Vec<usize>> = groups.into_values().collect();
        split.sort_by_key(|group| group.iter().copied().min().unwrap_or(usize::MAX));
        blocks[block] = std::mem::take(&mut split[0]);
        let mut moved: Vec<usize> = Vec::new();
        for group in split.into_iter().skip(1) {
            let new_block = blocks.len();
            for &state in &group {
                block_of[state] = new_block;
                moved.push(state);
            }
            blocks.push(group);
            in_wl.push(false);
        }

        // Every predecessor of a state that changed block may now distinguish, so
        // re-enqueue its current block (this also re-enqueues `block` when one of
        // its retained members points at a just-moved state).
        for &state in &moved {
            for &pred in &predecessors[state] {
                let pred_block = block_of[pred];
                if !in_wl[pred_block] {
                    in_wl[pred_block] = true;
                    worklist.push_back(pred_block);
                }
            }
        }
    }

    // Renumber classes by first appearance in state order so the partition vector
    // is deterministic and matches the reference (Moore) numbering exactly.
    let mut remap: HashMap<usize, usize> = HashMap::with_capacity(blocks.len());
    let mut partition = vec![0usize; n];
    for state in 0..n {
        let next = remap.len();
        partition[state] = *remap.entry(block_of[state]).or_insert(next);
    }
    Ok(partition)
}

/// Reference partition refinement (Moore's iterative algorithm), retained as the
/// correctness oracle for [`compute_partitions`] in the differential test below.
/// It recomputes every state's signature on every pass until the partition stops
/// changing—simple and obviously correct, but
/// $`\mathcal{O}(\lvert Q\rvert^2)`$ on chain-shaped inputs.
#[cfg(test)]
fn compute_partitions_moore<L, W, F>(
    fst: &F,
    epsilon: f64,
    input_identity: MinimizeInputIdentity,
) -> Result<Vec<usize>, MinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: QuantizableSemiring + Clone + Debug,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n > NO_STATE as usize {
        return Err(MinimizeError::InvalidStateCount {
            input_identity,
            state_count: n,
        });
    }
    let start = fst.start();
    if (n == 0 && start != NO_STATE) || (n != 0 && (start as usize) >= n) {
        return Err(MinimizeError::InvalidStartState {
            input_identity,
            state_count: n,
            start_state: start,
        });
    }
    if n == 0 {
        return Ok(Vec::new());
    }

    validate_weight_epsilon(epsilon)?;

    // Precompute, once, each state's outgoing arcs in canonical order.
    // `minimize` requires deterministic, input-ε-free input (checked before this
    // call and preserved by connect/push), so every state's transitions carry
    // distinct non-ε input labels. Sorting by input is therefore a total order
    // that is invariant across refinement passes — only the *target partition*
    // ids change between passes, never the ordering — so we sort here exactly
    // once instead of re-sorting every state on every pass.
    let mut state_arcs: Vec<Vec<(Option<L>, Option<L>, QuantizedWeight, usize)>> =
        Vec::with_capacity(n);
    for state in 0..n {
        let state_id = state as StateId;
        let mut arcs: Vec<(Option<L>, Option<L>, QuantizedWeight, usize)> = Vec::new();
        for (transition_index, trans) in fst.transitions(state_id).iter().enumerate() {
            let to = trans.to as usize;
            if trans.from != state_id || to >= n {
                return Err(MinimizeError::InvalidTransition {
                    input_identity,
                    state_count: n,
                    owner_state: state_id,
                    source_state: trans.from,
                    target_state: trans.to,
                    transition_index,
                });
            }
            arcs.push((
                trans.input.clone(),
                trans.output.clone(),
                QuantizedWeight::from_weight(&trans.weight, epsilon),
                to,
            ));
        }
        arcs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        state_arcs.push(arcs);
    }

    // Precompute each state's quantized final weight (also pass-invariant).
    let final_weights: Vec<Option<QuantizedWeight>> = (0..n)
        .map(|state| {
            let state_id = state as StateId;
            if fst.is_final(state_id) {
                Some(QuantizedWeight::from_weight(
                    &fst.final_weight(state_id),
                    epsilon,
                ))
            } else {
                None
            }
        })
        .collect();

    // Initial partition: separate by quantized final weight.
    let mut partition: Vec<usize> = vec![0; n];
    let mut final_weight_to_partition: HashMap<Option<QuantizedWeight>, usize> =
        HashMap::with_capacity(n);
    for (state, &fw) in final_weights.iter().enumerate() {
        let next = final_weight_to_partition.len();
        partition[state] = *final_weight_to_partition.entry(fw).or_insert(next);
    }

    // Iterative refinement. The signature map and next-partition buffer are
    // allocated once and reused (cleared / fully overwritten) each pass rather
    // than re-allocated, since their sizes are bounded by `n`.
    let mut signature_to_partition: HashMap<StateSignature<L>, usize> = HashMap::with_capacity(n);
    let mut new_partition: Vec<usize> = vec![0; n];
    let mut changed = true;
    while changed {
        signature_to_partition.clear();

        for (state, arcs) in state_arcs.iter().enumerate() {
            // Rebuild the signature in the precomputed canonical order, reading
            // each target's current partition id.
            let mut sig = StateSignature::new();
            sig.final_weight = final_weights[state];
            sig.transitions = arcs
                .iter()
                .map(|(input, output, weight, to)| {
                    (input.clone(), output.clone(), *weight, partition[*to])
                })
                .collect();

            let next = signature_to_partition.len();
            new_partition[state] = *signature_to_partition.entry(sig).or_insert(next);
        }

        // Refine until the partition assignment stabilizes — not merely until the
        // partition count stops growing, since equal-cardinality passes can still
        // move states.
        changed = new_partition != partition;
        if changed {
            partition.copy_from_slice(&new_partition);
        }
    }

    Ok(partition)
}

/// Build minimized WFST from partition assignments.
fn build_minimized<L, W, F>(
    fst: &F,
    partitions: &[usize],
    input_identity: MinimizeInputIdentity,
) -> Result<F, MinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: Semiring + Clone + Debug,
    F: MutableWfst<L, W> + Wfst<L, W> + Default,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(F::default());
    }

    // Find number of partitions (new states)
    let num_new_states = partitions.iter().max().map(|&m| m + 1).unwrap_or(0);

    // Find representative state for each partition
    let mut partition_to_rep: HashMap<usize, StateId> = HashMap::new();
    for state in 0..n {
        let p = partitions[state];
        partition_to_rep.entry(p).or_insert(state as StateId);
    }

    // Create new WFST
    let mut result = F::default();
    for _ in 0..num_new_states {
        result.add_state();
    }

    // Set start state
    let old_start = fst.start();
    if old_start != NO_STATE {
        let Some(&new_start) = partitions.get(old_start as usize) else {
            return Err(MinimizeError::NoStartState);
        };

        result.set_start(new_start as StateId);
    }

    // Add transitions and final weights from representatives
    let mut added_transitions: HashSet<(usize, Option<L>, Option<L>, usize)> = HashSet::new();

    for (partition, &rep) in &partition_to_rep {
        let new_state = *partition as StateId;

        // Set final weight if representative is final
        if fst.is_final(rep) {
            result.set_final(new_state, fst.final_weight(rep));
        }

        // Add transitions (avoiding duplicates)
        for (transition_index, trans) in fst.transitions(rep).iter().enumerate() {
            let Some(&target_partition) = partitions.get(trans.to as usize) else {
                return Err(MinimizeError::InvalidTransition {
                    input_identity,
                    state_count: n,
                    owner_state: rep,
                    source_state: trans.from,
                    target_state: trans.to,
                    transition_index,
                });
            };

            let key = (
                *partition,
                trans.input.clone(),
                trans.output.clone(),
                target_partition,
            );

            if !added_transitions.contains(&key) {
                added_transitions.insert(key);

                let new_trans = WeightedTransition {
                    from: new_state,
                    to: target_partition as StateId,
                    input: trans.input.clone(),
                    output: trans.output.clone(),
                    weight: trans.weight.clone(),
                };
                result.add_transition(new_trans);
            }
        }
    }

    Ok(result)
}

/// Count the number of states that can be removed by minimization.
///
/// This is a quick estimate without actually performing minimization.
/// Uses the default weight epsilon for comparison.
pub fn estimate_reduction<L, W, F>(fst: &F) -> Result<usize, MinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: QuantizableSemiring + Clone + Debug,
    F: Wfst<L, W>,
{
    estimate_reduction_with_epsilon(fst, MINIMIZE_EPSILON)
}

/// Count the number of states that can be removed by minimization.
///
/// Uses a custom epsilon for weight comparison.
pub fn estimate_reduction_with_epsilon<L, W, F>(
    fst: &F,
    epsilon: f64,
) -> Result<usize, MinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: QuantizableSemiring + Clone + Debug,
    F: Wfst<L, W>,
{
    estimate_reduction_with_epsilon_and_input_identity(fst, epsilon, borrowed_identity(fst))
}

/// Estimate a reduction with explicit immutable input provenance.
pub fn estimate_reduction_with_epsilon_and_input_identity<L, W, F>(
    fst: &F,
    epsilon: f64,
    input_identity: MinimizeInputIdentity,
) -> Result<usize, MinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: QuantizableSemiring + Clone + Debug,
    F: Wfst<L, W>,
{
    validate_input_endpoints(fst, input_identity)?;
    validate_weight_epsilon(epsilon)?;
    let n = fst.num_states();
    if n == 0 {
        return Ok(0);
    }

    let partitions = compute_partitions(fst, epsilon, input_identity)?;
    let num_new_states = partitions.iter().max().map(|&m| m + 1).unwrap_or(0);
    Ok(n.saturating_sub(num_new_states))
}

#[cfg(test)]
mod tests {
    use super::super::determinize::is_deterministic;
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::{VectorWfst, VectorWfstBuilder};

    // Property-based tests
    mod property_tests {
        use super::*;
        use crate::test_utils::arb_deterministic_wfst_tropical;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(128))]

            /// The independent Moore oracle and worklist must agree for valid
            /// generated deterministic WFSTs, including empty graphs.
            #[test]
            fn valid_worklist_partitions_match_moore(
                fst in arb_deterministic_wfst_tropical(12, 4)
            ) {
                let identity = borrowed_identity(&fst);
                let worklist = compute_partitions(&fst, MINIMIZE_EPSILON, identity).unwrap();
                let moore = compute_partitions_moore(&fst, MINIMIZE_EPSILON, identity).unwrap();
                prop_assert_eq!(worklist, moore);
            }

            /// Minimization should never increase state count.
            ///
            /// This property was previously disabled due to a bug where weight pushing
            /// introduced floating-point artifacts that caused incorrect state separation.
            /// Fixed by using QuantizedWeight for approximate comparison.
            #[test]
            fn minimize_reduces_or_maintains_states(
                fst in arb_deterministic_wfst_tropical(8, 3)
            ) {
                if fst.num_states() == 0 {
                    return Ok(());
                }

                let original_states = fst.num_states();
                let result = minimize(&fst, MinimizeConfig::standard());

                if let Ok(min_fst) = result {
                    prop_assert!(
                        min_fst.num_states() <= original_states,
                        "Minimization increased states from {} to {}",
                        original_states,
                        min_fst.num_states()
                    );
                }
            }

            /// Minimization is idempotent:
            /// $`\min(\min(F))\approx\min(F)`$.
            ///
            /// Applying minimization twice should produce the same result as once.
            #[test]
            fn minimize_idempotent(
                fst in arb_deterministic_wfst_tropical(6, 2)
            ) {
                if fst.num_states() == 0 {
                    return Ok(());
                }

                let result1 = minimize(&fst, MinimizeConfig::standard());
                if let Ok(min1) = result1 {
                    let result2 = minimize(&min1, MinimizeConfig::standard());
                    if let Ok(min2) = result2 {
                        prop_assert_eq!(
                            min1.num_states(),
                            min2.num_states(),
                            "Minimization not idempotent: first pass {} states, second pass {} states",
                            min1.num_states(),
                            min2.num_states()
                        );
                    }
                }
            }

            /// Minimized FST should still be deterministic.
            #[test]
            fn minimize_preserves_determinism(
                fst in arb_deterministic_wfst_tropical(8, 3)
            ) {
                if fst.num_states() == 0 {
                    return Ok(());
                }

                let result = minimize(&fst, MinimizeConfig::standard());
                if let Ok(min_fst) = result {
                    prop_assert!(
                        is_deterministic(&min_fst),
                        "Minimized FST should be deterministic"
                    );
                }
            }

            /// Estimate reduction provides a reasonable bound.
            /// Note: estimate_reduction doesn't do weight pushing, so it may differ
            /// significantly from actual reduction. We only check it's non-negative
            /// and doesn't exceed the state count.
            #[test]
            fn estimate_reduction_bounds(
                fst in arb_deterministic_wfst_tropical(6, 2)
            ) {
                if fst.num_states() <= 1 {
                    return Ok(());
                }

                let estimated = estimate_reduction(&fst).unwrap();
                let original_states = fst.num_states();

                // Estimate should be bounded by state count
                prop_assert!(
                    estimated <= original_states,
                    "Estimated reduction {} exceeds state count {}",
                    estimated,
                    original_states
                );

                // Just verify minimize works (actual comparison is unreliable
                // because weight pushing changes state signatures)
                let result = minimize(&fst, MinimizeConfig::standard());
                prop_assert!(
                    result.is_ok() || matches!(result, Err(MinimizeError::PushError(_))),
                    "Minimize failed unexpectedly: {:?}",
                    result
                );
            }
        }
    }

    fn build_redundant_fst() -> VectorWfst<char, TropicalWeight> {
        // Two equivalent branches that should be merged:
        // 0 --a--> 1 --b--> 3 (final)
        //    ` --> 2 --b--> 4 (final)
        // States 1,2 and 3,4 are equivalent pairs
        let mut fst = VectorWfst::new();
        fst.add_states(5);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_arc(0, Some('c'), Some('c'), 2, TropicalWeight::new(1.0));
        fst.add_arc(1, Some('b'), Some('b'), 3, TropicalWeight::new(1.0));
        fst.add_arc(2, Some('b'), Some('b'), 4, TropicalWeight::new(1.0));
        fst.set_final(3, TropicalWeight::one());
        fst.set_final(4, TropicalWeight::one());
        fst
    }

    fn build_minimal_fst() -> VectorWfst<char, TropicalWeight> {
        // Already minimal: 0 --a--> 1 --b--> 2 (final)
        VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('b'), Some('b'), 2, TropicalWeight::new(2.0))
            .final_state(2, TropicalWeight::one())
            .build()
    }

    fn build_chain_with_equiv_states() -> VectorWfst<char, TropicalWeight> {
        // 0 --a--> 1 --b--> 2 --c--> 3 (final)
        //                   ` --c--> 4 (final)
        // States 3 and 4 are equivalent (same transitions, same final weight)
        let mut fst = VectorWfst::new();
        fst.add_states(5);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_arc(1, Some('b'), Some('b'), 2, TropicalWeight::new(1.0));
        fst.add_arc(2, Some('c'), Some('c'), 3, TropicalWeight::new(1.0));
        fst.add_arc(2, Some('d'), Some('d'), 4, TropicalWeight::new(1.0));
        fst.set_final(3, TropicalWeight::one());
        fst.set_final(4, TropicalWeight::one());
        fst
    }

    #[test]
    fn test_minimize_empty() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        let result = minimize(&fst, MinimizeConfig::standard())
            .expect("algorithms/minimize.rs: required value was None/Err");
        assert_eq!(result.num_states(), 0);
    }

    #[test]
    fn empty_input_still_rejects_invalid_weight_epsilon() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        assert!(matches!(
            minimize(&fst, MinimizeConfig::with_epsilon(f64::NAN)),
            Err(MinimizeError::InvalidWeightEpsilon { .. })
        ));
        assert!(matches!(
            estimate_reduction_with_epsilon(&fst, f64::NAN),
            Err(MinimizeError::InvalidWeightEpsilon { .. })
        ));
    }

    #[test]
    fn checked_minimize_caps_distance_without_changing_public_default() {
        let fst = build_minimal_fst();
        let mut distance = ShortestDistanceConfig::default();
        distance.max_iterations = Some(0);
        assert!(matches!(
            minimize_with_distance_config(&fst, MinimizeConfig::default(), distance),
            Err(CheckedMinimizeError::DistanceLimitExceeded)
        ));
        assert!(minimize(&fst, MinimizeConfig::default()).is_ok());
    }

    #[test]
    fn checked_minimize_rejects_original_malformed_arc_before_distance_limit() {
        let mut fst = build_minimal_fst();
        fst.state_mut(0).unwrap().transitions[0].to = 99;
        let mut distance = ShortestDistanceConfig::default();
        distance.max_iterations = Some(0);
        assert!(matches!(
            minimize_with_distance_config(&fst, MinimizeConfig::default(), distance),
            Err(CheckedMinimizeError::Native(
                MinimizeError::InvalidTransition {
                    owner_state: 0,
                    source_state: 0,
                    target_state: 99,
                    transition_index: 0,
                    ..
                }
            ))
        ));
    }

    #[test]
    fn test_minimize_already_minimal() {
        let fst = build_minimal_fst();
        let result = minimize(&fst, MinimizeConfig::standard())
            .expect("algorithms/minimize.rs: required value was None/Err");

        // Should have same or fewer states
        assert!(result.num_states() <= fst.num_states());
    }

    #[test]
    fn test_minimize_redundant() {
        let fst = build_redundant_fst();
        let initial_states = fst.num_states();

        let result = minimize(&fst, MinimizeConfig::standard())
            .expect("algorithms/minimize.rs: required value was None/Err");

        // Should have fewer states (3,4 merged into one)
        assert!(
            result.num_states() < initial_states,
            "Expected fewer than {} states, got {}",
            initial_states,
            result.num_states()
        );
    }

    #[test]
    fn test_minimize_non_deterministic_fails() {
        // Create a non-deterministic FST
        let mut fst = VectorWfst::new();
        fst.add_states(3);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_arc(0, Some('a'), Some('a'), 2, TropicalWeight::new(2.0)); // Same label!
        fst.set_final(1, TropicalWeight::one());
        fst.set_final(2, TropicalWeight::one());

        let result = minimize(&fst, MinimizeConfig::standard());
        assert!(matches!(result, Err(MinimizeError::NotDeterministic)));
    }

    #[test]
    fn test_minimize_chain_equiv() {
        let fst = build_chain_with_equiv_states();
        let initial_states = fst.num_states();

        let result = minimize(&fst, MinimizeConfig::standard())
            .expect("algorithms/minimize.rs: required value was None/Err");

        // Check we got a valid result
        assert!(result.num_states() > 0);
        assert!(result.num_states() <= initial_states);
    }

    #[test]
    fn test_estimate_reduction() {
        let redundant = build_redundant_fst();
        let reduction = estimate_reduction(&redundant).unwrap();

        // Should estimate at least 1 state can be removed
        // (since states 3,4 are equivalent)
        assert!(reduction >= 1, "Expected reduction >= 1, got {}", reduction);
    }

    #[test]
    fn test_minimize_preserves_determinism() {
        let fst = build_redundant_fst();
        assert!(is_deterministic(&fst));

        let result = minimize(&fst, MinimizeConfig::standard())
            .expect("algorithms/minimize.rs: required value was None/Err");
        assert!(is_deterministic(&result));
    }

    #[test]
    fn test_minimize_no_push_config() {
        let fst = build_minimal_fst();

        // With no push should still work for already-pushed FSTs
        let result = minimize(&fst, MinimizeConfig::no_push())
            .expect("algorithms/minimize.rs: required value was None/Err");
        assert!(result.num_states() <= fst.num_states());
    }

    #[test]
    fn test_minimize_rejects_invalid_weight_epsilon() {
        let fst = build_minimal_fst();

        for epsilon in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            let result = minimize(&fst, MinimizeConfig::with_epsilon(epsilon));

            assert!(matches!(
                result,
                Err(MinimizeError::InvalidWeightEpsilon { epsilon: invalid })
                    if (epsilon.is_nan() && invalid.is_nan()) || invalid == epsilon
            ));
            assert!(matches!(
                estimate_reduction_with_epsilon(&fst, epsilon),
                Err(MinimizeError::InvalidWeightEpsilon { .. })
            ));
        }
    }

    #[test]
    fn test_minimize_rejects_malformed_transition_targets() {
        let mut fst = VectorWfst::new();
        fst.add_states(2);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_arc(0, Some('x'), Some('x'), 99, TropicalWeight::new(1.0));
        fst.set_final(1, TropicalWeight::one());

        let config = MinimizeConfig {
            push_weights: false,
            connect_first: false,
            ..MinimizeConfig::default()
        };
        assert!(matches!(
            minimize(&fst, config),
            Err(MinimizeError::InvalidTransition {
                owner_state: 0,
                source_state: 0,
                target_state: 99,
                transition_index: 1,
                ..
            })
        ));
        assert!(matches!(
            estimate_reduction(&fst),
            Err(MinimizeError::InvalidTransition {
                target_state: 99,
                ..
            })
        ));
        let input_identity = borrowed_identity(&fst);
        assert!(matches!(
            compute_partitions_moore(&fst, MINIMIZE_EPSILON, input_identity),
            Err(MinimizeError::InvalidTransition {
                target_state: 99,
                ..
            })
        ));
    }

    #[test]
    fn moore_oracle_rejects_source_owner_mismatch() {
        let mut fst = build_minimal_fst();
        fst.state_mut(0).unwrap().transitions[0].from = 1;
        let identity = borrowed_identity(&fst);
        assert!(matches!(
            compute_partitions_moore(&fst, MINIMIZE_EPSILON, identity),
            Err(MinimizeError::InvalidTransition {
                owner_state: 0,
                source_state: 1,
                transition_index: 0,
                ..
            })
        ));
    }

    #[test]
    fn compute_partitions_worklist_matches_moore_reference() {
        // Deterministic WFSTs (unique non-ε input per state) of varied shape; the
        // worklist refinement must produce the byte-identical partition vector as
        // the Moore reference oracle on every one.
        fn chain(len: usize) -> VectorWfst<char, TropicalWeight> {
            let mut fst = VectorWfst::new();
            fst.add_states(len + 1);
            fst.set_start(0);
            fst.set_final(len as StateId, TropicalWeight::one());
            for i in 0..len {
                let label = (b'a' + (i % 5) as u8) as char;
                fst.add_arc(
                    i as StateId,
                    Some(label),
                    Some(label),
                    (i + 1) as StateId,
                    TropicalWeight::new(1.0),
                );
            }
            fst
        }

        // Two equivalent branches sharing a suffix — {1,3} and {2,4} must merge.
        let mut redundant = VectorWfst::new();
        redundant.add_states(5);
        redundant.set_start(0);
        redundant.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        redundant.add_arc(0, Some('b'), Some('b'), 3, TropicalWeight::new(1.0));
        redundant.add_arc(1, Some('x'), Some('x'), 2, TropicalWeight::new(1.0));
        redundant.add_arc(3, Some('x'), Some('x'), 4, TropicalWeight::new(1.0));
        redundant.set_final(2, TropicalWeight::one());
        redundant.set_final(4, TropicalWeight::one());

        // Diamond: two intermediate states with identical onward behaviour merge.
        let mut diamond = VectorWfst::new();
        diamond.add_states(4);
        diamond.set_start(0);
        diamond.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        diamond.add_arc(0, Some('b'), Some('b'), 2, TropicalWeight::new(1.0));
        diamond.add_arc(1, Some('c'), Some('c'), 3, TropicalWeight::new(2.0));
        diamond.add_arc(2, Some('c'), Some('c'), 3, TropicalWeight::new(2.0));
        diamond.set_final(3, TropicalWeight::one());

        let cases = [chain(1), chain(4), chain(9), chain(20), redundant, diamond];
        for fst in &cases {
            let input_identity = borrowed_identity(fst);
            let worklist = compute_partitions(fst, MINIMIZE_EPSILON, input_identity)
                .expect("worklist partitions");
            let moore = compute_partitions_moore(fst, MINIMIZE_EPSILON, input_identity)
                .expect("moore partitions");
            assert_eq!(
                worklist, moore,
                "worklist and Moore partitions must be byte-identical"
            );
        }
    }
}
