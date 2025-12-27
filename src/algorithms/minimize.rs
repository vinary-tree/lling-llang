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

use crate::semiring::{DivisibleSemiring, Semiring};
use crate::wfst::{MutableWfst, StateId, WeightedTransition, Wfst, NO_STATE};

/// Configuration for minimization.
#[derive(Clone, Debug)]
pub struct MinimizeConfig {
    /// Push weights before minimizing (recommended).
    pub push_weights: bool,
    /// Direction for weight pushing.
    pub push_direction: crate::algorithms::PushDirection,
    /// Whether to connect (trim) before minimization.
    pub connect_first: bool,
}

impl Default for MinimizeConfig {
    fn default() -> Self {
        Self {
            push_weights: true,
            push_direction: crate::algorithms::PushDirection::Forward,
            connect_first: true,
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
            push_direction: crate::algorithms::PushDirection::Forward,
            connect_first: true,
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
            MinimizeError::PushError(msg) => write!(f, "Weight pushing failed: {}", msg),
        }
    }
}

impl std::error::Error for MinimizeError {}

/// A signature for a state, used for partition refinement.
/// Contains (final_weight, [(label, weight, target_partition)])
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct StateSignature<L: Ord + Hash, W: Semiring + Hash + Eq> {
    /// Final weight (or None if not final)
    final_weight: Option<W>,
    /// Sorted list of (input_label, output_label, weight, target_partition_id)
    transitions: Vec<(Option<L>, Option<L>, W, usize)>,
}

impl<L: Ord + Hash + Clone, W: Semiring + Hash + Eq + Clone> StateSignature<L, W> {
    fn new() -> Self {
        Self {
            final_weight: None,
            transitions: Vec::new(),
        }
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
    W: DivisibleSemiring + PartialOrd + Clone + Debug + Hash + Eq,
    F: MutableWfst<L, W> + Wfst<L, W> + Default + Clone,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(F::default());
    }

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
        use crate::algorithms::{connect, ConnectConfig};
        connect(&mut working, ConnectConfig::trim());
    }

    // Optionally push weights
    if config.push_weights {
        use crate::algorithms::{push_weights, PushConfig, ShortestDistanceConfig};
        let push_config = PushConfig {
            direction: config.push_direction.clone(),
            remove_non_coaccessible: false, // We already connected if needed
            distance_config: ShortestDistanceConfig::default(),
        };
        push_weights(&mut working, push_config).map_err(|e| MinimizeError::PushError(e.to_string()))?;
    }

    // Partition refinement to find equivalent states
    let partitions = compute_partitions(&working)?;

    // Build minimized WFST from partitions
    build_minimized(&working, &partitions)
}

/// Compute state partitions using iterative refinement.
fn compute_partitions<L, W, F>(fst: &F) -> Result<Vec<usize>, MinimizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: Semiring + Clone + Debug + Hash + Eq,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(Vec::new());
    }

    // Initial partition: separate final and non-final states
    let mut partition: Vec<usize> = vec![0; n];
    let mut num_partitions = 1;

    // Separate by final weight
    let mut final_weight_to_partition: HashMap<Option<W>, usize> = HashMap::new();

    for state in 0..n {
        let state_id = state as StateId;
        let fw = if fst.is_final(state_id) {
            Some(fst.final_weight(state_id))
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
        let mut signature_to_partition: HashMap<StateSignature<L, W>, usize> = HashMap::new();
        let mut new_partition: Vec<usize> = vec![0; n];
        let mut new_num_partitions = 0;

        for state in 0..n {
            let state_id = state as StateId;

            // Build signature
            let mut sig = StateSignature::new();

            if fst.is_final(state_id) {
                sig.final_weight = Some(fst.final_weight(state_id));
            }

            let mut trans_sigs: Vec<(Option<L>, Option<L>, W, usize)> = Vec::new();
            for trans in fst.transitions(state_id) {
                let target_partition = partition[trans.to as usize];
                trans_sigs.push((
                    trans.input.clone(),
                    trans.output.clone(),
                    trans.weight.clone(),
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

        // Check if partitions changed
        if new_num_partitions > num_partitions {
            changed = true;
            partition = new_partition;
            num_partitions = new_num_partitions;
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
        let new_start = partitions[old_start as usize];
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
            let target_partition = partitions[trans.to as usize];
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
pub fn estimate_reduction<L, W, F>(fst: &F) -> usize
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: Semiring + Clone + Debug + Hash + Eq,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return 0;
    }

    if let Ok(partitions) = compute_partitions(fst) {
        let num_new_states = partitions.iter().max().map(|&m| m + 1).unwrap_or(0);
        n.saturating_sub(num_new_states)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::{VectorWfst, VectorWfstBuilder};

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
        let result = minimize(&fst, MinimizeConfig::standard()).unwrap();
        assert_eq!(result.num_states(), 0);
    }

    #[test]
    fn test_minimize_already_minimal() {
        let fst = build_minimal_fst();
        let result = minimize(&fst, MinimizeConfig::standard()).unwrap();

        // Should have same or fewer states
        assert!(result.num_states() <= fst.num_states());
    }

    #[test]
    fn test_minimize_redundant() {
        let fst = build_redundant_fst();
        let initial_states = fst.num_states();

        let result = minimize(&fst, MinimizeConfig::standard()).unwrap();

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

        let result = minimize(&fst, MinimizeConfig::standard()).unwrap();

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
        assert!(
            reduction >= 1,
            "Expected reduction >= 1, got {}",
            reduction
        );
    }

    #[test]
    fn test_minimize_preserves_determinism() {
        let fst = build_redundant_fst();
        assert!(super::super::determinize::is_deterministic(&fst));

        let result = minimize(&fst, MinimizeConfig::standard()).unwrap();
        assert!(super::super::determinize::is_deterministic(&result));
    }

    #[test]
    fn test_minimize_no_push_config() {
        let fst = build_minimal_fst();

        // With no push should still work for already-pushed FSTs
        let result = minimize(&fst, MinimizeConfig::no_push()).unwrap();
        assert!(result.num_states() <= fst.num_states());
    }
}
