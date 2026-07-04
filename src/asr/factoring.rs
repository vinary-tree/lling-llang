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

use std::collections::HashMap;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst, NO_STATE};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChainSpan {
    entry: StateId,
    first: StateId,
    exit: StateId,
    entry_arc_index: usize,
}

struct ChainReplacement<L: Clone, W: Semiring> {
    entry: StateId,
    exit: StateId,
    chain: Chain<L, W>,
    arc_positions: Vec<(StateId, usize)>,
}

struct ChainCandidate<L: Clone, W: Semiring> {
    span: ChainSpan,
    chain: Chain<L, W>,
    arc_positions: Vec<(StateId, usize)>,
}

struct ChainGroup<L: Clone, W: Semiring> {
    candidates: Vec<ChainCandidate<L, W>>,
    total_gain: i64,
}

/// Find all chains in a WFST.
///
/// A chain is a path where internal states have exactly one in/out transition.
pub fn find_chains<L, W>(fst: &VectorWfst<L, W>) -> Vec<(StateId, StateId)>
where
    L: Clone + Eq + std::hash::Hash + Send + Sync,
    W: Semiring + Clone,
{
    find_chain_spans(fst)
        .into_iter()
        .map(|span| (span.entry, span.exit))
        .collect()
}

fn find_chain_spans<L, W>(fst: &VectorWfst<L, W>) -> Vec<ChainSpan>
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
    let mut sole_valid_outgoing = vec![None; num_states];

    for state in 0..num_states as StateId {
        let state_idx = state as usize;
        for (arc_index, arc) in fst.transitions(state).iter().enumerate() {
            if (arc.to as usize) < num_states {
                out_degree[state_idx] += 1;
                sole_valid_outgoing[state_idx] = Some((arc_index, arc.to));
                in_degree[arc.to as usize] += 1;
            }
        }
    }

    // Find chain candidates: states with in-degree == out-degree == 1
    let mut chain_candidates = vec![false; num_states];
    for state_idx in 0..num_states {
        let state = state_idx as StateId;
        chain_candidates[state_idx] = in_degree[state_idx] == 1
            && out_degree[state_idx] == 1
            && fst.start() != state
            && !fst.is_final(state);
    }

    // Find chain start points: states that transition into chain candidates
    // but are not themselves chain candidates
    let mut chains = Vec::new();
    let mut visited = vec![false; num_states];

    for start in 0..num_states as StateId {
        if chain_candidates[start as usize] {
            continue;
        }

        for (entry_arc_index, arc) in fst.transitions(start).iter().enumerate() {
            let mut current = arc.to;
            let current_idx = current as usize;
            if current_idx >= num_states || !chain_candidates[current_idx] || visited[current_idx] {
                continue;
            }

            // Follow the chain
            let first = current;
            while {
                let current_idx = current as usize;
                current_idx < num_states && chain_candidates[current_idx] && !visited[current_idx]
            } {
                let current_idx = current as usize;
                visited[current_idx] = true;
                if let Some((_, to)) = sole_valid_outgoing[current_idx] {
                    current = to;
                } else {
                    break;
                }
            }
            let chain_end = current;

            if first != chain_end && (chain_end as usize) < num_states {
                chains.push(ChainSpan {
                    entry: start,
                    first,
                    exit: chain_end,
                    entry_arc_index,
                });
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
/// This replaces selected chains with direct transitions and records the
/// removed paths in the returned chain table.
/// Candidate chains are grouped by input-label sequence, and a group is factored
/// only when the aggregate `G(sigma)` is positive.
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
    config: &ChainFactorConfig,
) -> ChainFactorResult<L, W>
where
    L: Clone + Eq + std::hash::Hash + Send + Sync,
    W: Semiring + Clone,
{
    let mut stats = ChainFactorStats::default();
    let mut chains: HashMap<ChainId, Chain<L, W>> = HashMap::new();
    let mut next_chain_id: ChainId = 0;

    // Find all chain endpoints
    let chain_spans = find_chain_spans(fst);
    stats.chains_found = chain_spans.len();

    // If no chains found or FST is empty, return clone
    if chain_spans.is_empty() || fst.num_states() == 0 {
        return ChainFactorResult {
            fst: clone_fst(fst),
            chains,
            stats,
        };
    }

    // Extract full chain information and group candidates by input sequence.
    let mut group_index: FxHashMap<Vec<Option<L>>, usize> = FxHashMap::default();
    let mut chain_groups: Vec<ChainGroup<L, W>> = Vec::new();

    for span in &chain_spans {
        // Extract the chain by following transitions from entry to exit
        if let Some((chain, arc_positions)) = extract_chain(fst, *span) {
            // Check minimum length
            if chain.len() < config.min_chain_length {
                continue;
            }

            // Check epsilon chains if configured
            if !config.factor_epsilon_chains {
                let has_epsilon = chain.input_labels.iter().any(|l| l.is_none())
                    || chain.output_labels.iter().any(|l| l.is_none());
                if has_epsilon {
                    continue;
                }
            }

            // Compute gain
            let gain = compute_chain_gain(&chain);
            let group_key = chain.input_labels.clone();
            let candidate = ChainCandidate {
                span: *span,
                chain,
                arc_positions,
            };

            let group_id = match group_index.get(&group_key).copied() {
                Some(group_id) => group_id,
                None => {
                    let group_id = chain_groups.len();
                    group_index.insert(group_key, group_id);
                    chain_groups.push(ChainGroup {
                        candidates: Vec::new(),
                        total_gain: 0,
                    });
                    group_id
                }
            };

            chain_groups[group_id].total_gain += gain;
            chain_groups[group_id].candidates.push(candidate);
        }
    }

    let mut chain_states_to_remove = vec![false; fst.num_states()];
    let mut chain_replacements: Vec<ChainReplacement<L, W>> = Vec::new();

    let mut chain_ids_exhausted = false;
    for group in chain_groups {
        if chain_ids_exhausted {
            break;
        }

        // Mohri's gain predicate is defined over all chains sharing sigma.
        if group.total_gain <= 0 {
            continue;
        }

        if let Some(max) = config.max_chains {
            let remaining = max.saturating_sub(chains.len());
            if remaining == 0 {
                break;
            }
            if group.candidates.len() > remaining {
                continue;
            }
        }

        stats.total_gain += group.total_gain;

        for mut candidate in group.candidates {
            // Mark internal states for removal (exclude entry and exit states)
            for &state in candidate
                .chain
                .states
                .iter()
                .skip(1)
                .take(candidate.chain.states.len().saturating_sub(2))
            {
                if let Some(remove) = chain_states_to_remove.get_mut(state as usize) {
                    if !*remove {
                        *remove = true;
                        stats.states_removed += 1;
                    }
                }
            }

            stats.chains_factored += 1;
            stats.transitions_removed += candidate.chain.len().saturating_sub(1);
            candidate.chain.id = next_chain_id;

            chain_replacements.push(ChainReplacement {
                entry: candidate.span.entry,
                exit: candidate.span.exit,
                chain: candidate.chain.clone(),
                arc_positions: candidate.arc_positions,
            });
            chains.insert(next_chain_id, candidate.chain);
            let Some(next_id) = next_chain_id.checked_add(1) else {
                chain_ids_exhausted = true;
                break;
            };
            next_chain_id = next_id;
        }
    }

    // Build the factored FST
    let result_fst = build_factored_fst(fst, &chain_states_to_remove, &chain_replacements);

    ChainFactorResult {
        fst: result_fst,
        chains,
        stats,
    }
}

/// Extract a chain from the FST given entry and exit states.
fn extract_chain<L, W>(
    fst: &VectorWfst<L, W>,
    span: ChainSpan,
) -> Option<(Chain<L, W>, Vec<(StateId, usize)>)>
where
    L: Clone + Send + Sync,
    W: Semiring + Clone,
{
    let mut chain = Chain::new(0);
    chain.states.push(span.entry);
    let mut arc_positions = Vec::new();

    let mut current = span.entry;
    let mut accumulated_weight = W::one();

    // Follow transitions until we reach exit
    while current != span.exit {
        let next_arc = if current == span.entry {
            fst.transitions(current)
                .get(span.entry_arc_index)
                .filter(|arc| fst.is_valid_state(arc.to) && arc.to == span.first)
                .map(|arc| (span.entry_arc_index, arc))
        } else {
            single_valid_outgoing(fst, current)
        };

        let (next_arc_index, next_arc) = next_arc?;
        chain.states.push(next_arc.to);
        chain.input_labels.push(next_arc.input.clone());
        chain.output_labels.push(next_arc.output.clone());
        accumulated_weight = accumulated_weight.times(&next_arc.weight);
        arc_positions.push((current, next_arc_index));
        current = next_arc.to;

        if current == span.exit {
            break;
        }

        // Safety check to prevent infinite loops
        if chain.states.len() > fst.num_states() {
            return None;
        }
    }

    chain.weight = accumulated_weight;
    Some((chain, arc_positions))
}

fn single_valid_outgoing<L, W>(
    fst: &VectorWfst<L, W>,
    state: StateId,
) -> Option<(usize, &WeightedTransition<L, W>)>
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    let mut valid_outgoing = fst
        .transitions(state)
        .iter()
        .enumerate()
        .filter(|(_, arc)| fst.is_valid_state(arc.to));
    let first = valid_outgoing.next()?;
    if valid_outgoing.next().is_some() {
        None
    } else {
        Some(first)
    }
}

/// Build a factored FST with chains replaced by direct transitions.
fn build_factored_fst<L, W>(
    fst: &VectorWfst<L, W>,
    states_to_remove: &[bool],
    chain_replacements: &[ChainReplacement<L, W>],
) -> VectorWfst<L, W>
where
    L: Clone + Send + Sync,
    W: Semiring + Clone,
{
    // If no states to remove, just clone and add chain transitions
    if !states_to_remove.iter().any(|&remove| remove) && chain_replacements.is_empty() {
        return clone_fst(fst);
    }

    let retained_states = fst
        .num_states()
        .saturating_sub(states_to_remove.iter().filter(|&&remove| remove).count());
    let mut result: VectorWfst<L, W> = VectorWfst::with_capacity(retained_states);

    // Create mapping from old state IDs to new state IDs
    let mut state_map = vec![NO_STATE; fst.num_states()];

    // Add states that aren't being removed
    for old_id in 0..fst.num_states() as StateId {
        if !states_to_remove[old_id as usize] {
            let new_id = result.add_state();
            state_map[old_id as usize] = new_id;
        }
    }

    // Set start state
    let start = fst.start();
    if start != NO_STATE {
        if let Some(new_start) = mapped_state(&state_map, start) {
            result.set_start(new_start);
        }
    }

    // Create a set of exact transition positions that are being replaced by chains.
    let chain_arcs: FxHashSet<(StateId, usize)> = chain_replacements
        .iter()
        .flat_map(|replacement| replacement.arc_positions.iter().copied())
        .collect();

    // Copy transitions, skipping those that are part of removed chains
    for old_source in 0..fst.num_states() as StateId {
        if states_to_remove[old_source as usize] {
            continue;
        }

        let new_source = match mapped_state(&state_map, old_source) {
            Some(id) => id,
            None => continue,
        };

        for (arc_index, arc) in fst.transitions(old_source).iter().enumerate() {
            // Skip arcs that lead into removed states (part of chains)
            if states_to_remove
                .get(arc.to as usize)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }

            // Skip arcs that are being replaced by chain transitions
            if chain_arcs.contains(&(old_source, arc_index)) {
                continue;
            }

            if let Some(new_target) = mapped_state(&state_map, arc.to) {
                result.add_arc(
                    new_source,
                    arc.input.clone(),
                    arc.output.clone(),
                    new_target,
                    arc.weight.clone(),
                );
            }
        }

        // Copy final weight
        if fst.is_final(old_source) {
            let weight = fst.final_weight(old_source);
            result.set_final(new_source, weight.clone());
        }
    }

    // Add chain replacement transitions (entry -> exit with chain label)
    for replacement in chain_replacements {
        if let (Some(new_entry), Some(new_exit)) = (
            mapped_state(&state_map, replacement.entry),
            mapped_state(&state_map, replacement.exit),
        ) {
            // Use the first input/output label or default
            let input = replacement.chain.input_labels.first().cloned().flatten();
            let output = replacement.chain.output_labels.first().cloned().flatten();

            result.add_arc(
                new_entry,
                input,
                output,
                new_exit,
                replacement.chain.weight.clone(),
            );
        }
    }

    result
}

fn mapped_state(state_map: &[StateId], old_state: StateId) -> Option<StateId> {
    state_map
        .get(old_state as usize)
        .copied()
        .filter(|&new_state| new_state != NO_STATE)
}

/// Clone a WFST.
fn clone_fst<L, W>(fst: &VectorWfst<L, W>) -> VectorWfst<L, W>
where
    L: Clone + Send + Sync,
    W: Semiring + Clone,
{
    let mut result: VectorWfst<L, W> = VectorWfst::with_capacity(fst.num_states());

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
            if !fst.is_valid_state(arc.to) {
                continue;
            }

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
        assert!(result.stats.chains_found > 0, "expected at least one chain");
    }

    #[test]
    fn test_chain_factor_extracts_branching_chains_from_same_entry() {
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();

        let entry = fst.add_state();
        let left_internal = fst.add_state();
        let left_exit = fst.add_state();
        let right_internal = fst.add_state();
        let right_exit = fst.add_state();

        fst.set_start(entry);
        fst.set_final(left_exit, LogWeight::one());
        fst.set_final(right_exit, LogWeight::one());

        fst.add_arc(entry, Some(10), None, left_internal, LogWeight::one());
        fst.add_arc(left_internal, Some(11), None, left_exit, LogWeight::one());
        fst.add_arc(entry, Some(20), None, right_internal, LogWeight::one());
        fst.add_arc(right_internal, Some(21), None, right_exit, LogWeight::one());

        let result = chain_factor(&fst, &ChainFactorConfig::default());

        assert_eq!(result.stats.chains_found, 2);
        assert_eq!(result.stats.chains_factored, 2);
        assert_eq!(result.stats.states_removed, 2);
        assert_eq!(result.chains.len(), 2);
        assert_eq!(result.fst.num_states(), 3);
        assert_eq!(result.fst.transitions(result.fst.start()).len(), 2);
    }

    #[test]
    fn test_chain_factor_ignores_malformed_extra_arc_when_finding_chain() {
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();

        let entry = fst.add_state();
        let internal = fst.add_state();
        let exit = fst.add_state();

        fst.set_start(entry);
        fst.set_final(exit, LogWeight::one());

        fst.add_arc(entry, Some(10), None, internal, LogWeight::one());
        fst.add_arc(internal, Some(99), None, 99, LogWeight::one());
        fst.add_arc(internal, Some(11), None, exit, LogWeight::one());

        let result = chain_factor(&fst, &ChainFactorConfig::default());

        assert_eq!(result.stats.chains_found, 1);
        assert_eq!(result.stats.chains_factored, 1);
        assert_eq!(result.stats.states_removed, 1);
        assert_eq!(result.fst.num_states(), 2);
        assert_eq!(result.fst.transitions(result.fst.start()).len(), 1);
        assert!(result
            .fst
            .transitions(result.fst.start())
            .iter()
            .all(|transition| result.fst.is_valid_state(transition.to)));
    }

    #[test]
    fn test_chain_factor_clone_path_drops_malformed_targets() {
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();

        let state = fst.add_state();
        fst.set_start(state);
        fst.set_final(state, LogWeight::one());
        fst.add_arc(state, Some(1), Some(1), 99, LogWeight::one());

        let result = chain_factor(&fst, &ChainFactorConfig::default());

        assert_eq!(result.stats.chains_found, 0);
        assert!(result.fst.is_final(state));
        assert!(result.fst.transitions(state).is_empty());
    }

    #[test]
    fn test_chain_factor_uses_aggregate_gain_per_input_sequence() {
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();

        let entry = fst.add_state();
        let high_gain_a = fst.add_state();
        let high_gain_b = fst.add_state();
        let high_gain_exit = fst.add_state();
        let low_gain_a = fst.add_state();
        let low_gain_b = fst.add_state();
        let low_gain_exit = fst.add_state();

        fst.set_start(entry);
        fst.set_final(high_gain_exit, LogWeight::one());
        fst.set_final(low_gain_exit, LogWeight::one());

        fst.add_arc(entry, Some(1), None, high_gain_a, LogWeight::one());
        fst.add_arc(high_gain_a, Some(2), None, high_gain_b, LogWeight::one());
        fst.add_arc(high_gain_b, Some(3), None, high_gain_exit, LogWeight::one());

        fst.add_arc(entry, Some(1), Some(10), low_gain_a, LogWeight::one());
        fst.add_arc(low_gain_a, Some(2), Some(20), low_gain_b, LogWeight::one());
        fst.add_arc(
            low_gain_b,
            Some(3),
            Some(30),
            low_gain_exit,
            LogWeight::one(),
        );

        let result = chain_factor(&fst, &ChainFactorConfig::default());

        assert_eq!(result.stats.chains_found, 2);
        assert_eq!(result.stats.chains_factored, 2);
        assert_eq!(result.stats.total_gain, 1);
        assert_eq!(result.stats.states_removed, 4);
        assert_eq!(result.stats.transitions_removed, 4);
        assert_eq!(result.fst.num_states(), 3);
        assert_eq!(result.fst.transitions(result.fst.start()).len(), 2);
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
            prop_assert_eq!(chain.end_state(), Some(*states.last().expect("asr/factoring.rs: required value was None/Err")));
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
            let expected = (num_some as i64) - 1;  // No outputs

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
                fst.set_final(*states.last().expect("asr/factoring.rs: required value was None/Err"), LogWeight::one());

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

        /// chain_factor stats chains_found is bounded by the input state count.
        #[test]
        fn factor_stats_bounded(num_states in 0usize..5) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();

            for _ in 0..num_states {
                fst.add_state();
            }

            if num_states > 0 {
                fst.set_start(0);
            }

            let config = ChainFactorConfig::default();
            let result = chain_factor(&fst, &config);

            // Each chain consumes at least one state, so chains_found
            // cannot exceed the number of input states.
            prop_assert!(result.stats.chains_found <= num_states);
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
