//! Weighted determinization algorithm for WFSTs.
//!
//! Determinization produces a WFST where at most one path can match any
//! input string, while preserving the weighted language.
//!
//! # Algorithm
//!
//! Uses the weighted powerset construction from Mohri's work:
//! - States are weighted subsets: sets of (state, residual_weight) pairs
//! - Initial subset: {(start, 1̄)}
//! - For each input label, compute all reachable states with combined weights
//! - Residual weight = w + arc_weight - min_weight (normalized)
//!
//! # Complexity
//!
//! - Worst case: exponential in number of states (powerset construction)
//! - In practice: often linear for unambiguous automata
//! - Acyclic: guaranteed to terminate
//!
//! # Requirements
//!
//! - Semiring must be divisible (for weight normalization)
//! - Semiring should be weakly left-divisible for correctness
//!
//! # References
//!
//! - Mohri, M. (2009). "Weighted Automata Algorithms"
//! - Mohri, M., Pereira, F., & Riley, M. (2002). "WFSTs in Speech Recognition"

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::Debug;
use std::hash::Hash;

use crate::semiring::{DivisibleSemiring, Semiring, TotallyOrderedSemiring};
use crate::wfst::{MutableWfst, StateId, WeightedTransition, Wfst, NO_STATE};

use super::connect::{connect, ConnectConfig};
use super::epsilon_removal::{remove_epsilon, EpsilonRemovalConfig};

/// Configuration for determinization.
#[derive(Clone, Debug)]
pub struct DeterminizeConfig {
    /// Maximum number of states in the output (prevents runaway)
    pub max_states: Option<usize>,
    /// Whether to epsilon-remove first (recommended)
    pub remove_epsilon_first: bool,
    /// Whether to connect (trim) after determinization
    pub connect_after: bool,
}

impl Default for DeterminizeConfig {
    fn default() -> Self {
        Self {
            max_states: Some(1_000_000),
            remove_epsilon_first: true,
            connect_after: true,
        }
    }
}

impl DeterminizeConfig {
    /// Standard determinization with default limits.
    pub fn standard() -> Self {
        Self::default()
    }

    /// Unlimited determinization (use with caution).
    pub fn unlimited() -> Self {
        Self {
            max_states: None,
            remove_epsilon_first: true,
            connect_after: true,
        }
    }
}

/// Error during determinization.
#[derive(Clone, Debug, PartialEq)]
pub enum DeterminizeError {
    /// No start state defined.
    NoStartState,
    /// Maximum state limit exceeded.
    StateLimitExceeded {
        /// The maximum number of states allowed.
        limit: usize,
    },
    /// The WFST is not determinizable (cycles with certain weight patterns).
    NotDeterminizable {
        /// Description of why determinization failed.
        reason: String,
    },
}

impl std::fmt::Display for DeterminizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeterminizeError::NoStartState => write!(f, "WFST has no start state"),
            DeterminizeError::StateLimitExceeded { limit } => {
                write!(f, "Determinization exceeded {} state limit", limit)
            }
            DeterminizeError::NotDeterminizable { reason } => {
                write!(f, "WFST is not determinizable: {}", reason)
            }
        }
    }
}

impl std::error::Error for DeterminizeError {}

/// A weighted subset is a set of (state, residual_weight) pairs.
/// We use BTreeMap for consistent ordering (needed for hashing).
type WeightedSubset<W> = BTreeMap<StateId, W>;

/// Create a canonical key for a weighted subset (for deduplication).
fn subset_key<W: Semiring + Clone>(subset: &WeightedSubset<W>) -> Vec<(StateId, W)> {
    subset.iter().map(|(&s, w)| (s, w.clone())).collect()
}

/// Find the minimum weight in a weighted subset.
///
/// Uses `TotallyOrderedSemiring::total_cmp` for safe comparison without
/// the `unwrap_or(Equal)` fallback that could hide comparison failures.
fn min_weight<W: TotallyOrderedSemiring + Clone>(subset: &WeightedSubset<W>) -> W {
    subset
        .values()
        .cloned()
        .min_by(|a, b| a.total_cmp(b))
        .unwrap_or_else(W::zero)
}

fn has_input_epsilon_transitions<L, W, F>(fst: &F) -> bool
where
    L: Clone,
    W: Semiring,
    F: Wfst<L, W>,
{
    (0..fst.num_states() as StateId).any(|state| {
        fst.transitions(state)
            .iter()
            .any(|trans| fst.is_valid_state(trans.to) && trans.input.is_none())
    })
}

/// Determinize a WFST.
///
/// This produces a deterministic WFST where for each state, all outgoing
/// transitions have distinct input labels. The weighted language is preserved.
///
/// # Type Parameters
///
/// - `L`: Label type (must be Eq + Hash + Clone + Ord)
/// - `W`: Weight type (must be DivisibleSemiring + TotallyOrderedSemiring)
/// - `F`: WFST type
///
/// # Requirements
///
/// The weight semiring must implement `TotallyOrderedSemiring` to ensure safe
/// weight comparisons. This is a compile-time guarantee that replaces the
/// previous runtime fallback when `PartialOrd` comparisons returned `None`.
///
/// # Returns
///
/// A new determinized WFST, or an error if determinization fails.
///
/// # Example
///
/// ```ignore
/// use lling_llang::algorithms::{determinize, DeterminizeConfig};
///
/// let mut fst = build_some_wfst();
/// let det_fst = determinize(&fst, DeterminizeConfig::standard())?;
/// ```
pub fn determinize<L, W, F>(fst: &F, config: DeterminizeConfig) -> Result<F, DeterminizeError>
where
    L: Clone + Eq + Hash + Ord + Debug,
    W: DivisibleSemiring + TotallyOrderedSemiring + Clone + Debug + Hash + Eq,
    F: MutableWfst<L, W> + Wfst<L, W> + Default,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(F::default());
    }

    let start = fst.start();
    if start == NO_STATE {
        return Err(DeterminizeError::NoStartState);
    }

    if config.remove_epsilon_first && has_input_epsilon_transitions(fst) {
        let mut epsilon_free = fst.clone();
        remove_epsilon(
            &mut epsilon_free,
            EpsilonRemovalConfig {
                connect: false,
                ..EpsilonRemovalConfig::default()
            },
        )
        .map_err(|err| DeterminizeError::NotDeterminizable {
            reason: format!("epsilon removal before determinization failed: {}", err),
        })?;

        let mut next_config = config.clone();
        next_config.remove_epsilon_first = false;
        return determinize(&epsilon_free, next_config);
    }

    // Create the output WFST
    let mut result = F::default();

    // Map from weighted subset to output state ID
    let mut subset_to_state: HashMap<Vec<(StateId, W)>, StateId> = HashMap::new();

    // Queue of (output_state, weighted_subset) pairs to process
    let mut queue: VecDeque<(StateId, WeightedSubset<W>)> = VecDeque::new();

    // Initial subset: {(start, 1̄)}
    let mut initial_subset: WeightedSubset<W> = BTreeMap::new();
    initial_subset.insert(start, W::one());

    // Create initial state in result
    let initial_state = result.add_state();
    result.set_start(initial_state);

    let initial_key = subset_key(&initial_subset);
    subset_to_state.insert(initial_key, initial_state);
    queue.push_back((initial_state, initial_subset));

    // Main determinization loop
    while let Some((output_state, subset)) = queue.pop_front() {
        // Check state limit
        if let Some(limit) = config.max_states {
            if result.num_states() > limit {
                return Err(DeterminizeError::StateLimitExceeded { limit });
            }
        }

        // Compute final weight for this subset
        // Final weight is ⊕ of all final weights for states in the subset
        let mut final_weight = W::zero();
        for (&state, residual) in &subset {
            if fst.is_final(state) {
                let fw = fst.final_weight(state);
                // Final weight = residual ⊗ original_final_weight
                final_weight = final_weight.plus(&residual.times(&fw));
            }
        }
        if !final_weight.is_zero() {
            result.set_final(output_state, final_weight);
        }

        // Group outgoing transitions by input label
        let mut label_to_targets: HashMap<Option<L>, Vec<(StateId, W, Option<L>)>> = HashMap::new();

        for (&state, residual) in &subset {
            for trans in fst.transitions(state) {
                if !fst.is_valid_state(trans.to) {
                    continue;
                }

                // Combined weight = residual ⊗ arc_weight
                let combined = residual.times(&trans.weight);

                label_to_targets
                    .entry(trans.input.clone())
                    .or_default()
                    .push((trans.to, combined, trans.output.clone()));
            }
        }

        // Process each input label
        for (input_label, targets) in label_to_targets {
            let Some(input_label) = input_label else {
                return Err(DeterminizeError::NotDeterminizable {
                    reason: "input-epsilon transitions remain after epsilon preprocessing"
                        .to_string(),
                });
            };

            // Build the target weighted subset
            let mut target_subset: WeightedSubset<W> = BTreeMap::new();

            let mut output_label: Option<Option<L>> = None;

            for (target_state, weight, out) in &targets {
                // Merge weights for same target state using ⊕
                target_subset
                    .entry(*target_state)
                    .and_modify(|w| *w = w.plus(weight))
                    .or_insert_with(|| weight.clone());

                match &output_label {
                    Some(existing) if existing != out => {
                        return Err(DeterminizeError::NotDeterminizable {
                            reason: format!(
                                "conflicting output labels for input {:?}: {:?} and {:?}",
                                input_label, existing, out
                            ),
                        });
                    }
                    None => output_label = Some(out.clone()),
                    _ => {}
                }
            }

            if target_subset.is_empty() {
                continue;
            }

            // Normalize: find minimum weight and factor it out
            let min_w = min_weight(&target_subset);

            // Normalized subset: divide each weight by the minimum
            let mut normalized_subset: WeightedSubset<W> = BTreeMap::new();
            for (&state, weight) in &target_subset {
                // normalized = weight / min_w
                if let Some(normalized) = weight.divide(&min_w) {
                    normalized_subset.insert(state, normalized);
                } else {
                    // Division failed, use original (shouldn't happen for valid semirings)
                    normalized_subset.insert(state, weight.clone());
                }
            }

            // Look up or create state for normalized subset
            let normalized_key = subset_key(&normalized_subset);
            let target_output_state = if let Some(&existing) = subset_to_state.get(&normalized_key)
            {
                existing
            } else {
                let new_state = result.add_state();
                subset_to_state.insert(normalized_key, new_state);
                queue.push_back((new_state, normalized_subset));
                new_state
            };

            // Add transition with minimum weight
            let trans = WeightedTransition {
                from: output_state,
                to: target_output_state,
                input: Some(input_label),
                output: output_label.unwrap_or(None),
                weight: min_w,
            };
            result.add_transition(trans);
        }
    }

    // Optionally connect (trim) the result
    if config.connect_after {
        connect(&mut result, ConnectConfig::trim());
    }

    Ok(result)
}

/// Check if a WFST is deterministic.
///
/// A WFST is deterministic if:
/// 1. It has at most one start state
/// 2. For each state, all outgoing transitions have distinct input labels
/// 3. There are no epsilon transitions on the input
pub fn is_deterministic<L, W, F>(fst: &F) -> bool
where
    L: Clone + Eq + Hash,
    W: Semiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return true;
    }

    let start = fst.start();
    if start == NO_STATE {
        return true; // Empty language is deterministic
    }

    for state in 0..n {
        let state_id = state as StateId;
        let mut seen_labels: std::collections::HashSet<Option<&L>> =
            std::collections::HashSet::new();

        for trans in fst.transitions(state_id) {
            if !fst.is_valid_state(trans.to) {
                continue;
            }

            // Epsilon input makes it non-deterministic
            if trans.input.is_none() {
                return false;
            }

            // Duplicate input label makes it non-deterministic
            if !seen_labels.insert(trans.input.as_ref()) {
                return false;
            }
        }
    }

    true
}

/// Count the degree of non-determinism for a WFST.
///
/// Returns the maximum number of transitions with the same input label
/// from any single state. A deterministic WFST has degree 1.
pub fn non_determinism_degree<L, W, F>(fst: &F) -> usize
where
    L: Clone + Eq + Hash,
    W: Semiring,
    F: Wfst<L, W>,
{
    let n = fst.num_states();
    if n == 0 {
        return 0;
    }

    let mut max_degree = 0;

    for state in 0..n {
        let state_id = state as StateId;
        let mut label_counts: HashMap<Option<&L>, usize> = HashMap::new();

        for trans in fst.transitions(state_id) {
            if !fst.is_valid_state(trans.to) {
                continue;
            }

            *label_counts.entry(trans.input.as_ref()).or_insert(0) += 1;
        }

        if let Some(&count) = label_counts.values().max() {
            max_degree = max_degree.max(count);
        }
    }

    max_degree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::{VectorWfst, VectorWfstBuilder};

    // Property-based tests
    mod property_tests {
        use super::*;
        use crate::test_utils::arb_deterministic_wfst_tropical;
        use proptest::prelude::*;

        proptest! {
            /// Determinize should always produce a deterministic output.
            #[test]
            fn determinize_produces_deterministic(
                fst in arb_deterministic_wfst_tropical(8, 3)
            ) {
                let result = determinize(&fst, DeterminizeConfig::standard());
                if let Ok(det_fst) = result {
                    prop_assert!(
                        is_deterministic(&det_fst),
                        "Determinized FST should be deterministic"
                    );
                }
            }

            /// Determinizing a deterministic FST should not increase state count significantly.
            #[test]
            fn determinize_already_deterministic(
                fst in arb_deterministic_wfst_tropical(8, 3)
            ) {
                if fst.num_states() == 0 {
                    return Ok(());
                }

                prop_assert!(is_deterministic(&fst), "Test FST should be deterministic");

                let result = determinize(&fst, DeterminizeConfig::standard());
                if let Ok(det_fst) = result {
                    // Determinizing a deterministic FST shouldn't dramatically increase states
                    // (it may increase slightly due to trim/connect behavior)
                    prop_assert!(
                        det_fst.num_states() <= fst.num_states() + 2,
                        "Determinizing deterministic FST grew from {} to {} states",
                        fst.num_states(),
                        det_fst.num_states()
                    );
                }
            }

            /// Determinize is idempotent: det(det(F)) ≈ det(F).
            #[test]
            fn determinize_idempotent(
                fst in arb_deterministic_wfst_tropical(6, 2)
            ) {
                if fst.num_states() == 0 {
                    return Ok(());
                }

                let det1 = determinize(&fst, DeterminizeConfig::standard());
                if let Ok(det1_fst) = det1 {
                    let det2 = determinize(&det1_fst, DeterminizeConfig::standard());
                    if let Ok(det2_fst) = det2 {
                        // Both should be deterministic
                        prop_assert!(is_deterministic(&det1_fst));
                        prop_assert!(is_deterministic(&det2_fst));

                        // State count should be similar (idempotent)
                        prop_assert!(
                            det2_fst.num_states() <= det1_fst.num_states() + 1,
                            "det(det(F)) has {} states, det(F) has {}",
                            det2_fst.num_states(),
                            det1_fst.num_states()
                        );
                    }
                }
            }

            /// Non-determinism degree should be 1 for deterministic FSTs.
            #[test]
            fn non_determinism_degree_deterministic(
                fst in arb_deterministic_wfst_tropical(8, 3)
            ) {
                if fst.num_states() == 0 {
                    return Ok(());
                }

                let degree = non_determinism_degree(&fst);
                prop_assert!(
                    degree <= 1,
                    "Deterministic FST should have degree 0 or 1, got {}",
                    degree
                );
            }
        }
    }

    fn build_deterministic_fst() -> VectorWfst<char, TropicalWeight> {
        // Already deterministic: 0 --a--> 1 --b--> 2 (final)
        VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
            .arc(1, Some('b'), Some('b'), 2, TropicalWeight::new(2.0))
            .final_state(2, TropicalWeight::one())
            .build()
    }

    fn build_non_deterministic_fst() -> VectorWfst<char, TropicalWeight> {
        // Non-deterministic: two 'a' transitions from state 0
        // 0 --a(1)--> 1
        // 0 --a(2)--> 2
        // 1 --b--> 3 (final)
        // 2 --c--> 3 (final)
        let mut fst = VectorWfst::new();
        fst.add_states(4);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_arc(0, Some('a'), Some('a'), 2, TropicalWeight::new(2.0));
        fst.add_arc(1, Some('b'), Some('b'), 3, TropicalWeight::new(1.0));
        fst.add_arc(2, Some('c'), Some('c'), 3, TropicalWeight::new(1.0));
        fst.set_final(3, TropicalWeight::one());
        fst
    }

    fn build_diamond_non_det() -> VectorWfst<char, TropicalWeight> {
        // Diamond: two paths with same labels, should merge
        // 0 --a--> 1 --b--> 3 (final)
        // 0 --a--> 2 --b--> 3 (final)
        let mut fst = VectorWfst::new();
        fst.add_states(4);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_arc(0, Some('a'), Some('a'), 2, TropicalWeight::new(2.0));
        fst.add_arc(1, Some('b'), Some('b'), 3, TropicalWeight::new(1.0));
        fst.add_arc(2, Some('b'), Some('b'), 3, TropicalWeight::new(1.0));
        fst.set_final(3, TropicalWeight::one());
        fst
    }

    fn build_epsilon_chain() -> VectorWfst<char, TropicalWeight> {
        let mut fst = VectorWfst::new();
        fst.add_states(4);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_epsilon(1, 2, TropicalWeight::new(0.5));
        fst.add_arc(2, Some('b'), Some('b'), 3, TropicalWeight::new(2.0));
        fst.set_final(3, TropicalWeight::one());
        fst
    }

    fn build_output_conflict_fst() -> VectorWfst<char, TropicalWeight> {
        let mut fst = VectorWfst::new();
        fst.add_states(3);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0));
        fst.add_arc(0, Some('a'), Some('y'), 2, TropicalWeight::new(2.0));
        fst.set_final(1, TropicalWeight::one());
        fst.set_final(2, TropicalWeight::one());
        fst
    }

    fn build_output_only_input_epsilon_fst() -> VectorWfst<char, TropicalWeight> {
        let mut fst = VectorWfst::new();
        fst.add_states(2);
        fst.set_start(0);
        fst.add_arc(0, None, Some('x'), 1, TropicalWeight::one());
        fst.set_final(1, TropicalWeight::one());
        fst
    }

    fn build_malformed_target_fst() -> VectorWfst<char, TropicalWeight> {
        let mut fst = VectorWfst::new();
        fst.add_states(2);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_arc(0, Some('a'), Some('a'), 99, TropicalWeight::new(0.1));
        fst.add_arc(0, None, None, 100, TropicalWeight::new(0.2));
        fst.set_final(1, TropicalWeight::one());
        fst
    }

    #[test]
    fn test_is_deterministic_true() {
        let fst = build_deterministic_fst();
        assert!(is_deterministic(&fst));
    }

    #[test]
    fn test_is_deterministic_false() {
        let fst = build_non_deterministic_fst();
        assert!(!is_deterministic(&fst));
    }

    #[test]
    fn test_non_determinism_degree() {
        let det_fst = build_deterministic_fst();
        assert_eq!(non_determinism_degree(&det_fst), 1);

        let nondet_fst = build_non_deterministic_fst();
        assert_eq!(non_determinism_degree(&nondet_fst), 2);
    }

    #[test]
    fn test_determinism_predicates_ignore_malformed_targets() {
        let fst = build_malformed_target_fst();

        assert!(is_deterministic(&fst));
        assert_eq!(non_determinism_degree(&fst), 1);
        assert!(!has_input_epsilon_transitions(&fst));
    }

    #[test]
    fn test_determinize_already_deterministic() {
        let fst = build_deterministic_fst();
        let result = determinize(&fst, DeterminizeConfig::standard())
            .expect("algorithms/determinize.rs: required value was None/Err");

        assert!(is_deterministic(&result));
        // Should have same structure (3 states for chain)
        assert!(result.num_states() <= 3);
    }

    #[test]
    fn test_determinize_simple_non_det() {
        let fst = build_non_deterministic_fst();
        assert!(!is_deterministic(&fst));

        let result = determinize(&fst, DeterminizeConfig::standard())
            .expect("algorithms/determinize.rs: required value was None/Err");
        assert!(is_deterministic(&result));

        // After 'a', we should be in a merged state
        // Then 'b' and 'c' lead to different outcomes
    }

    #[test]
    fn test_determinize_diamond() {
        let fst = build_diamond_non_det();
        assert!(!is_deterministic(&fst));

        let result = determinize(&fst, DeterminizeConfig::standard())
            .expect("algorithms/determinize.rs: required value was None/Err");
        assert!(is_deterministic(&result));

        // Diamond should collapse to: 0 --a--> 1 --b--> 2
        // Output should have fewer states than input
        assert!(result.num_states() <= fst.num_states());
    }

    #[test]
    fn test_determinize_removes_true_epsilons_first() {
        let fst = build_epsilon_chain();
        assert!(has_input_epsilon_transitions(&fst));

        let result = determinize(&fst, DeterminizeConfig::standard())
            .expect("epsilon preprocessing should make the chain determinizable");

        assert!(is_deterministic(&result));
        assert!(!has_input_epsilon_transitions(&result));
    }

    #[test]
    fn test_determinize_without_epsilon_prepass_rejects_input_epsilon() {
        let fst = build_epsilon_chain();
        let result = determinize(
            &fst,
            DeterminizeConfig {
                remove_epsilon_first: false,
                ..DeterminizeConfig::standard()
            },
        );

        assert!(matches!(
            result,
            Err(DeterminizeError::NotDeterminizable { .. })
        ));
    }

    #[test]
    fn test_determinize_rejects_conflicting_outputs() {
        let fst = build_output_conflict_fst();
        let result = determinize(&fst, DeterminizeConfig::standard());

        assert!(matches!(
            result,
            Err(DeterminizeError::NotDeterminizable { .. })
        ));
    }

    #[test]
    fn test_determinize_rejects_output_only_input_epsilon() {
        let fst = build_output_only_input_epsilon_fst();
        let result = determinize(&fst, DeterminizeConfig::standard());

        assert!(matches!(
            result,
            Err(DeterminizeError::NotDeterminizable { .. })
        ));
    }

    #[test]
    fn test_determinize_ignores_malformed_transition_targets() {
        let fst = build_malformed_target_fst();
        let result = determinize(
            &fst,
            DeterminizeConfig {
                connect_after: false,
                ..DeterminizeConfig::standard()
            },
        )
        .expect("malformed targets should be ignored during determinization");

        assert!(is_deterministic(&result));
        assert_eq!(result.num_states(), 2);

        let transitions = result.transitions(result.start());
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 1);
        assert_eq!(transitions[0].input, Some('a'));
        assert_eq!(transitions[0].weight.value(), 1.0);
    }

    #[test]
    fn test_determinize_empty() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();
        let result = determinize(&fst, DeterminizeConfig::standard())
            .expect("algorithms/determinize.rs: required value was None/Err");
        assert_eq!(result.num_states(), 0);
    }

    #[test]
    fn test_determinize_weight_preservation() {
        // Create a simple non-deterministic FST where we can verify weights
        // 0 --a(w=1)--> 1 (final, w=0)
        // 0 --a(w=3)--> 2 (final, w=0)
        // After determinization: 0 --a(w=min(1,3)=1)--> {1,2} (final)
        // The final weight should incorporate the residuals
        let mut fst = VectorWfst::new();
        fst.add_states(3);
        fst.set_start(0);
        fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
        fst.add_arc(0, Some('a'), Some('a'), 2, TropicalWeight::new(3.0));
        fst.set_final(1, TropicalWeight::one()); // w=0
        fst.set_final(2, TropicalWeight::one()); // w=0

        let result = determinize(&fst, DeterminizeConfig::standard())
            .expect("algorithms/determinize.rs: required value was None/Err");
        assert!(is_deterministic(&result));

        // Should have 2 states: start and merged final
        assert_eq!(result.num_states(), 2);

        // The 'a' transition should have weight 1.0 (minimum)
        let start = result.start();
        let trans: Vec<_> = result.transitions(start).to_vec();
        assert_eq!(trans.len(), 1);
        assert_eq!(trans[0].weight.value(), 1.0);
    }

    #[test]
    fn test_determinize_state_limit() {
        let fst = build_non_deterministic_fst();

        let config = DeterminizeConfig {
            max_states: Some(1), // Very low limit
            remove_epsilon_first: false,
            connect_after: false,
        };

        let result = determinize(&fst, config);
        assert!(matches!(
            result,
            Err(DeterminizeError::StateLimitExceeded { .. })
        ));
    }
}
