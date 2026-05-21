//! Lookahead scoring for beam search optimization.
//!
//! This module provides lookahead tables that estimate the future cost of
//! reaching a final state from any given state. These estimates improve
//! beam search pruning by allowing comparison of paths at different stages
//! of completion.
//!
//! ## Overview
//!
//! During beam search, hypotheses at different positions have incomparable
//! scores - a hypothesis that has processed 3 words naturally has a higher
//! accumulated cost than one that has processed 1 word. Lookahead scoring
//! normalizes these by adding an estimate of the remaining cost.
//!
//! ## Algorithm
//!
//! The lookahead score for state q is V(q), the backward potential:
//! V(q) = -log(Σ_{paths from q to final} exp(-path_weight))
//!
//! This is exactly what log-semiring pushing computes, so we can reuse
//! those potentials for lookahead.
//!
//! ## Usage
//!
//! There are two main use cases:
//!
//! 1. **Pre-computed lookahead**: Build a lookahead table before search
//! 2. **Dynamic lookahead**: Compute lookahead on-the-fly during search
//!
//! ## References
//!
//! - Mohri, Pereira, Riley (2002): "WFSTs in Speech Recognition"
//! - NVIDIA GPU Decoder papers discuss lookahead in context of beam pruning

use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{StateId, Wfst, NO_STATE};

use super::log_push::{compute_log_potentials, LogPushError};

/// Configuration for lookahead table construction.
#[derive(Clone, Debug)]
pub struct LookaheadConfig {
    /// Whether to cache the lookahead table.
    pub cache: bool,
    /// Use infinity for unreachable states instead of error.
    pub allow_unreachable: bool,
}

impl Default for LookaheadConfig {
    fn default() -> Self {
        Self {
            cache: true,
            allow_unreachable: true,
        }
    }
}

/// A precomputed lookahead table for efficient future-cost estimation.
///
/// The table maps each state to its backward potential, which represents
/// the total probability mass of all paths from that state to any final state.
///
/// # Usage
///
/// ```ignore
/// use lling_llang::optimization::{build_lookahead_table, LookaheadConfig};
///
/// let table = build_lookahead_table(&fst, LookaheadConfig::default())?;
///
/// // During beam search:
/// for hyp in hypotheses {
///     let lookahead = table.get(hyp.state);
///     let normalized_score = hyp.score.times(&lookahead);
///     // Use normalized_score for pruning comparison
/// }
/// ```
#[derive(Clone, Debug)]
pub struct LookaheadTable {
    /// Backward potentials for each state.
    potentials: Vec<LogWeight>,
    /// Total weight (potential at start state).
    total_weight: LogWeight,
    /// Number of reachable states.
    num_reachable: usize,
}

impl LookaheadTable {
    /// Get the lookahead score for a state.
    ///
    /// Returns the backward potential V(q), which is the total probability
    /// mass of all paths from state q to any final state.
    ///
    /// # Arguments
    ///
    /// * `state` - The state to query
    ///
    /// # Returns
    ///
    /// The backward potential as a LogWeight. Returns LogWeight::zero() for
    /// out-of-bounds states.
    pub fn get(&self, state: StateId) -> LogWeight {
        let idx = state as usize;
        if idx < self.potentials.len() {
            self.potentials[idx].clone()
        } else {
            LogWeight::zero()
        }
    }

    /// Get the lookahead score as a raw f64 value.
    ///
    /// Returns the backward potential value, or f64::INFINITY if unreachable.
    pub fn get_value(&self, state: StateId) -> f64 {
        let idx = state as usize;
        if idx < self.potentials.len() {
            self.potentials[idx].value()
        } else {
            f64::INFINITY
        }
    }

    /// Check if a state is reachable (has a finite lookahead).
    pub fn is_reachable(&self, state: StateId) -> bool {
        let idx = state as usize;
        if idx < self.potentials.len() {
            !self.potentials[idx].is_zero()
        } else {
            false
        }
    }

    /// Get the total weight of the WFST.
    ///
    /// This is the backward potential at the start state, representing
    /// the total probability mass of all paths through the WFST.
    pub fn total_weight(&self) -> &LogWeight {
        &self.total_weight
    }

    /// Get the number of states with reachable final states.
    pub fn num_reachable(&self) -> usize {
        self.num_reachable
    }

    /// Get the total number of states in the table.
    pub fn num_states(&self) -> usize {
        self.potentials.len()
    }

    /// Compute a normalized score by combining accumulated weight with lookahead.
    ///
    /// This gives a score that estimates the total path weight if we were to
    /// complete the path from the current state to a final state.
    ///
    /// # Arguments
    ///
    /// * `state` - The current state
    /// * `accumulated` - The accumulated weight to reach this state
    ///
    /// # Returns
    ///
    /// The combined score: accumulated ⊗ lookahead(state)
    pub fn normalize_score(&self, state: StateId, accumulated: &LogWeight) -> LogWeight {
        accumulated.times(&self.get(state))
    }
}

/// Build a lookahead table for a WFST.
///
/// Computes backward potentials for all states, which can be used during
/// beam search to estimate future costs and improve pruning decisions.
///
/// # Arguments
///
/// * `fst` - The WFST to build lookahead for
/// * `config` - Configuration options
///
/// # Returns
///
/// * `Ok(LookaheadTable)` - The lookahead table
/// * `Err(LogPushError)` - If the WFST has no path to final states
///
/// # Complexity
///
/// O(|Q| + |E|) for acyclic WFSTs
///
/// # Example
///
/// ```ignore
/// use lling_llang::optimization::{build_lookahead_table, LookaheadConfig};
///
/// let table = build_lookahead_table(&recognition_fst, LookaheadConfig::default())?;
///
/// // Use during beam search
/// let future_estimate = table.get(current_state);
/// let total_estimate = current_score.times(&future_estimate);
/// ```
pub fn build_lookahead_table<L, F>(
    fst: &F,
    config: LookaheadConfig,
) -> Result<LookaheadTable, LogPushError>
where
    L: Clone,
    F: Wfst<L, LogWeight>,
{
    let n = fst.num_states();
    if n == 0 {
        return Ok(LookaheadTable {
            potentials: Vec::new(),
            total_weight: LogWeight::zero(),
            num_reachable: 0,
        });
    }

    if fst.start() == NO_STATE {
        return Err(LogPushError::NoStartState);
    }

    // Compute backward potentials using log semiring
    let potentials = match compute_log_potentials(fst) {
        Ok(p) => p,
        Err(e) => {
            if config.allow_unreachable {
                // Return table with all zeros (unreachable)
                return Ok(LookaheadTable {
                    potentials: vec![LogWeight::zero(); n],
                    total_weight: LogWeight::zero(),
                    num_reachable: 0,
                });
            } else {
                return Err(e);
            }
        }
    };

    // Get total weight from start state
    let start = fst.start() as usize;
    let total_weight = if start < potentials.len() {
        potentials[start].clone()
    } else {
        LogWeight::zero()
    };

    // Count reachable states
    let num_reachable = potentials.iter().filter(|p| !p.is_zero()).count();

    Ok(LookaheadTable {
        potentials,
        total_weight,
        num_reachable,
    })
}

/// Compute lookahead for a single state on-the-fly.
///
/// This is useful when you only need the lookahead for a few states
/// and don't want to precompute the entire table.
///
/// Note: For repeated queries, use `build_lookahead_table` instead.
///
/// # Arguments
///
/// * `fst` - The WFST
/// * `state` - The state to compute lookahead for
///
/// # Returns
///
/// The backward potential for the state, or LogWeight::zero() if unreachable.
pub fn compute_lookahead_single<L, F>(fst: &F, state: StateId) -> LogWeight
where
    L: Clone,
    F: Wfst<L, LogWeight>,
{
    // For a single state, we still need to compute all backward potentials
    // due to the recursive nature of the computation.
    // This function is mainly for API convenience.
    match compute_log_potentials(fst) {
        Ok(potentials) => {
            let idx = state as usize;
            if idx < potentials.len() {
                potentials[idx].clone()
            } else {
                LogWeight::zero()
            }
        }
        Err(_) => LogWeight::zero(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::{MutableWfst as MutableWfstTrait, VectorWfst};

    fn build_simple_chain() -> VectorWfst<char, LogWeight> {
        let mut fst = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s2, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s1, Some('b'), Some('b'), s2, LogWeight::new(2.0));
        fst
    }

    fn build_parallel_paths() -> VectorWfst<char, LogWeight> {
        let mut fst = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(2.0));
        fst
    }

    #[test]
    fn test_build_lookahead_chain() {
        let fst = build_simple_chain();
        let table =
            build_lookahead_table(&fst, LookaheadConfig::default()).expect("Should build table");

        assert_eq!(table.num_states(), 3);
        assert_eq!(table.num_reachable(), 3);

        // State 2 (final): lookahead = 0.0 (one in log space)
        assert!(table.get(2).approx_eq(&LogWeight::one(), 0.001));

        // State 1: lookahead = 2.0 (cost to reach final)
        assert!(table.get(1).approx_eq(&LogWeight::new(2.0), 0.001));

        // State 0: lookahead = 3.0 (total path cost)
        assert!(table.get(0).approx_eq(&LogWeight::new(3.0), 0.001));
    }

    #[test]
    fn test_lookahead_normalize_score() {
        let fst = build_simple_chain();
        let table =
            build_lookahead_table(&fst, LookaheadConfig::default()).expect("Should build table");

        // If we've accumulated 1.0 weight to reach state 1,
        // the normalized score should be 1.0 + 2.0 = 3.0
        let accumulated = LogWeight::new(1.0);
        let normalized = table.normalize_score(1, &accumulated);

        assert!(
            normalized.approx_eq(&LogWeight::new(3.0), 0.001),
            "Normalized score should be 3.0, got {:?}",
            normalized
        );
    }

    #[test]
    fn test_lookahead_parallel() {
        let fst = build_parallel_paths();
        let table =
            build_lookahead_table(&fst, LookaheadConfig::default()).expect("Should build table");

        // State 1 (final): lookahead = 0.0
        assert!(table.get(1).approx_eq(&LogWeight::one(), 0.001));

        // State 0: lookahead = logadd(1.0, 2.0) ≈ 0.687
        let expected = -((-1.0_f64).exp() + (-2.0_f64).exp()).ln();
        assert!(
            table.get(0).approx_eq(&LogWeight::new(expected), 0.001),
            "State 0 lookahead should be {:?}, got {:?}",
            expected,
            table.get(0)
        );
    }

    #[test]
    fn test_lookahead_empty() {
        let fst: VectorWfst<char, LogWeight> = VectorWfst::new();
        let table =
            build_lookahead_table(&fst, LookaheadConfig::default()).expect("Should handle empty");

        assert_eq!(table.num_states(), 0);
        assert_eq!(table.num_reachable(), 0);
        assert!(table.total_weight().is_zero());
    }

    #[test]
    fn test_lookahead_out_of_bounds() {
        let fst = build_simple_chain();
        let table =
            build_lookahead_table(&fst, LookaheadConfig::default()).expect("Should build table");

        // Out of bounds state should return zero
        assert!(table.get(100).is_zero());
        assert_eq!(table.get_value(100), f64::INFINITY);
        assert!(!table.is_reachable(100));
    }

    #[test]
    fn test_compute_lookahead_single() {
        let fst = build_simple_chain();

        let lookahead_0 = compute_lookahead_single(&fst, 0);
        let lookahead_1 = compute_lookahead_single(&fst, 1);
        let lookahead_2 = compute_lookahead_single(&fst, 2);

        assert!(lookahead_0.approx_eq(&LogWeight::new(3.0), 0.001));
        assert!(lookahead_1.approx_eq(&LogWeight::new(2.0), 0.001));
        assert!(lookahead_2.approx_eq(&LogWeight::one(), 0.001));
    }

    #[test]
    fn test_lookahead_total_weight() {
        let fst = build_simple_chain();
        let table =
            build_lookahead_table(&fst, LookaheadConfig::default()).expect("Should build table");

        // Total weight should equal the lookahead at start state
        assert!(table.total_weight().approx_eq(&LogWeight::new(3.0), 0.001));
    }

    #[test]
    fn test_lookahead_unreachable_state() {
        let mut fst: VectorWfst<char, LogWeight> = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        let s3 = fst.add_state(); // This state has no path to final
        fst.set_start(s0);
        fst.set_final(s2, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
        fst.add_arc(s1, Some('b'), Some('b'), s2, LogWeight::new(2.0));
        // s3 has no transitions

        let table =
            build_lookahead_table(&fst, LookaheadConfig::default()).expect("Should build table");

        // States 0, 1, 2 should be reachable
        assert!(table.is_reachable(s0));
        assert!(table.is_reachable(s1));
        assert!(table.is_reachable(s2));

        // State 3 should not be reachable to finals
        assert!(!table.is_reachable(s3));

        // num_reachable should be 3
        assert_eq!(table.num_reachable(), 3);
    }
}
