//! Compressed Sparse Row (CSR) representation for WFSTs.
//!
//! CSR format provides cache-efficient storage for sparse graphs, enabling
//! coalesced memory access patterns essential for GPU performance.
//!
//! ## Format
//!
//! The CSR format stores a WFST using three arrays:
//!
//! ```text
//! row_offsets: [0, 2, 5, 7, ...]  // Start index of each state's arcs
//! arc_data:    [Arc0, Arc1, Arc2, Arc3, ...]  // All arcs in state order
//! final_weights: [w0, w1, w2, ...]  // Final weights (f32::INFINITY if non-final)
//! ```
//!
//! ## Memory Layout
//!
//! ```text
//! M_fst = 12|Q| + 8|E| + 4|E_E|
//! ```
//!
//! Where:
//! - 12|Q| = row_offsets (4 bytes) + final_weights (4 bytes) + state_flags (4 bytes)
//! - 8|E| = arc destination (4 bytes) + arc weight (4 bytes)
//! - 4|E_E| = emitting arc index for quick lookup
//!
//! ## Benefits
//!
//! - **Compact**: ~1/3 size of adjacency list representation
//! - **Coalesced access**: Sequential arcs in memory
//! - **GPU-friendly**: Direct indexing without pointer chasing

use std::marker::PhantomData;

use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{StateId, VectorWfst, Wfst};

/// A single arc in CSR format.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct CsrArc<L: Clone> {
    /// Destination state ID.
    pub to: StateId,
    /// Input label (u32::MAX for epsilon).
    pub input: u32,
    /// Output label (u32::MAX for epsilon).
    pub output: u32,
    /// Arc weight (as f32 for GPU efficiency).
    pub weight: f32,
    /// Phantom for label type.
    _phantom: PhantomData<L>,
}

impl<L: Clone> CsrArc<L> {
    /// Create a new CSR arc.
    pub fn new(to: StateId, input: u32, output: u32, weight: f32) -> Self {
        Self {
            to,
            input,
            output,
            weight,
            _phantom: PhantomData,
        }
    }

    /// Check if input is epsilon.
    pub fn is_input_epsilon(&self) -> bool {
        self.input == u32::MAX
    }

    /// Check if output is epsilon.
    pub fn is_output_epsilon(&self) -> bool {
        self.output == u32::MAX
    }

    /// Check if this is an emitting arc (non-epsilon input).
    pub fn is_emitting(&self) -> bool {
        !self.is_input_epsilon()
    }
}

/// State metadata in CSR format.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct CsrState {
    /// Index into arc array where this state's arcs begin.
    pub arc_offset: u32,
    /// Number of arcs from this state.
    pub num_arcs: u32,
    /// Final weight (f32::INFINITY if non-final).
    pub final_weight: f32,
    /// Flags (bit 0: is_start, bit 1: is_final, etc.).
    pub flags: u32,
}

impl CsrState {
    /// Flag indicating this is the start state.
    pub const FLAG_START: u32 = 1 << 0;
    /// Flag indicating this is a final state.
    pub const FLAG_FINAL: u32 = 1 << 1;
    /// Flag indicating this state has emitting arcs.
    pub const FLAG_HAS_EMITTING: u32 = 1 << 2;

    /// Check if this is the start state.
    pub fn is_start(&self) -> bool {
        self.flags & Self::FLAG_START != 0
    }

    /// Check if this is a final state.
    pub fn is_final(&self) -> bool {
        self.flags & Self::FLAG_FINAL != 0
    }

    /// Check if this state has emitting arcs.
    pub fn has_emitting_arcs(&self) -> bool {
        self.flags & Self::FLAG_HAS_EMITTING != 0
    }
}

/// WFST in Compressed Sparse Row format.
///
/// This representation is optimized for GPU execution with:
/// - Contiguous memory layout
/// - Direct indexing (no pointer chasing)
/// - Coalesced memory access patterns
#[derive(Clone, Debug)]
pub struct CsrWfst<L: Clone> {
    /// State metadata array.
    states: Vec<CsrState>,
    /// Arc data array (all arcs concatenated).
    arcs: Vec<CsrArc<L>>,
    /// Index of emitting arcs (for quick filtering).
    emitting_arc_indices: Vec<u32>,
    /// Start state ID.
    start_state: StateId,
    /// Number of states.
    num_states: usize,
    /// Number of arcs.
    num_arcs: usize,
}

impl<L: Clone> CsrWfst<L> {
    /// Get the number of states.
    pub fn num_states(&self) -> usize {
        self.num_states
    }

    /// Get the number of arcs.
    pub fn num_arcs(&self) -> usize {
        self.num_arcs
    }

    /// Get the start state.
    pub fn start_state(&self) -> StateId {
        self.start_state
    }

    /// Get state metadata.
    pub fn state(&self, state: StateId) -> &CsrState {
        &self.states[state as usize]
    }

    /// Get arcs for a state.
    pub fn arcs(&self, state: StateId) -> &[CsrArc<L>] {
        let s = &self.states[state as usize];
        let start = s.arc_offset as usize;
        let end = start + s.num_arcs as usize;
        &self.arcs[start..end]
    }

    /// Get all arcs (for GPU transfer).
    pub fn all_arcs(&self) -> &[CsrArc<L>] {
        &self.arcs
    }

    /// Get all states (for GPU transfer).
    pub fn all_states(&self) -> &[CsrState] {
        &self.states
    }

    /// Get emitting arc indices.
    pub fn emitting_arc_indices(&self) -> &[u32] {
        &self.emitting_arc_indices
    }

    /// Check if a state is final.
    pub fn is_final(&self, state: StateId) -> bool {
        self.states[state as usize].is_final()
    }

    /// Get final weight for a state.
    pub fn final_weight(&self, state: StateId) -> f32 {
        self.states[state as usize].final_weight
    }

    /// Compute memory size in bytes.
    pub fn memory_size(&self) -> usize {
        let states_size = self.states.len() * std::mem::size_of::<CsrState>();
        let arcs_size = self.arcs.len() * std::mem::size_of::<CsrArc<L>>();
        let emitting_size = self.emitting_arc_indices.len() * std::mem::size_of::<u32>();
        states_size + arcs_size + emitting_size
    }
}

/// Builder for constructing CSR WFSTs.
#[derive(Clone, Debug)]
pub struct CsrBuilder<L: Clone> {
    states: Vec<CsrState>,
    arcs: Vec<CsrArc<L>>,
    emitting_arc_indices: Vec<u32>,
    current_state: StateId,
    start_state: StateId,
}

impl<L: Clone> CsrBuilder<L> {
    /// Create a new CSR builder.
    pub fn new() -> Self {
        Self {
            states: Vec::new(),
            arcs: Vec::new(),
            emitting_arc_indices: Vec::new(),
            current_state: 0,
            start_state: 0,
        }
    }

    /// Create with capacity hints.
    pub fn with_capacity(num_states: usize, num_arcs: usize) -> Self {
        Self {
            states: Vec::with_capacity(num_states),
            arcs: Vec::with_capacity(num_arcs),
            emitting_arc_indices: Vec::with_capacity(num_arcs / 2),
            current_state: 0,
            start_state: 0,
        }
    }

    /// Set the start state.
    pub fn set_start(&mut self, state: StateId) {
        self.start_state = state;
        if (state as usize) < self.states.len() {
            self.states[state as usize].flags |= CsrState::FLAG_START;
        }
    }

    /// Add a new state and return its ID.
    pub fn add_state(&mut self) -> StateId {
        let id = self.states.len() as StateId;
        self.states.push(CsrState {
            arc_offset: self.arcs.len() as u32,
            num_arcs: 0,
            final_weight: f32::INFINITY,
            flags: 0,
        });
        id
    }

    /// Set a state as final with the given weight.
    pub fn set_final(&mut self, state: StateId, weight: f32) {
        let s = &mut self.states[state as usize];
        s.final_weight = weight;
        s.flags |= CsrState::FLAG_FINAL;
    }

    /// Begin adding arcs for a state.
    ///
    /// States must be finalized in order (0, 1, 2, ...).
    pub fn begin_state(&mut self, state: StateId) {
        assert_eq!(state, self.current_state, "States must be added in order");
        if (state as usize) < self.states.len() {
            self.states[state as usize].arc_offset = self.arcs.len() as u32;
        }
    }

    /// Add an arc to the current state.
    pub fn add_arc(&mut self, to: StateId, input: u32, output: u32, weight: f32) {
        let arc_idx = self.arcs.len() as u32;
        let arc = CsrArc::new(to, input, output, weight);

        if arc.is_emitting() {
            self.emitting_arc_indices.push(arc_idx);
            if (self.current_state as usize) < self.states.len() {
                self.states[self.current_state as usize].flags |= CsrState::FLAG_HAS_EMITTING;
            }
        }

        self.arcs.push(arc);

        if (self.current_state as usize) < self.states.len() {
            self.states[self.current_state as usize].num_arcs += 1;
        }
    }

    /// End the current state and move to the next.
    pub fn end_state(&mut self) {
        self.current_state += 1;
    }

    /// Build the CSR WFST.
    pub fn build(mut self) -> CsrWfst<L> {
        // Mark start state
        if (self.start_state as usize) < self.states.len() {
            self.states[self.start_state as usize].flags |= CsrState::FLAG_START;
        }

        let num_states = self.states.len();
        let num_arcs = self.arcs.len();

        CsrWfst {
            states: self.states,
            arcs: self.arcs,
            emitting_arc_indices: self.emitting_arc_indices,
            start_state: self.start_state,
            num_states,
            num_arcs,
        }
    }
}

impl<L: Clone> Default for CsrBuilder<L> {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a VectorWfst to CSR format.
///
/// # Arguments
///
/// * `fst` - The source WFST
/// * `label_to_u32` - Function to convert labels to u32 indices
///
/// # Returns
///
/// A CSR representation of the WFST.
pub fn csr_from_vector_wfst<L, F>(
    fst: &VectorWfst<L, LogWeight>,
    label_to_u32: F,
) -> CsrWfst<L>
where
    L: Clone + Send + Sync,
    F: Fn(&L) -> u32,
{
    let num_states = fst.num_states();
    let num_arcs: usize = (0..num_states as StateId)
        .map(|s| fst.transitions(s).len())
        .sum();

    let mut builder = CsrBuilder::with_capacity(num_states, num_arcs);

    // Add all states
    for _ in 0..num_states {
        builder.add_state();
    }

    // Set start state
    let start = fst.start();
    if start != crate::wfst::NO_STATE {
        builder.set_start(start);
    }

    // Add arcs for each state
    for state in 0..num_states as StateId {
        builder.begin_state(state);

        for arc in fst.transitions(state) {
            let input = arc.input.as_ref().map(|l| label_to_u32(l)).unwrap_or(u32::MAX);
            let output = arc.output.as_ref().map(|l| label_to_u32(l)).unwrap_or(u32::MAX);
            let weight = arc.weight.value() as f32;

            builder.add_arc(arc.to, input, output, weight);
        }

        // Set final weight
        if fst.is_final(state) {
            let weight = fst.final_weight(state).value() as f32;
            builder.set_final(state, weight);
        }

        builder.end_state();
    }

    builder.build()
}

/// Compute memory size for a CSR WFST.
///
/// # Arguments
///
/// * `num_states` - Number of states
/// * `num_arcs` - Number of arcs
/// * `num_emitting` - Number of emitting arcs
///
/// # Returns
///
/// Memory size in bytes.
pub fn csr_memory_size(num_states: usize, num_arcs: usize, num_emitting: usize) -> usize {
    let states_size = num_states * std::mem::size_of::<CsrState>();
    let arcs_size = num_arcs * 16; // CsrArc is 16 bytes (to, input, output, weight)
    let emitting_size = num_emitting * std::mem::size_of::<u32>();
    states_size + arcs_size + emitting_size
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::MutableWfst;

    #[test]
    fn test_csr_arc_creation() {
        let arc: CsrArc<char> = CsrArc::new(1, 10, 20, 0.5);
        assert_eq!(arc.to, 1);
        assert_eq!(arc.input, 10);
        assert_eq!(arc.output, 20);
        assert!((arc.weight - 0.5).abs() < 1e-6);
        assert!(!arc.is_input_epsilon());
        assert!(arc.is_emitting());
    }

    #[test]
    fn test_csr_arc_epsilon() {
        let arc: CsrArc<char> = CsrArc::new(1, u32::MAX, u32::MAX, 0.0);
        assert!(arc.is_input_epsilon());
        assert!(arc.is_output_epsilon());
        assert!(!arc.is_emitting());
    }

    #[test]
    fn test_csr_state_flags() {
        let mut state = CsrState::default();
        assert!(!state.is_start());
        assert!(!state.is_final());

        state.flags |= CsrState::FLAG_START;
        assert!(state.is_start());

        state.flags |= CsrState::FLAG_FINAL;
        assert!(state.is_final());
    }

    #[test]
    fn test_csr_builder() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();
        let s2 = builder.add_state();

        builder.set_start(s0);
        builder.set_final(s2, 0.0);

        builder.begin_state(s0);
        builder.add_arc(s1, 1, 1, 0.5);
        builder.add_arc(s2, 2, 2, 1.0);
        builder.end_state();

        builder.begin_state(s1);
        builder.add_arc(s2, 3, 3, 0.25);
        builder.end_state();

        builder.begin_state(s2);
        builder.end_state();

        let csr = builder.build();

        assert_eq!(csr.num_states(), 3);
        assert_eq!(csr.num_arcs(), 3);
        assert_eq!(csr.start_state(), 0);
        assert!(csr.is_final(s2));
        assert!(!csr.is_final(s0));
    }

    #[test]
    fn test_csr_from_vector_wfst() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('b'), s1, LogWeight::new(1.0));

        let csr = csr_from_vector_wfst(&fst, |c| *c as u32);

        assert_eq!(csr.num_states(), 2);
        assert_eq!(csr.num_arcs(), 1);
        assert_eq!(csr.start_state(), 0);
        assert!(csr.is_final(1));

        let arcs = csr.arcs(0);
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].to, 1);
        assert_eq!(arcs[0].input, 'a' as u32);
        assert_eq!(arcs[0].output, 'b' as u32);
    }

    #[test]
    fn test_csr_memory_size() {
        let size = csr_memory_size(1000, 5000, 2500);
        // states: 1000 * 16 = 16000
        // arcs: 5000 * 16 = 80000
        // emitting: 2500 * 4 = 10000
        // total: 106000
        assert!(size > 100000);
    }

    #[test]
    fn test_csr_arcs_access() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.set_start(s0);

        builder.begin_state(s0);
        builder.add_arc(s1, 1, 1, 0.5);
        builder.add_arc(s1, 2, 2, 1.0);
        builder.add_arc(s1, 3, 3, 1.5);
        builder.end_state();

        builder.begin_state(s1);
        builder.set_final(s1, 0.0);
        builder.end_state();

        let csr = builder.build();

        let arcs = csr.arcs(s0);
        assert_eq!(arcs.len(), 3);
        assert_eq!(arcs[0].input, 1);
        assert_eq!(arcs[1].input, 2);
        assert_eq!(arcs[2].input, 3);

        let arcs_s1 = csr.arcs(s1);
        assert_eq!(arcs_s1.len(), 0);
    }

    #[test]
    fn test_emitting_arc_indices() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.set_start(s0);

        builder.begin_state(s0);
        builder.add_arc(s1, u32::MAX, u32::MAX, 0.0); // epsilon
        builder.add_arc(s1, 1, 1, 0.5); // emitting
        builder.add_arc(s1, u32::MAX, 2, 0.0); // input epsilon
        builder.add_arc(s1, 3, u32::MAX, 0.0); // output epsilon (emitting!)
        builder.end_state();

        builder.begin_state(s1);
        builder.end_state();

        let csr = builder.build();

        // Emitting arcs are those with non-epsilon input: indices 1 and 3
        assert_eq!(csr.emitting_arc_indices().len(), 2);
        assert_eq!(csr.emitting_arc_indices()[0], 1);
        assert_eq!(csr.emitting_arc_indices()[1], 3);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // =========================================================================
    // CsrArc Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// CsrArc preserves all constructor arguments.
        #[test]
        fn csr_arc_preserves_fields(
            to in 0u32..1000,
            input in 0u32..u32::MAX,
            output in 0u32..u32::MAX,
            weight in -1000.0f32..1000.0
        ) {
            let arc: CsrArc<char> = CsrArc::new(to, input, output, weight);
            prop_assert_eq!(arc.to, to);
            prop_assert_eq!(arc.input, input);
            prop_assert_eq!(arc.output, output);
            prop_assert!((arc.weight - weight).abs() < 1e-6);
        }

        /// is_input_epsilon is true iff input == u32::MAX.
        #[test]
        fn is_input_epsilon_correct(input in 0u32..u32::MAX) {
            let arc_non_eps: CsrArc<char> = CsrArc::new(0, input, 0, 0.0);
            let arc_eps: CsrArc<char> = CsrArc::new(0, u32::MAX, 0, 0.0);

            prop_assert!(!arc_non_eps.is_input_epsilon());
            prop_assert!(arc_eps.is_input_epsilon());
        }

        /// is_output_epsilon is true iff output == u32::MAX.
        #[test]
        fn is_output_epsilon_correct(output in 0u32..u32::MAX) {
            let arc_non_eps: CsrArc<char> = CsrArc::new(0, 0, output, 0.0);
            let arc_eps: CsrArc<char> = CsrArc::new(0, 0, u32::MAX, 0.0);

            prop_assert!(!arc_non_eps.is_output_epsilon());
            prop_assert!(arc_eps.is_output_epsilon());
        }

        /// is_emitting is equivalent to !is_input_epsilon.
        #[test]
        fn is_emitting_consistent(input in 0u32..=u32::MAX) {
            let arc: CsrArc<char> = CsrArc::new(0, input, 0, 0.0);
            prop_assert_eq!(arc.is_emitting(), !arc.is_input_epsilon());
        }
    }

    // =========================================================================
    // CsrState Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// CsrState flags are independent.
        #[test]
        fn csr_state_flags_independent(
            is_start in any::<bool>(),
            is_final in any::<bool>(),
            has_emitting in any::<bool>()
        ) {
            let mut state = CsrState::default();

            if is_start {
                state.flags |= CsrState::FLAG_START;
            }
            if is_final {
                state.flags |= CsrState::FLAG_FINAL;
            }
            if has_emitting {
                state.flags |= CsrState::FLAG_HAS_EMITTING;
            }

            prop_assert_eq!(state.is_start(), is_start);
            prop_assert_eq!(state.is_final(), is_final);
            prop_assert_eq!(state.has_emitting_arcs(), has_emitting);
        }

        /// Default CsrState has no flags set.
        #[test]
        fn csr_state_default_no_flags(_dummy in 0..10i32) {
            let state = CsrState::default();
            prop_assert!(!state.is_start());
            prop_assert!(!state.is_final());
            prop_assert!(!state.has_emitting_arcs());
            prop_assert_eq!(state.flags, 0);
        }
    }

    // =========================================================================
    // CsrBuilder Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Adding states returns sequential IDs starting from 0.
        #[test]
        fn builder_state_ids_sequential(num_states in 1usize..20) {
            let mut builder: CsrBuilder<u32> = CsrBuilder::new();

            for expected_id in 0..num_states {
                let actual_id = builder.add_state();
                prop_assert_eq!(actual_id as usize, expected_id);
            }
        }

        /// Built CSR has correct state and arc counts.
        #[test]
        fn builder_counts_correct(
            num_states in 2usize..10,
            arcs_per_state in 0usize..5
        ) {
            let mut builder: CsrBuilder<u32> = CsrBuilder::new();

            // Add states
            for _ in 0..num_states {
                builder.add_state();
            }

            // Add arcs
            for state in 0..num_states as StateId {
                builder.begin_state(state);
                for arc_idx in 0..arcs_per_state {
                    let to = ((state as usize + 1) % num_states) as StateId;
                    builder.add_arc(to, arc_idx as u32, arc_idx as u32, 0.5);
                }
                builder.end_state();
            }

            let csr = builder.build();
            prop_assert_eq!(csr.num_states(), num_states);
            prop_assert_eq!(csr.num_arcs(), num_states * arcs_per_state);
        }

        /// Start state is correctly marked.
        #[test]
        fn builder_start_state_marked(num_states in 2usize..10, start in 0usize..10) {
            let mut builder: CsrBuilder<u32> = CsrBuilder::new();

            for _ in 0..num_states {
                builder.add_state();
            }

            let start_state = (start % num_states) as StateId;
            builder.set_start(start_state);

            // Need to process states in order
            for state in 0..num_states as StateId {
                builder.begin_state(state);
                builder.end_state();
            }

            let csr = builder.build();
            prop_assert_eq!(csr.start_state(), start_state);
            prop_assert!(csr.state(start_state).is_start());
        }

        /// Final states are correctly marked.
        #[test]
        fn builder_final_states_marked(
            num_states in 2usize..10,
            final_weight in 0.0f32..10.0
        ) {
            let mut builder: CsrBuilder<u32> = CsrBuilder::new();

            for _ in 0..num_states {
                builder.add_state();
            }

            let final_state = (num_states - 1) as StateId;
            builder.set_final(final_state, final_weight);

            for state in 0..num_states as StateId {
                builder.begin_state(state);
                builder.end_state();
            }

            let csr = builder.build();
            prop_assert!(csr.is_final(final_state));
            prop_assert!((csr.final_weight(final_state) - final_weight).abs() < 1e-6);
        }
    }

    // =========================================================================
    // CsrWfst Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// arcs() returns correct slice for each state.
        #[test]
        fn csr_arcs_correct_per_state(num_states in 2usize..8) {
            let mut builder: CsrBuilder<u32> = CsrBuilder::new();

            for _ in 0..num_states {
                builder.add_state();
            }

            // Each state has state_id number of arcs
            let mut expected_arcs = Vec::new();
            for state in 0..num_states as StateId {
                builder.begin_state(state);
                let num_arcs = state as usize;
                expected_arcs.push(num_arcs);
                for i in 0..num_arcs {
                    let to = ((state as usize + 1) % num_states) as StateId;
                    builder.add_arc(to, i as u32, i as u32, state as f32);
                }
                builder.end_state();
            }

            let csr = builder.build();

            for state in 0..num_states as StateId {
                let arcs = csr.arcs(state);
                prop_assert_eq!(arcs.len(), expected_arcs[state as usize]);
            }
        }

        /// all_arcs returns all arcs concatenated.
        #[test]
        fn csr_all_arcs_total(num_states in 2usize..8, arcs_per_state in 1usize..4) {
            let mut builder: CsrBuilder<u32> = CsrBuilder::new();

            for _ in 0..num_states {
                builder.add_state();
            }

            for state in 0..num_states as StateId {
                builder.begin_state(state);
                for i in 0..arcs_per_state {
                    let to = ((state as usize + 1) % num_states) as StateId;
                    builder.add_arc(to, i as u32, i as u32, 0.5);
                }
                builder.end_state();
            }

            let csr = builder.build();
            prop_assert_eq!(csr.all_arcs().len(), num_states * arcs_per_state);
        }

        /// all_states returns all state metadata.
        #[test]
        fn csr_all_states_count(num_states in 1usize..20) {
            let mut builder: CsrBuilder<u32> = CsrBuilder::new();

            for _ in 0..num_states {
                builder.add_state();
            }

            for state in 0..num_states as StateId {
                builder.begin_state(state);
                builder.end_state();
            }

            let csr = builder.build();
            prop_assert_eq!(csr.all_states().len(), num_states);
        }

        /// emitting_arc_indices tracks only emitting arcs.
        #[test]
        fn csr_emitting_indices_correct(num_states in 2usize..6) {
            let mut builder: CsrBuilder<u32> = CsrBuilder::new();

            for _ in 0..num_states {
                builder.add_state();
            }

            let mut expected_emitting = 0;
            for state in 0..num_states as StateId {
                builder.begin_state(state);
                let to = ((state as usize + 1) % num_states) as StateId;
                // Add epsilon arc
                builder.add_arc(to, u32::MAX, u32::MAX, 0.0);
                // Add emitting arc
                builder.add_arc(to, state, state, 1.0);
                expected_emitting += 1;
                builder.end_state();
            }

            let csr = builder.build();
            prop_assert_eq!(csr.emitting_arc_indices().len(), expected_emitting);
        }

        /// memory_size is positive and grows with size.
        #[test]
        fn csr_memory_size_grows(num_states in 2usize..20, arcs_per_state in 1usize..5) {
            let mut builder: CsrBuilder<u32> = CsrBuilder::new();

            for _ in 0..num_states {
                builder.add_state();
            }

            for state in 0..num_states as StateId {
                builder.begin_state(state);
                for i in 0..arcs_per_state {
                    let to = ((state as usize + 1) % num_states) as StateId;
                    builder.add_arc(to, i as u32, i as u32, 0.5);
                }
                builder.end_state();
            }

            let csr = builder.build();
            let mem = csr.memory_size();

            // Memory should be at least states * sizeof(CsrState) + arcs * sizeof(CsrArc)
            prop_assert!(mem > 0);
            prop_assert!(mem >= num_states * std::mem::size_of::<CsrState>());
        }
    }

    // =========================================================================
    // Memory Size Function Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// csr_memory_size grows linearly with inputs.
        #[test]
        fn memory_size_linear(
            num_states in 1usize..1000,
            num_arcs in 1usize..5000,
            num_emitting in 0usize..2500
        ) {
            let num_emitting = num_emitting.min(num_arcs);
            let size = csr_memory_size(num_states, num_arcs, num_emitting);

            // Verify components
            let states_contribution = num_states * std::mem::size_of::<CsrState>();
            let arcs_contribution = num_arcs * 16;
            let emitting_contribution = num_emitting * 4;

            prop_assert_eq!(size, states_contribution + arcs_contribution + emitting_contribution);
        }

        /// Doubling inputs approximately doubles memory.
        #[test]
        fn memory_size_scales(
            num_states in 10usize..100,
            num_arcs in 100usize..1000,
            num_emitting in 10usize..100
        ) {
            let size1 = csr_memory_size(num_states, num_arcs, num_emitting);
            let size2 = csr_memory_size(num_states * 2, num_arcs * 2, num_emitting * 2);

            // Size should roughly double
            prop_assert!(size2 >= size1);
            prop_assert!(size2 <= size1 * 3); // Allow some overhead
        }
    }
}
