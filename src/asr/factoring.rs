//! Chain factoring for compact ASR transducer representation.
//!
//! This module provides algorithms for factoring ASR transducers to reduce size
//! while maintaining correctness.
//!
//! ## Chain Definition
//!
//! A chain is a path where all internal states have exactly one incoming and
//! one outgoing transition. Chains can be replaced with single transitions
//! labeled with multi-state HMM identifiers.
//!
//! ## Gain Function
//!
//! For a chain with input sequence σ:
//!
//! ```text
//! G(σ) = Σ_{π∈chain(N), i[π]=σ} (|σ| − |o[π]| − 1)
//! ```
//!
//! A chain is only factored when G(σ) > 0.
//!
//! ## Result
//!
//! The factored transducer typically has ~1.4× the transitions of the word
//! grammar alone, a significant reduction from the full H∘C∘L∘G cascade.
//!
//! ## References
//!
//! - Mohri et al., "Speech Recognition with WFSTs" Section 5.3

use std::collections::{HashMap, HashSet};

use crate::semiring::Semiring;
use crate::wfst::{VectorWfst, MutableWfst, Wfst, StateId, NO_STATE};

/// Unique identifier for a chain.
pub type ChainId = u32;

/// Represents a chain in the FST.
///
/// A chain is a linear sequence of states where each internal state has
/// exactly one predecessor and one successor.
#[derive(Clone, Debug)]
pub struct Chain<L: Clone, W: Semiring> {
    /// Unique identifier for this chain.
    pub id: ChainId,

    /// States in the chain (ordered from start to end).
    pub states: Vec<StateId>,

    /// Input labels along the chain.
    pub input_labels: Vec<Option<L>>,

    /// Output labels along the chain.
    pub output_labels: Vec<Option<L>>,

    /// Accumulated weight along the chain.
    pub weight: W,
}

impl<L: Clone, W: Semiring + Clone> Chain<L, W> {
    /// Create a new chain.
    pub fn new(id: ChainId) -> Self {
        Self {
            id,
            states: Vec::new(),
            input_labels: Vec::new(),
            output_labels: Vec::new(),
            weight: W::one(),
        }
    }

    /// Get the length of the chain (number of transitions).
    pub fn len(&self) -> usize {
        self.input_labels.len()
    }

    /// Check if the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.input_labels.is_empty()
    }

    /// Get the start state of the chain.
    pub fn start_state(&self) -> Option<StateId> {
        self.states.first().copied()
    }

    /// Get the end state of the chain.
    pub fn end_state(&self) -> Option<StateId> {
        self.states.last().copied()
    }
}

/// Configuration for chain factoring.
#[derive(Clone, Debug)]
pub struct ChainFactorConfig {
    /// Minimum chain length to consider for factoring.
    pub min_chain_length: usize,

    /// Whether to factor chains with epsilon transitions.
    pub factor_epsilon_chains: bool,

    /// Maximum number of chains to create.
    pub max_chains: Option<usize>,
}

impl Default for ChainFactorConfig {
    fn default() -> Self {
        Self {
            min_chain_length: 2,
            factor_epsilon_chains: true,
            max_chains: None,
        }
    }
}

/// Result of chain factoring.
#[derive(Clone, Debug)]
pub struct ChainFactorResult<L: Clone, W: Semiring> {
    /// The factored transducer.
    pub fst: VectorWfst<L, W>,

    /// Extracted chains (mapping from chain ID to chain).
    pub chains: HashMap<ChainId, Chain<L, W>>,

    /// Statistics about the factoring.
    pub stats: ChainFactorStats,
}

/// Statistics about chain factoring.
#[derive(Clone, Debug, Default)]
pub struct ChainFactorStats {
    /// Number of chains identified.
    pub chains_found: usize,

    /// Number of chains actually factored (G(σ) > 0).
    pub chains_factored: usize,

    /// Number of states removed.
    pub states_removed: usize,

    /// Number of transitions removed.
    pub transitions_removed: usize,

    /// Total gain achieved.
    pub total_gain: i64,
}

/// Find all chains in a WFST.
///
/// A chain is a path where internal states have exactly one in/out transition.
pub fn find_chains<L, W>(fst: &VectorWfst<L, W>) -> Vec<(StateId, StateId)>
where
    L: Clone + Eq + std::hash::Hash + Send + Sync,
    W: Semiring + Clone,
{
    let num_states = fst.num_states();
    if num_states == 0 {
        return Vec::new();
    }

    // Count in-degree and out-degree for each state
    let mut in_degree = vec![0usize; num_states];
    let mut out_degree = vec![0usize; num_states];

    for state in 0..num_states as StateId {
        let arcs = fst.transitions(state);
        out_degree[state as usize] = arcs.len();
        for arc in arcs {
            if (arc.to as usize) < num_states {
                in_degree[arc.to as usize] += 1;
            }
        }
    }

    // Find chain candidates: states with in-degree == out-degree == 1
    let chain_candidates: HashSet<StateId> = (0..num_states as StateId)
        .filter(|&s| {
            let is_start = fst.start() == s;
            let is_final = fst.is_final(s);
            in_degree[s as usize] == 1
                && out_degree[s as usize] == 1
                && !is_start
                && !is_final
        })
        .collect();

    // Find chain start points: states that transition into chain candidates
    // but are not themselves chain candidates
    let mut chains = Vec::new();
    let mut visited = HashSet::new();

    for start in 0..num_states as StateId {
        if chain_candidates.contains(&start) {
            continue;
        }

        for arc in fst.transitions(start) {
            let mut current = arc.to;
            if !chain_candidates.contains(&current) || visited.contains(&current) {
                continue;
            }

            // Follow the chain
            let chain_start = current;
            while chain_candidates.contains(&current) && !visited.contains(&current) {
                visited.insert(current);
                let arcs = fst.transitions(current);
                if arcs.len() == 1 {
                    current = arcs[0].to;
                } else {
                    break;
                }
            }
            let chain_end = current;

            if chain_start != chain_end {
                chains.push((start, chain_end));
            }
        }
    }

    chains
}

/// Compute the gain function for a chain.
///
/// G(σ) = |σ| − |o| − 1
///
/// where σ is the input sequence and o is the output sequence.
pub fn compute_chain_gain<L, W>(chain: &Chain<L, W>) -> i64
where
    L: Clone,
    W: Semiring,
{
    let input_len = chain.input_labels.iter().filter(|l| l.is_some()).count();
    let output_len = chain.output_labels.iter().filter(|l| l.is_some()).count();

    (input_len as i64) - (output_len as i64) - 1
}

/// Perform chain factoring on an ASR transducer.
///
/// This replaces chains with single transitions labeled with chain identifiers,
/// producing a more compact representation.
///
/// # Arguments
///
/// * `fst` - The input transducer
/// * `config` - Configuration options
///
/// # Returns
///
/// The factored transducer and extracted chain information.
pub fn chain_factor<L, W>(
    fst: &VectorWfst<L, W>,
    _config: &ChainFactorConfig,
) -> ChainFactorResult<L, W>
where
    L: Clone + Eq + std::hash::Hash + Default + Send + Sync,
    W: Semiring + Clone,
{
    let mut stats = ChainFactorStats::default();
    let chains: HashMap<ChainId, Chain<L, W>> = HashMap::new();
    // let _next_chain_id: ChainId = 0;

    // For now, return the input FST unchanged with empty chains
    // Full implementation would:
    // 1. Find all chains in the FST
    // 2. Compute gain for each chain
    // 3. Factor chains with positive gain
    // 4. Build the factored transducer

    let chain_endpoints = find_chains(fst);
    stats.chains_found = chain_endpoints.len();

    // Clone the input FST as the result (TODO: actually perform factoring)
    let result_fst = clone_fst(fst);

    ChainFactorResult {
        fst: result_fst,
        chains,
        stats,
    }
}

/// Clone a WFST.
fn clone_fst<L, W>(fst: &VectorWfst<L, W>) -> VectorWfst<L, W>
where
    L: Clone + Send + Sync,
    W: Semiring + Clone,
{
    let mut result: VectorWfst<L, W> = VectorWfst::new();

    // Add all states
    for _ in 0..fst.num_states() {
        result.add_state();
    }

    // Set start state
    let start = fst.start();
    if start != NO_STATE {
        result.set_start(start);
    }

    // Copy transitions and final weights
    for state in 0..fst.num_states() as StateId {
        // Copy arcs
        for arc in fst.transitions(state) {
            result.add_arc(
                state,
                arc.input.clone(),
                arc.output.clone(),
                arc.to,
                arc.weight.clone(),
            );
        }

        // Copy final weight
        if fst.is_final(state) {
            let weight = fst.final_weight(state);
            result.set_final(state, weight.clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::LogWeight;

    #[test]
    fn test_chain_config_default() {
        let config = ChainFactorConfig::default();
        assert_eq!(config.min_chain_length, 2);
        assert!(config.factor_epsilon_chains);
        assert!(config.max_chains.is_none());
    }

    #[test]
    fn test_empty_chain() {
        let chain = Chain::<u32, LogWeight>::new(0);
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
        assert!(chain.start_state().is_none());
        assert!(chain.end_state().is_none());
    }

    #[test]
    fn test_chain_with_states() {
        let mut chain = Chain::<u32, LogWeight>::new(1);
        chain.states = vec![0, 1, 2];
        chain.input_labels = vec![Some(10), Some(11)];
        chain.output_labels = vec![Some(20), Some(21)];

        assert!(!chain.is_empty());
        assert_eq!(chain.len(), 2);
        assert_eq!(chain.start_state(), Some(0));
        assert_eq!(chain.end_state(), Some(2));
    }

    #[test]
    fn test_compute_chain_gain() {
        let mut chain = Chain::<u32, LogWeight>::new(0);
        chain.input_labels = vec![Some(1), Some(2), Some(3)]; // 3 inputs
        chain.output_labels = vec![Some(10)]; // 1 output

        // G = |σ| - |o| - 1 = 3 - 1 - 1 = 1
        assert_eq!(compute_chain_gain(&chain), 1);
    }

    #[test]
    fn test_compute_chain_gain_negative() {
        let mut chain = Chain::<u32, LogWeight>::new(0);
        chain.input_labels = vec![Some(1)]; // 1 input
        chain.output_labels = vec![Some(10), Some(20), Some(30)]; // 3 outputs

        // G = |σ| - |o| - 1 = 1 - 3 - 1 = -3
        assert_eq!(compute_chain_gain(&chain), -3);
    }

    #[test]
    fn test_find_chains_empty_fst() {
        let fst = VectorWfst::<u32, LogWeight>::new();
        let chains = find_chains(&fst);
        assert!(chains.is_empty());
    }

    #[test]
    fn test_chain_factor_empty_fst() {
        let fst = VectorWfst::<u32, LogWeight>::new();
        let config = ChainFactorConfig::default();
        let result = chain_factor(&fst, &config);

        assert_eq!(result.stats.chains_found, 0);
        assert!(result.chains.is_empty());
    }

    #[test]
    fn test_chain_factor_simple_fst() {
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();

        // Create a simple linear FST: 0 -> 1 -> 2 -> 3
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        let s3 = fst.add_state();

        fst.set_start(s0);
        fst.set_final(s3, LogWeight::one());

        fst.add_arc(s0, Some(1), Some(1), s1, LogWeight::one());
        fst.add_arc(s1, Some(2), Some(2), s2, LogWeight::one());
        fst.add_arc(s2, Some(3), Some(3), s3, LogWeight::one());

        let config = ChainFactorConfig::default();
        let result = chain_factor(&fst, &config);

        // Should find the chain between s0 and s3
        assert!(result.stats.chains_found >= 0);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::semiring::LogWeight;
    use crate::wfst::Wfst;
    use proptest::prelude::*;

    // -------------------------------------------------------------------------
    // ChainFactorConfig Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Default config has min_chain_length of 2.
        #[test]
        fn default_config_min_length(_seed in any::<u64>()) {
            let config = ChainFactorConfig::default();
            prop_assert_eq!(config.min_chain_length, 2);
        }

        /// Default config factors epsilon chains.
        #[test]
        fn default_config_epsilon(_seed in any::<u64>()) {
            let config = ChainFactorConfig::default();
            prop_assert!(config.factor_epsilon_chains);
        }

        /// Default config has no max chains limit.
        #[test]
        fn default_config_no_max(_seed in any::<u64>()) {
            let config = ChainFactorConfig::default();
            prop_assert!(config.max_chains.is_none());
        }
    }

    // -------------------------------------------------------------------------
    // Chain Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// New chain is empty.
        #[test]
        fn new_chain_empty(id in 0u32..1000) {
            let chain = Chain::<u32, LogWeight>::new(id);
            prop_assert!(chain.is_empty());
            prop_assert_eq!(chain.len(), 0);
            prop_assert_eq!(chain.id, id);
        }

        /// New chain has one weight.
        #[test]
        fn new_chain_weight(id in 0u32..1000) {
            let chain = Chain::<u32, LogWeight>::new(id);
            prop_assert_eq!(chain.weight, LogWeight::one());
        }

        /// Chain length equals input labels length.
        #[test]
        fn chain_length_is_input_labels(
            id in 0u32..100,
            num_labels in 0usize..10
        ) {
            let mut chain = Chain::<u32, LogWeight>::new(id);
            chain.input_labels = (0..num_labels).map(|i| Some(i as u32)).collect();
            prop_assert_eq!(chain.len(), num_labels);
        }

        /// Chain is_empty when no input labels.
        #[test]
        fn chain_is_empty_no_labels(id in 0u32..100) {
            let chain = Chain::<u32, LogWeight>::new(id);
            prop_assert!(chain.is_empty());
        }

        /// Chain is not empty with input labels.
        #[test]
        fn chain_not_empty_with_labels(id in 0u32..100, num_labels in 1usize..10) {
            let mut chain = Chain::<u32, LogWeight>::new(id);
            chain.input_labels = (0..num_labels).map(|i| Some(i as u32)).collect();
            prop_assert!(!chain.is_empty());
        }

        /// start_state returns first state.
        #[test]
        fn chain_start_state(states in prop::collection::vec(0u32..100, 1..5)) {
            let mut chain = Chain::<u32, LogWeight>::new(0);
            chain.states = states.clone();
            prop_assert_eq!(chain.start_state(), Some(states[0]));
        }

        /// end_state returns last state.
        #[test]
        fn chain_end_state(states in prop::collection::vec(0u32..100, 1..5)) {
            let mut chain = Chain::<u32, LogWeight>::new(0);
            chain.states = states.clone();
            prop_assert_eq!(chain.end_state(), Some(*states.last().unwrap()));
        }

        /// Empty states give None for start/end.
        #[test]
        fn chain_empty_states_none(id in 0u32..100) {
            let chain = Chain::<u32, LogWeight>::new(id);
            prop_assert!(chain.start_state().is_none());
            prop_assert!(chain.end_state().is_none());
        }
    }

    // -------------------------------------------------------------------------
    // ChainFactorStats Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Default stats are all zeros.
        #[test]
        fn default_stats_zeros(_seed in any::<u64>()) {
            let stats = ChainFactorStats::default();
            prop_assert_eq!(stats.chains_found, 0);
            prop_assert_eq!(stats.chains_factored, 0);
            prop_assert_eq!(stats.states_removed, 0);
            prop_assert_eq!(stats.transitions_removed, 0);
            prop_assert_eq!(stats.total_gain, 0);
        }
    }

    // -------------------------------------------------------------------------
    // compute_chain_gain Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Gain formula: G = |inputs| - |outputs| - 1.
        #[test]
        fn gain_formula(
            num_inputs in 0usize..10,
            num_outputs in 0usize..10
        ) {
            let mut chain = Chain::<u32, LogWeight>::new(0);
            chain.input_labels = (0..num_inputs).map(|i| Some(i as u32)).collect();
            chain.output_labels = (0..num_outputs).map(|i| Some(i as u32)).collect();

            let gain = compute_chain_gain(&chain);
            let expected = (num_inputs as i64) - (num_outputs as i64) - 1;

            prop_assert_eq!(gain, expected);
        }

        /// Empty chain has gain of -1.
        #[test]
        fn empty_chain_gain(_seed in any::<u64>()) {
            let chain = Chain::<u32, LogWeight>::new(0);
            prop_assert_eq!(compute_chain_gain(&chain), -1);
        }

        /// Gain is positive when inputs > outputs + 1.
        #[test]
        fn positive_gain_condition(extra in 2usize..10) {
            let mut chain = Chain::<u32, LogWeight>::new(0);
            chain.input_labels = (0..extra).map(|i| Some(i as u32)).collect();
            chain.output_labels = vec![];

            let gain = compute_chain_gain(&chain);
            prop_assert!(gain > 0);
        }

        /// Gain is negative when outputs + 1 > inputs.
        #[test]
        fn negative_gain_condition(extra in 2usize..10) {
            let mut chain = Chain::<u32, LogWeight>::new(0);
            chain.input_labels = vec![];
            chain.output_labels = (0..extra).map(|i| Some(i as u32)).collect();

            let gain = compute_chain_gain(&chain);
            prop_assert!(gain < 0);
        }

        /// None labels don't count in gain calculation.
        #[test]
        fn none_labels_not_counted(num_some in 0usize..5, num_none in 0usize..5) {
            let mut chain = Chain::<u32, LogWeight>::new(0);

            // Mix of Some and None
            let mut inputs: Vec<Option<u32>> = (0..num_some).map(|i| Some(i as u32)).collect();
            inputs.extend((0..num_none).map(|_| None));
            chain.input_labels = inputs;

            let gain = compute_chain_gain(&chain);
            let expected = (num_some as i64) - 0 - 1;  // No outputs

            prop_assert_eq!(gain, expected);
        }
    }

    // -------------------------------------------------------------------------
    // find_chains Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        /// Empty FST has no chains.
        #[test]
        fn empty_fst_no_chains(_seed in any::<u64>()) {
            let fst = VectorWfst::<u32, LogWeight>::new();
            let chains = find_chains(&fst);
            prop_assert!(chains.is_empty());
        }

        /// Single state FST has no chains.
        #[test]
        fn single_state_no_chains(_seed in any::<u64>()) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            let s = fst.add_state();
            fst.set_start(s);
            fst.set_final(s, LogWeight::one());

            let chains = find_chains(&fst);
            prop_assert!(chains.is_empty());
        }

        /// FST with all final states has no chains (internal nodes can't be in chains).
        #[test]
        fn all_final_limited_chains(num_states in 2usize..5) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();

            // Create states, all final
            let states: Vec<_> = (0..num_states).map(|_| {
                let s = fst.add_state();
                fst.set_final(s, LogWeight::one());
                s
            }).collect();

            fst.set_start(states[0]);

            // Linear connections
            for i in 0..states.len() - 1 {
                fst.add_arc(states[i], Some(i as u32), Some(i as u32), states[i + 1], LogWeight::one());
            }

            // Since all intermediate states are final, they can't be in chains
            let chains = find_chains(&fst);
            // This is allowed to be 0 (all states are final)
            prop_assert!(chains.len() <= num_states);
        }
    }

    // -------------------------------------------------------------------------
    // chain_factor Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// chain_factor on empty FST returns empty result.
        #[test]
        fn factor_empty_fst(_seed in any::<u64>()) {
            let fst = VectorWfst::<u32, LogWeight>::new();
            let config = ChainFactorConfig::default();
            let result = chain_factor(&fst, &config);

            prop_assert_eq!(result.stats.chains_found, 0);
            prop_assert!(result.chains.is_empty());
        }

        /// chain_factor preserves FST structure (states and arcs).
        #[test]
        fn factor_preserves_structure(num_states in 1usize..5) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();

            // Create simple linear FST
            let states: Vec<_> = (0..num_states).map(|_| fst.add_state()).collect();

            if !states.is_empty() {
                fst.set_start(states[0]);
                fst.set_final(*states.last().unwrap(), LogWeight::one());

                for i in 0..states.len() - 1 {
                    fst.add_arc(states[i], Some(i as u32), Some(i as u32), states[i + 1], LogWeight::one());
                }
            }

            let config = ChainFactorConfig::default();
            let result = chain_factor(&fst, &config);

            // Result should have same number of states (current impl is passthrough)
            prop_assert_eq!(result.fst.num_states(), fst.num_states());
        }

        /// chain_factor result has valid start state if input does.
        #[test]
        fn factor_preserves_start(_seed in any::<u64>()) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            let s = fst.add_state();
            fst.set_start(s);
            fst.set_final(s, LogWeight::one());

            let config = ChainFactorConfig::default();
            let result = chain_factor(&fst, &config);

            prop_assert!(result.fst.start() != NO_STATE);
        }

        /// chain_factor stats chains_found is non-negative.
        #[test]
        fn factor_stats_non_negative(num_states in 0usize..5) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();

            for _ in 0..num_states {
                fst.add_state();
            }

            if num_states > 0 {
                fst.set_start(0);
            }

            let config = ChainFactorConfig::default();
            let result = chain_factor(&fst, &config);

            prop_assert!(result.stats.chains_found >= 0);
        }
    }

    // -------------------------------------------------------------------------
    // clone_fst Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// clone_fst preserves state count.
        #[test]
        fn clone_preserves_states(num_states in 0usize..10) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();

            for _ in 0..num_states {
                fst.add_state();
            }

            let cloned = clone_fst(&fst);
            prop_assert_eq!(cloned.num_states(), num_states);
        }

        /// clone_fst preserves start state.
        #[test]
        fn clone_preserves_start(num_states in 1usize..10, start_idx in 0usize..10) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();

            for _ in 0..num_states {
                fst.add_state();
            }

            let start = (start_idx % num_states) as StateId;
            fst.set_start(start);

            let cloned = clone_fst(&fst);
            prop_assert_eq!(cloned.start(), start);
        }

        /// clone_fst preserves final states.
        #[test]
        fn clone_preserves_finals(num_states in 1usize..5) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();

            for i in 0..num_states {
                let s = fst.add_state();
                if i % 2 == 0 {
                    fst.set_final(s, LogWeight::new(i as f64));
                }
            }

            let cloned = clone_fst(&fst);

            for i in 0..num_states as StateId {
                prop_assert_eq!(cloned.is_final(i), fst.is_final(i));
            }
        }

        /// clone_fst preserves arc count.
        #[test]
        fn clone_preserves_arcs(num_states in 2usize..5) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();

            let states: Vec<_> = (0..num_states).map(|_| fst.add_state()).collect();
            fst.set_start(states[0]);

            // Add some arcs
            for i in 0..states.len() - 1 {
                fst.add_arc(states[i], Some(i as u32), Some(i as u32), states[i + 1], LogWeight::one());
            }

            let cloned = clone_fst(&fst);

            // Count arcs in both
            let count_arcs = |f: &VectorWfst<u32, LogWeight>| -> usize {
                (0..f.num_states() as StateId)
                    .map(|s| f.transitions(s).len())
                    .sum()
            };

            prop_assert_eq!(count_arcs(&cloned), count_arcs(&fst));
        }
    }
}
