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
use crate::wfst::{VectorWfst, MutableWfst, Wfst, StateId, NO_STATE};

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
        let grammar = LatticeGrammar::new(fst, RescorePass::FirstPass)
            .with_density(5.0);

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
