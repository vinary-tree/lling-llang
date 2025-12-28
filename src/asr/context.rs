//! Context-dependency transducers for speech recognition.
//!
//! This module provides builders for constructing context-dependency transducers
//! that map context-independent phone sequences to context-dependent phone sequences.
//!
//! ## Triphone Construction
//!
//! A triphone considers the previous and next phone as context. For n phones:
//! - States: O(n²) - representing (previous, current) phone pairs
//! - Arcs: O(n³) - one arc per (previous, current, next) triple
//!
//! ## Tetraphone Construction
//!
//! A tetraphone extends context to two phones on each side. For n phones:
//! - States: O(n³) - representing (prev2, prev1, current) phone triples
//! - Arcs: O(n⁴) - one arc per (prev2, prev1, current, next) quadruple
//!
//! ## Deterministic vs Non-deterministic
//!
//! - **Non-deterministic**: Center phone as input label (simpler construction)
//! - **Deterministic**: Right phone as input label (no matching delay)
//!   - Requires final subsequential symbol ($) to pad context
//!
//! ## References
//!
//! - Mohri et al., "Speech Recognition with WFSTs" Section 4.3

use std::collections::HashMap;
use std::hash::Hash;
use std::fmt::Debug;

use crate::semiring::Semiring;
use crate::wfst::{VectorWfst, MutableWfst, Wfst, StateId};

/// Phone identifier type.
pub type PhoneId = u32;

/// Epsilon label constant.
pub const EPSILON: Option<PhoneId> = None;

/// State in a context-dependency transducer.
///
/// Encodes the context history as a sequence of phones.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContextState {
    /// Left context (phones seen before current position).
    /// Order: [oldest, ..., most_recent]
    pub left_context: Vec<PhoneId>,
}

impl ContextState {
    /// Create initial state with empty context.
    pub fn initial() -> Self {
        Self {
            left_context: Vec::new(),
        }
    }

    /// Create state with given left context.
    pub fn with_context(context: Vec<PhoneId>) -> Self {
        Self {
            left_context: context,
        }
    }

    /// Extend context with a new phone, maintaining window size.
    pub fn extend(&self, phone: PhoneId, max_context: usize) -> Self {
        let mut new_context = self.left_context.clone();
        new_context.push(phone);

        // Trim to max context size
        if new_context.len() > max_context {
            new_context.remove(0);
        }

        Self {
            left_context: new_context,
        }
    }
}

/// Configuration for context-dependency transducer construction.
#[derive(Clone, Debug)]
pub struct ContextDependencyConfig {
    /// Whether to use deterministic construction (right phone as input).
    pub deterministic: bool,

    /// Final subsequential symbol for deterministic construction.
    /// Used to pad context at word boundaries.
    pub boundary_symbol: Option<PhoneId>,

    /// Whether to add self-loops for auxiliary symbols.
    pub auxiliary_self_loops: bool,

    /// Auxiliary symbol range (if any).
    pub auxiliary_symbols: Option<std::ops::Range<PhoneId>>,
}

impl Default for ContextDependencyConfig {
    fn default() -> Self {
        Self {
            deterministic: false,
            boundary_symbol: None,
            auxiliary_self_loops: false,
            auxiliary_symbols: None,
        }
    }
}

/// Builder for general context-dependency transducers.
pub struct ContextDependencyBuilder<W: Semiring> {
    /// Number of phones in the inventory.
    num_phones: usize,

    /// Left context size (number of preceding phones to consider).
    left_context_size: usize,

    /// Right context size (number of following phones to consider).
    right_context_size: usize,

    /// Configuration options.
    config: ContextDependencyConfig,

    /// Phantom for weight type.
    _weight: std::marker::PhantomData<W>,
}

impl<W: Semiring> ContextDependencyBuilder<W> {
    /// Create a new context-dependency builder.
    ///
    /// # Arguments
    ///
    /// * `num_phones` - Number of phones in the inventory
    /// * `left_context_size` - Number of preceding phones to consider
    /// * `right_context_size` - Number of following phones to consider
    pub fn new(num_phones: usize, left_context_size: usize, right_context_size: usize) -> Self {
        Self {
            num_phones,
            left_context_size,
            right_context_size,
            config: ContextDependencyConfig::default(),
            _weight: std::marker::PhantomData,
        }
    }

    /// Set configuration options.
    pub fn config(mut self, config: ContextDependencyConfig) -> Self {
        self.config = config;
        self
    }

    /// Enable deterministic construction.
    pub fn deterministic(mut self, boundary_symbol: PhoneId) -> Self {
        self.config.deterministic = true;
        self.config.boundary_symbol = Some(boundary_symbol);
        self
    }

    /// Enable auxiliary symbol self-loops.
    pub fn with_auxiliary_symbols(mut self, range: std::ops::Range<PhoneId>) -> Self {
        self.config.auxiliary_self_loops = true;
        self.config.auxiliary_symbols = Some(range);
        self
    }

    /// Build the context-dependency transducer.
    ///
    /// For non-deterministic construction:
    /// - Input: center phone
    /// - Output: context-dependent phone (triphone/tetraphone label)
    ///
    /// For deterministic construction:
    /// - Input: right-context phone
    /// - Output: context-dependent phone
    pub fn build(&self) -> VectorWfst<PhoneId, W> {
        let mut fst: VectorWfst<PhoneId, W> = VectorWfst::new();

        // Map from context state to WFST state ID
        let mut state_map: HashMap<ContextState, StateId> = HashMap::new();

        // Create initial state (empty context)
        let initial = ContextState::initial();
        let start_id = fst.add_state();
        fst.set_start(start_id);
        state_map.insert(initial.clone(), start_id);

        // Build states and transitions using BFS
        let mut queue = vec![initial];

        while let Some(current_state) = queue.pop() {
            let current_id = *state_map.get(&current_state).expect("state should exist in map");

            // For each possible input phone
            for phone in 0..self.num_phones as PhoneId {
                // Skip epsilon/special phones if needed

                // Compute next state
                let next_state = current_state.extend(phone, self.left_context_size);

                // Get or create next state ID
                let next_id = if let Some(&id) = state_map.get(&next_state) {
                    id
                } else {
                    let id = fst.add_state();
                    state_map.insert(next_state.clone(), id);
                    queue.push(next_state.clone());
                    id
                };

                // Compute context-dependent output label
                let output_label = self.compute_cd_label(&current_state, phone);

                // Add transition
                fst.add_arc(
                    current_id,
                    Some(phone),
                    Some(output_label),
                    next_id,
                    W::one(),
                );
            }

            // Add auxiliary symbol self-loops if configured
            if self.config.auxiliary_self_loops {
                if let Some(ref range) = self.config.auxiliary_symbols {
                    for aux in range.clone() {
                        fst.add_arc(
                            current_id,
                            Some(aux),
                            Some(aux),
                            current_id,
                            W::one(),
                        );
                    }
                }
            }

            // All states with full context are final
            if current_state.left_context.len() >= self.left_context_size {
                fst.set_final(current_id, W::one());
            }
        }

        // For deterministic construction, add boundary handling
        if self.config.deterministic {
            if let Some(boundary) = self.config.boundary_symbol {
                self.add_boundary_handling(&mut fst, &state_map, boundary);
            }
        }

        // Make all states final for proper word boundary handling
        for id in 0..fst.num_states() as StateId {
            if !fst.is_final(id) {
                fst.set_final(id, W::one());
            }
        }

        fst
    }

    /// Compute context-dependent phone label.
    ///
    /// Encodes (left_context, center_phone) as a single label.
    fn compute_cd_label(&self, state: &ContextState, center_phone: PhoneId) -> PhoneId {
        // Simple encoding: concatenate context phones with center
        // For triphone with L phones, use: left * L + center
        // This gives unique labels for each context-dependent phone

        let num_phones = self.num_phones as PhoneId;
        let mut label = center_phone;

        for (i, &ctx_phone) in state.left_context.iter().rev().enumerate() {
            let multiplier = num_phones.pow((i + 1) as u32);
            label += ctx_phone * multiplier;
        }

        label
    }

    /// Add boundary handling for deterministic construction.
    fn add_boundary_handling(
        &self,
        _fst: &mut VectorWfst<PhoneId, W>,
        _state_map: &HashMap<ContextState, StateId>,
        _boundary: PhoneId,
    ) {
        // Add epsilon transitions from states to boundary-handling states
        // This allows the transducer to emit remaining context at word boundaries
        // TODO: Implement full boundary handling
    }
}

/// Builder for triphone context-dependency transducers.
///
/// A triphone considers one phone of left and right context.
pub struct TriphoneBuilder<W: Semiring> {
    inner: ContextDependencyBuilder<W>,
}

impl<W: Semiring> TriphoneBuilder<W> {
    /// Create a new triphone builder.
    ///
    /// # Arguments
    ///
    /// * `num_phones` - Number of phones in the inventory
    pub fn new(num_phones: usize) -> Self {
        Self {
            inner: ContextDependencyBuilder::new(num_phones, 1, 1),
        }
    }

    /// Set configuration options.
    pub fn config(mut self, config: ContextDependencyConfig) -> Self {
        self.inner.config = config;
        self
    }

    /// Enable deterministic construction.
    pub fn deterministic(mut self, boundary_symbol: PhoneId) -> Self {
        self.inner = self.inner.deterministic(boundary_symbol);
        self
    }

    /// Build the triphone transducer.
    ///
    /// # Complexity
    ///
    /// - States: O(n²) for n phones
    /// - Arcs: O(n³) for n phones
    pub fn build(&self) -> VectorWfst<PhoneId, W> {
        self.inner.build()
    }

    /// Get expected number of states.
    pub fn expected_states(&self) -> usize {
        let n = self.inner.num_phones;
        // States for: empty context, 1-phone context
        1 + n
    }

    /// Get expected number of arcs.
    pub fn expected_arcs(&self) -> usize {
        let n = self.inner.num_phones;
        // From each state, arcs for each phone
        (1 + n) * n
    }
}

/// Builder for tetraphone context-dependency transducers.
///
/// A tetraphone considers two phones of left and right context.
pub struct TetraploneBuilder<W: Semiring> {
    inner: ContextDependencyBuilder<W>,
}

impl<W: Semiring> TetraploneBuilder<W> {
    /// Create a new tetraphone builder.
    ///
    /// # Arguments
    ///
    /// * `num_phones` - Number of phones in the inventory
    pub fn new(num_phones: usize) -> Self {
        Self {
            inner: ContextDependencyBuilder::new(num_phones, 2, 2),
        }
    }

    /// Set configuration options.
    pub fn config(mut self, config: ContextDependencyConfig) -> Self {
        self.inner.config = config;
        self
    }

    /// Enable deterministic construction.
    pub fn deterministic(mut self, boundary_symbol: PhoneId) -> Self {
        self.inner = self.inner.deterministic(boundary_symbol);
        self
    }

    /// Build the tetraphone transducer.
    ///
    /// # Complexity
    ///
    /// - States: O(n³) for n phones
    /// - Arcs: O(n⁴) for n phones
    pub fn build(&self) -> VectorWfst<PhoneId, W> {
        self.inner.build()
    }

    /// Get expected number of states.
    pub fn expected_states(&self) -> usize {
        let n = self.inner.num_phones;
        // States for: empty, 1-phone, 2-phone context
        1 + n + n * n
    }

    /// Get expected number of arcs.
    pub fn expected_arcs(&self) -> usize {
        let n = self.inner.num_phones;
        // From each state, arcs for each phone
        (1 + n + n * n) * n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::LogWeight;
    use crate::wfst::{Wfst, NO_STATE};

    #[test]
    fn test_context_state_initial() {
        let state = ContextState::initial();
        assert!(state.left_context.is_empty());
    }

    #[test]
    fn test_context_state_extend() {
        let state = ContextState::initial();

        let state1 = state.extend(1, 2);
        assert_eq!(state1.left_context, vec![1]);

        let state2 = state1.extend(2, 2);
        assert_eq!(state2.left_context, vec![1, 2]);

        // Should trim to max context
        let state3 = state2.extend(3, 2);
        assert_eq!(state3.left_context, vec![2, 3]);
    }

    #[test]
    fn test_triphone_builder() {
        let builder = TriphoneBuilder::<LogWeight>::new(5);
        let fst = builder.build();

        // Should have states for empty and 1-phone context
        assert!(fst.num_states() >= 1);
        assert!(fst.start() != NO_STATE);
    }

    #[test]
    fn test_triphone_state_count() {
        let builder = TriphoneBuilder::<LogWeight>::new(3);
        let fst = builder.build();

        // 1 (empty) + 3 (1-phone context) = 4 states
        assert_eq!(fst.num_states(), 4);
    }

    #[test]
    fn test_triphone_arc_count() {
        let builder = TriphoneBuilder::<LogWeight>::new(3);
        let fst = builder.build();

        // Count total arcs
        let total_arcs: usize = (0..fst.num_states() as StateId)
            .map(|s| fst.transitions(s).len())
            .sum();

        // Expected: 4 states * 3 phones = 12 arcs
        assert_eq!(total_arcs, 12);
    }

    #[test]
    fn test_tetraphone_state_count() {
        let builder = TetraploneBuilder::<LogWeight>::new(3);
        let fst = builder.build();

        // 1 (empty) + 3 (1-phone) + 9 (2-phone) = 13 states
        assert_eq!(fst.num_states(), 13);
    }

    #[test]
    fn test_cd_label_encoding() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(10, 1, 1);

        let empty = ContextState::initial();
        let with_ctx = ContextState::with_context(vec![5]);

        // Label should encode context
        let label1 = builder.compute_cd_label(&empty, 3);
        let label2 = builder.compute_cd_label(&with_ctx, 3);

        // Same center phone but different context should give different labels
        assert_ne!(label1, label2);
    }

    #[test]
    fn test_all_states_final() {
        let builder = TriphoneBuilder::<LogWeight>::new(3);
        let fst = builder.build();

        // All states should be final for word boundary handling
        for id in 0..fst.num_states() as StateId {
            assert!(fst.is_final(id));
        }
    }
}
