//! Lattice rescoring for multi-pass ASR recognition.
//!
//! This module provides functionality for rescoring word lattices with
//! improved language models or acoustic models in a multi-pass recognition
//! framework.
//!
//! ## Multi-Pass Recognition
//!
//! 1. **First pass**: Generate word lattices with simpler models (e.g., bigram LM)
//! 2. **Second pass**: Use lattice as "grammar" G for rescoring with better models
//!
//! ## Optimization
//!
//! Rescoring at the L∘G level provides:
//! - ~50% reduction in median lattice states/arcs
//! - 9× speedup compared to unoptimized rescoring
//!
//! ## Example
//!
//! ```text
//! First-pass: C∘L∘G → 0.18× RT
//! Optimized:  C∘min(det(L∘G)) → 0.02× RT
//! ```
//!
//! ## References
//!
//! - Mohri et al., "Speech Recognition with WFSTs" Section 6

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst, NO_STATE};

/// Configuration for lattice rescoring.
#[derive(Clone, Debug)]
pub struct RescoreConfig {
    /// Whether to apply determinization before rescoring.
    pub determinize: bool,

    /// Whether to apply minimization before rescoring.
    pub minimize: bool,

    /// Pruning threshold (prune paths with score > best + threshold).
    pub pruning_threshold: Option<f64>,

    /// Maximum number of states in rescored lattice.
    pub max_states: Option<usize>,

    /// Weight for interpolating old and new scores.
    /// new_score = (1 - alpha) * old_score + alpha * rescore_weight
    pub interpolation_alpha: f64,
}

impl Default for RescoreConfig {
    fn default() -> Self {
        Self {
            determinize: true,
            minimize: true,
            pruning_threshold: None,
            max_states: None,
            interpolation_alpha: 1.0, // Full replacement by default
        }
    }
}

/// Rescoring pass type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RescorePass {
    /// First pass with simple models.
    FirstPass,
    /// Second pass with improved models.
    SecondPass,
    /// Additional passes for further refinement.
    AdditionalPass(u32),
}

/// Grammar for rescoring (typically the lattice from previous pass).
#[derive(Clone, Debug)]
pub struct LatticeGrammar<L: Clone, W: Semiring> {
    /// The lattice as an FST.
    pub fst: VectorWfst<L, W>,

    /// Which pass generated this lattice.
    pub source_pass: RescorePass,

    /// Statistics about the source lattice.
    pub stats: LatticeStats,
}

/// Statistics about a lattice.
#[derive(Clone, Debug, Default)]
pub struct LatticeStats {
    /// Number of states.
    pub num_states: usize,

    /// Number of arcs.
    pub num_arcs: usize,

    /// Average arcs per state.
    pub avg_arcs_per_state: f64,

    /// Lattice density (arcs per frame).
    pub density: Option<f64>,
}

impl<L: Clone + Send + Sync, W: Semiring + Clone> LatticeGrammar<L, W> {
    /// Create a new lattice grammar from an FST.
    pub fn new(fst: VectorWfst<L, W>, source_pass: RescorePass) -> Self {
        let num_states = fst.num_states();
        let num_arcs: usize = (0..num_states as StateId)
            .map(|s| fst.transitions(s).len())
            .sum();

        let avg_arcs_per_state = if num_states > 0 {
            num_arcs as f64 / num_states as f64
        } else {
            0.0
        };

        let stats = LatticeStats {
            num_states,
            num_arcs,
            avg_arcs_per_state,
            density: None,
        };

        Self {
            fst,
            source_pass,
            stats,
        }
    }

    /// Set the density (arcs per frame).
    pub fn with_density(mut self, density: f64) -> Self {
        self.stats.density = Some(density);
        self
    }
}

/// Result of lattice rescoring.
#[derive(Clone, Debug)]
pub struct RescoreResult<L: Clone, W: Semiring> {
    /// The rescored lattice.
    pub lattice: VectorWfst<L, W>,

    /// Statistics about the rescoring.
    pub stats: RescoreStats,
}

/// Statistics about rescoring.
#[derive(Clone, Debug, Default)]
pub struct RescoreStats {
    /// Number of states in input lattice.
    pub input_states: usize,

    /// Number of states in output lattice.
    pub output_states: usize,

    /// Number of arcs in input lattice.
    pub input_arcs: usize,

    /// Number of arcs in output lattice.
    pub output_arcs: usize,

    /// State reduction ratio.
    pub state_reduction: f64,

    /// Arc reduction ratio.
    pub arc_reduction: f64,
}

impl RescoreStats {
    /// Compute reduction ratios.
    pub fn compute_reductions(&mut self) {
        if self.input_states > 0 {
            self.state_reduction = 1.0 - (self.output_states as f64 / self.input_states as f64);
        }
        if self.input_arcs > 0 {
            self.arc_reduction = 1.0 - (self.output_arcs as f64 / self.input_arcs as f64);
        }
    }
}

/// Rescore a lattice with a new language model.
///
/// This function takes a word lattice from a first-pass recognition
/// and rescores it using a better language model.
///
/// # Arguments
///
/// * `lattice` - The input word lattice
/// * `new_lm` - The new language model transducer
/// * `config` - Configuration options
///
/// # Returns
///
/// The rescored lattice with statistics.
pub fn rescore_lattice<L, W>(
    lattice: &LatticeGrammar<L, W>,
    _new_lm: &VectorWfst<L, W>,
    _config: &RescoreConfig,
) -> RescoreResult<L, W>
where
    L: Clone + Eq + std::hash::Hash + Default + Send + Sync,
    W: Semiring + Clone,
{
    let mut stats = RescoreStats {
        input_states: lattice.fst.num_states(),
        input_arcs: count_arcs(&lattice.fst),
        ..Default::default()
    };

    // For now, return the input lattice unchanged
    // Full implementation would:
    // 1. Compose lattice with new LM: L ∘ G_new
    // 2. Apply determinization if configured
    // 3. Apply minimization if configured
    // 4. Apply pruning if configured

    let result_lattice = clone_lattice(&lattice.fst);

    stats.output_states = result_lattice.num_states();
    stats.output_arcs = count_arcs(&result_lattice);
    stats.compute_reductions();

    RescoreResult {
        lattice: result_lattice,
        stats,
    }
}

/// Count total arcs in an FST.
fn count_arcs<L, W>(fst: &VectorWfst<L, W>) -> usize
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    (0..fst.num_states() as StateId)
        .map(|s| fst.transitions(s).len())
        .sum()
}

/// Clone a lattice.
fn clone_lattice<L, W>(fst: &VectorWfst<L, W>) -> VectorWfst<L, W>
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

/// Multi-pass rescoring with multiple language models.
///
/// Performs iterative rescoring with progressively better models.
pub fn multi_pass_rescore<L, W>(
    initial_lattice: &LatticeGrammar<L, W>,
    lm_sequence: &[VectorWfst<L, W>],
    config: &RescoreConfig,
) -> Vec<RescoreResult<L, W>>
where
    L: Clone + Eq + std::hash::Hash + Default + Send + Sync,
    W: Semiring + Clone,
{
    let mut results = Vec::with_capacity(lm_sequence.len());
    let mut current_lattice = initial_lattice.clone();

    for (i, lm) in lm_sequence.iter().enumerate() {
        let result = rescore_lattice(&current_lattice, lm, config);

        // Update current lattice for next pass
        current_lattice = LatticeGrammar::new(
            result.lattice.clone(),
            RescorePass::AdditionalPass(i as u32 + 1),
        );

        results.push(result);
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::LogWeight;

    #[test]
    fn test_rescore_config_default() {
        let config = RescoreConfig::default();
        assert!(config.determinize);
        assert!(config.minimize);
        assert!(config.pruning_threshold.is_none());
        assert_eq!(config.interpolation_alpha, 1.0);
    }

    #[test]
    fn test_rescore_pass() {
        assert_eq!(RescorePass::FirstPass, RescorePass::FirstPass);
        assert_ne!(RescorePass::FirstPass, RescorePass::SecondPass);
        assert_eq!(
            RescorePass::AdditionalPass(1),
            RescorePass::AdditionalPass(1)
        );
    }

    #[test]
    fn test_lattice_grammar_new() {
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some(1), Some(1), s1, LogWeight::one());

        let grammar = LatticeGrammar::new(fst, RescorePass::FirstPass);

        assert_eq!(grammar.stats.num_states, 2);
        assert_eq!(grammar.stats.num_arcs, 1);
        assert_eq!(grammar.stats.avg_arcs_per_state, 0.5);
    }

    #[test]
    fn test_lattice_grammar_with_density() {
        let fst = VectorWfst::<u32, LogWeight>::new();
        let grammar = LatticeGrammar::new(fst, RescorePass::FirstPass).with_density(5.0);

        assert_eq!(grammar.stats.density, Some(5.0));
    }

    #[test]
    fn test_rescore_stats_compute_reductions() {
        let mut stats = RescoreStats {
            input_states: 100,
            output_states: 50,
            input_arcs: 200,
            output_arcs: 100,
            ..Default::default()
        };

        stats.compute_reductions();

        assert_eq!(stats.state_reduction, 0.5);
        assert_eq!(stats.arc_reduction, 0.5);
    }

    #[test]
    fn test_rescore_lattice_empty() {
        let fst = VectorWfst::<u32, LogWeight>::new();
        let lattice = LatticeGrammar::new(fst, RescorePass::FirstPass);
        let lm = VectorWfst::<u32, LogWeight>::new();
        let config = RescoreConfig::default();

        let result = rescore_lattice(&lattice, &lm, &config);

        assert_eq!(result.stats.input_states, 0);
        assert_eq!(result.stats.output_states, 0);
    }

    #[test]
    fn test_rescore_lattice_simple() {
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some(1), Some(1), s1, LogWeight::new(0.5));

        let lattice = LatticeGrammar::new(fst, RescorePass::FirstPass);
        let lm = VectorWfst::<u32, LogWeight>::new();
        let config = RescoreConfig::default();

        let result = rescore_lattice(&lattice, &lm, &config);

        assert_eq!(result.stats.input_states, 2);
        assert_eq!(result.stats.output_states, 2);
        assert_eq!(result.stats.input_arcs, 1);
        assert_eq!(result.stats.output_arcs, 1);
    }

    #[test]
    fn test_multi_pass_rescore() {
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();
        let s0 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s0, LogWeight::one());

        let lattice = LatticeGrammar::new(fst, RescorePass::FirstPass);

        let lm1 = VectorWfst::<u32, LogWeight>::new();
        let lm2 = VectorWfst::<u32, LogWeight>::new();
        let lm_sequence = vec![lm1, lm2];

        let config = RescoreConfig::default();
        let results = multi_pass_rescore(&lattice, &lm_sequence, &config);

        assert_eq!(results.len(), 2);
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
    // RescoreConfig Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Default config enables determinization.
        #[test]
        fn default_config_determinize(_seed in any::<u64>()) {
            let config = RescoreConfig::default();
            prop_assert!(config.determinize);
        }

        /// Default config enables minimization.
        #[test]
        fn default_config_minimize(_seed in any::<u64>()) {
            let config = RescoreConfig::default();
            prop_assert!(config.minimize);
        }

        /// Default config has no pruning threshold.
        #[test]
        fn default_config_no_pruning(_seed in any::<u64>()) {
            let config = RescoreConfig::default();
            prop_assert!(config.pruning_threshold.is_none());
        }

        /// Default config has no max states limit.
        #[test]
        fn default_config_no_max_states(_seed in any::<u64>()) {
            let config = RescoreConfig::default();
            prop_assert!(config.max_states.is_none());
        }

        /// Default config has interpolation_alpha of 1.0.
        #[test]
        fn default_config_alpha(_seed in any::<u64>()) {
            let config = RescoreConfig::default();
            prop_assert!((config.interpolation_alpha - 1.0).abs() < 1e-10);
        }
    }

    // -------------------------------------------------------------------------
    // RescorePass Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// RescorePass equality is reflexive.
        #[test]
        fn pass_equality_reflexive(pass_num in 0u32..100) {
            let pass = RescorePass::AdditionalPass(pass_num);
            prop_assert_eq!(pass, pass);
        }

        /// FirstPass != SecondPass.
        #[test]
        fn first_ne_second(_seed in any::<u64>()) {
            prop_assert_ne!(RescorePass::FirstPass, RescorePass::SecondPass);
        }

        /// Different AdditionalPass numbers are different.
        #[test]
        fn different_additional_passes(a in 0u32..100, b in 100u32..200) {
            prop_assert_ne!(
                RescorePass::AdditionalPass(a),
                RescorePass::AdditionalPass(b)
            );
        }
    }

    // -------------------------------------------------------------------------
    // LatticeStats Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Default LatticeStats has zeros.
        #[test]
        fn default_lattice_stats(_seed in any::<u64>()) {
            let stats = LatticeStats::default();
            prop_assert_eq!(stats.num_states, 0);
            prop_assert_eq!(stats.num_arcs, 0);
            prop_assert!((stats.avg_arcs_per_state - 0.0).abs() < 1e-10);
            prop_assert!(stats.density.is_none());
        }
    }

    // -------------------------------------------------------------------------
    // LatticeGrammar Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        /// LatticeGrammar counts states correctly.
        #[test]
        fn lattice_grammar_state_count(num_states in 0usize..10) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            for _ in 0..num_states {
                fst.add_state();
            }

            let grammar = LatticeGrammar::new(fst, RescorePass::FirstPass);
            prop_assert_eq!(grammar.stats.num_states, num_states);
        }

        /// LatticeGrammar counts arcs correctly.
        #[test]
        fn lattice_grammar_arc_count(num_states in 2usize..6) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            let states: Vec<_> = (0..num_states).map(|_| fst.add_state()).collect();

            // Add arcs: each state (except last) connects to next
            for i in 0..states.len() - 1 {
                fst.add_arc(states[i], Some(i as u32), Some(i as u32), states[i + 1], LogWeight::one());
            }

            let grammar = LatticeGrammar::new(fst, RescorePass::FirstPass);
            prop_assert_eq!(grammar.stats.num_arcs, num_states - 1);
        }

        /// LatticeGrammar avg_arcs_per_state is correct.
        #[test]
        fn lattice_grammar_avg_arcs(num_states in 2usize..6) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            let states: Vec<_> = (0..num_states).map(|_| fst.add_state()).collect();

            for i in 0..states.len() - 1 {
                fst.add_arc(states[i], Some(i as u32), Some(i as u32), states[i + 1], LogWeight::one());
            }

            let grammar = LatticeGrammar::new(fst, RescorePass::FirstPass);
            let expected_avg = (num_states - 1) as f64 / num_states as f64;
            prop_assert!((grammar.stats.avg_arcs_per_state - expected_avg).abs() < 1e-10);
        }

        /// LatticeGrammar preserves source_pass.
        #[test]
        fn lattice_grammar_source_pass(pass_num in 0u32..100) {
            let fst = VectorWfst::<u32, LogWeight>::new();
            let pass = RescorePass::AdditionalPass(pass_num);
            let grammar = LatticeGrammar::new(fst, pass);

            prop_assert_eq!(grammar.source_pass, RescorePass::AdditionalPass(pass_num));
        }

        /// with_density sets density.
        #[test]
        fn lattice_grammar_density(density in 0.0f64..100.0) {
            let fst = VectorWfst::<u32, LogWeight>::new();
            let grammar = LatticeGrammar::new(fst, RescorePass::FirstPass)
                .with_density(density);

            prop_assert_eq!(grammar.stats.density, Some(density));
        }

        /// Empty FST has zero avg_arcs_per_state.
        #[test]
        fn empty_fst_zero_avg(_seed in any::<u64>()) {
            let fst = VectorWfst::<u32, LogWeight>::new();
            let grammar = LatticeGrammar::new(fst, RescorePass::FirstPass);

            prop_assert!((grammar.stats.avg_arcs_per_state - 0.0).abs() < 1e-10);
        }
    }

    // -------------------------------------------------------------------------
    // RescoreStats Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Default RescoreStats has zeros.
        #[test]
        fn default_rescore_stats(_seed in any::<u64>()) {
            let stats = RescoreStats::default();
            prop_assert_eq!(stats.input_states, 0);
            prop_assert_eq!(stats.output_states, 0);
            prop_assert_eq!(stats.input_arcs, 0);
            prop_assert_eq!(stats.output_arcs, 0);
            prop_assert!((stats.state_reduction - 0.0).abs() < 1e-10);
            prop_assert!((stats.arc_reduction - 0.0).abs() < 1e-10);
        }

        /// compute_reductions calculates state reduction correctly.
        #[test]
        fn state_reduction_correct(
            input_states in 1usize..1000,
            output_states in 0usize..1000
        ) {
            let output_states = output_states.min(input_states);
            let mut stats = RescoreStats {
                input_states,
                output_states,
                ..Default::default()
            };

            stats.compute_reductions();

            let expected = 1.0 - (output_states as f64 / input_states as f64);
            prop_assert!((stats.state_reduction - expected).abs() < 1e-10);
        }

        /// compute_reductions calculates arc reduction correctly.
        #[test]
        fn arc_reduction_correct(
            input_arcs in 1usize..1000,
            output_arcs in 0usize..1000
        ) {
            let output_arcs = output_arcs.min(input_arcs);
            let mut stats = RescoreStats {
                input_arcs,
                output_arcs,
                ..Default::default()
            };

            stats.compute_reductions();

            let expected = 1.0 - (output_arcs as f64 / input_arcs as f64);
            prop_assert!((stats.arc_reduction - expected).abs() < 1e-10);
        }

        /// Zero input states means no state reduction computed.
        #[test]
        fn zero_input_no_reduction(_seed in any::<u64>()) {
            let mut stats = RescoreStats {
                input_states: 0,
                output_states: 0,
                ..Default::default()
            };

            stats.compute_reductions();

            // state_reduction should remain 0 (division avoided)
            prop_assert!((stats.state_reduction - 0.0).abs() < 1e-10);
        }
    }

    // -------------------------------------------------------------------------
    // rescore_lattice Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// rescore_lattice on empty lattice returns empty result.
        #[test]
        fn rescore_empty_lattice(_seed in any::<u64>()) {
            let fst = VectorWfst::<u32, LogWeight>::new();
            let lattice = LatticeGrammar::new(fst, RescorePass::FirstPass);
            let lm = VectorWfst::<u32, LogWeight>::new();
            let config = RescoreConfig::default();

            let result = rescore_lattice(&lattice, &lm, &config);

            prop_assert_eq!(result.stats.input_states, 0);
            prop_assert_eq!(result.stats.output_states, 0);
        }

        /// rescore_lattice preserves state count (current passthrough impl).
        #[test]
        fn rescore_preserves_states(num_states in 1usize..10) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            for _ in 0..num_states {
                fst.add_state();
            }
            if num_states > 0 {
                fst.set_start(0);
            }

            let lattice = LatticeGrammar::new(fst, RescorePass::FirstPass);
            let lm = VectorWfst::<u32, LogWeight>::new();
            let config = RescoreConfig::default();

            let result = rescore_lattice(&lattice, &lm, &config);

            prop_assert_eq!(result.stats.input_states, num_states);
            prop_assert_eq!(result.stats.output_states, num_states);
        }

        /// rescore_lattice preserves arc count (current passthrough impl).
        #[test]
        fn rescore_preserves_arcs(num_states in 2usize..6) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            let states: Vec<_> = (0..num_states).map(|_| fst.add_state()).collect();
            fst.set_start(states[0]);

            for i in 0..states.len() - 1 {
                fst.add_arc(states[i], Some(i as u32), Some(i as u32), states[i + 1], LogWeight::one());
            }

            let lattice = LatticeGrammar::new(fst, RescorePass::FirstPass);
            let lm = VectorWfst::<u32, LogWeight>::new();
            let config = RescoreConfig::default();

            let result = rescore_lattice(&lattice, &lm, &config);

            let expected_arcs = num_states - 1;
            prop_assert_eq!(result.stats.input_arcs, expected_arcs);
            prop_assert_eq!(result.stats.output_arcs, expected_arcs);
        }
    }

    // -------------------------------------------------------------------------
    // multi_pass_rescore Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(15))]

        /// multi_pass_rescore returns correct number of results.
        #[test]
        fn multi_pass_result_count(num_passes in 0usize..5) {
            let fst = VectorWfst::<u32, LogWeight>::new();
            let lattice = LatticeGrammar::new(fst, RescorePass::FirstPass);

            let lms: Vec<_> = (0..num_passes)
                .map(|_| VectorWfst::<u32, LogWeight>::new())
                .collect();

            let config = RescoreConfig::default();
            let results = multi_pass_rescore(&lattice, &lms, &config);

            prop_assert_eq!(results.len(), num_passes);
        }

        /// multi_pass_rescore with empty LM sequence returns empty results.
        #[test]
        fn multi_pass_empty_lms(_seed in any::<u64>()) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            let s = fst.add_state();
            fst.set_start(s);
            fst.set_final(s, LogWeight::one());

            let lattice = LatticeGrammar::new(fst, RescorePass::FirstPass);
            let lms: Vec<VectorWfst<u32, LogWeight>> = vec![];

            let config = RescoreConfig::default();
            let results = multi_pass_rescore(&lattice, &lms, &config);

            prop_assert!(results.is_empty());
        }
    }

    // -------------------------------------------------------------------------
    // clone_lattice Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// clone_lattice preserves state count.
        #[test]
        fn clone_lattice_states(num_states in 0usize..10) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            for _ in 0..num_states {
                fst.add_state();
            }

            let cloned = clone_lattice(&fst);
            prop_assert_eq!(cloned.num_states(), num_states);
        }

        /// clone_lattice preserves start state.
        #[test]
        fn clone_lattice_start(num_states in 1usize..10) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            for _ in 0..num_states {
                fst.add_state();
            }
            fst.set_start(0);

            let cloned = clone_lattice(&fst);
            prop_assert_eq!(cloned.start(), 0);
        }

        /// clone_lattice preserves final states.
        #[test]
        fn clone_lattice_finals(num_states in 1usize..5) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            for i in 0..num_states {
                let s = fst.add_state();
                if i % 2 == 0 {
                    fst.set_final(s, LogWeight::new(1.0));
                }
            }

            let cloned = clone_lattice(&fst);

            for i in 0..num_states as StateId {
                prop_assert_eq!(cloned.is_final(i), fst.is_final(i));
            }
        }
    }

    // -------------------------------------------------------------------------
    // count_arcs Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        /// count_arcs returns correct count for linear FST.
        #[test]
        fn count_arcs_linear(num_states in 2usize..10) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            let states: Vec<_> = (0..num_states).map(|_| fst.add_state()).collect();

            for i in 0..states.len() - 1 {
                fst.add_arc(states[i], Some(i as u32), Some(i as u32), states[i + 1], LogWeight::one());
            }

            prop_assert_eq!(count_arcs(&fst), num_states - 1);
        }

        /// count_arcs returns 0 for empty FST.
        #[test]
        fn count_arcs_empty(_seed in any::<u64>()) {
            let fst = VectorWfst::<u32, LogWeight>::new();
            prop_assert_eq!(count_arcs(&fst), 0);
        }

        /// count_arcs returns 0 for FST with no arcs.
        #[test]
        fn count_arcs_no_arcs(num_states in 1usize..10) {
            let mut fst = VectorWfst::<u32, LogWeight>::new();
            for _ in 0..num_states {
                fst.add_state();
            }

            prop_assert_eq!(count_arcs(&fst), 0);
        }
    }
}
