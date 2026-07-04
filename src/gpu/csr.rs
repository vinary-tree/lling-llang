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

use std::fmt;
use std::marker::PhantomData;

use crate::semiring::LogWeight;
use crate::wfst::{StateId, VectorWfst, Wfst, NO_STATE};

const INVALID_CSR_STATE: CsrState = CsrState {
    arc_offset: 0,
    num_arcs: 0,
    final_weight: f32::INFINITY,
    flags: 0,
};

/// Error returned by checked CSR builder operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsrBuilderError {
    /// The builder cannot represent another state with the `u32` state ID type.
    StateCountOverflow {
        /// Number of states already present in the builder.
        num_states: usize,
    },
    /// The builder cannot represent another arc with `u32` CSR offsets.
    ArcCountOverflow {
        /// Number of arcs already present in the builder.
        num_arcs: usize,
    },
    /// States were not begun in ascending order.
    StateOutOfOrder {
        /// State ID expected by the builder.
        expected: StateId,
        /// State ID supplied by the caller.
        actual: StateId,
    },
    /// The builder's current state cursor does not refer to an existing state.
    InvalidCurrentState {
        /// State cursor currently held by the builder.
        current_state: StateId,
        /// Number of states present in the builder.
        num_states: usize,
    },
    /// A single state cannot represent another outgoing arc in `u32` metadata.
    StateArcCountOverflow {
        /// State whose outgoing arc count overflowed.
        state: StateId,
    },
}

impl fmt::Display for CsrBuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StateCountOverflow { num_states } => write!(
                f,
                "CSR builder cannot represent {} states with u32 state IDs",
                num_states
            ),
            Self::ArcCountOverflow { num_arcs } => write!(
                f,
                "CSR builder cannot represent arc index {} with u32 offsets",
                num_arcs
            ),
            Self::StateOutOfOrder { expected, actual } => write!(
                f,
                "CSR builder state {} is out of order; expected {}",
                actual, expected
            ),
            Self::InvalidCurrentState {
                current_state,
                num_states,
            } => write!(
                f,
                "CSR builder current state {} is invalid for {} states",
                current_state, num_states
            ),
            Self::StateArcCountOverflow { state } => write!(
                f,
                "CSR builder state {} has too many arcs for u32 metadata",
                state
            ),
        }
    }
}

impl std::error::Error for CsrBuilderError {}

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
#[derive(Clone, Copy, Debug)]
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

impl Default for CsrState {
    fn default() -> Self {
        INVALID_CSR_STATE
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
        self.get_state(state).unwrap_or(&INVALID_CSR_STATE)
    }

    /// Get state metadata if the state exists.
    pub fn get_state(&self, state: StateId) -> Option<&CsrState> {
        self.states.get(state as usize)
    }

    /// Get arcs for a state.
    pub fn arcs(&self, state: StateId) -> &[CsrArc<L>] {
        self.get_arcs(state).unwrap_or(&[])
    }

    /// Get arcs for a state if the state exists and its CSR range is valid.
    pub fn get_arcs(&self, state: StateId) -> Option<&[CsrArc<L>]> {
        let s = self.get_state(state)?;
        let start = s.arc_offset as usize;
        let end = start.checked_add(s.num_arcs as usize)?;
        self.arcs.get(start..end)
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
        self.state(state).is_final()
    }

    /// Get final weight for a state.
    pub fn final_weight(&self, state: StateId) -> f32 {
        self.state(state).final_weight
    }

    /// Compute memory size in bytes, returning `None` on overflow.
    pub fn checked_memory_size(&self) -> Option<usize> {
        let states_size = self
            .states
            .len()
            .checked_mul(std::mem::size_of::<CsrState>())?;
        let arcs_size = self
            .arcs
            .len()
            .checked_mul(std::mem::size_of::<CsrArc<L>>())?;
        let emitting_size = self
            .emitting_arc_indices
            .len()
            .checked_mul(std::mem::size_of::<u32>())?;
        states_size
            .checked_add(arcs_size)?
            .checked_add(emitting_size)
    }

    /// Compute memory size in bytes.
    pub fn memory_size(&self) -> usize {
        self.checked_memory_size().unwrap_or(usize::MAX)
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
            start_state: NO_STATE,
        }
    }

    /// Create with capacity hints.
    pub fn with_capacity(num_states: usize, num_arcs: usize) -> Self {
        Self {
            states: Vec::with_capacity(num_states),
            arcs: Vec::with_capacity(num_arcs),
            emitting_arc_indices: Vec::with_capacity(num_arcs / 2),
            current_state: 0,
            start_state: NO_STATE,
        }
    }

    /// Set the start state.
    pub fn set_start(&mut self, state: StateId) {
        self.clear_start_flags();
        self.start_state = state;
        if (state as usize) < self.states.len() {
            self.states[state as usize].flags |= CsrState::FLAG_START;
        }
    }

    /// Add a new state and return its ID.
    pub fn add_state(&mut self) -> StateId {
        self.try_add_state().unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to add a new state and return its ID.
    pub fn try_add_state(&mut self) -> Result<StateId, CsrBuilderError> {
        let id = usize_to_state_id(self.states.len())?;
        self.states.push(CsrState {
            arc_offset: usize_to_arc_index(self.arcs.len())?,
            num_arcs: 0,
            final_weight: f32::INFINITY,
            flags: 0,
        });
        Ok(id)
    }

    /// Set a state as final with the given weight.
    pub fn set_final(&mut self, state: StateId, weight: f32) {
        if let Some(s) = self.states.get_mut(state as usize) {
            s.final_weight = weight;
            s.flags |= CsrState::FLAG_FINAL;
        }
    }

    /// Begin adding arcs for a state.
    ///
    /// States must be finalized in order (0, 1, 2, ...).
    pub fn begin_state(&mut self, state: StateId) {
        match self.try_begin_state(state) {
            Ok(()) | Err(CsrBuilderError::InvalidCurrentState { .. }) => {}
            Err(err) => panic!("{err}"),
        }
    }

    /// Try to begin adding arcs for a state.
    ///
    /// States must be finalized in order (0, 1, 2, ...).
    pub fn try_begin_state(&mut self, state: StateId) -> Result<(), CsrBuilderError> {
        if state != self.current_state {
            return Err(CsrBuilderError::StateOutOfOrder {
                expected: self.current_state,
                actual: state,
            });
        }

        let offset = usize_to_arc_index(self.arcs.len())?;
        let num_states = self.states.len();
        let Some(state_data) = self.states.get_mut(state as usize) else {
            return Err(CsrBuilderError::InvalidCurrentState {
                current_state: state,
                num_states,
            });
        };

        state_data.arc_offset = offset;
        Ok(())
    }

    /// Add an arc to the current state.
    pub fn add_arc(&mut self, to: StateId, input: u32, output: u32, weight: f32) {
        match self.try_add_arc(to, input, output, weight) {
            Ok(()) | Err(CsrBuilderError::InvalidCurrentState { .. }) => {}
            Err(err) => panic!("{err}"),
        }
    }

    /// Try to add an arc to the current state.
    pub fn try_add_arc(
        &mut self,
        to: StateId,
        input: u32,
        output: u32,
        weight: f32,
    ) -> Result<(), CsrBuilderError> {
        let arc_idx = usize_to_arc_index(self.arcs.len())?;
        let num_states = self.states.len();
        let Some(state) = self.states.get_mut(self.current_state as usize) else {
            return Err(CsrBuilderError::InvalidCurrentState {
                current_state: self.current_state,
                num_states,
            });
        };

        if state.num_arcs == u32::MAX {
            return Err(CsrBuilderError::StateArcCountOverflow {
                state: self.current_state,
            });
        }

        let arc = CsrArc::new(to, input, output, weight);

        if arc.is_emitting() {
            self.emitting_arc_indices.push(arc_idx);
            state.flags |= CsrState::FLAG_HAS_EMITTING;
        }

        self.arcs.push(arc);
        state.num_arcs += 1;
        Ok(())
    }

    /// End the current state and move to the next.
    pub fn end_state(&mut self) {
        self.current_state = self.current_state.saturating_add(1);
    }

    /// Build the CSR WFST.
    pub fn build(mut self) -> CsrWfst<L> {
        self.try_retain_valid_arcs()
            .unwrap_or_else(|err| panic!("{err}"));

        self.finish_build()
    }

    /// Try to build the CSR WFST.
    pub fn try_build(mut self) -> Result<CsrWfst<L>, CsrBuilderError> {
        self.try_retain_valid_arcs()?;
        Ok(self.finish_build())
    }

    fn finish_build(mut self) -> CsrWfst<L> {
        let num_states = self.states.len();
        let start_state = if (self.start_state as usize) < num_states {
            self.start_state
        } else {
            NO_STATE
        };

        // Mark start state
        self.clear_start_flags();
        if (start_state as usize) < self.states.len() {
            self.states[start_state as usize].flags |= CsrState::FLAG_START;
        }

        let num_arcs = self.arcs.len();

        CsrWfst {
            states: self.states,
            arcs: self.arcs,
            emitting_arc_indices: self.emitting_arc_indices,
            start_state,
            num_states,
            num_arcs,
        }
    }

    fn try_retain_valid_arcs(&mut self) -> Result<(), CsrBuilderError> {
        let num_states = self.states.len();
        let old_arcs = std::mem::take(&mut self.arcs);
        self.emitting_arc_indices.clear();
        self.arcs.reserve(old_arcs.len());

        for (state_id, state) in self.states.iter_mut().enumerate() {
            let start = state.arc_offset as usize;
            let end = start
                .saturating_add(state.num_arcs as usize)
                .min(old_arcs.len());

            state.arc_offset = usize_to_arc_index(self.arcs.len())?;
            state.num_arcs = 0;
            state.flags &= !CsrState::FLAG_HAS_EMITTING;

            if start >= end {
                continue;
            }

            for arc in &old_arcs[start..end] {
                if (arc.to as usize) >= num_states {
                    continue;
                }

                if state.num_arcs == u32::MAX {
                    return Err(CsrBuilderError::StateArcCountOverflow {
                        state: state_id as StateId,
                    });
                }

                if arc.is_emitting() {
                    self.emitting_arc_indices
                        .push(usize_to_arc_index(self.arcs.len())?);
                    state.flags |= CsrState::FLAG_HAS_EMITTING;
                }

                self.arcs.push(arc.clone());
                state.num_arcs += 1;
            }
        }

        Ok(())
    }

    fn clear_start_flags(&mut self) {
        for state in &mut self.states {
            state.flags &= !CsrState::FLAG_START;
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
pub fn csr_from_vector_wfst<L, F>(fst: &VectorWfst<L, LogWeight>, label_to_u32: F) -> CsrWfst<L>
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
            let input = arc
                .input
                .as_ref()
                .map(|l| label_to_u32(l))
                .unwrap_or(u32::MAX);
            let output = arc
                .output
                .as_ref()
                .map(|l| label_to_u32(l))
                .unwrap_or(u32::MAX);
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

/// Compute memory size for a CSR WFST, returning `None` on overflow.
///
/// # Arguments
///
/// * `num_states` - Number of states
/// * `num_arcs` - Number of arcs
/// * `num_emitting` - Number of emitting arcs
///
/// # Returns
///
/// Memory size in bytes, or `None` if the calculation overflows `usize`.
pub fn checked_csr_memory_size(
    num_states: usize,
    num_arcs: usize,
    num_emitting: usize,
) -> Option<usize> {
    let states_size = num_states.checked_mul(std::mem::size_of::<CsrState>())?;
    let arcs_size = num_arcs.checked_mul(std::mem::size_of::<CsrArc<()>>())?;
    let emitting_size = num_emitting.checked_mul(std::mem::size_of::<u32>())?;
    states_size
        .checked_add(arcs_size)?
        .checked_add(emitting_size)
}

/// Compute memory size for a CSR WFST.
///
/// Returns `usize::MAX` if the size calculation overflows. Use
/// [`checked_csr_memory_size`] when overflow needs to be distinguished from a
/// very large valid size.
pub fn csr_memory_size(num_states: usize, num_arcs: usize, num_emitting: usize) -> usize {
    checked_csr_memory_size(num_states, num_arcs, num_emitting).unwrap_or(usize::MAX)
}

fn usize_to_state_id(value: usize) -> Result<StateId, CsrBuilderError> {
    if value <= StateId::MAX as usize {
        Ok(value as StateId)
    } else {
        Err(CsrBuilderError::StateCountOverflow { num_states: value })
    }
}

fn usize_to_arc_index(value: usize) -> Result<u32, CsrBuilderError> {
    if value <= u32::MAX as usize {
        Ok(value as u32)
    } else {
        Err(CsrBuilderError::ArcCountOverflow { num_arcs: value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::Semiring;
    use crate::wfst::{MutableWfst, WeightedTransition};

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
        assert!(state.final_weight.is_infinite());

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
    fn test_empty_builder_has_no_start_state() {
        let csr: CsrWfst<u32> = CsrBuilder::new().build();

        assert_eq!(csr.start_state(), NO_STATE);
        assert_eq!(csr.num_states(), 0);
        assert_eq!(csr.num_arcs(), 0);
    }

    #[test]
    fn test_csr_builder_replaces_start_state_flag() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();
        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.set_start(s0);
        builder.set_start(s1);

        builder.begin_state(s0);
        builder.end_state();
        builder.begin_state(s1);
        builder.end_state();

        let csr = builder.build();

        assert_eq!(csr.start_state(), s1);
        assert!(!csr.state(s0).is_start());
        assert!(csr.state(s1).is_start());
    }

    #[test]
    fn test_csr_builder_invalid_start_clears_stale_flag() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();
        let s0 = builder.add_state();

        builder.set_start(s0);
        builder.set_start(99);

        builder.begin_state(s0);
        builder.end_state();

        let csr = builder.build();

        assert_eq!(csr.start_state(), NO_STATE);
        assert!(!csr.state(s0).is_start());
    }

    #[test]
    fn test_csr_invalid_state_access_is_total() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();
        let s0 = builder.add_state();
        builder.begin_state(s0);
        builder.end_state();
        let csr = builder.build();

        assert!(csr.get_state(99).is_none());
        assert!(csr.get_arcs(99).is_none());
        assert_eq!(csr.arcs(99).len(), 0);
        assert!(!csr.is_final(99));
        assert!(csr.final_weight(99).is_infinite());
        assert!(!csr.state(99).is_final());
    }

    #[test]
    fn test_csr_builder_ignores_arcs_without_current_state() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();

        builder.add_arc(0, 1, 1, 0.5);
        let csr = builder.build();

        assert_eq!(csr.num_states(), 0);
        assert_eq!(csr.num_arcs(), 0);
        assert!(csr.emitting_arc_indices().is_empty());
    }

    #[test]
    fn test_csr_builder_try_add_arc_reports_missing_current_state() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();

        assert_eq!(
            builder.try_add_arc(0, 1, 1, 0.5),
            Err(CsrBuilderError::InvalidCurrentState {
                current_state: 0,
                num_states: 0,
            })
        );
    }

    #[test]
    fn test_csr_builder_try_begin_state_rejects_out_of_order_state() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();
        let s0 = builder.add_state();
        let s1 = builder.add_state();

        assert_eq!(
            builder.try_begin_state(s1),
            Err(CsrBuilderError::StateOutOfOrder {
                expected: s0,
                actual: s1,
            })
        );
    }

    #[test]
    #[should_panic(expected = "CSR builder state 1 is out of order; expected 0")]
    fn test_csr_builder_begin_state_preserves_panic_contract() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();
        let _s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.begin_state(s1);
    }

    #[test]
    fn test_csr_builder_prunes_malformed_arc_targets() {
        let mut builder: CsrBuilder<u32> = CsrBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.begin_state(s0);
        builder.add_arc(s1, 1, 1, 0.5);
        builder.add_arc(99, 2, 2, 1.0);
        builder.end_state();

        builder.begin_state(s1);
        builder.end_state();

        let csr = builder.build();
        let arcs = csr.arcs(s0);

        assert_eq!(csr.num_arcs(), 1);
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].to, s1);
        assert_eq!(csr.emitting_arc_indices(), &[0]);
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
    fn test_csr_from_vector_wfst_prunes_malformed_targets() {
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.set_transitions(
            s0,
            vec![
                WeightedTransition::new(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0)),
                WeightedTransition::new(s0, Some('x'), Some('x'), 99, LogWeight::new(2.0)),
            ],
        );

        let csr = csr_from_vector_wfst(&fst, |c| *c as u32);
        let arcs = csr.arcs(s0);

        assert_eq!(csr.num_arcs(), 1);
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].input, 'a' as u32);
        assert_eq!(arcs[0].to, s1);
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
    fn test_csr_memory_size_overflow_is_explicit() {
        assert_eq!(checked_csr_memory_size(usize::MAX, 1, 1), None);
        assert_eq!(csr_memory_size(usize::MAX, 1, 1), usize::MAX);
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
