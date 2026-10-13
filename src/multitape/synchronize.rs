//! Synchronization operations for multi-tape WFSTs.
//!
//! Synchronization ensures that all tapes advance at approximately the same rate,
//! bounding the "delay" between tapes.

use std::collections::HashMap;
use std::hash::Hash;

use super::label::MultiTapeLabel;
use super::traits::MultiTapeWfst;
use super::transition::MultiTapeTransition;
use super::vector::VectorMultiTapeWfst;
use crate::semiring::Semiring;
use crate::wfst::{
    synchronize as scalar_synchronize, MutableWfst, StateId, VectorWfst, WeightedTransition,
    NO_STATE,
};

#[cfg(test)]
use super::builder::MultiTapeWfstBuilder;

/// Configuration for synchronization.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Maximum allowed delay between any two tapes.
    pub max_delay: usize,
    /// Whether to allow epsilon-only transitions.
    pub allow_epsilon: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            max_delay: 1,
            allow_epsilon: true,
        }
    }
}

impl SyncConfig {
    /// Create a new config with the given max delay.
    pub fn new(max_delay: usize) -> Self {
        Self {
            max_delay,
            allow_epsilon: true,
        }
    }

    /// Set whether epsilon transitions are allowed.
    pub fn with_epsilon(mut self, allow: bool) -> Self {
        self.allow_epsilon = allow;
        self
    }
}

/// Delay state for each tape.
///
/// Tracks how many symbols ahead or behind each tape is relative to a reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TapeDelay<const N: usize> {
    /// Delay for each tape (positive = ahead, negative = behind).
    delays: [i32; N],
}

impl<const N: usize> TapeDelay<N> {
    /// Create a zero-delay state.
    pub fn zero() -> Self {
        Self { delays: [0; N] }
    }

    /// Get the delay for a specific tape.
    pub fn delay(&self, tape: usize) -> i32 {
        self.delays.get(tape).copied().unwrap_or(0)
    }

    /// Update delay when consuming a label.
    pub fn consume<L>(&self, label: &MultiTapeLabel<L, N>) -> Self {
        let mut new_delays = self.delays;
        for (i, l) in label.labels.iter().enumerate() {
            if l.is_some() {
                new_delays[i] += 1;
            }
        }
        // Normalize to minimum delay of 0
        let min = *new_delays.iter().min().unwrap_or(&0);
        for d in &mut new_delays {
            *d -= min;
        }
        Self { delays: new_delays }
    }

    /// Check if this delay is within bounds.
    pub fn is_bounded(&self, max_delay: usize) -> bool {
        let max = *self.delays.iter().max().unwrap_or(&0);
        let min = *self.delays.iter().min().unwrap_or(&0);
        (max - min) as usize <= max_delay
    }

    /// Get the maximum delay difference.
    pub fn max_difference(&self) -> usize {
        let max = *self.delays.iter().max().unwrap_or(&0);
        let min = *self.delays.iter().min().unwrap_or(&0);
        (max - min) as usize
    }
}

impl<const N: usize> Default for TapeDelay<N> {
    fn default() -> Self {
        Self::zero()
    }
}

/// A synchronized multi-tape WFST.
///
/// This wrapper ensures that all tapes advance at approximately the same rate.
#[derive(Debug, Clone)]
pub struct SynchronizedMultiTape<L, W: Semiring, const N: usize> {
    /// The synchronized WFST.
    wfst: VectorMultiTapeWfst<L, W, N>,
    /// Configuration used for synchronization.
    config: SyncConfig,
}

impl<L, W: Semiring, const N: usize> SynchronizedMultiTape<L, W, N> {
    /// Get the underlying WFST.
    pub fn wfst(&self) -> &VectorMultiTapeWfst<L, W, N> {
        &self.wfst
    }

    /// Consume and return the underlying WFST.
    pub fn into_wfst(self) -> VectorMultiTapeWfst<L, W, N> {
        self.wfst
    }

    /// Get the configuration used.
    pub fn config(&self) -> &SyncConfig {
        &self.config
    }
}

/// Synchronize a multi-tape WFST to ensure bounded delay between tapes.
///
/// This creates a new WFST where states are pairs of (original state, delay state).
/// Only transitions that keep the delay within bounds are included.
pub fn synchronize<L, W, T, const N: usize>(
    source: &T,
    config: SyncConfig,
) -> SynchronizedMultiTape<L, W, N>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
    T: MultiTapeWfst<L, W, N>,
{
    let mut builder = VectorMultiTapeWfst::<L, W, N>::new();

    // Map from (source_state, delay) to new state
    let mut state_map: HashMap<(StateId, TapeDelay<N>), StateId> = HashMap::new();

    // Queue of states to process
    let mut queue: Vec<(StateId, TapeDelay<N>)> = Vec::new();

    // Add initial state
    let initial_delay = TapeDelay::zero();
    let initial_source = source.start();
    let initial_new = builder.add_state();
    state_map.insert((initial_source, initial_delay.clone()), initial_new);
    builder.set_start(initial_new);
    queue.push((initial_source, initial_delay));

    // Check if initial state is final
    if source.is_final(initial_source) {
        builder.set_final(initial_new, source.final_weight(initial_source));
    }

    // Process states
    while let Some((src_state, delay)) = queue.pop() {
        let current = *state_map
            .get(&(src_state, delay.clone()))
            .expect("State not found");

        for trans in source.transitions(src_state) {
            // Skip epsilon-only transitions if not allowed
            if !config.allow_epsilon && trans.is_epsilon() {
                continue;
            }

            // Compute new delay
            let new_delay = delay.consume(&trans.labels);

            // Check if within bounds
            if !new_delay.is_bounded(config.max_delay) {
                continue;
            }

            // Get or create target state
            let target_key = (trans.to, new_delay.clone());
            let target = if let Some(&s) = state_map.get(&target_key) {
                s
            } else {
                let s = builder.add_state();
                state_map.insert(target_key.clone(), s);
                queue.push((trans.to, new_delay.clone()));

                // Check if final
                if source.is_final(trans.to) {
                    builder.set_final(s, source.final_weight(trans.to));
                }

                s
            };

            // Add transition
            builder.add_transition(MultiTapeTransition::new(
                current,
                trans.labels.clone(),
                target,
                trans.weight.clone(),
            ));
        }
    }

    SynchronizedMultiTape {
        wfst: builder,
        config,
    }
}

/// Check whether every reachable path has a delay at most `max_delay`.
pub fn has_bounded_delay<L, W, T, const N: usize>(source: &T, max_delay: usize) -> bool
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
    T: MultiTapeWfst<L, W, N>,
{
    compute_max_delay(source).is_some_and(|delay| delay <= max_delay)
}

/// Compute the exact maximum pairwise tape delay over all reachable paths.
///
/// Returns `None` when some reachable cycle advances two tapes by different
/// amounts or the result cannot be represented. Such a cycle can be repeated
/// to make the delay unbounded. The analysis uses the scalar WFST delay proof
/// for every tape pair and retains the greatest finite result.
pub fn compute_max_delay<L, W, T, const N: usize>(source: &T) -> Option<usize>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
    T: MultiTapeWfst<L, W, N>,
{
    let mut max_found = 0;
    for first_tape in 0..N {
        for second_tape in first_tape + 1..N {
            let pair = scalar_pair_projection(source, first_tape, second_tape)?;
            let delay = scalar_synchronize::compute_max_delay(&pair)?;
            max_found = max_found.max(delay);
        }
    }
    Some(max_found)
}

fn scalar_pair_projection<L, W, T, const N: usize>(
    source: &T,
    first_tape: usize,
    second_tape: usize,
) -> Option<VectorWfst<L, W>>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
    T: MultiTapeWfst<L, W, N>,
{
    let states: Vec<StateId> = source.states().collect();
    if states.len() >= NO_STATE as usize {
        return None;
    }
    let mut pair = VectorWfst::new();
    let mut remap = HashMap::with_capacity(states.len());
    for &state in &states {
        let mapped = pair.add_state();
        if remap.insert(state, mapped).is_some() {
            return None;
        }
        if source.is_final(state) {
            pair.set_final(mapped, source.final_weight(state));
        }
    }
    pair.set_start(remap.get(&source.start()).copied().unwrap_or(NO_STATE));
    for &state in &states {
        let from = remap[&state];
        for transition in source.transitions(state) {
            let Some(&to) = remap.get(&transition.to) else {
                continue;
            };
            pair.add_transition(WeightedTransition::new(
                from,
                transition.tape_label(first_tape).cloned(),
                transition.tape_label(second_tape).cloned(),
                to,
                transition.weight.clone(),
            ));
        }
    }
    Some(pair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_tape_delay_zero() {
        let delay: TapeDelay<3> = TapeDelay::zero();
        assert_eq!(delay.delay(0), 0);
        assert_eq!(delay.delay(1), 0);
        assert_eq!(delay.delay(2), 0);
    }

    #[test]
    fn test_tape_delay_consume() {
        let delay: TapeDelay<3> = TapeDelay::zero();

        // Consume label on tape 0 only
        let label: MultiTapeLabel<char, 3> = MultiTapeLabel::single(0, 'a');
        let new_delay = delay.consume(&label);

        // Tape 0 should be ahead, normalized
        assert!(new_delay.delay(0) > new_delay.delay(1));
    }

    #[test]
    fn test_tape_delay_bounded() {
        let delay: TapeDelay<2> = TapeDelay { delays: [0, 0] };
        assert!(delay.is_bounded(0));
        assert!(delay.is_bounded(1));

        let delay2: TapeDelay<2> = TapeDelay { delays: [0, 2] };
        assert!(!delay2.is_bounded(1));
        assert!(delay2.is_bounded(2));
    }

    #[test]
    fn test_sync_config() {
        let config = SyncConfig::new(2);
        assert_eq!(config.max_delay, 2);
        assert!(config.allow_epsilon);

        let config2 = config.with_epsilon(false);
        assert!(!config2.allow_epsilon);
    }

    fn make_synchronized_mt() -> VectorMultiTapeWfst<char, TropicalWeight, 2> {
        let mut builder = MultiTapeWfstBuilder::<char, TropicalWeight, 2>::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();
        let s2 = builder.add_final_state(TropicalWeight::one());

        builder.set_start(s0);

        // Synchronized transitions (both tapes advance together)
        builder.add_transition(
            s0,
            s1,
            MultiTapeLabel::from_values(['a', 'x']),
            TropicalWeight::one(),
        );
        builder.add_transition(
            s1,
            s2,
            MultiTapeLabel::from_values(['b', 'y']),
            TropicalWeight::one(),
        );

        builder.build()
    }

    fn make_unsynchronized_mt() -> VectorMultiTapeWfst<char, TropicalWeight, 2> {
        let mut builder = MultiTapeWfstBuilder::<char, TropicalWeight, 2>::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();
        let s2 = builder.add_state();
        let s3 = builder.add_final_state(TropicalWeight::one());

        builder.set_start(s0);

        // First tape advances twice before second tape
        builder.add_transition(
            s0,
            s1,
            MultiTapeLabel::single(0, 'a'),
            TropicalWeight::one(),
        );
        builder.add_transition(
            s1,
            s2,
            MultiTapeLabel::single(0, 'b'),
            TropicalWeight::one(),
        );
        // Then second tape catches up
        builder.add_transition(
            s2,
            s3,
            MultiTapeLabel::pair(0, 'c', 1, 'x'),
            TropicalWeight::one(),
        );

        builder.build()
    }

    #[test]
    fn test_has_bounded_delay_synchronized() {
        let mt = make_synchronized_mt();
        assert!(has_bounded_delay(&mt, 0));
        assert!(has_bounded_delay(&mt, 1));
    }

    #[test]
    fn test_has_bounded_delay_unsynchronized() {
        let mt = make_unsynchronized_mt();
        assert!(!has_bounded_delay(&mt, 0));
        assert!(!has_bounded_delay(&mt, 1));
        assert!(has_bounded_delay(&mt, 2));
    }

    #[test]
    fn test_compute_max_delay() {
        let mt = make_synchronized_mt();
        assert_eq!(compute_max_delay(&mt), Some(0));

        let mt2 = make_unsynchronized_mt();
        assert_eq!(compute_max_delay(&mt2), Some(2));
    }

    #[test]
    fn divergent_cycle_has_no_finite_maximum_delay() {
        let mut builder = MultiTapeWfstBuilder::<char, TropicalWeight, 3>::new();
        let state = builder.add_state();
        builder.set_start(state);
        builder.set_final(state, TropicalWeight::one());
        builder.add_transition(
            state,
            state,
            MultiTapeLabel::new([Some('a'), None, None]),
            TropicalWeight::one(),
        );
        let graph = builder.build();
        assert_eq!(compute_max_delay(&graph), None);
        assert!(!has_bounded_delay(&graph, usize::MAX));
    }

    #[test]
    fn balanced_cycle_has_zero_maximum_delay() {
        let mut builder = MultiTapeWfstBuilder::<char, TropicalWeight, 3>::new();
        let state = builder.add_state();
        builder.set_start(state);
        builder.set_final(state, TropicalWeight::one());
        builder.add_transition(
            state,
            state,
            MultiTapeLabel::from_values(['a', 'b', 'c']),
            TropicalWeight::one(),
        );
        let graph = builder.build();
        assert_eq!(compute_max_delay(&graph), Some(0));
        assert!(has_bounded_delay(&graph, 0));
    }

    #[test]
    fn test_synchronize_already_sync() {
        let mt = make_synchronized_mt();
        let synced = synchronize(&mt, SyncConfig::new(0));

        assert_eq!(synced.wfst().num_states(), 3);
        assert_eq!(synced.wfst().num_transitions(), 2);
    }

    #[test]
    fn test_synchronize_removes_unsync_paths() {
        let mt = make_unsynchronized_mt();
        let synced = synchronize(&mt, SyncConfig::new(1));

        // With max delay 1, the path should be blocked
        assert_eq!(synced.wfst().num_transitions(), 1);
    }

    #[test]
    fn test_synchronize_allows_bounded_paths() {
        let mt = make_unsynchronized_mt();
        let synced = synchronize(&mt, SyncConfig::new(2));

        // With max delay 2, the path should be allowed
        assert_eq!(synced.wfst().num_transitions(), 3);
    }
}
