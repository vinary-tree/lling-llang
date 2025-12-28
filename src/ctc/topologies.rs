//! CTC topology implementations.
//!
//! This module provides WFST implementations of various CTC topologies:
//! - Correct-CTC: Standard complete graph (N states, N² arcs)
//! - Compact-CTC: Reduced graph with blank back-off (N states, 3N-2 arcs)
//! - Minimal-CTC: Smallest possible graph (1 state, N arcs)
//! - Selfless variants: Remove non-blank self-loops for wide context models

use crate::semiring::Semiring;
use crate::wfst::{StateId, VectorWfst, MutableWfst};

/// CTC label type (vocabulary index).
///
/// Label 0 is reserved for the blank token.
pub type CtcLabel = u32;

/// The blank token index (always 0 in CTC).
pub const BLANK: CtcLabel = 0;

/// Information about a CTC topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtcTopologyInfo {
    /// Number of states in the topology.
    pub num_states: usize,
    /// Number of arcs (transitions) in the topology.
    pub num_arcs: usize,
    /// Number of vocabulary units (including blank).
    pub vocab_size: usize,
    /// Whether this is a selfless variant (no non-blank self-loops).
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
    #[inline]
    pub fn fst_mut(&mut self) -> &mut VectorWfst<CtcLabel, W> {
        &mut self.fst
    }

    /// Consume and return the underlying WFST.
    #[inline]
    pub fn into_fst(self) -> VectorWfst<CtcLabel, W> {
        self.fst
    }

    /// Get topology information.
    #[inline]
    pub fn info(&self) -> CtcTopologyInfo {
        self.info
    }

    /// Get the vocabulary size (including blank).
    #[inline]
    pub fn vocab_size(&self) -> usize {
        self.info.vocab_size
    }
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
/// - Self-loop: s --l:l--> s (emit l and stay)
/// - To other: s --l:l--> l (emit l and go to state l)
///
/// Note: Blank (label 0) emits epsilon on output.
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
    assert!(vocab_size >= 1, "vocab_size must be at least 1 (for blank)");

    let num_arcs = vocab_size * vocab_size;
    let mut fst = VectorWfst::with_capacity(vocab_size);

    // Add states (one per vocabulary unit)
    for _ in 0..vocab_size {
        fst.add_state();
    }

    // Set start state (blank state = 0)
    fst.set_start(0);

    // All states are final
    for s in 0..vocab_size as StateId {
        fst.set_final(s, W::one());
    }

    // Pre-allocate transitions
    for s in 0..vocab_size as StateId {
        fst.reserve_transitions(s, vocab_size);
    }

    // Add transitions: complete graph with self-loops
    for from in 0..vocab_size as StateId {
        for label in 0..vocab_size as CtcLabel {
            let to = label as StateId;
            // Blank (0) outputs epsilon, others output themselves
            let output = if label == BLANK { None } else { Some(label) };
            fst.add_arc(from, Some(label), output, to, W::one());
        }
    }

    CtcTopology {
        fst,
        info: CtcTopologyInfo {
            num_states: vocab_size,
            num_arcs,
            vocab_size,
            selfless: false,
        },
    }
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
    assert!(vocab_size >= 1, "vocab_size must be at least 1 (for blank)");

    let num_arcs = 3 * vocab_size - 2;
    let mut fst = VectorWfst::with_capacity(vocab_size);

    // Add states
    for _ in 0..vocab_size {
        fst.add_state();
    }

    // Set start state (blank = 0)
    fst.set_start(0);

    // All states are final
    for s in 0..vocab_size as StateId {
        fst.set_final(s, W::one());
    }

    // Pre-allocate transitions
    fst.reserve_transitions(0, vocab_size); // Blank state has N outgoing arcs
    for s in 1..vocab_size as StateId {
        fst.reserve_transitions(s, 2); // Non-blank states have 2 arcs (self-loop + to blank)
    }

    // Blank state (0) can transition to any label
    for label in 0..vocab_size as CtcLabel {
        let to = label as StateId;
        let output = if label == BLANK { None } else { Some(label) };
        fst.add_arc(0, Some(label), output, to, W::one());
    }

    // Non-blank states: self-loop + epsilon back to blank
    for s in 1..vocab_size as StateId {
        let label = s as CtcLabel;

        // Self-loop: stay on same label
        fst.add_arc(s, Some(label), Some(label), s, W::one());

        // Epsilon back to blank (for transitioning to different label)
        fst.add_epsilon(s, 0, W::one());
    }

    CtcTopology {
        fst,
        info: CtcTopologyInfo {
            num_states: vocab_size,
            num_arcs,
            vocab_size,
            selfless: false,
        },
    }
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
    assert!(vocab_size >= 1, "vocab_size must be at least 1 (for blank)");

    let mut fst = VectorWfst::with_capacity(1);

    // Single state
    let state = fst.add_state();
    fst.set_start(state);
    fst.set_final(state, W::one());

    // Pre-allocate transitions
    fst.reserve_transitions(state, vocab_size);

    // All labels loop back to the single state
    for label in 0..vocab_size as CtcLabel {
        let output = if label == BLANK { None } else { Some(label) };
        fst.add_arc(state, Some(label), output, state, W::one());
    }

    CtcTopology {
        fst,
        info: CtcTopologyInfo {
            num_states: 1,
            num_arcs: vocab_size,
            vocab_size,
            selfless: true, // Minimal-CTC is inherently selfless
        },
    }
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
    assert!(vocab_size >= 1, "vocab_size must be at least 1 (for blank)");

    // N² - (N-1) = N² - N + 1: remove N-1 non-blank self-loops
    let num_arcs = vocab_size * vocab_size - (vocab_size - 1);
    let mut fst = VectorWfst::with_capacity(vocab_size);

    // Add states
    for _ in 0..vocab_size {
        fst.add_state();
    }

    // Set start state (blank = 0)
    fst.set_start(0);

    // All states are final
    for s in 0..vocab_size as StateId {
        fst.set_final(s, W::one());
    }

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

            let output = if label == BLANK { None } else { Some(label) };
            fst.add_arc(from, Some(label), output, to, W::one());
        }
    }

    CtcTopology {
        fst,
        info: CtcTopologyInfo {
            num_states: vocab_size,
            num_arcs,
            vocab_size,
            selfless: true,
        },
    }
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
    assert!(vocab_size >= 1, "vocab_size must be at least 1 (for blank)");

    // 3N - 2 - (N-1) = 2N - 1: remove N-1 non-blank self-loops
    let num_arcs = 2 * vocab_size - 1;
    let mut fst = VectorWfst::with_capacity(vocab_size);

    // Add states
    for _ in 0..vocab_size {
        fst.add_state();
    }

    // Set start state (blank = 0)
    fst.set_start(0);

    // All states are final
    for s in 0..vocab_size as StateId {
        fst.set_final(s, W::one());
    }

    // Pre-allocate transitions
    fst.reserve_transitions(0, vocab_size); // Blank state has N outgoing arcs
    for s in 1..vocab_size as StateId {
        fst.reserve_transitions(s, 1); // Non-blank states only have epsilon to blank
    }

    // Blank state (0) can transition to any label
    for label in 0..vocab_size as CtcLabel {
        let to = label as StateId;
        let output = if label == BLANK { None } else { Some(label) };
        fst.add_arc(0, Some(label), output, to, W::one());
    }

    // Non-blank states: only epsilon back to blank (no self-loops)
    for s in 1..vocab_size as StateId {
        fst.add_epsilon(s, 0, W::one());
    }

    CtcTopology {
        fst,
        info: CtcTopologyInfo {
            num_states: vocab_size,
            num_arcs,
            vocab_size,
            selfless: true,
        },
    }
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
            let blank_arc = fst.transitions(s)
                .iter()
                .find(|t| t.input == Some(0))
                .expect("Should have blank arc");

            assert_eq!(blank_arc.output, None); // Epsilon output
            assert_eq!(blank_arc.to, 0); // Goes to blank state
        }
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
            let eps_arc = fst.transitions(s)
                .iter()
                .find(|t| t.is_epsilon())
                .expect("Should have epsilon arc");

            assert_eq!(eps_arc.to, 0); // Goes to blank state
        }
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
        let blank_self = fst.transitions(0)
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
            assert_eq!(selfless_c.fst().total_transitions(), selfless_c.info().num_arcs);
            assert_eq!(selfless_k.fst().total_transitions(), selfless_k.info().num_arcs);
        }
    }

    #[test]
    fn test_large_vocabulary() {
        // Test with realistic vocabulary sizes
        let correct = correct_ctc::<LogWeight>(1000);
        let compact = compact_ctc::<LogWeight>(1000);
        let minimal = minimal_ctc::<LogWeight>(1000);

        assert_eq!(correct.info().num_arcs, 1_000_000); // 1M arcs
        assert_eq!(compact.info().num_arcs, 2998);      // ~3K arcs
        assert_eq!(minimal.info().num_arcs, 1000);      // 1K arcs

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
    #[should_panic(expected = "vocab_size must be at least 1")]
    fn test_empty_vocabulary_panics() {
        let _ = correct_ctc::<LogWeight>(0);
    }
}
