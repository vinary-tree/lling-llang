//! CTC topology implementations.
//!
//! This module provides WFST implementations of various CTC topologies:
//! - Correct-CTC: Standard complete graph (N states, N² arcs)
//! - Compact-CTC: Reduced graph with blank back-off (N states, 3N-2 arcs)
//! - Minimal-CTC: Smallest possible graph (1 state, N arcs)
//! - Selfless variants: Remove non-blank self-loops for wide context models

use std::fmt;

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst, NO_STATE};

/// CTC label type (vocabulary index).
///
/// Label 0 is reserved for the blank token.
pub type CtcLabel = u32;

/// The blank token index (always 0 in CTC).
pub const BLANK: CtcLabel = 0;

const ARC_COUNT_OVERFLOW: &str = "CTC topology arc count exceeds usize";

/// Error returned by fallible CTC topology constructors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CtcTopologyError {
    /// The vocabulary is empty, so it cannot contain the required blank label.
    EmptyVocabulary,
    /// The vocabulary would require a state ID reserved for the absent-state sentinel.
    VocabSizeExceedsStateSpace {
        /// Requested vocabulary size.
        vocab_size: usize,
        /// Largest vocabulary size representable by concrete WFST state IDs.
        max_vocab_size: usize,
    },
    /// The topology's closed-form arc count overflowed `usize`.
    ArcCountOverflow {
        /// Human-readable topology name.
        topology: &'static str,
        /// Requested vocabulary size.
        vocab_size: usize,
    },
}

impl fmt::Display for CtcTopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyVocabulary => write!(f, "vocab_size must be at least 1 (for blank)"),
            Self::VocabSizeExceedsStateSpace { max_vocab_size, .. } => write!(
                f,
                "vocab_size exceeds maximum concrete WFST states ({})",
                max_vocab_size
            ),
            Self::ArcCountOverflow { .. } => write!(f, "{ARC_COUNT_OVERFLOW}"),
        }
    }
}

impl std::error::Error for CtcTopologyError {}

/// Information about a CTC topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtcTopologyInfo {
    /// Number of states in the topology.
    pub num_states: usize,
    /// Number of arcs (transitions) in the topology.
    pub num_arcs: usize,
    /// Number of vocabulary units (including blank).
    pub vocab_size: usize,
    /// Whether this topology guarantees no CTC repeat-absorbing non-blank self-loops.
    ///
    /// This is a topology-shape guarantee, not a claim that the graph has no
    /// structural self-loops at all. Arbitrary mutation through [`CtcTopology::fst_mut`]
    /// clears the guarantee conservatively.
    pub selfless: bool,
}

/// A CTC topology represented as a WFST.
///
/// The WFST maps input labels (frame emissions) to output labels (token sequence).
/// Blank tokens (label 0) map to epsilon on output, allowing multiple frames
/// to emit "nothing" or to separate repeated tokens.
#[derive(Clone, Debug)]
pub struct CtcTopology<W: Semiring> {
    /// The underlying WFST.
    fst: VectorWfst<CtcLabel, W>,
    /// Topology information.
    info: CtcTopologyInfo,
}

impl<W: Semiring> CtcTopology<W> {
    /// Get the underlying WFST.
    #[inline]
    pub fn fst(&self) -> &VectorWfst<CtcLabel, W> {
        &self.fst
    }

    /// Get mutable access to the underlying WFST.
    ///
    /// Arbitrary graph edits can invalidate topology-shape guarantees, so this
    /// conservatively clears the `selfless` guarantee. [`Self::info`] recomputes
    /// state and arc counts from the current WFST after mutation.
    #[inline]
    pub fn fst_mut(&mut self) -> &mut VectorWfst<CtcLabel, W> {
        self.info.selfless = false;
        &mut self.fst
    }

    /// Consume and return the underlying WFST.
    #[inline]
    pub fn into_fst(self) -> VectorWfst<CtcLabel, W> {
        self.fst
    }

    /// Get topology information.
    ///
    /// State and arc counts are derived from the current WFST, so metadata stays
    /// consistent after supported mutations through [`Self::fst_mut`].
    #[inline]
    pub fn info(&self) -> CtcTopologyInfo {
        let mut info = self.info;
        info.num_states = self.fst.num_states();
        info.num_arcs = self.fst.total_transitions();
        info
    }

    /// Get the vocabulary size (including blank).
    #[inline]
    pub fn vocab_size(&self) -> usize {
        self.info.vocab_size
    }
}

#[inline]
fn validate_vocab_size(vocab_size: usize) -> Result<(), CtcTopologyError> {
    if vocab_size < 1 {
        return Err(CtcTopologyError::EmptyVocabulary);
    }

    let max_vocab_size = max_vocab_size();
    if vocab_size > max_vocab_size {
        return Err(CtcTopologyError::VocabSizeExceedsStateSpace {
            vocab_size,
            max_vocab_size,
        });
    }

    Ok(())
}

#[inline]
fn max_vocab_size() -> usize {
    // NO_STATE is reserved as the absent-state sentinel, so concrete state ids
    // must stay in 0..NO_STATE.
    NO_STATE as usize
}

#[inline]
fn ctc_output(from: StateId, label: CtcLabel) -> Option<CtcLabel> {
    if label == BLANK || from == label {
        None
    } else {
        Some(label)
    }
}

fn initialized_vocab_fst<W: Semiring>(vocab_size: usize) -> VectorWfst<CtcLabel, W> {
    let mut fst = VectorWfst::with_capacity(vocab_size);

    for _ in 0..vocab_size {
        fst.add_state();
    }

    fst.set_start(BLANK);
    for state in 0..vocab_size as StateId {
        fst.set_final(state, W::one());
    }

    fst
}

#[inline]
fn correct_arc_count(vocab_size: usize) -> Result<usize, CtcTopologyError> {
    vocab_size
        .checked_mul(vocab_size)
        .ok_or(CtcTopologyError::ArcCountOverflow {
            topology: "Correct-CTC",
            vocab_size,
        })
}

#[inline]
fn compact_arc_count(vocab_size: usize) -> Result<usize, CtcTopologyError> {
    vocab_size
        .checked_mul(3)
        .and_then(|count| count.checked_sub(2))
        .ok_or(CtcTopologyError::ArcCountOverflow {
            topology: "Compact-CTC",
            vocab_size,
        })
}

#[inline]
fn selfless_correct_arc_count(vocab_size: usize) -> Result<usize, CtcTopologyError> {
    correct_arc_count(vocab_size)?
        .checked_sub(vocab_size - 1)
        .ok_or(CtcTopologyError::ArcCountOverflow {
            topology: "Selfless Correct-CTC",
            vocab_size,
        })
}

#[inline]
fn selfless_compact_arc_count(vocab_size: usize) -> Result<usize, CtcTopologyError> {
    vocab_size
        .checked_mul(2)
        .and_then(|count| count.checked_sub(1))
        .ok_or(CtcTopologyError::ArcCountOverflow {
            topology: "Selfless Compact-CTC",
            vocab_size,
        })
}

/// Create a Correct-CTC topology (standard CTC).
///
/// This is the original CTC topology with a complete directed graph.
/// Each state represents a label, and there are transitions from every
/// state to every other state (including self-loops).
///
/// # Structure
///
/// - **States**: N (one per vocabulary unit, including blank)
/// - **Arcs**: N² (complete graph with self-loops)
/// - **Start state**: State 0 (blank)
/// - **Final states**: All states are final
///
/// # Transitions
///
/// For each state s and label l:
/// - Non-blank self-loop: s --l:ε--> s (repeat frame, emit nothing)
/// - To other: s --l:l--> l (emit l and go to state l)
///
/// Blank (label 0) also emits epsilon on output. These epsilon outputs encode
/// the CTC collapse operator before downstream language-model composition.
///
/// # Parameters
///
/// - `vocab_size`: Number of vocabulary units including blank (N)
///
/// # Example
///
/// ```
/// use lling_llang::ctc::correct_ctc;
/// use lling_llang::semiring::LogWeight;
///
/// let ctc = correct_ctc::<LogWeight>(5);
/// assert_eq!(ctc.info().num_states, 5);
/// assert_eq!(ctc.info().num_arcs, 25); // 5²
/// ```
pub fn correct_ctc<W: Semiring>(vocab_size: usize) -> CtcTopology<W> {
    try_correct_ctc(vocab_size).unwrap_or_else(|err| panic!("{err}"))
}

/// Try to create a Correct-CTC topology without panicking.
///
/// Returns [`CtcTopologyError`] when `vocab_size` is empty, exceeds the
/// concrete WFST state-ID space, or its `N²` arc count cannot fit in `usize`.
pub fn try_correct_ctc<W: Semiring>(vocab_size: usize) -> Result<CtcTopology<W>, CtcTopologyError> {
    validate_vocab_size(vocab_size)?;

    let num_arcs = correct_arc_count(vocab_size)?;
    let mut fst = initialized_vocab_fst(vocab_size);

    // Pre-allocate transitions
    for s in 0..vocab_size as StateId {
        fst.reserve_transitions(s, vocab_size);
    }

    // Add transitions: complete graph with self-loops
    for from in 0..vocab_size as StateId {
        for label in 0..vocab_size as CtcLabel {
            let to = label as StateId;
            fst.add_arc(from, Some(label), ctc_output(from, label), to, W::one());
        }
    }

    Ok(CtcTopology {
        fst,
        info: CtcTopologyInfo {
            num_states: vocab_size,
            num_arcs,
            vocab_size,
            selfless: false,
        },
    })
}

/// Create a Compact-CTC topology.
///
/// This topology reduces graph size by using the blank state as a "back-off"
/// destination. Non-blank states can transition back to blank via epsilon,
/// then blank can transition to any label.
///
/// # Structure
///
/// - **States**: N (same as Correct-CTC)
/// - **Arcs**: 3N - 2
///   - N arcs from blank to each label
///   - N-1 arcs from each non-blank back to blank (epsilon)
///   - N-1 self-loops on non-blank states
/// - **Start state**: State 0 (blank)
/// - **Final states**: All states are final
///
/// # Benefits
///
/// - 1.5× smaller graph than Correct-CTC
/// - 2× memory reduction for LF-MMI training
/// - **No accuracy loss** compared to Correct-CTC
///
/// # Training Note
///
/// For training with Compact-CTC, use frame interleaving:
/// - Even frames: acoustic posteriors
/// - Odd frames: probability 1 for epsilon transition
///
/// # Parameters
///
/// - `vocab_size`: Number of vocabulary units including blank (N)
///
/// # Example
///
/// ```
/// use lling_llang::ctc::compact_ctc;
/// use lling_llang::semiring::LogWeight;
///
/// let ctc = compact_ctc::<LogWeight>(10);
/// assert_eq!(ctc.info().num_states, 10);
/// assert_eq!(ctc.info().num_arcs, 28); // 3*10 - 2
/// ```
pub fn compact_ctc<W: Semiring>(vocab_size: usize) -> CtcTopology<W> {
    try_compact_ctc(vocab_size).unwrap_or_else(|err| panic!("{err}"))
}

/// Try to create a Compact-CTC topology without panicking.
///
/// Returns [`CtcTopologyError`] when `vocab_size` is empty, exceeds the
/// concrete WFST state-ID space, or its `3N - 2` arc count cannot fit in
/// `usize`.
pub fn try_compact_ctc<W: Semiring>(vocab_size: usize) -> Result<CtcTopology<W>, CtcTopologyError> {
    validate_vocab_size(vocab_size)?;

    let num_arcs = compact_arc_count(vocab_size)?;
    let mut fst = initialized_vocab_fst(vocab_size);

    // Pre-allocate transitions
    fst.reserve_transitions(0, vocab_size); // Blank state has N outgoing arcs
    for s in 1..vocab_size as StateId {
        fst.reserve_transitions(s, 2); // Non-blank states have 2 arcs (self-loop + to blank)
    }

    // Blank state (0) can transition to any label
    for label in 0..vocab_size as CtcLabel {
        let to = label as StateId;
        fst.add_arc(0, Some(label), ctc_output(0, label), to, W::one());
    }

    // Non-blank states: self-loop + epsilon back to blank
    for s in 1..vocab_size as StateId {
        let label = s as CtcLabel;

        // Self-loop: stay on same label
        fst.add_arc(s, Some(label), ctc_output(s, label), s, W::one());

        // Epsilon back to blank (for transitioning to different label)
        fst.add_epsilon(s, 0, W::one());
    }

    Ok(CtcTopology {
        fst,
        info: CtcTopologyInfo {
            num_states: vocab_size,
            num_arcs,
            vocab_size,
            selfless: false,
        },
    })
}

/// Create a Minimal-CTC topology.
///
/// This is the smallest possible CTC topology with a single state.
/// It only allows blank-to-epsilon transduction, removing all self-loops
/// and direct transitions between non-blank labels.
///
/// # Structure
///
/// - **States**: 1
/// - **Arcs**: N (one per vocabulary unit)
/// - **Start state**: State 0
/// - **Final state**: State 0
///
/// # Characteristics
///
/// - Removes all non-blank self-loops
/// - Removes direct transitions between non-blank units
/// - Encourages "peaky" CTC behavior (blank-dominant)
///
/// # Benefits
///
/// - 2× smaller decoding graphs than Correct-CTC
/// - 4× memory reduction for LF-MMI training
/// - Usable for both training and decoding
///
/// # Trade-off
///
/// - Slight accuracy penalty (~0.2% WER increase)
/// - Best for memory-constrained scenarios
///
/// # Parameters
///
/// - `vocab_size`: Number of vocabulary units including blank (N)
///
/// # Example
///
/// ```
/// use lling_llang::ctc::minimal_ctc;
/// use lling_llang::semiring::LogWeight;
///
/// let ctc = minimal_ctc::<LogWeight>(100);
/// assert_eq!(ctc.info().num_states, 1);
/// assert_eq!(ctc.info().num_arcs, 100); // N
/// ```
pub fn minimal_ctc<W: Semiring>(vocab_size: usize) -> CtcTopology<W> {
    try_minimal_ctc(vocab_size).unwrap_or_else(|err| panic!("{err}"))
}

/// Try to create a Minimal-CTC topology without panicking.
///
/// Returns [`CtcTopologyError`] when `vocab_size` is empty or exceeds the
/// concrete WFST state-ID space.
pub fn try_minimal_ctc<W: Semiring>(vocab_size: usize) -> Result<CtcTopology<W>, CtcTopologyError> {
    validate_vocab_size(vocab_size)?;

    let mut fst = VectorWfst::with_capacity(1);

    // Single state
    let state = fst.add_state();
    fst.set_start(state);
    fst.set_final(state, W::one());

    // Pre-allocate transitions
    fst.reserve_transitions(state, vocab_size);

    // All labels loop back to the single state
    for label in 0..vocab_size as CtcLabel {
        fst.add_arc(
            state,
            Some(label),
            ctc_output(state, label),
            state,
            W::one(),
        );
    }

    Ok(CtcTopology {
        fst,
        info: CtcTopologyInfo {
            num_states: 1,
            num_arcs: vocab_size,
            vocab_size,
            selfless: true, // Minimal-CTC is inherently selfless
        },
    })
}

/// Create a Selfless Correct-CTC topology.
///
/// This is the Correct-CTC topology with non-blank self-loops removed.
/// Self-loops allow a label to be repeated on consecutive frames without
/// emitting a new token. Removing them forces the model to use blank
/// to separate repeated tokens.
///
/// # Benefits
///
/// - Better accuracy for wide context window models (Conformer, etc.)
/// - Slightly smaller graph (N-1 fewer arcs)
///
/// # When to Use
///
/// | Context Window | Recommended |
/// |----------------|-------------|
/// | Short (γ=0.25, ~11 frames) | Standard (with self-loops) |
/// | Long (γ=1.0) | Selfless |
/// | Unlimited (Conformer) | Selfless |
///
/// # Parameters
///
/// - `vocab_size`: Number of vocabulary units including blank (N)
///
/// # Example
///
/// ```
/// use lling_llang::ctc::{correct_ctc, selfless_correct_ctc};
/// use lling_llang::semiring::LogWeight;
///
/// let correct = correct_ctc::<LogWeight>(10);
/// let selfless = selfless_correct_ctc::<LogWeight>(10);
///
/// // Selfless has N-1 fewer arcs (no non-blank self-loops)
/// assert_eq!(correct.info().num_arcs - selfless.info().num_arcs, 9);
/// ```
pub fn selfless_correct_ctc<W: Semiring>(vocab_size: usize) -> CtcTopology<W> {
    try_selfless_correct_ctc(vocab_size).unwrap_or_else(|err| panic!("{err}"))
}

/// Try to create a Selfless Correct-CTC topology without panicking.
///
/// Returns [`CtcTopologyError`] when `vocab_size` is empty, exceeds the
/// concrete WFST state-ID space, or its `N² - N + 1` arc count cannot fit in
/// `usize`.
pub fn try_selfless_correct_ctc<W: Semiring>(
    vocab_size: usize,
) -> Result<CtcTopology<W>, CtcTopologyError> {
    validate_vocab_size(vocab_size)?;

    // N² - (N-1) = N² - N + 1: remove N-1 non-blank self-loops
    let num_arcs = selfless_correct_arc_count(vocab_size)?;
    let mut fst = initialized_vocab_fst(vocab_size);

    // Pre-allocate transitions
    for s in 0..vocab_size as StateId {
        // Blank state has all transitions, non-blank miss their self-loop
        let num_trans = if s == 0 { vocab_size } else { vocab_size - 1 };
        fst.reserve_transitions(s, num_trans);
    }

    // Add transitions: complete graph WITHOUT non-blank self-loops
    for from in 0..vocab_size as StateId {
        for label in 0..vocab_size as CtcLabel {
            let to = label as StateId;

            // Skip non-blank self-loops
            if from != 0 && from == to {
                continue;
            }

            fst.add_arc(from, Some(label), ctc_output(from, label), to, W::one());
        }
    }

    Ok(CtcTopology {
        fst,
        info: CtcTopologyInfo {
            num_states: vocab_size,
            num_arcs,
            vocab_size,
            selfless: true,
        },
    })
}

/// Create a Selfless Compact-CTC topology.
///
/// This is the Compact-CTC topology with non-blank self-loops removed.
/// The back-off structure to blank is preserved, but labels cannot repeat
/// on consecutive frames without going through blank.
///
/// # Benefits
///
/// - Better accuracy for wide context window models
/// - Smallest graph with back-off structure
///
/// # Parameters
///
/// - `vocab_size`: Number of vocabulary units including blank (N)
///
/// # Example
///
/// ```
/// use lling_llang::ctc::{compact_ctc, selfless_compact_ctc};
/// use lling_llang::semiring::LogWeight;
///
/// let compact = compact_ctc::<LogWeight>(10);
/// let selfless = selfless_compact_ctc::<LogWeight>(10);
///
/// // Selfless has N-1 fewer arcs (no non-blank self-loops)
/// assert_eq!(compact.info().num_arcs - selfless.info().num_arcs, 9);
/// ```
pub fn selfless_compact_ctc<W: Semiring>(vocab_size: usize) -> CtcTopology<W> {
    try_selfless_compact_ctc(vocab_size).unwrap_or_else(|err| panic!("{err}"))
}

/// Try to create a Selfless Compact-CTC topology without panicking.
///
/// Returns [`CtcTopologyError`] when `vocab_size` is empty, exceeds the
/// concrete WFST state-ID space, or its `2N - 1` arc count cannot fit in
/// `usize`.
pub fn try_selfless_compact_ctc<W: Semiring>(
    vocab_size: usize,
) -> Result<CtcTopology<W>, CtcTopologyError> {
    validate_vocab_size(vocab_size)?;

    // 3N - 2 - (N-1) = 2N - 1: remove N-1 non-blank self-loops
    let num_arcs = selfless_compact_arc_count(vocab_size)?;
    let mut fst = initialized_vocab_fst(vocab_size);

    // Pre-allocate transitions
    fst.reserve_transitions(0, vocab_size); // Blank state has N outgoing arcs
    for s in 1..vocab_size as StateId {
        fst.reserve_transitions(s, 1); // Non-blank states only have epsilon to blank
    }

    // Blank state (0) can transition to any label
    for label in 0..vocab_size as CtcLabel {
        let to = label as StateId;
        fst.add_arc(0, Some(label), ctc_output(0, label), to, W::one());
    }

    // Non-blank states: only epsilon back to blank (no self-loops)
    for s in 1..vocab_size as StateId {
        fst.add_epsilon(s, 0, W::one());
    }

    Ok(CtcTopology {
        fst,
        info: CtcTopologyInfo {
            num_states: vocab_size,
            num_arcs,
            vocab_size,
            selfless: true,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::{LogWeight, TropicalWeight};
    use crate::wfst::Wfst;

    #[test]
    fn test_correct_ctc_structure() {
        let ctc = correct_ctc::<LogWeight>(5);
        let fst = ctc.fst();

        // Check states
        assert_eq!(fst.num_states(), 5);
        assert_eq!(fst.start(), 0);

        // All states should be final
        for s in 0..5 {
            assert!(fst.is_final(s));
        }

        // Check arc count
        assert_eq!(fst.total_transitions(), 25);

        // Each state should have 5 outgoing arcs
        for s in 0..5 {
            assert_eq!(fst.transitions(s).len(), 5);
        }
    }

    #[test]
    fn test_correct_ctc_blank_epsilon() {
        let ctc = correct_ctc::<LogWeight>(3);
        let fst = ctc.fst();

        // From any state, blank (0) should output epsilon
        for s in 0..3 {
            let blank_arc = fst
                .transitions(s)
                .iter()
                .find(|t| t.input == Some(0))
                .expect("Should have blank arc");

            assert_eq!(blank_arc.output, None); // Epsilon output
            assert_eq!(blank_arc.to, 0); // Goes to blank state
        }
    }

    #[test]
    fn test_correct_ctc_repeated_label_outputs_epsilon() {
        let ctc = correct_ctc::<LogWeight>(4);
        let fst = ctc.fst();

        let repeat_arc = fst
            .transitions(2)
            .iter()
            .find(|t| t.input == Some(2) && t.to == 2)
            .expect("state 2 should have a repeat self-loop");
        assert_eq!(repeat_arc.output, None);

        let change_arc = fst
            .transitions(2)
            .iter()
            .find(|t| t.input == Some(3) && t.to == 3)
            .expect("state 2 should transition to label 3");
        assert_eq!(change_arc.output, Some(3));
    }

    #[test]
    fn test_compact_ctc_structure() {
        let ctc = compact_ctc::<LogWeight>(5);
        let fst = ctc.fst();

        assert_eq!(fst.num_states(), 5);
        assert_eq!(fst.total_transitions(), 13); // 3*5 - 2 = 13

        // Blank state (0) should have 5 outgoing arcs
        assert_eq!(fst.transitions(0).len(), 5);

        // Non-blank states should have 2 arcs each (self-loop + epsilon to blank)
        for s in 1..5 {
            assert_eq!(fst.transitions(s).len(), 2);
        }
    }

    #[test]
    fn test_compact_ctc_back_off() {
        let ctc = compact_ctc::<LogWeight>(4);
        let fst = ctc.fst();

        // Each non-blank state should have epsilon to blank
        for s in 1..4 {
            let eps_arc = fst
                .transitions(s)
                .iter()
                .find(|t| t.is_epsilon())
                .expect("Should have epsilon arc");

            assert_eq!(eps_arc.to, 0); // Goes to blank state
        }
    }

    #[test]
    fn test_compact_ctc_repeated_label_outputs_epsilon() {
        let ctc = compact_ctc::<LogWeight>(4);
        let fst = ctc.fst();

        let repeat_arc = fst
            .transitions(2)
            .iter()
            .find(|t| t.input == Some(2) && t.to == 2)
            .expect("state 2 should have a repeat self-loop");
        assert_eq!(repeat_arc.output, None);

        let entry_arc = fst
            .transitions(0)
            .iter()
            .find(|t| t.input == Some(2) && t.to == 2)
            .expect("blank state should enter label 2");
        assert_eq!(entry_arc.output, Some(2));
    }

    #[test]
    fn test_minimal_ctc_structure() {
        let ctc = minimal_ctc::<LogWeight>(10);
        let fst = ctc.fst();

        assert_eq!(fst.num_states(), 1);
        assert_eq!(fst.total_transitions(), 10);
        assert_eq!(fst.start(), 0);
        assert!(fst.is_final(0));

        // All arcs loop back to state 0
        for t in fst.transitions(0) {
            assert_eq!(t.to, 0);
        }
    }

    #[test]
    fn test_selfless_correct_ctc_no_self_loops() {
        let ctc = selfless_correct_ctc::<LogWeight>(4);
        let fst = ctc.fst();

        // Non-blank states should not have self-loops
        for s in 1..4 {
            for t in fst.transitions(s) {
                assert!(
                    t.to != s || t.input == Some(0),
                    "State {} should not have non-blank self-loop",
                    s
                );
            }
        }

        // Blank state (0) CAN have self-loop (blank:blank->0)
        let blank_self = fst
            .transitions(0)
            .iter()
            .find(|t| t.input == Some(0) && t.to == 0);
        assert!(blank_self.is_some());
    }

    #[test]
    fn test_selfless_compact_ctc_no_self_loops() {
        let ctc = selfless_compact_ctc::<LogWeight>(4);
        let fst = ctc.fst();

        // Non-blank states should only have epsilon to blank
        for s in 1..4 {
            assert_eq!(fst.transitions(s).len(), 1);
            let t = &fst.transitions(s)[0];
            assert!(t.is_epsilon());
            assert_eq!(t.to, 0);
        }
    }

    #[test]
    fn test_topology_arc_counts() {
        for n in [5, 10, 50, 100] {
            let correct = correct_ctc::<TropicalWeight>(n);
            let compact = compact_ctc::<TropicalWeight>(n);
            let minimal = minimal_ctc::<TropicalWeight>(n);
            let selfless_c = selfless_correct_ctc::<TropicalWeight>(n);
            let selfless_k = selfless_compact_ctc::<TropicalWeight>(n);

            assert_eq!(correct.info().num_arcs, n * n);
            assert_eq!(compact.info().num_arcs, 3 * n - 2);
            assert_eq!(minimal.info().num_arcs, n);
            assert_eq!(selfless_c.info().num_arcs, n * n - (n - 1));
            assert_eq!(selfless_k.info().num_arcs, 2 * n - 1);

            // Verify arc counts match actual
            assert_eq!(correct.fst().total_transitions(), correct.info().num_arcs);
            assert_eq!(compact.fst().total_transitions(), compact.info().num_arcs);
            assert_eq!(minimal.fst().total_transitions(), minimal.info().num_arcs);
            assert_eq!(
                selfless_c.fst().total_transitions(),
                selfless_c.info().num_arcs
            );
            assert_eq!(
                selfless_k.fst().total_transitions(),
                selfless_k.info().num_arcs
            );
        }
    }

    #[test]
    fn test_large_vocabulary() {
        // Test with realistic vocabulary sizes
        let correct = correct_ctc::<LogWeight>(1000);
        let compact = compact_ctc::<LogWeight>(1000);
        let minimal = minimal_ctc::<LogWeight>(1000);

        assert_eq!(correct.info().num_arcs, 1_000_000); // 1M arcs
        assert_eq!(compact.info().num_arcs, 2998); // ~3K arcs
        assert_eq!(minimal.info().num_arcs, 1000); // 1K arcs

        // Compact is ~334× smaller than correct
        assert!(correct.info().num_arcs / compact.info().num_arcs > 300);

        // Minimal is ~1000× smaller than correct
        assert_eq!(correct.info().num_arcs / minimal.info().num_arcs, 1000);
    }

    #[test]
    fn test_info_consistency() {
        let ctc = correct_ctc::<LogWeight>(10);
        let info = ctc.info();

        assert_eq!(info.num_states, ctc.fst().num_states());
        assert_eq!(info.num_arcs, ctc.fst().total_transitions());
        assert_eq!(info.vocab_size, 10);
        assert!(!info.selfless);

        let selfless = selfless_correct_ctc::<LogWeight>(10);
        assert!(selfless.info().selfless);
    }

    #[test]
    fn test_info_reflects_mutated_fst_counts() {
        let mut ctc = correct_ctc::<LogWeight>(3);
        let original = ctc.info();

        {
            let fst = ctc.fst_mut();
            let extra = fst.add_state();
            fst.set_final(extra, LogWeight::one());
            fst.add_arc(0, Some(1), Some(1), extra, LogWeight::one());
        }

        let info = ctc.info();
        assert_eq!(info.num_states, original.num_states + 1);
        assert_eq!(info.num_arcs, original.num_arcs + 1);
        assert_eq!(info.num_states, ctc.fst().num_states());
        assert_eq!(info.num_arcs, ctc.fst().total_transitions());
    }

    #[test]
    fn test_fst_mut_clears_selfless_guarantee() {
        let mut ctc = selfless_compact_ctc::<LogWeight>(3);
        assert!(ctc.info().selfless);

        ctc.fst_mut().add_arc(1, Some(1), None, 1, LogWeight::one());

        assert!(!ctc.info().selfless);
    }

    #[test]
    #[should_panic(expected = "vocab_size must be at least 1")]
    fn test_empty_vocabulary_panics() {
        let _ = correct_ctc::<LogWeight>(0);
    }

    #[test]
    fn test_try_topologies_reject_empty_vocabulary() {
        let results = [
            try_correct_ctc::<LogWeight>(0),
            try_compact_ctc::<LogWeight>(0),
            try_minimal_ctc::<LogWeight>(0),
            try_selfless_correct_ctc::<LogWeight>(0),
            try_selfless_compact_ctc::<LogWeight>(0),
        ];

        for result in results {
            assert!(matches!(result, Err(CtcTopologyError::EmptyVocabulary)));
        }
    }

    #[test]
    fn test_try_topologies_match_infallible_constructors() {
        assert_eq!(
            try_correct_ctc::<LogWeight>(4)
                .expect("valid topology")
                .info(),
            correct_ctc::<LogWeight>(4).info()
        );
        assert_eq!(
            try_compact_ctc::<LogWeight>(4)
                .expect("valid topology")
                .info(),
            compact_ctc::<LogWeight>(4).info()
        );
        assert_eq!(
            try_minimal_ctc::<LogWeight>(4)
                .expect("valid topology")
                .info(),
            minimal_ctc::<LogWeight>(4).info()
        );
        assert_eq!(
            try_selfless_correct_ctc::<LogWeight>(4)
                .expect("valid topology")
                .info(),
            selfless_correct_ctc::<LogWeight>(4).info()
        );
        assert_eq!(
            try_selfless_compact_ctc::<LogWeight>(4)
                .expect("valid topology")
                .info(),
            selfless_compact_ctc::<LogWeight>(4).info()
        );
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn test_try_vocabulary_above_state_id_space_returns_error() {
        let requested = max_vocab_size() + 1;
        let result = try_minimal_ctc::<LogWeight>(requested);

        match result {
            Err(CtcTopologyError::VocabSizeExceedsStateSpace {
                vocab_size,
                max_vocab_size: max,
            }) => {
                assert_eq!(vocab_size, requested);
                assert_eq!(max, max_vocab_size());
            }
            _ => panic!("expected state-space error"),
        }
    }

    #[test]
    #[cfg(target_pointer_width = "32")]
    fn test_try_correct_ctc_reports_arc_count_overflow_before_allocation() {
        let vocab_size = 65_536;
        let result = try_correct_ctc::<LogWeight>(vocab_size);

        match result {
            Err(CtcTopologyError::ArcCountOverflow {
                topology,
                vocab_size,
            }) => {
                assert_eq!(topology, "Correct-CTC");
                assert_eq!(vocab_size, 65_536);
            }
            _ => panic!("expected arc-count overflow"),
        }
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    #[should_panic(expected = "vocab_size exceeds maximum concrete WFST states")]
    fn test_vocabulary_above_state_id_space_panics_before_truncation() {
        let _ = minimal_ctc::<LogWeight>(max_vocab_size() + 1);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::semiring::{LogWeight, TropicalWeight};
    use crate::wfst::Wfst;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        // =====================================================================
        // Topology Size Properties
        // =====================================================================

        /// Correct-CTC has N states and N² arcs.
        #[test]
        fn correct_ctc_size(n in 1usize..50) {
            let ctc = correct_ctc::<LogWeight>(n);
            prop_assert_eq!(ctc.info().num_states, n);
            prop_assert_eq!(ctc.info().num_arcs, n * n);
            prop_assert_eq!(ctc.fst().num_states(), n);
            prop_assert_eq!(ctc.fst().total_transitions(), n * n);
        }

        /// Compact-CTC has N states and 3N-2 arcs.
        #[test]
        fn compact_ctc_size(n in 1usize..50) {
            let ctc = compact_ctc::<LogWeight>(n);
            prop_assert_eq!(ctc.info().num_states, n);
            prop_assert_eq!(ctc.info().num_arcs, 3 * n - 2);
            prop_assert_eq!(ctc.fst().num_states(), n);
            prop_assert_eq!(ctc.fst().total_transitions(), 3 * n - 2);
        }

        /// Minimal-CTC has 1 state and N arcs.
        #[test]
        fn minimal_ctc_size(n in 1usize..100) {
            let ctc = minimal_ctc::<LogWeight>(n);
            prop_assert_eq!(ctc.info().num_states, 1);
            prop_assert_eq!(ctc.info().num_arcs, n);
            prop_assert_eq!(ctc.fst().num_states(), 1);
            prop_assert_eq!(ctc.fst().total_transitions(), n);
        }

        /// Selfless Correct-CTC has N states and N²-N+1 arcs.
        #[test]
        fn selfless_correct_ctc_size(n in 1usize..50) {
            let ctc = selfless_correct_ctc::<LogWeight>(n);
            prop_assert_eq!(ctc.info().num_states, n);
            // N² - (N-1) = N² - N + 1
            let expected_arcs = n * n - (n - 1);
            prop_assert_eq!(ctc.info().num_arcs, expected_arcs);
            prop_assert_eq!(ctc.fst().total_transitions(), expected_arcs);
        }

        /// Selfless Compact-CTC has N states and 2N-1 arcs.
        #[test]
        fn selfless_compact_ctc_size(n in 1usize..50) {
            let ctc = selfless_compact_ctc::<LogWeight>(n);
            prop_assert_eq!(ctc.info().num_states, n);
            prop_assert_eq!(ctc.info().num_arcs, 2 * n - 1);
            prop_assert_eq!(ctc.fst().total_transitions(), 2 * n - 1);
        }

        // =====================================================================
        // Start and Final State Properties
        // =====================================================================

        /// Correct-CTC: start state is 0, all states are final.
        #[test]
        fn correct_ctc_start_final(n in 1usize..20) {
            let ctc = correct_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            prop_assert_eq!(fst.start(), 0);
            for s in 0..n as StateId {
                prop_assert!(fst.is_final(s), "State {} should be final", s);
            }
        }

        /// Compact-CTC: start state is 0, all states are final.
        #[test]
        fn compact_ctc_start_final(n in 1usize..20) {
            let ctc = compact_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            prop_assert_eq!(fst.start(), 0);
            for s in 0..n as StateId {
                prop_assert!(fst.is_final(s), "State {} should be final", s);
            }
        }

        /// Minimal-CTC: single state is both start and final.
        #[test]
        fn minimal_ctc_start_final(n in 1usize..100) {
            let ctc = minimal_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            prop_assert_eq!(fst.start(), 0);
            prop_assert!(fst.is_final(0));
        }

        // =====================================================================
        // Blank Token Properties
        // =====================================================================

        /// Correct-CTC emits epsilon for blanks and non-blank repeats.
        #[test]
        fn correct_ctc_ctc_collapse_outputs(n in 2usize..20) {
            let ctc = correct_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            for s in 0..n as StateId {
                for t in fst.transitions(s) {
                    if t.input == Some(BLANK) {
                        prop_assert_eq!(t.output, None, "Blank should output epsilon");
                    } else if t.input == Some(s as CtcLabel) {
                        prop_assert_eq!(t.output, None, "Repeat self-loop should output epsilon");
                    } else {
                        prop_assert_eq!(t.output, t.input, "Label change should output itself");
                    }
                }
            }
        }

        /// Blank (label 0) always outputs epsilon in Compact-CTC.
        #[test]
        fn compact_ctc_blank_epsilon(n in 2usize..20) {
            let ctc = compact_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            // Check blank state arcs
            for t in fst.transitions(0) {
                if t.input == Some(BLANK) {
                    prop_assert_eq!(t.output, None, "Blank should output epsilon");
                } else if t.input.is_some() {
                    prop_assert_eq!(t.output, t.input, "Non-blank should output itself");
                }
            }
        }

        /// Blank (label 0) always outputs epsilon in Minimal-CTC.
        #[test]
        fn minimal_ctc_blank_epsilon(n in 1usize..50) {
            let ctc = minimal_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            for t in fst.transitions(0) {
                if t.input == Some(BLANK) {
                    prop_assert_eq!(t.output, None, "Blank should output epsilon");
                } else {
                    prop_assert_eq!(t.output, t.input, "Non-blank should output itself");
                }
            }
        }

        // =====================================================================
        // Selfless Properties
        // =====================================================================

        /// Selfless Correct-CTC has no non-blank self-loops.
        #[test]
        fn selfless_correct_no_self_loops(n in 2usize..20) {
            let ctc = selfless_correct_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            // Non-blank states should not have self-loops
            for s in 1..n as StateId {
                for t in fst.transitions(s) {
                    if t.to == s {
                        // If there's a self-loop, it must be blank
                        prop_assert_eq!(t.input, Some(BLANK),
                            "State {} has non-blank self-loop with label {:?}", s, t.input);
                    }
                }
            }

            prop_assert!(ctc.info().selfless);
        }

        /// Selfless Compact-CTC has no non-blank self-loops.
        #[test]
        fn selfless_compact_no_self_loops(n in 2usize..20) {
            let ctc = selfless_compact_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            // Non-blank states should only have epsilon to blank
            for s in 1..n as StateId {
                for t in fst.transitions(s) {
                    prop_assert!(t.to != s || t.is_epsilon(),
                        "State {} has non-epsilon self-loop", s);
                }
            }

            prop_assert!(ctc.info().selfless);
        }

        /// Minimal-CTC is inherently selfless.
        #[test]
        fn minimal_ctc_selfless(n in 1usize..50) {
            let ctc = minimal_ctc::<LogWeight>(n);
            prop_assert!(ctc.info().selfless);
        }

        /// Non-selfless topologies are not marked as selfless.
        #[test]
        fn standard_topologies_not_selfless(n in 2usize..20) {
            let correct = correct_ctc::<LogWeight>(n);
            let compact = compact_ctc::<LogWeight>(n);

            prop_assert!(!correct.info().selfless);
            prop_assert!(!compact.info().selfless);
        }

        // =====================================================================
        // Selfless Arc Count Difference
        // =====================================================================

        /// Selfless Correct-CTC has exactly N-1 fewer arcs than Correct-CTC.
        #[test]
        fn selfless_correct_arc_difference(n in 2usize..50) {
            let correct = correct_ctc::<LogWeight>(n);
            let selfless = selfless_correct_ctc::<LogWeight>(n);

            let diff = correct.info().num_arcs - selfless.info().num_arcs;
            prop_assert_eq!(diff, n - 1, "Should have N-1 fewer arcs");
        }

        /// Selfless Compact-CTC has exactly N-1 fewer arcs than Compact-CTC.
        #[test]
        fn selfless_compact_arc_difference(n in 2usize..50) {
            let compact = compact_ctc::<LogWeight>(n);
            let selfless = selfless_compact_ctc::<LogWeight>(n);

            let diff = compact.info().num_arcs - selfless.info().num_arcs;
            prop_assert_eq!(diff, n - 1, "Should have N-1 fewer arcs");
        }

        // =====================================================================
        // Compact-CTC Back-off Structure
        // =====================================================================

        /// Compact-CTC: non-blank states have epsilon back-off to blank.
        #[test]
        fn compact_ctc_back_off(n in 2usize..20) {
            let ctc = compact_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            for s in 1..n as StateId {
                let has_eps_to_blank = fst.transitions(s)
                    .iter()
                    .any(|t| t.is_epsilon() && t.to == 0);
                prop_assert!(has_eps_to_blank,
                    "State {} should have epsilon transition to blank", s);
            }
        }

        /// Selfless Compact-CTC: non-blank states have ONLY epsilon back-off.
        #[test]
        fn selfless_compact_ctc_only_back_off(n in 2usize..20) {
            let ctc = selfless_compact_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            for s in 1..n as StateId {
                prop_assert_eq!(fst.transitions(s).len(), 1,
                    "Non-blank state {} should have exactly 1 transition", s);

                let t = &fst.transitions(s)[0];
                prop_assert!(t.is_epsilon(), "Should be epsilon transition");
                prop_assert_eq!(t.to, 0, "Should go to blank state");
            }
        }

        // =====================================================================
        // Complete Graph Properties (Correct-CTC)
        // =====================================================================

        /// Correct-CTC: every state can reach every other state in one step.
        #[test]
        fn correct_ctc_complete_graph(n in 2usize..15) {
            let ctc = correct_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            for from in 0..n as StateId {
                let destinations: std::collections::HashSet<_> = fst.transitions(from)
                    .iter()
                    .map(|t| t.to)
                    .collect();

                for to in 0..n as StateId {
                    prop_assert!(destinations.contains(&to),
                        "State {} should have transition to state {}", from, to);
                }
            }
        }

        /// Correct-CTC: each state has exactly N outgoing arcs.
        #[test]
        fn correct_ctc_outdegree(n in 1usize..20) {
            let ctc = correct_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            for s in 0..n as StateId {
                prop_assert_eq!(fst.transitions(s).len(), n,
                    "State {} should have {} transitions", s, n);
            }
        }

        // =====================================================================
        // Minimal-CTC Properties
        // =====================================================================

        /// Minimal-CTC: all transitions are self-loops to the single state.
        #[test]
        fn minimal_ctc_all_self_loops(n in 1usize..50) {
            let ctc = minimal_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            for t in fst.transitions(0) {
                prop_assert_eq!(t.to, 0, "All transitions should go to state 0");
            }
        }

        /// Minimal-CTC: has transition for every label.
        #[test]
        fn minimal_ctc_all_labels(n in 1usize..50) {
            let ctc = minimal_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            let labels: std::collections::HashSet<_> = fst.transitions(0)
                .iter()
                .filter_map(|t| t.input)
                .collect();

            for label in 0..n as CtcLabel {
                prop_assert!(labels.contains(&label), "Should have arc for label {}", label);
            }
        }

        // =====================================================================
        // Vocabulary Size Properties
        // =====================================================================

        /// Vocab size accessor matches constructor parameter.
        #[test]
        fn vocab_size_matches(n in 1usize..100) {
            prop_assert_eq!(correct_ctc::<LogWeight>(n).vocab_size(), n);
            prop_assert_eq!(compact_ctc::<LogWeight>(n).vocab_size(), n);
            prop_assert_eq!(minimal_ctc::<LogWeight>(n).vocab_size(), n);
            prop_assert_eq!(selfless_correct_ctc::<LogWeight>(n).vocab_size(), n);
            prop_assert_eq!(selfless_compact_ctc::<LogWeight>(n).vocab_size(), n);
        }

        /// Info vocab_size matches topology's vocab_size method.
        #[test]
        fn info_vocab_size_consistent(n in 1usize..50) {
            let ctc = correct_ctc::<LogWeight>(n);
            prop_assert_eq!(ctc.info().vocab_size, ctc.vocab_size());
        }

        // =====================================================================
        // Weight Properties
        // =====================================================================

        /// All transitions have unit weight.
        #[test]
        fn all_unit_weights(n in 1usize..20) {
            let ctc = correct_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            for s in 0..n as StateId {
                for t in fst.transitions(s) {
                    prop_assert_eq!(t.weight, LogWeight::one(),
                        "Transition weight should be one");
                }
            }
        }

        /// All final weights are one.
        #[test]
        fn all_final_weights_one(n in 1usize..20) {
            let ctc = correct_ctc::<LogWeight>(n);
            let fst = ctc.fst();

            for s in 0..n as StateId {
                if fst.is_final(s) {
                    let w = fst.final_weight(s);
                    prop_assert_eq!(w, LogWeight::one(),
                        "Final weight should be one");
                }
            }
        }

        // =====================================================================
        // Size Comparison Properties
        // =====================================================================

        /// Minimal-CTC is smaller than Compact-CTC for n > 1.
        #[test]
        fn minimal_smaller_than_compact(n in 2usize..50) {
            let compact = compact_ctc::<LogWeight>(n);
            let minimal = minimal_ctc::<LogWeight>(n);

            prop_assert!(minimal.info().num_arcs < compact.info().num_arcs,
                "Minimal ({}) should be smaller than Compact ({})",
                minimal.info().num_arcs, compact.info().num_arcs);
        }

        /// Compact-CTC is smaller than Correct-CTC for n > 2.
        #[test]
        fn compact_smaller_than_correct(n in 3usize..50) {
            let correct = correct_ctc::<LogWeight>(n);
            let compact = compact_ctc::<LogWeight>(n);

            prop_assert!(compact.info().num_arcs < correct.info().num_arcs,
                "Compact ({}) should be smaller than Correct ({})",
                compact.info().num_arcs, correct.info().num_arcs);
        }

        /// Size ordering: minimal < selfless_compact < compact < selfless_correct < correct.
        #[test]
        fn topology_size_ordering(n in 4usize..30) {
            let correct = correct_ctc::<LogWeight>(n);
            let selfless_c = selfless_correct_ctc::<LogWeight>(n);
            let compact = compact_ctc::<LogWeight>(n);
            let selfless_k = selfless_compact_ctc::<LogWeight>(n);
            let minimal = minimal_ctc::<LogWeight>(n);

            prop_assert!(minimal.info().num_arcs < selfless_k.info().num_arcs);
            prop_assert!(selfless_k.info().num_arcs < compact.info().num_arcs);
            prop_assert!(compact.info().num_arcs < selfless_c.info().num_arcs);
            prop_assert!(selfless_c.info().num_arcs < correct.info().num_arcs);
        }

        // =====================================================================
        // Generic Over Semiring Properties
        // =====================================================================

        /// Topologies work with TropicalWeight.
        #[test]
        fn works_with_tropical(n in 1usize..20) {
            let ctc = correct_ctc::<TropicalWeight>(n);
            prop_assert_eq!(ctc.info().num_states, n);
            prop_assert_eq!(ctc.fst().num_states(), n);
        }

        /// into_fst preserves the FST structure.
        #[test]
        fn into_fst_preserves_structure(n in 1usize..20) {
            let ctc = correct_ctc::<LogWeight>(n);
            let info = ctc.info();
            let fst = ctc.into_fst();

            prop_assert_eq!(fst.num_states(), info.num_states);
            prop_assert_eq!(fst.total_transitions(), info.num_arcs);
        }

        /// fst_mut allows modification.
        #[test]
        fn fst_mut_allows_modification(n in 2usize..10) {
            let mut ctc = correct_ctc::<LogWeight>(n);
            let original_arcs = ctc.fst().total_transitions();

            // Add an extra arc
            ctc.fst_mut().add_arc(0, Some(0), None, 0, LogWeight::new(1.0));

            prop_assert_eq!(ctc.fst().total_transitions(), original_arcs + 1);
        }
    }
}
