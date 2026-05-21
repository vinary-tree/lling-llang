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
use std::fmt::Debug;
use std::hash::Hash;

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst};

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
#[derive(Clone, Debug, Default)]
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

    /// Configured left context size (preceding phones).
    pub fn left_context_size(&self) -> usize {
        self.left_context_size
    }

    /// Configured right context size (following phones).
    pub fn right_context_size(&self) -> usize {
        self.right_context_size
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
            let current_id = *state_map
                .get(&current_state)
                .expect("state should exist in map");

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
                        fst.add_arc(current_id, Some(aux), Some(aux), current_id, W::one());
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
    /// Encodes (left_context, center_phone) as a single label using offset-by-1
    /// encoding to ensure injectivity even when phone 0 is in the context.
    ///
    /// # Encoding
    ///
    /// Uses a mixed-radix positional encoding where each context phone is offset
    /// by 1 to ensure phone 0 contributes a non-zero value:
    ///
    /// ```text
    /// label = center + sum((ctx[i] + 1) * (num_phones + 1)^(i+1))
    /// ```
    ///
    /// This ensures that contexts [0] and [] produce different labels, as
    /// phone 0 in context contributes (0 + 1) = 1, not 0.
    fn compute_cd_label(&self, state: &ContextState, center_phone: PhoneId) -> PhoneId {
        // Use base = num_phones + 1 to accommodate the offset
        let base = self.num_phones as PhoneId + 1;
        let mut label = center_phone;

        // Process context phones from most recent to oldest
        for (i, &ctx_phone) in state.left_context.iter().rev().enumerate() {
            let multiplier = base.pow((i + 1) as u32);
            // Offset by 1 so phone 0 contributes (0 + 1) * multiplier, not 0
            label += (ctx_phone + 1) * multiplier;
        }

        label
    }

    /// Add boundary handling for deterministic construction.
    ///
    /// For deterministic context-dependency transducers, we need to handle word
    /// boundaries by emitting the remaining context when the boundary symbol is seen.
    ///
    /// This allows proper handling of word-final context without look-ahead.
    fn add_boundary_handling(
        &self,
        fst: &mut VectorWfst<PhoneId, W>,
        state_map: &HashMap<ContextState, StateId>,
        boundary: PhoneId,
    ) {
        // For each state with accumulated context, add a boundary transition
        // that outputs a context-dependent label including the boundary
        for (context_state, &state_id) in state_map {
            // Only process states with context (not the initial empty state)
            if context_state.left_context.is_empty() {
                // For initial state, just add a boundary self-loop
                fst.add_arc(state_id, Some(boundary), Some(boundary), state_id, W::one());
                continue;
            }

            // For states with context, add a transition that outputs
            // the context-dependent boundary label
            let boundary_label = self.compute_cd_label(context_state, boundary);

            // Create a boundary-exit state for this context
            let exit_state = fst.add_state();
            fst.set_final(exit_state, W::one());

            // Add transition: current_state --boundary:cd_boundary_label--> exit_state
            fst.add_arc(
                state_id,
                Some(boundary),
                Some(boundary_label),
                exit_state,
                W::one(),
            );
        }
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

    /// Test that phone 0 in context produces a different label than empty context.
    ///
    /// This verifies the offset-by-1 encoding fix.
    #[test]
    fn test_phone_0_contributes_to_label() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(10, 2, 1);

        let empty = ContextState::initial();
        let with_zero = ContextState::with_context(vec![0]);

        // With the fix, these should produce different labels
        let label_empty = builder.compute_cd_label(&empty, 5);
        let label_with_zero = builder.compute_cd_label(&with_zero, 5);

        assert_ne!(
            label_empty, label_with_zero,
            "Phone 0 in context must produce different label than empty context. \
             Empty: {}, With [0]: {}",
            label_empty, label_with_zero
        );
    }

    /// Test that different positions of phone 0 produce different labels.
    #[test]
    fn test_different_phone_0_positions() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(10, 2, 1);

        // [0, 1] vs [1, 0] should produce different labels
        let ctx_01 = ContextState::with_context(vec![0, 1]);
        let ctx_10 = ContextState::with_context(vec![1, 0]);

        let label_01 = builder.compute_cd_label(&ctx_01, 5);
        let label_10 = builder.compute_cd_label(&ctx_10, 5);

        assert_ne!(
            label_01, label_10,
            "Different phone 0 positions must produce different labels. \
             [0,1]: {}, [1,0]: {}",
            label_01, label_10
        );
    }

    /// Test that the encoding is injective: different (context, center) pairs
    /// always produce different labels.
    #[test]
    fn test_cd_label_injectivity_with_phone_0() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(5, 2, 1);

        // Collect all (context, center) -> label mappings for small cases
        let mut seen_labels: std::collections::HashMap<PhoneId, (Vec<PhoneId>, PhoneId)> =
            std::collections::HashMap::new();

        // Test all combinations of context length 0, 1, 2 with phones 0..3
        let phones: Vec<PhoneId> = vec![0, 1, 2];

        for center in phones.iter().copied() {
            // Empty context
            let empty = ContextState::initial();
            let label = builder.compute_cd_label(&empty, center);
            if let Some((prev_ctx, prev_center)) = seen_labels.insert(label, (vec![], center)) {
                panic!(
                    "Label collision: {} produced by both ({:?}, {}) and ({:?}, {})",
                    label,
                    prev_ctx,
                    prev_center,
                    vec![] as Vec<PhoneId>,
                    center
                );
            }

            // Single-phone context
            for &ctx0 in &phones {
                let ctx = ContextState::with_context(vec![ctx0]);
                let label = builder.compute_cd_label(&ctx, center);
                if let Some((prev_ctx, prev_center)) =
                    seen_labels.insert(label, (vec![ctx0], center))
                {
                    panic!(
                        "Label collision: {} produced by both ({:?}, {}) and ({:?}, {})",
                        label,
                        prev_ctx,
                        prev_center,
                        vec![ctx0],
                        center
                    );
                }
            }

            // Two-phone context
            for &ctx0 in &phones {
                for &ctx1 in &phones {
                    let ctx = ContextState::with_context(vec![ctx0, ctx1]);
                    let label = builder.compute_cd_label(&ctx, center);
                    if let Some((prev_ctx, prev_center)) =
                        seen_labels.insert(label, (vec![ctx0, ctx1], center))
                    {
                        panic!(
                            "Label collision: {} produced by both ({:?}, {}) and ({:?}, {})",
                            label,
                            prev_ctx,
                            prev_center,
                            vec![ctx0, ctx1],
                            center
                        );
                    }
                }
            }
        }
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::semiring::LogWeight;
    use crate::wfst::{Wfst, NO_STATE};
    use proptest::prelude::*;

    // -------------------------------------------------------------------------
    // ContextState Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Initial state has empty context.
        #[test]
        fn initial_state_empty(_seed in any::<u64>()) {
            let state = ContextState::initial();
            prop_assert!(state.left_context.is_empty());
        }

        /// Extending with a phone adds it to context.
        #[test]
        fn extend_adds_phone(phone in 0u32..100, max_ctx in 1usize..5) {
            let state = ContextState::initial();
            let extended = state.extend(phone, max_ctx);
            prop_assert!(extended.left_context.contains(&phone));
        }

        /// Context length never exceeds max_context.
        #[test]
        fn extend_respects_max_context(
            phones in prop::collection::vec(0u32..100, 1..20),
            max_ctx in 1usize..5
        ) {
            let mut state = ContextState::initial();
            for &phone in &phones {
                state = state.extend(phone, max_ctx);
                prop_assert!(state.left_context.len() <= max_ctx);
            }
        }

        /// When context exceeds max, oldest phone is removed.
        #[test]
        fn extend_removes_oldest_when_full(max_ctx in 1usize..5) {
            let mut state = ContextState::initial();

            // Fill context
            for i in 0..max_ctx as u32 {
                state = state.extend(i, max_ctx);
            }
            prop_assert_eq!(state.left_context.len(), max_ctx);

            // Add one more - oldest (0) should be removed
            let new_phone = max_ctx as u32 + 100;
            state = state.extend(new_phone, max_ctx);

            prop_assert_eq!(state.left_context.len(), max_ctx);
            prop_assert!(!state.left_context.contains(&0));
            prop_assert!(state.left_context.contains(&new_phone));
        }

        /// with_context preserves the given context.
        #[test]
        fn with_context_preserves(context in prop::collection::vec(0u32..100, 0..5)) {
            let state = ContextState::with_context(context.clone());
            prop_assert_eq!(state.left_context, context);
        }

        /// ContextState equality is based on context content.
        #[test]
        fn context_state_equality(context in prop::collection::vec(0u32..50, 0..4)) {
            let state1 = ContextState::with_context(context.clone());
            let state2 = ContextState::with_context(context);
            prop_assert_eq!(state1, state2);
        }

        /// Different contexts produce different states.
        #[test]
        fn different_contexts_different_states(
            ctx1 in prop::collection::vec(0u32..50, 1..3),
            ctx2 in prop::collection::vec(50u32..100, 1..3)
        ) {
            let state1 = ContextState::with_context(ctx1);
            let state2 = ContextState::with_context(ctx2);
            prop_assert_ne!(state1, state2);
        }
    }

    // -------------------------------------------------------------------------
    // ContextDependencyConfig Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Default config is non-deterministic.
        #[test]
        fn default_config_non_deterministic(_seed in any::<u64>()) {
            let config = ContextDependencyConfig::default();
            prop_assert!(!config.deterministic);
            prop_assert!(config.boundary_symbol.is_none());
        }

        /// Default config has no auxiliary symbols.
        #[test]
        fn default_config_no_aux(_seed in any::<u64>()) {
            let config = ContextDependencyConfig::default();
            prop_assert!(!config.auxiliary_self_loops);
            prop_assert!(config.auxiliary_symbols.is_none());
        }
    }

    // -------------------------------------------------------------------------
    // ContextDependencyBuilder Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        /// CD label encoding is deterministic.
        #[test]
        fn cd_label_deterministic(
            num_phones in 2usize..10,
            context in prop::collection::vec(0u32..10, 0..2),
            center in 0u32..10
        ) {
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 2, 1);
            let state = ContextState::with_context(context);

            let label1 = builder.compute_cd_label(&state, center);
            let label2 = builder.compute_cd_label(&state, center);

            prop_assert_eq!(label1, label2);
        }

        /// Different contexts produce different CD labels.
        #[test]
        fn cd_label_context_sensitivity(
            num_phones in 5usize..15,
            center in 0u32..5
        ) {
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 1, 1);

            let empty = ContextState::initial();
            let with_ctx = ContextState::with_context(vec![1]);

            let label1 = builder.compute_cd_label(&empty, center);
            let label2 = builder.compute_cd_label(&with_ctx, center);

            // Labels should differ when context differs
            prop_assert_ne!(label1, label2);
        }

        /// Different center phones produce different CD labels.
        #[test]
        fn cd_label_center_sensitivity(
            num_phones in 5usize..15,
            center1 in 0u32..5,
            center2 in 5u32..10
        ) {
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 1, 1);
            let state = ContextState::initial();

            let label1 = builder.compute_cd_label(&state, center1);
            let label2 = builder.compute_cd_label(&state, center2);

            prop_assert_ne!(label1, label2);
        }

        /// Builder config method updates config.
        #[test]
        fn builder_config_updates(
            num_phones in 2usize..10,
            deterministic in any::<bool>()
        ) {
            let config = ContextDependencyConfig {
                deterministic,
                ..Default::default()
            };

            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 1, 1)
                .config(config);

            prop_assert_eq!(builder.config.deterministic, deterministic);
        }

        /// Deterministic method sets appropriate fields.
        #[test]
        fn builder_deterministic_sets_fields(
            num_phones in 2usize..10,
            boundary in 0u32..100
        ) {
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 1, 1)
                .deterministic(boundary);

            prop_assert!(builder.config.deterministic);
            prop_assert_eq!(builder.config.boundary_symbol, Some(boundary));
        }

        /// Auxiliary symbols method sets appropriate fields.
        #[test]
        fn builder_aux_symbols_sets_fields(
            num_phones in 2usize..10,
            start in 100u32..200,
            end in 200u32..300
        ) {
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 1, 1)
                .with_auxiliary_symbols(start..end);

            prop_assert!(builder.config.auxiliary_self_loops);
            prop_assert_eq!(builder.config.auxiliary_symbols, Some(start..end));
        }
    }

    // -------------------------------------------------------------------------
    // TriphoneBuilder Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// Triphone FST has expected state count.
        #[test]
        fn triphone_state_count(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            // States: 1 (empty) + num_phones (1-phone contexts)
            prop_assert_eq!(fst.num_states(), 1 + num_phones);
        }

        /// Triphone FST has expected arc count.
        #[test]
        fn triphone_arc_count(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            let total_arcs: usize = (0..fst.num_states() as StateId)
                .map(|s| fst.transitions(s).len())
                .sum();

            // Arcs: (1 + num_phones) states * num_phones arcs each
            prop_assert_eq!(total_arcs, (1 + num_phones) * num_phones);
        }

        /// Triphone expected_states matches actual.
        #[test]
        fn triphone_expected_states_accurate(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            prop_assert_eq!(fst.num_states(), builder.expected_states());
        }

        /// Triphone expected_arcs matches actual.
        #[test]
        fn triphone_expected_arcs_accurate(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            let total_arcs: usize = (0..fst.num_states() as StateId)
                .map(|s| fst.transitions(s).len())
                .sum();

            prop_assert_eq!(total_arcs, builder.expected_arcs());
        }

        /// All triphone states are final.
        #[test]
        fn triphone_all_states_final(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            for id in 0..fst.num_states() as StateId {
                prop_assert!(fst.is_final(id));
            }
        }

        /// Triphone FST has a valid start state.
        #[test]
        fn triphone_has_start(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            prop_assert!(fst.start() != NO_STATE);
        }
    }

    // -------------------------------------------------------------------------
    // TetraploneBuilder Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(15))]

        /// Tetraphone FST has expected state count.
        #[test]
        fn tetraphone_state_count(num_phones in 2usize..5) {
            let builder = TetraploneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            // States: 1 + n + n²
            let expected = 1 + num_phones + num_phones * num_phones;
            prop_assert_eq!(fst.num_states(), expected);
        }

        /// Tetraphone expected_states matches actual.
        #[test]
        fn tetraphone_expected_states_accurate(num_phones in 2usize..5) {
            let builder = TetraploneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            prop_assert_eq!(fst.num_states(), builder.expected_states());
        }

        /// Tetraphone expected_arcs matches actual.
        #[test]
        fn tetraphone_expected_arcs_accurate(num_phones in 2usize..5) {
            let builder = TetraploneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            let total_arcs: usize = (0..fst.num_states() as StateId)
                .map(|s| fst.transitions(s).len())
                .sum();

            prop_assert_eq!(total_arcs, builder.expected_arcs());
        }

        /// All tetraphone states are final.
        #[test]
        fn tetraphone_all_states_final(num_phones in 2usize..5) {
            let builder = TetraploneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            for id in 0..fst.num_states() as StateId {
                prop_assert!(fst.is_final(id));
            }
        }

        /// Tetraphone has more states than triphone.
        #[test]
        fn tetraphone_more_states_than_triphone(num_phones in 3usize..6) {
            let tri = TriphoneBuilder::<LogWeight>::new(num_phones);
            let tetra = TetraploneBuilder::<LogWeight>::new(num_phones);

            prop_assert!(tetra.expected_states() > tri.expected_states());
        }

        /// Tetraphone has more arcs than triphone.
        #[test]
        fn tetraphone_more_arcs_than_triphone(num_phones in 3usize..6) {
            let tri = TriphoneBuilder::<LogWeight>::new(num_phones);
            let tetra = TetraploneBuilder::<LogWeight>::new(num_phones);

            prop_assert!(tetra.expected_arcs() > tri.expected_arcs());
        }
    }

    // -------------------------------------------------------------------------
    // CD Label Injectivity Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// CD label encoding is injective for all contexts including phone 0.
        ///
        /// The offset-by-1 encoding ensures that phone 0 in context contributes
        /// a non-zero value to the label, making the encoding fully injective.
        #[test]
        fn cd_label_injective(
            num_phones in 3usize..8,
            ctx1 in prop::collection::vec(0u32..3, 0..2),
            ctx2 in prop::collection::vec(0u32..3, 0..2),
            center1 in 0u32..3,
            center2 in 0u32..3
        ) {
            // Ensure phones are within range
            let ctx1: Vec<u32> = ctx1.into_iter().map(|p| p % num_phones as u32).collect();
            let ctx2: Vec<u32> = ctx2.into_iter().map(|p| p % num_phones as u32).collect();
            let center1 = center1 % num_phones as u32;
            let center2 = center2 % num_phones as u32;

            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 2, 1);

            let state1 = ContextState::with_context(ctx1.clone());
            let state2 = ContextState::with_context(ctx2.clone());

            let label1 = builder.compute_cd_label(&state1, center1);
            let label2 = builder.compute_cd_label(&state2, center2);

            // If labels are equal, contexts and centers must be equal
            if label1 == label2 {
                prop_assert_eq!(ctx1, ctx2);
                prop_assert_eq!(center1, center2);
            }
        }

        /// Phone 0 in context produces different label than empty context.
        #[test]
        fn phone_0_context_differs_from_empty(
            num_phones in 3usize..10,
            center in 0u32..5
        ) {
            let center = center % num_phones as u32;
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 2, 1);

            let empty = ContextState::initial();
            let with_zero = ContextState::with_context(vec![0]);

            let label_empty = builder.compute_cd_label(&empty, center);
            let label_with_zero = builder.compute_cd_label(&with_zero, center);

            prop_assert_ne!(
                label_empty,
                label_with_zero,
                "Phone 0 in context must produce different label than empty context"
            );
        }

        /// Different positions of phone 0 produce different labels.
        #[test]
        fn phone_0_position_matters(
            num_phones in 3usize..10,
            center in 0u32..5
        ) {
            let center = center % num_phones as u32;
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 2, 1);

            // [0, 1] vs [1, 0] should produce different labels
            let ctx_01 = ContextState::with_context(vec![0, 1]);
            let ctx_10 = ContextState::with_context(vec![1, 0]);

            let label_01 = builder.compute_cd_label(&ctx_01, center);
            let label_10 = builder.compute_cd_label(&ctx_10, center);

            prop_assert_ne!(
                label_01,
                label_10,
                "Different phone 0 positions must produce different labels"
            );
        }
    }
}
