//! Synchronization algorithm for WFSTs.
//!
//! Synchronization normalizes input/output label shifting along paths to produce
//! a synchronized transducer where the delay (difference between input and output
//! lengths) is either zero or varies strictly monotonically.
//!
//! # Background
//!
//! For a path π, the **delay** is d[π] = |o[π]| - |i[π]| (output length minus input).
//! A transducer has **bounded delays** iff all cycles have zero delay.
//!
//! The **string delay** represents the accumulated difference:
//! - If d ≥ 0: suffix of output of length d
//! - If d < 0: suffix of input of length |d|
//!
//! # Algorithm
//!
//! States in the synchronized transducer are triplets (q, x, y) where:
//! - q: original state ID
//! - x: input delay string (residual input consumed but not yet output)
//! - y: output delay string (residual output not yet matched by input)
//!
//! Only one of x or y is non-empty at any time.
//!
//! Based on Mohri's "Weighted Automata Algorithms" (Section 6.5).
//!
//! # Complexity
//!
//! O((|Q| + |E|)(|Σ|^d[T] + |Δ|^d[T])) where d[T] is the maximum delay.
//!
//! # Example
//!
//! ```
//! use lling_llang::wfst::{VectorWfst, VectorWfstBuilder, MutableWfst, Wfst, LazyWfst};
//! use lling_llang::wfst::synchronize::{synchronize, has_bounded_delay};
//! use lling_llang::semiring::{Semiring, TropicalWeight};
//!
//! // Create a simple transducer with delay
//! // This has bounded delay: output 'x' then 'y' for inputs 'a' then 'b'
//! let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
//!     .add_states(3)
//!     .start(0)
//!     .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())  // delay = 0
//!     .arc(1, Some('b'), None, 2, TropicalWeight::one())       // delay = -1 (input only)
//!     .arc(2, None, Some('y'), 2, TropicalWeight::one())       // NOT a cycle that repeats
//!     .final_state(2, TropicalWeight::one())
//!     .build();
//!
//! // Note: The above still has a self-loop issue. Let's use a simpler example:
//! let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
//!     .add_states(3)
//!     .start(0)
//!     .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())  // delay = 0
//!     .arc(1, Some('b'), None, 2, TropicalWeight::one())       // delay = -1
//!     .final_state(2, TropicalWeight::one())
//!     .build();
//!
//! // Check if transducer has bounded delay
//! assert!(has_bounded_delay(&fst));
//!
//! // Synchronize the transducer
//! let synced = synchronize(&fst);
//!
//! // The synchronized transducer normalizes delays
//! assert_eq!(synced.start(), 0);
//! ```

use std::collections::VecDeque;
use std::hash::Hash;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use super::lazy::{LazyState, LazyWfstWrapper, StateSource};
use super::traits::Wfst;
use super::transition::WeightedTransition;
use super::types::{StateId, NO_STATE};
use crate::semiring::Semiring;

#[cfg(test)]
use super::traits::LazyWfst;
#[cfg(test)]
use super::vector::{VectorWfst, VectorWfstBuilder};

// =============================================================================
// String Delay Type
// =============================================================================

/// Represents a string delay (accumulated input or output difference).
///
/// In a synchronized transducer, at most one of input_delay or output_delay
/// is non-empty at any point.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StringDelay<L> {
    /// Input delay: consumed input symbols not yet matched by output.
    pub input: SmallVec<[L; 4]>,
    /// Output delay: produced output symbols not yet matched by input.
    pub output: SmallVec<[L; 4]>,
}

impl<L> StringDelay<L> {
    /// Create an empty delay (synchronized state).
    #[inline]
    pub fn empty() -> Self {
        Self {
            input: SmallVec::new(),
            output: SmallVec::new(),
        }
    }

    /// Check if the delay is empty (fully synchronized).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.input.is_empty() && self.output.is_empty()
    }

    /// Get the total delay length.
    #[inline]
    pub fn len(&self) -> usize {
        self.input.len() + self.output.len()
    }
}

impl<L: Clone> StringDelay<L> {
    /// Get the first symbol from the delay (car operation).
    ///
    /// Returns the first input symbol if input delay is non-empty,
    /// otherwise the first output symbol if output delay is non-empty.
    #[inline]
    pub fn car_input(&self) -> Option<L> {
        self.input.first().cloned()
    }

    /// Get the first output symbol from the delay.
    #[inline]
    pub fn car_output(&self) -> Option<L> {
        self.output.first().cloned()
    }

    /// Remove and return the first symbols (cdr operation).
    pub fn cdr(&self) -> Self {
        let input = if self.input.len() > 1 {
            self.input[1..].iter().cloned().collect()
        } else {
            SmallVec::new()
        };

        let output = if self.output.len() > 1 {
            self.output[1..].iter().cloned().collect()
        } else {
            SmallVec::new()
        };

        Self { input, output }
    }
}

impl<L: Eq> StringDelay<L> {
    /// Synchronize accumulated input and output.
    ///
    /// Cancels common prefix between input and output delays,
    /// leaving only the residual difference.
    pub fn sync(mut input: SmallVec<[L; 4]>, mut output: SmallVec<[L; 4]>) -> Self {
        let common_prefix_len = input
            .iter()
            .zip(output.iter())
            .take_while(|(input_symbol, output_symbol)| input_symbol == output_symbol)
            .count();

        if common_prefix_len > 0 {
            drop(input.drain(0..common_prefix_len));
            drop(output.drain(0..common_prefix_len));
        }

        Self { input, output }
    }
}

impl<L: Clone + Eq> StringDelay<L> {
    /// Extend input delay with new symbols.
    pub fn extend_input(&self, symbols: impl IntoIterator<Item = L>) -> Self {
        let mut input: SmallVec<[L; 4]> = self.input.clone();
        input.extend(symbols);
        Self::sync(input, self.output.clone())
    }

    /// Extend output delay with new symbols.
    pub fn extend_output(&self, symbols: impl IntoIterator<Item = L>) -> Self {
        let mut output: SmallVec<[L; 4]> = self.output.clone();
        output.extend(symbols);
        Self::sync(self.input.clone(), output)
    }
}

fn synchronized_transition_step<L: Clone + Eq>(
    delay: &StringDelay<L>,
    input_label: Option<&L>,
    output_label: Option<&L>,
) -> (Option<L>, Option<L>, StringDelay<L>) {
    let mut queued_input: SmallVec<[L; 4]> = delay.input.clone();
    let mut queued_output: SmallVec<[L; 4]> = delay.output.clone();

    if let Some(label) = input_label {
        queued_input.push(label.clone());
    }
    if let Some(label) = output_label {
        queued_output.push(label.clone());
    }

    let out_input = if queued_input.is_empty() {
        None
    } else {
        Some(queued_input.remove(0))
    };
    let out_output = if queued_output.is_empty() {
        None
    } else {
        Some(queued_output.remove(0))
    };

    let next_delay = StringDelay::sync(queued_input, queued_output);
    (out_input, out_output, next_delay)
}

#[inline]
fn next_state_id(len: usize) -> Option<StateId> {
    if len < NO_STATE as usize {
        Some(len as StateId)
    } else {
        None
    }
}

// =============================================================================
// Synchronized State
// =============================================================================

/// A state in the synchronized transducer.
///
/// Consists of the original state plus accumulated string delays.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SyncState<L> {
    /// Original state ID from the input transducer.
    pub original: StateId,
    /// Accumulated string delay.
    pub delay: StringDelay<L>,
    /// Whether this is a draining state (emitting residual delay at final).
    pub draining: bool,
}

impl<L: Clone> SyncState<L> {
    /// Create an initial synchronized state.
    pub fn initial(original: StateId) -> Self {
        Self {
            original,
            delay: StringDelay::empty(),
            draining: false,
        }
    }

    /// Create a draining state for final states with non-empty delay.
    pub fn draining(delay: StringDelay<L>) -> Self {
        Self {
            original: NO_STATE, // Special marker for draining states
            delay,
            draining: true,
        }
    }
}

// =============================================================================
// Synchronization Source (Lazy Implementation)
// =============================================================================

#[derive(Clone)]
struct SyncRegistry<L> {
    /// Mapping from SyncState to StateId in the synchronized transducer.
    state_map: FxHashMap<SyncState<L>, StateId>,
    /// Reverse mapping from StateId to SyncState.
    state_index: Vec<SyncState<L>>,
}

/// Lazy synchronization of a WFST.
///
/// Computes synchronized states on demand, following Mohri's algorithm.
#[derive(Clone)]
pub struct SyncSource<L, W, T>
where
    W: Semiring,
    T: Wfst<L, W>,
{
    /// The original transducer.
    fst: T,
    /// Shared registry for synchronized states created during lazy expansion.
    registry: Arc<RwLock<SyncRegistry<L>>>,
    /// Maximum delay bound (for detecting unbounded delays).
    max_delay: usize,
    _phantom: std::marker::PhantomData<W>,
}

impl<L, W, T> SyncSource<L, W, T>
where
    W: Semiring,
    L: Clone + Eq + Hash + Send + Sync,
    T: Wfst<L, W>,
{
    /// Create a new synchronization source.
    ///
    /// # Arguments
    ///
    /// * `fst` - The input transducer to synchronize
    /// * `max_delay` - Maximum allowed delay (for bounded delay check)
    pub fn new(fst: T, max_delay: usize) -> Self {
        let initial_capacity = fst.num_states().max(1);
        let mut state_map =
            FxHashMap::with_capacity_and_hasher(initial_capacity, Default::default());
        let mut state_index = Vec::with_capacity(initial_capacity);

        // Register the initial state
        let start = fst.start();
        if fst.is_valid_state(start) {
            let initial = SyncState::initial(start);
            state_map.insert(initial.clone(), 0);
            state_index.push(initial);
        }

        Self {
            fst,
            registry: Arc::new(RwLock::new(SyncRegistry {
                state_map,
                state_index,
            })),
            max_delay,
            _phantom: std::marker::PhantomData,
        }
    }

    fn read_registry(&self) -> RwLockReadGuard<'_, SyncRegistry<L>> {
        self.registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_registry(&self) -> RwLockWriteGuard<'_, SyncRegistry<L>> {
        self.registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Get the synchronized state for a state ID.
    fn get_sync_state(&self, state: StateId) -> Option<SyncState<L>> {
        self.read_registry()
            .state_index
            .get(state as usize)
            .cloned()
    }

    /// Get or create a state ID for a synchronized state.
    fn get_or_create_state(&self, sync_state: SyncState<L>) -> Option<StateId> {
        let mut registry = self.write_registry();

        if let Some(&id) = registry.state_map.get(&sync_state) {
            return Some(id);
        }

        let id = next_state_id(registry.state_index.len())?;
        registry.state_map.insert(sync_state.clone(), id);
        registry.state_index.push(sync_state);
        Some(id)
    }
}

impl<L, W, T> StateSource<L, W> for SyncSource<L, W, T>
where
    W: Semiring,
    L: Clone + Eq + Hash + Send + Sync,
    T: Wfst<L, W>,
{
    fn compute_state(&self, state: StateId) -> LazyState<L, W> {
        let sync_state = match self.get_sync_state(state) {
            Some(s) => s,
            None => return LazyState::non_final(SmallVec::new()),
        };

        if !sync_state.draining && !self.fst.is_valid_state(sync_state.original) {
            return LazyState::non_final(SmallVec::new());
        }

        let mut transitions: SmallVec<[WeightedTransition<L, W>; 4]> = if sync_state.draining {
            SmallVec::with_capacity(1)
        } else {
            let original = sync_state.original;
            let transition_count = self.fst.transitions(original).len();
            let final_drain = self.fst.is_final(original) && !sync_state.delay.is_empty();
            SmallVec::with_capacity(transition_count.saturating_add(usize::from(final_drain)))
        };

        if sync_state.draining {
            // Draining state: emit one symbol from residual delay
            if sync_state.delay.is_empty() {
                // Fully drained - this is a final state
                return LazyState::final_state(W::one(), SmallVec::new());
            }

            // Create transition to drain one symbol
            let input_label = sync_state.delay.car_input();
            let output_label = sync_state.delay.car_output();
            let next_delay = sync_state.delay.cdr();

            let Some(next_id) = self.get_or_create_state(SyncState::draining(next_delay)) else {
                return LazyState::non_final(transitions);
            };
            transitions.push(WeightedTransition::new(
                state,
                input_label,
                output_label,
                next_id,
                W::one(),
            ));
            LazyState::non_final(transitions)
        } else {
            // Normal state: process transitions from original transducer
            let original = sync_state.original;

            // Check if original is final with empty delay
            if self.fst.is_final(original) && sync_state.delay.is_empty() {
                let final_weight = self.fst.final_weight(original);
                // Process outgoing transitions
                for trans in self.fst.transitions(original) {
                    if let Some(next_trans) = self.compute_transition(state, &sync_state, trans) {
                        transitions.push(next_trans);
                    }
                }
                return LazyState::final_state(final_weight, transitions);
            }

            // Check if original is final with non-empty delay (need to drain)
            if self.fst.is_final(original) && !sync_state.delay.is_empty() {
                let final_weight = self.fst.final_weight(original);

                let input_label = sync_state.delay.car_input();
                let output_label = sync_state.delay.car_output();
                let next_delay = sync_state.delay.cdr();

                if let Some(next_id) = self.get_or_create_state(SyncState::draining(next_delay)) {
                    transitions.push(WeightedTransition::new(
                        state,
                        input_label,
                        output_label,
                        next_id,
                        final_weight,
                    ));
                }
            }

            // Process outgoing transitions
            for trans in self.fst.transitions(original) {
                if let Some(next_trans) = self.compute_transition(state, &sync_state, trans) {
                    transitions.push(next_trans);
                }
            }

            LazyState::non_final(transitions)
        }
    }

    fn start(&self) -> StateId {
        if self.fst.is_valid_state(self.fst.start()) {
            0
        } else {
            NO_STATE
        }
    }

    fn num_states_hint(&self) -> Option<usize> {
        // Upper bound: original states * delay combinations
        // In practice, much smaller due to path constraints
        Some(self.read_registry().state_index.len())
    }
}

impl<L, W, T> SyncSource<L, W, T>
where
    W: Semiring,
    L: Clone + Eq + Hash + Send + Sync,
    T: Wfst<L, W>,
{
    /// Compute a synchronized transition.
    fn compute_transition(
        &self,
        from_state: StateId,
        sync_state: &SyncState<L>,
        trans: &WeightedTransition<L, W>,
    ) -> Option<WeightedTransition<L, W>> {
        if !self.fst.is_valid_state(trans.to) {
            return None;
        }

        let (out_input, out_output, next_delay) = synchronized_transition_step(
            &sync_state.delay,
            trans.input.as_ref(),
            trans.output.as_ref(),
        );

        // Check delay bound
        if next_delay.len() > self.max_delay {
            // Delay exceeded - this path is invalid for bounded-delay transducers
            return None;
        }

        // Create the target synchronized state
        let next_sync = SyncState {
            original: trans.to,
            delay: next_delay,
            draining: false,
        };

        let next_id = self.get_or_create_state(next_sync)?;

        Some(WeightedTransition::new(
            from_state,
            out_input,
            out_output,
            next_id,
            trans.weight,
        ))
    }
}

// =============================================================================
// Mutable Synchronization Source
// =============================================================================

/// Mutable synchronization source that can create new states on demand.
///
/// This is the proper implementation that handles state creation during traversal.
#[derive(Clone)]
pub struct MutableSyncSource<L, W, T>
where
    W: Semiring,
    T: Wfst<L, W>,
{
    /// The original transducer.
    fst: T,
    /// Mapping from SyncState to StateId.
    state_map: FxHashMap<SyncState<L>, StateId>,
    /// Reverse mapping from StateId to SyncState.
    state_index: Vec<SyncState<L>>,
    /// Maximum delay bound.
    max_delay: usize,
    /// Computed transitions cache.
    computed_transitions: FxHashMap<StateId, SmallVec<[WeightedTransition<L, W>; 4]>>,
    /// Final state info cache.
    final_states: FxHashMap<StateId, W>,
    _phantom: std::marker::PhantomData<W>,
}

impl<L, W, T> MutableSyncSource<L, W, T>
where
    W: Semiring,
    L: Clone + Eq + Hash + Send + Sync,
    T: Wfst<L, W>,
{
    /// Create a new mutable synchronization source.
    pub fn new(fst: T, max_delay: usize) -> Self {
        let initial_capacity = fst.num_states().max(1);
        let mut state_map =
            FxHashMap::with_capacity_and_hasher(initial_capacity, Default::default());
        let mut state_index = Vec::with_capacity(initial_capacity);

        // Register the initial state
        let start = fst.start();
        if fst.is_valid_state(start) {
            let initial = SyncState::initial(start);
            state_map.insert(initial.clone(), 0);
            state_index.push(initial);
        }

        Self {
            fst,
            state_map,
            state_index,
            max_delay,
            computed_transitions: FxHashMap::with_capacity_and_hasher(
                initial_capacity,
                Default::default(),
            ),
            final_states: FxHashMap::with_capacity_and_hasher(initial_capacity, Default::default()),
            _phantom: std::marker::PhantomData,
        }
    }

    fn try_get_or_create_state(&mut self, sync_state: SyncState<L>) -> Option<StateId> {
        if let Some(&id) = self.state_map.get(&sync_state) {
            return Some(id);
        }

        let id = next_state_id(self.state_index.len())?;
        self.state_map.insert(sync_state.clone(), id);
        self.state_index.push(sync_state);
        Some(id)
    }

    /// Get or create a state ID for a synchronized state.
    pub fn get_or_create_state(&mut self, sync_state: SyncState<L>) -> StateId {
        self.try_get_or_create_state(sync_state).unwrap_or(NO_STATE)
    }

    /// Expand a state, computing its transitions and final status.
    pub fn expand_state(&mut self, state: StateId) {
        if self.computed_transitions.contains_key(&state) {
            return;
        }

        let sync_state = match self.state_index.get(state as usize) {
            Some(s) => s.clone(),
            None => return,
        };

        if !sync_state.draining && !self.fst.is_valid_state(sync_state.original) {
            self.computed_transitions.insert(state, SmallVec::new());
            return;
        }

        let mut transitions: SmallVec<[WeightedTransition<L, W>; 4]> = if sync_state.draining {
            SmallVec::with_capacity(1)
        } else {
            let original = sync_state.original;
            let transition_count = self.fst.transitions(original).len();
            let final_drain = self.fst.is_final(original) && !sync_state.delay.is_empty();
            SmallVec::with_capacity(transition_count.saturating_add(usize::from(final_drain)))
        };

        if sync_state.draining {
            // Handle draining state
            if sync_state.delay.is_empty() {
                // Fully drained - final state
                self.final_states.insert(state, W::one());
            } else {
                // Emit one symbol
                let input_label = sync_state.delay.car_input();
                let output_label = sync_state.delay.car_output();
                let next_delay = sync_state.delay.cdr();

                let next_sync = SyncState::draining(next_delay);
                if let Some(next_id) = self.try_get_or_create_state(next_sync) {
                    transitions.push(WeightedTransition::new(
                        state,
                        input_label,
                        output_label,
                        next_id,
                        W::one(),
                    ));
                }
            }
        } else {
            let original = sync_state.original;

            // Check final status
            if self.fst.is_final(original) {
                if sync_state.delay.is_empty() {
                    // Final with no delay
                    self.final_states
                        .insert(state, self.fst.final_weight(original));
                } else {
                    // Need to drain
                    let input_label = sync_state.delay.car_input();
                    let output_label = sync_state.delay.car_output();
                    let next_delay = sync_state.delay.cdr();

                    let next_sync = SyncState::draining(next_delay);
                    if let Some(next_id) = self.try_get_or_create_state(next_sync) {
                        transitions.push(WeightedTransition::new(
                            state,
                            input_label,
                            output_label,
                            next_id,
                            self.fst.final_weight(original),
                        ));
                    }
                }
            }

            // Collect transitions first to avoid borrow conflict
            let fst_transitions: Vec<_> = self.fst.transitions(original).to_vec();

            // Process outgoing transitions
            for trans in &fst_transitions {
                if let Some(new_trans) = self.compute_transition(state, &sync_state, trans) {
                    transitions.push(new_trans);
                }
            }
        }

        self.computed_transitions.insert(state, transitions);
    }

    /// Compute a synchronized transition.
    fn compute_transition(
        &mut self,
        from_state: StateId,
        sync_state: &SyncState<L>,
        trans: &WeightedTransition<L, W>,
    ) -> Option<WeightedTransition<L, W>> {
        if !self.fst.is_valid_state(trans.to) {
            return None;
        }

        let (out_input, out_output, next_delay) = synchronized_transition_step(
            &sync_state.delay,
            trans.input.as_ref(),
            trans.output.as_ref(),
        );

        // Check bound
        if next_delay.len() > self.max_delay {
            return None;
        }

        // Create target state
        let next_sync = SyncState {
            original: trans.to,
            delay: next_delay,
            draining: false,
        };

        let next_id = self.try_get_or_create_state(next_sync)?;

        Some(WeightedTransition::new(
            from_state,
            out_input,
            out_output,
            next_id,
            trans.weight,
        ))
    }

    /// Get transitions for a state.
    pub fn transitions(&self, state: StateId) -> &[WeightedTransition<L, W>] {
        self.computed_transitions
            .get(&state)
            .map(|t| t.as_slice())
            .unwrap_or(&[])
    }

    /// Check if a state is final.
    pub fn is_final(&self, state: StateId) -> bool {
        self.final_states.contains_key(&state)
    }

    /// Get final weight.
    pub fn final_weight(&self, state: StateId) -> W {
        self.final_states
            .get(&state)
            .copied()
            .unwrap_or_else(W::zero)
    }

    /// Get start state.
    pub fn start(&self) -> StateId {
        if self.fst.is_valid_state(self.fst.start()) {
            0
        } else {
            NO_STATE
        }
    }

    /// Get number of created states.
    pub fn num_states(&self) -> usize {
        self.state_index.len()
    }

    /// Check if a state has been expanded.
    pub fn is_expanded(&self, state: StateId) -> bool {
        self.computed_transitions.contains_key(&state)
    }
}

// =============================================================================
// Bounded Delay Check
// =============================================================================

#[derive(Clone, Copy, Debug)]
struct DelayArc {
    to: StateId,
    delta: i64,
}

#[derive(Clone, Copy, Debug)]
struct ComponentArc {
    to: usize,
    delta: i64,
}

#[derive(Clone, Copy, Debug)]
struct DelayAnalysis {
    max_delay: usize,
}

#[inline]
fn transition_delay<L, W>(trans: &WeightedTransition<L, W>) -> i64
where
    W: Semiring,
{
    i64::from(trans.output.is_some()) - i64::from(trans.input.is_some())
}

#[inline]
fn checked_abs_usize(value: i64) -> Option<usize> {
    usize::try_from(value.unsigned_abs()).ok()
}

fn analyze_delay_graph<L, W, T>(fst: &T) -> Option<DelayAnalysis>
where
    W: Semiring,
    L: Clone + Eq + Hash + Send + Sync,
    T: Wfst<L, W>,
{
    let start = fst.start();
    if start == NO_STATE || !fst.is_valid_state(start) {
        return Some(DelayAnalysis { max_delay: 0 });
    }

    let num_states = fst.num_states();
    if num_states > NO_STATE as usize {
        return None;
    }

    let mut reachable = vec![false; num_states];
    let mut forward: Vec<SmallVec<[DelayArc; 4]>> =
        (0..num_states).map(|_| SmallVec::new()).collect();
    let mut reverse: Vec<SmallVec<[StateId; 4]>> =
        (0..num_states).map(|_| SmallVec::new()).collect();

    let mut stack = vec![start];
    reachable[start as usize] = true;

    while let Some(state) = stack.pop() {
        let state_idx = state as usize;

        for trans in fst.transitions(state) {
            if !fst.is_valid_state(trans.to) {
                continue;
            }

            let to_idx = trans.to as usize;
            let delta = transition_delay(trans);
            forward[state_idx].push(DelayArc {
                to: trans.to,
                delta,
            });
            reverse[to_idx].push(state);

            if !reachable[to_idx] {
                reachable[to_idx] = true;
                stack.push(trans.to);
            }
        }
    }

    let reachable_count = reachable
        .iter()
        .filter(|&&is_reachable| is_reachable)
        .count();
    let mut seen = vec![false; num_states];
    let mut finish_order = Vec::with_capacity(reachable_count);

    for (state_idx, &is_reachable) in reachable.iter().enumerate() {
        if !is_reachable || seen[state_idx] {
            continue;
        }

        let state = state_idx as StateId;
        seen[state_idx] = true;
        let mut dfs_stack = vec![(state, false)];

        while let Some((state, expanded)) = dfs_stack.pop() {
            let idx = state as usize;

            if expanded {
                finish_order.push(state);
                continue;
            }

            dfs_stack.push((state, true));
            for arc in forward[idx].iter().rev() {
                let to_idx = arc.to as usize;
                if reachable[to_idx] && !seen[to_idx] {
                    seen[to_idx] = true;
                    dfs_stack.push((arc.to, false));
                }
            }
        }
    }

    let unassigned = usize::MAX;
    let mut component = vec![unassigned; num_states];
    let mut components: Vec<Vec<StateId>> = Vec::new();

    for &root in finish_order.iter().rev() {
        let root_idx = root as usize;
        if component[root_idx] != unassigned {
            continue;
        }

        let comp_id = components.len();
        let mut nodes = Vec::new();
        let mut stack = vec![root];
        component[root_idx] = comp_id;

        while let Some(state) = stack.pop() {
            nodes.push(state);
            for &pred in &reverse[state as usize] {
                let pred_idx = pred as usize;
                if reachable[pred_idx] && component[pred_idx] == unassigned {
                    component[pred_idx] = comp_id;
                    stack.push(pred);
                }
            }
        }

        components.push(nodes);
    }

    let mut potential = vec![0i64; num_states];
    let mut potential_set = vec![false; num_states];

    for (comp_id, nodes) in components.iter().enumerate() {
        for &root in nodes {
            let root_idx = root as usize;
            if potential_set[root_idx] {
                continue;
            }

            potential_set[root_idx] = true;
            potential[root_idx] = 0;
            let mut stack = vec![root];

            while let Some(state) = stack.pop() {
                let state_idx = state as usize;
                for arc in &forward[state_idx] {
                    let to_idx = arc.to as usize;
                    if component[to_idx] != comp_id {
                        continue;
                    }

                    let expected = potential[state_idx].checked_add(arc.delta)?;
                    if potential_set[to_idx] {
                        if potential[to_idx] != expected {
                            return None;
                        }
                    } else {
                        potential_set[to_idx] = true;
                        potential[to_idx] = expected;
                        stack.push(arc.to);
                    }
                }
            }
        }
    }

    let comp_count = components.len();
    let mut comp_edges: Vec<SmallVec<[ComponentArc; 4]>> =
        (0..comp_count).map(|_| SmallVec::new()).collect();
    let mut indegree = vec![0usize; comp_count];

    for (from_idx, arcs) in forward.iter().enumerate() {
        if !reachable[from_idx] {
            continue;
        }

        let from_comp = component[from_idx];
        for arc in arcs {
            let to_idx = arc.to as usize;
            let to_comp = component[to_idx];
            if from_comp == to_comp {
                continue;
            }

            let delta = potential[from_idx]
                .checked_add(arc.delta)?
                .checked_sub(potential[to_idx])?;

            comp_edges[from_comp].push(ComponentArc { to: to_comp, delta });
            indegree[to_comp] += 1;
        }
    }

    let start_comp = component[start as usize];
    let start_offset = 0i64.checked_sub(potential[start as usize])?;
    let mut min_entry = vec![None; comp_count];
    let mut max_entry = vec![None; comp_count];
    min_entry[start_comp] = Some(start_offset);
    max_entry[start_comp] = Some(start_offset);

    let mut queue = VecDeque::with_capacity(comp_count);
    for (comp_id, &degree) in indegree.iter().enumerate() {
        if degree == 0 {
            queue.push_back(comp_id);
        }
    }

    let mut visited_components = 0usize;
    let mut max_delay = 0usize;

    while let Some(comp_id) = queue.pop_front() {
        visited_components += 1;

        if let (Some(min_c), Some(max_c)) = (min_entry[comp_id], max_entry[comp_id]) {
            for &state in &components[comp_id] {
                let state_potential = potential[state as usize];
                let min_delay = min_c.checked_add(state_potential)?;
                let max_candidate = checked_abs_usize(min_delay)?;
                max_delay = max_delay.max(max_candidate);

                let max_delay_value = max_c.checked_add(state_potential)?;
                let max_candidate = checked_abs_usize(max_delay_value)?;
                max_delay = max_delay.max(max_candidate);
            }

            for edge in &comp_edges[comp_id] {
                let min_candidate = min_c.checked_add(edge.delta)?;
                match &mut min_entry[edge.to] {
                    Some(current) if min_candidate < *current => *current = min_candidate,
                    None => min_entry[edge.to] = Some(min_candidate),
                    _ => {}
                }

                let max_candidate = max_c.checked_add(edge.delta)?;
                match &mut max_entry[edge.to] {
                    Some(current) if max_candidate > *current => *current = max_candidate,
                    None => max_entry[edge.to] = Some(max_candidate),
                    _ => {}
                }
            }
        }

        for edge in &comp_edges[comp_id] {
            indegree[edge.to] -= 1;
            if indegree[edge.to] == 0 {
                queue.push_back(edge.to);
            }
        }
    }

    if visited_components == comp_count {
        Some(DelayAnalysis { max_delay })
    } else {
        None
    }
}

/// Check if a transducer has bounded delay.
///
/// A transducer has bounded delay iff all cycles have zero delay
/// (equal input and output lengths).
///
/// # Arguments
///
/// * `fst` - The transducer to check
///
/// # Returns
///
/// `true` if the transducer has bounded delay, `false` otherwise.
pub fn has_bounded_delay<L, W, T>(fst: &T) -> bool
where
    W: Semiring,
    L: Clone + Eq + Hash + Send + Sync,
    T: Wfst<L, W>,
{
    analyze_delay_graph(fst).is_some()
}

/// Compute the maximum delay of a transducer.
///
/// Returns `None` if the transducer has unbounded delay (cycles with non-zero delay)
/// or if delay arithmetic overflows.
///
/// # Arguments
///
/// * `fst` - The transducer to analyze
///
/// # Returns
///
/// `Some(max_delay)` if bounded and computable, `None` otherwise.
pub fn compute_max_delay<L, W, T>(fst: &T) -> Option<usize>
where
    W: Semiring,
    L: Clone + Eq + Hash + Send + Sync,
    T: Wfst<L, W>,
{
    analyze_delay_graph(fst).map(|analysis| analysis.max_delay)
}

// =============================================================================
// Type Aliases and Convenience Functions
// =============================================================================

/// Type alias for a lazy synchronized WFST.
pub type SyncWfst<L, W, T> = LazyWfstWrapper<SyncSource<L, W, T>, L, W>;

/// Synchronize a transducer lazily.
///
/// Creates a lazy synchronized version of the input transducer.
/// States are computed on demand during traversal.
///
/// # Arguments
///
/// * `fst` - The transducer to synchronize
///
/// # Returns
///
/// A lazy WFST representing the synchronized transducer.
///
/// # Panics
///
/// May produce incorrect results if the transducer has unbounded delay.
/// Use [`has_bounded_delay`] to check first.
pub fn synchronize<L, W, T>(fst: &T) -> SyncWfst<L, W, T>
where
    W: Semiring,
    L: Clone + Eq + Hash + Send + Sync,
    T: Wfst<L, W>,
{
    let max_delay = compute_max_delay(fst).unwrap_or(100);
    let source = SyncSource::new(fst.clone(), max_delay);
    LazyWfstWrapper::new(source)
}

/// Synchronize a transducer with a specific delay bound.
///
/// # Arguments
///
/// * `fst` - The transducer to synchronize
/// * `max_delay` - Maximum allowed delay (paths exceeding this are pruned)
///
/// # Returns
///
/// A lazy WFST representing the synchronized transducer.
pub fn synchronize_bounded<L, W, T>(fst: &T, max_delay: usize) -> SyncWfst<L, W, T>
where
    W: Semiring,
    L: Clone + Eq + Hash + Send + Sync,
    T: Wfst<L, W>,
{
    let source = SyncSource::new(fst.clone(), max_delay);
    LazyWfstWrapper::new(source)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_string_delay_empty() {
        let delay: StringDelay<char> = StringDelay::empty();
        assert!(delay.is_empty());
        assert_eq!(delay.len(), 0);
    }

    #[test]
    fn test_string_delay_sync_cancellation() {
        // Input "ab" and output "a" should leave output empty, input "b"
        let input: SmallVec<[char; 4]> = smallvec::smallvec!['a', 'b'];
        let output: SmallVec<[char; 4]> = smallvec::smallvec!['a'];

        let delay = StringDelay::sync(input, output);
        assert_eq!(delay.input.as_slice(), &['b']);
        assert!(delay.output.is_empty());
    }

    #[test]
    fn test_string_delay_sync_cancels_long_prefix_once() {
        let input: SmallVec<[usize; 4]> = (0..32).chain([100]).collect();
        let output: SmallVec<[usize; 4]> = (0..32).chain([200, 201]).collect();

        let delay = StringDelay::sync(input, output);
        assert_eq!(delay.input.as_slice(), &[100]);
        assert_eq!(delay.output.as_slice(), &[200, 201]);
    }

    #[test]
    fn test_string_delay_sync_accepts_non_clone_labels() {
        #[derive(Debug, PartialEq, Eq)]
        struct NonCloneLabel(char);

        let input: SmallVec<[NonCloneLabel; 4]> =
            smallvec::smallvec![NonCloneLabel('a'), NonCloneLabel('b')];
        let output: SmallVec<[NonCloneLabel; 4]> = smallvec::smallvec![NonCloneLabel('a')];

        let delay = StringDelay::sync(input, output);
        assert_eq!(delay.input.as_slice(), &[NonCloneLabel('b')]);
        assert!(delay.output.is_empty());
    }

    #[test]
    fn test_string_delay_empty_accepts_non_clone_labels() {
        #[derive(Debug, PartialEq, Eq)]
        struct NonCloneLabel;

        let delay: StringDelay<NonCloneLabel> = StringDelay::empty();
        assert!(delay.is_empty());
        assert_eq!(delay.len(), 0);
    }

    #[test]
    fn test_synchronized_transition_step_preserves_equal_labels() {
        let delay = StringDelay::empty();

        let (input, output, next_delay) =
            synchronized_transition_step(&delay, Some(&'a'), Some(&'a'));

        assert_eq!(input, Some('a'));
        assert_eq!(output, Some('a'));
        assert!(next_delay.is_empty());
    }

    #[test]
    fn test_string_delay_sync_no_common_prefix() {
        let input: SmallVec<[char; 4]> = smallvec::smallvec!['a', 'b'];
        let output: SmallVec<[char; 4]> = smallvec::smallvec!['x', 'y'];

        let delay = StringDelay::sync(input, output);
        assert_eq!(delay.input.as_slice(), &['a', 'b']);
        assert_eq!(delay.output.as_slice(), &['x', 'y']);
    }

    #[test]
    fn test_string_delay_car_cdr() {
        let delay = StringDelay {
            input: smallvec::smallvec!['a', 'b'],
            output: smallvec::smallvec![],
        };

        assert_eq!(delay.car_input(), Some('a'));
        assert_eq!(delay.car_output(), None);

        let cdr = delay.cdr();
        assert_eq!(cdr.input.as_slice(), &['b']);
    }

    #[test]
    fn test_has_bounded_delay_simple() {
        // Simple transducer with equal input/output
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
            .final_state(1, TropicalWeight::one())
            .build();

        assert!(has_bounded_delay(&fst));
    }

    #[test]
    fn test_has_bounded_delay_with_epsilon() {
        // Transducer with epsilon transitions
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), None, 1, TropicalWeight::one()) // Input only
            .arc(1, None, Some('x'), 2, TropicalWeight::one()) // Output only
            .final_state(2, TropicalWeight::one())
            .build();

        assert!(has_bounded_delay(&fst));
    }

    #[test]
    fn test_compute_max_delay() {
        // Transducer with delay of 1 (linear, no cycles)
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
            .arc(1, Some('b'), None, 2, TropicalWeight::one()) // Delay = -1 (input only)
            .final_state(2, TropicalWeight::one())
            .build();

        let max_delay = compute_max_delay(&fst);
        assert!(max_delay.is_some());
        assert!(max_delay.expect("wfst/synchronize.rs: required value was None/Err") >= 1);
    }

    #[test]
    fn test_compute_max_delay_accepts_acyclic_join_with_different_delays() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(4)
            .start(0)
            .arc(0, Some('a'), None, 1, TropicalWeight::one())
            .arc(1, None, None, 3, TropicalWeight::one())
            .arc(0, None, Some('x'), 2, TropicalWeight::one())
            .arc(2, None, None, 3, TropicalWeight::one())
            .final_state(3, TropicalWeight::one())
            .build();

        assert!(has_bounded_delay(&fst));
        assert_eq!(compute_max_delay(&fst), Some(1));
    }

    #[test]
    fn test_compute_max_delay_handles_zero_delay_scc_linearly() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), None, 1, TropicalWeight::one())
            .arc(1, None, Some('x'), 2, TropicalWeight::one())
            .arc(2, Some('b'), None, 1, TropicalWeight::one())
            .final_state(2, TropicalWeight::one())
            .build();

        assert!(has_bounded_delay(&fst));
        assert_eq!(compute_max_delay(&fst), Some(1));
    }

    #[test]
    fn test_synchronize_ignores_malformed_targets() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(1)
            .start(0)
            .arc(0, Some('a'), None, 99, TropicalWeight::one())
            .final_state(0, TropicalWeight::one())
            .build();

        assert!(has_bounded_delay(&fst));
        assert_eq!(compute_max_delay(&fst), Some(0));

        let mut synced = synchronize_bounded(&fst, 4);
        assert!(synced.transitions_lazy(0).is_empty());
        assert!(synced.is_final(0));
    }

    #[test]
    fn test_unbounded_delay_cycle() {
        // Transducer with unbounded delay (cycle with non-zero delay)
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
            .arc(1, None, Some('y'), 1, TropicalWeight::one()) // Self-loop that increases delay
            .final_state(1, TropicalWeight::one())
            .build();

        // This has unbounded delay because the cycle increases output without input
        assert!(!has_bounded_delay(&fst));
        assert!(compute_max_delay(&fst).is_none());
    }

    #[test]
    fn test_bounded_delay_zero_cycle() {
        // Transducer with cycle that has zero delay
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
            .arc(1, Some('b'), Some('y'), 1, TropicalWeight::one()) // Self-loop with zero delay
            .final_state(1, TropicalWeight::one())
            .build();

        // This has bounded delay because the cycle has equal input/output
        assert!(has_bounded_delay(&fst));
        assert!(compute_max_delay(&fst).is_some());
    }

    #[test]
    fn test_synchronize_simple() {
        // Simple 1:1 transducer
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
            .final_state(1, TropicalWeight::one())
            .build();

        let synced = synchronize(&fst);

        assert_eq!(synced.start(), 0);
        assert_eq!(synced.num_states(), 1); // Initially just the start state
    }

    #[test]
    fn test_lazy_synchronize_preserves_identity_arc_labels() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::one())
            .final_state(1, TropicalWeight::one())
            .build();

        let mut synced = synchronize(&fst);
        let transitions = synced.transitions_lazy(0).to_vec();

        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].input, Some('a'));
        assert_eq!(transitions[0].output, Some('a'));

        let target = transitions[0].to;
        synced.expand(target);
        assert!(synced.is_final(target));
    }

    #[test]
    fn test_lazy_synchronize_allocates_distinct_discovered_states() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
            .arc(0, Some('b'), Some('y'), 2, TropicalWeight::one())
            .final_state(1, TropicalWeight::one())
            .final_state(2, TropicalWeight::one())
            .build();

        let mut synced = synchronize_bounded(&fst, 2);
        let transitions = synced.transitions_lazy(0).to_vec();

        assert_eq!(transitions.len(), 2);
        assert_ne!(transitions[0].to, transitions[1].to);

        synced.expand(transitions[0].to);
        synced.expand(transitions[1].to);
        assert!(synced.is_final(transitions[0].to));
        assert!(synced.is_final(transitions[1].to));
    }

    #[test]
    fn test_lazy_synchronize_recovers_poisoned_registry() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
            .final_state(1, TropicalWeight::one())
            .build();

        let source = SyncSource::new(fst, 2);
        let poisoning_source = source.clone();
        assert!(catch_unwind(AssertUnwindSafe(|| {
            let _guard = match poisoning_source.registry.write() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            std::panic::panic_any("poison synchronization registry");
        }))
        .is_err());

        assert!(source.get_sync_state(0).is_some());

        let computed = source.compute_state(0);
        assert_eq!(computed.transitions().unwrap_or(&[]).len(), 1);
    }

    #[test]
    fn test_sync_state_initial() {
        let state: SyncState<char> = SyncState::initial(5);
        assert_eq!(state.original, 5);
        assert!(state.delay.is_empty());
        assert!(!state.draining);
    }

    #[test]
    fn test_sync_state_draining() {
        let delay = StringDelay {
            input: smallvec::smallvec!['a'],
            output: smallvec::smallvec![],
        };
        let state: SyncState<char> = SyncState::draining(delay.clone());
        assert_eq!(state.original, NO_STATE);
        assert!(!state.delay.is_empty());
        assert!(state.draining);
    }

    #[test]
    fn test_mutable_sync_source_basic() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
            .final_state(1, TropicalWeight::one())
            .build();

        let mut sync = MutableSyncSource::new(fst, 10);

        assert_eq!(sync.start(), 0);
        assert_eq!(sync.num_states(), 1);

        // Expand start state
        sync.expand_state(0);

        // Should have created the next state
        assert!(sync.num_states() >= 1);
        assert!(sync.is_expanded(0));
    }

    #[test]
    fn test_mutable_sync_source_preserves_identity_arc_labels() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(2)
            .start(0)
            .arc(0, Some('a'), Some('a'), 1, TropicalWeight::one())
            .final_state(1, TropicalWeight::one())
            .build();

        let mut sync = MutableSyncSource::new(fst, 10);
        sync.expand_state(0);
        let transitions = sync.transitions(0);

        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].input, Some('a'));
        assert_eq!(transitions[0].output, Some('a'));

        let target = transitions[0].to;
        sync.expand_state(target);
        assert!(sync.is_final(target));
    }

    #[test]
    fn test_mutable_sync_source_delayed_output() {
        // Transducer: input 'a' produces outputs 'x' then 'y'
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(3)
            .start(0)
            .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
            .arc(1, None, Some('y'), 2, TropicalWeight::one())
            .final_state(2, TropicalWeight::one())
            .build();

        let mut sync = MutableSyncSource::new(fst, 10);

        // Expand states
        sync.expand_state(0);
        sync.expand_state(1);

        assert!(sync.is_expanded(0));
        assert!(sync.is_expanded(1));
    }

    #[test]
    fn test_empty_transducer() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

        assert!(has_bounded_delay(&fst));
        assert_eq!(compute_max_delay(&fst), Some(0));

        let synced = synchronize(&fst);
        assert_eq!(synced.start(), NO_STATE);
    }

    #[test]
    fn test_single_final_state() {
        let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
            .add_states(1)
            .start(0)
            .final_state(0, TropicalWeight::one())
            .build();

        assert!(has_bounded_delay(&fst));
        assert_eq!(compute_max_delay(&fst), Some(0));

        let synced = synchronize(&fst);
        assert_eq!(synced.start(), 0);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::test_utils::arb_tropical_wfst;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Single-state transducers with no arcs always have bounded delay.
        #[test]
        fn single_state_has_bounded_delay(
            fst in arb_tropical_wfst(1, 0)
        ) {
            prop_assert!(has_bounded_delay(&fst));
        }

        /// Single-state transducers have max delay of 0.
        #[test]
        fn single_state_max_delay_zero(
            fst in arb_tropical_wfst(1, 0)
        ) {
            let max_delay = compute_max_delay(&fst);
            prop_assert!(max_delay.is_some());
            prop_assert_eq!(max_delay.expect("wfst/synchronize.rs: required value was None/Err"), 0);
        }

        /// Synchronization preserves start state validity for bounded-delay FSTs.
        #[test]
        fn synchronize_preserves_start_validity(
            fst in arb_tropical_wfst(3, 1)
        ) {
            // Only synchronize if it has bounded delay
            if has_bounded_delay(&fst) {
                let synced = synchronize(&fst);
                // If original has a start, synced should too
                if fst.start() != NO_STATE {
                    prop_assert!(synced.start() != NO_STATE);
                }
            }
        }

        /// Bounded delay implies max_delay returns Some (for small FSTs).
        /// Note: compute_max_delay may return None only if delay arithmetic overflows.
        #[test]
        fn bounded_delay_implies_max_delay_some(
            fst in arb_tropical_wfst(3, 1)
        ) {
            if has_bounded_delay(&fst) {
                // For small FSTs, bounded delay should imply computable max delay
                prop_assert!(compute_max_delay(&fst).is_some());
            }
        }

        /// Unbounded delay implies max_delay returns None.
        #[test]
        fn unbounded_delay_implies_max_delay_none(
            fst in arb_tropical_wfst(3, 1)
        ) {
            if !has_bounded_delay(&fst) {
                prop_assert!(compute_max_delay(&fst).is_none());
            }
        }

        /// StringDelay::empty is truly empty.
        #[test]
        fn string_delay_empty_properties(_dummy in 0..1i32) {
            let delay: StringDelay<char> = StringDelay::empty();
            prop_assert!(delay.is_empty());
            prop_assert_eq!(delay.len(), 0);
            prop_assert!(delay.car_input().is_none());
            prop_assert!(delay.car_output().is_none());
        }

        /// StringDelay sync with identical strings results in empty.
        #[test]
        fn string_delay_sync_identical_is_empty(chars in prop::collection::vec(any::<char>(), 0..5)) {
            let input: SmallVec<[char; 4]> = chars.iter().cloned().collect();
            let output: SmallVec<[char; 4]> = chars.iter().cloned().collect();
            let delay = StringDelay::sync(input, output);
            prop_assert!(delay.is_empty());
        }

        /// SyncState::initial creates non-draining state with empty delay.
        #[test]
        fn sync_state_initial_properties(state_id in 0..100u32) {
            let state: SyncState<char> = SyncState::initial(state_id);
            prop_assert_eq!(state.original, state_id);
            prop_assert!(state.delay.is_empty());
            prop_assert!(!state.draining);
        }

        /// MutableSyncSource maintains state count >= 1 for non-empty FST.
        #[test]
        fn mutable_sync_source_state_count(
            fst in arb_tropical_wfst(4, 2)
        ) {
            if fst.start() != NO_STATE && has_bounded_delay(&fst) {
                let sync = MutableSyncSource::new(fst, 10);
                prop_assert!(sync.num_states() >= 1);
            }
        }

        /// MutableSyncSource start matches FST start validity.
        #[test]
        fn mutable_sync_source_start_consistency(
            fst in arb_tropical_wfst(4, 2)
        ) {
            if has_bounded_delay(&fst) {
                let sync: MutableSyncSource<char, TropicalWeight, _> = MutableSyncSource::new(fst.clone(), 10);
                if fst.start() == NO_STATE {
                    prop_assert_eq!(sync.start(), NO_STATE);
                } else {
                    prop_assert_eq!(sync.start(), 0);
                }
            }
        }
    }
}
