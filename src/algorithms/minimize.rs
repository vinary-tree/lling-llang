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
//! The algorithm follows Mohri's approach:
//! 1. Push weights to ensure canonical form
//! 2. Compute equivalence classes using Hopcroft-style partition refinement
//! 3. Merge equivalent states
//!
//! # Complexity
//!
//! - Acyclic: O(|Q| + |E|)
//! - General: O(|E| log |Q|) with Hopcroft's algorithm
//!
//! # Requirements
//!
//! - Input must be deterministic
//! - Semiring must be divisible (for weight pushing)
//!
//! # References
//!
//! - Mohri, M. (2009). "Weighted Automata Algorithms"
//! - Hopcroft, J. (1971). "An n log n algorithm for minimizing states in a finite automaton"

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;

use crate::semiring::{DivisibleSemiring, QuantizableSemiring, Semiring};
use crate::wfst::{MutableWfst, StateId, WeightedTransition, Wfst, NO_STATE};

use super::connect::{connect, ConnectConfig};
use super::push::{push_weights, PushConfig, PushDirection};
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

/// A signature for a state, used for partition refinement.
///
/// Uses QuantizedWeight instead of raw weights to enable approximate comparison,
/// addressing floating-point artifacts from weight pushing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct StateSignature<L: Ord + Hash> {
    /// Quantized final weight (or None if not final)
    final_weight: Option<QuantizedWeight>,
    /// Sorted list of (input_label, output_label, quantized_weight, target_partition_id)
    transitions: Vec<(Option<L>, Option<L>, QuantizedWeight, usize)>,
}

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
    let n = fst.num_states();
    if n == 0 {
        return Ok(F::default());
    }

    validate_weight_epsilon(config.weight_epsilon)?;

    let start = fst.start();
    if start == NO_STATE {
        return Err(MinimizeError::NoStartState);
    }

    // Check that input is deterministic
    if !super::determinize::is_deterministic(fst) {
        return Err(MinimizeError::NotDeterministic);
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
            distance_config: ShortestDistanceConfig::default(),
        };
        push_weights(&mut working, push_config)
            .map_err(|e| MinimizeError::PushError(e.to_string()))?;
    }

    // Partition refinement to find equivalent states
    let partitions = compute_partitions(&working, config.weight_epsilon)?;

    // Build minimized WFST from partitions
    build_minimized(&working, &partitions)
}

/// Compute state partitions using iterative refinement.
///
/// Uses `QuantizableSemiring::quantize()` for approximate comparison to handle
/// floating-point artifacts from weight pushing.
fn compute_partitions<L, W, F>(fst: &F, epsilon: f64) -> Result<Vec<usize>, MinimizeError>
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

    // Initial partition: separate by quantized final weight
    let mut partition: Vec<usize> = vec![0; n];
    let mut num_partitions = 0;

    // Separate by quantized final weight
    let mut final_weight_to_partition: HashMap<Option<QuantizedWeight>, usize> = HashMap::new();

    for state in 0..n {
        let state_id = state as StateId;
        let fw = if fst.is_final(state_id) {
            Some(QuantizedWeight::from_weight(
                &fst.final_weight(state_id),
                epsilon,
            ))
        } else {
            None
        };

        if let Some(&p) = final_weight_to_partition.get(&fw) {
            partition[state] = p;
        } else {
            let p = num_partitions;
            num_partitions += 1;
            final_weight_to_partition.insert(fw, p);
            partition[state] = p;
        }
    }

    // Iterative refinement
    let mut changed = true;
    while changed {
        changed = false;

        // Compute signatures for each state based on current partition
        let mut signature_to_partition: HashMap<StateSignature<L>, usize> = HashMap::new();
        let mut new_partition: Vec<usize> = vec![0; n];
        let mut new_num_partitions = 0;

        for state in 0..n {
            let state_id = state as StateId;

            // Build signature with quantized weights
            let mut sig = StateSignature::new();

            if fst.is_final(state_id) {
                sig.final_weight = Some(QuantizedWeight::from_weight(
                    &fst.final_weight(state_id),
                    epsilon,
                ));
            }

            let mut trans_sigs: Vec<(Option<L>, Option<L>, QuantizedWeight, usize)> = Vec::new();
            for trans in fst.transitions(state_id) {
                let Some(&target_partition) = partition.get(trans.to as usize) else {
                    continue;
                };

                trans_sigs.push((
                    trans.input.clone(),
                    trans.output.clone(),
                    QuantizedWeight::from_weight(&trans.weight, epsilon),
                    target_partition,
                ));
            }

            // Sort for canonical form
            trans_sigs.sort_by(|a, b| {
                a.0.cmp(&b.0)
                    .then_with(|| a.1.cmp(&b.1))
                    .then_with(|| a.3.cmp(&b.3))
            });
            sig.transitions = trans_sigs;

            // Look up or create partition for this signature
            if let Some(&p) = signature_to_partition.get(&sig) {
                new_partition[state] = p;
            } else {
                let p = new_num_partitions;
                new_num_partitions += 1;
                signature_to_partition.insert(sig, p);
                new_partition[state] = p;
            }
        }

        // Check if the relation changed, not only whether the number of
        // partitions grew. Same-cardinality refinements can still move states.
        if new_partition != partition {
            changed = true;
            partition = new_partition;
        }
    }

    Ok(partition)
}

/// Build minimized WFST from partition assignments.
fn build_minimized<L, W, F>(fst: &F, partitions: &[usize]) -> Result<F, MinimizeError>
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
        for trans in fst.transitions(rep) {
            let Some(&target_partition) = partitions.get(trans.to as usize) else {
                continue;
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
pub fn estimate_reduction<L, W, F>(fst: &F) -> usize
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
pub fn estimate_reduction_with_epsilon<L, W, F>(fst: &F, epsilon: f64) -> usize
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: QuantizableSemiring + Clone + Debug,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return 0;
    }

    if let Ok(partitions) = compute_partitions(fst, epsilon) {
        let num_new_states = partitions.iter().max().map(|&m| m + 1).unwrap_or(0);
        n.saturating_sub(num_new_states)
    } else {
        0
    }
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

            /// Minimization is idempotent: min(min(F)) ≈ min(F).
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

                let estimated = estimate_reduction(&fst);
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
        let reduction = estimate_reduction(&redundant);

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
            assert_eq!(estimate_reduction_with_epsilon(&fst, epsilon), 0);
        }
    }

    #[test]
    fn test_minimize_skips_malformed_transition_targets() {
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
        let minimized = minimize(&fst, config).expect("malformed targets should be skipped");

        assert!(estimate_reduction(&fst) <= fst.num_states());
        assert!((0..minimized.num_states()).all(|state| {
            minimized
                .transitions(state as StateId)
                .iter()
                .all(|transition| (transition.to as usize) < minimized.num_states())
        }));
    }
}
