//! Pre-built test WFSTs and lattices for common test scenarios.
//!
//! This module provides factory functions for creating commonly used
//! test fixtures. These are useful for unit tests that need predictable
//! WFST structures.

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst};

// =============================================================================
// Basic WFST Fixtures
// =============================================================================

/// Create a single-state WFST (accepts empty string only).
///
/// ```text
/// (0) ═══ [final]
/// ```
pub fn single_state_wfst<L, W>() -> VectorWfst<L, W>
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    let mut fst = VectorWfst::new();
    let s0 = fst.add_state();
    fst.set_start(s0);
    fst.set_final(s0, W::one());
    fst
}

/// Create a linear WFST with n states.
///
/// ```text
/// (0) ──a:a──► (1) ──b:b──► (2) ... ──z:z──► (n-1) [final]
/// ```
///
/// Labels cycle through 'a' to 'z'.
pub fn linear_wfst<W>(num_states: usize) -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::new();

    for _ in 0..num_states {
        fst.add_state();
    }

    if num_states > 0 {
        fst.set_start(0);
        fst.set_final((num_states - 1) as StateId, W::one());

        for i in 0..(num_states - 1) {
            let label = char::from(b'a' + (i % 26) as u8);
            fst.add_arc(
                i as StateId,
                Some(label),
                Some(label),
                (i + 1) as StateId,
                W::one(),
            );
        }
    }

    fst
}

/// Create a linear WFST with custom labels and weights.
pub fn linear_wfst_custom<L, W>(labels: &[(L, W)]) -> VectorWfst<L, W>
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    let num_states = labels.len() + 1;
    let mut fst = VectorWfst::new();

    for _ in 0..num_states {
        fst.add_state();
    }

    if num_states > 0 {
        fst.set_start(0);
        fst.set_final((num_states - 1) as StateId, W::one());

        for (i, (label, weight)) in labels.iter().enumerate() {
            fst.add_arc(
                i as StateId,
                Some(label.clone()),
                Some(label.clone()),
                (i + 1) as StateId,
                *weight,
            );
        }
    }

    fst
}

/// Create a branching WFST with multiple parallel paths.
///
/// ```text
///              ┌───a:a───► (1)
/// (0) [start] ─┼───b:b───► (2) ──► (4) [final]
///              └───c:c───► (3)
/// ```
///
/// Creates `num_branches` parallel paths from start to end.
pub fn branching_wfst<W>(num_branches: usize) -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::new();

    // Start state
    let start = fst.add_state();
    fst.set_start(start);

    // Branch states
    let mut branch_states = Vec::with_capacity(num_branches);
    for _ in 0..num_branches {
        branch_states.push(fst.add_state());
    }

    // End state
    let end = fst.add_state();
    fst.set_final(end, W::one());

    // Connect start to branches
    for (i, &branch) in branch_states.iter().enumerate() {
        let label = char::from(b'a' + (i % 26) as u8);
        fst.add_arc(start, Some(label), Some(label), branch, W::one());
    }

    // Connect branches to end
    for &branch in &branch_states {
        fst.add_epsilon(branch, end, W::one());
    }

    fst
}

/// Create a diamond-shaped WFST.
///
/// ```text
///              ┌───a:a───► (1) ───┐
/// (0) [start] ─┤                   ├──► (3) [final]
///              └───b:b───► (2) ───┘
/// ```
pub fn diamond_wfst<W>() -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    let s3 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s3, W::one());

    fst.add_arc(s0, Some('a'), Some('a'), s1, W::one());
    fst.add_arc(s0, Some('b'), Some('b'), s2, W::one());
    fst.add_arc(s1, Some('c'), Some('c'), s3, W::one());
    fst.add_arc(s2, Some('d'), Some('d'), s3, W::one());

    fst
}

/// Create a diamond WFST with custom weights.
///
/// ```text
///              ┌──a:a(w1)──► (1) ──c:c(w3)──┐
/// (0) [start] ─┤                             ├──► (3) [final]
///              └──b:b(w2)──► (2) ──d:d(w4)──┘
/// ```
pub fn diamond_wfst_weighted<W>(w1: W, w2: W, w3: W, w4: W) -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    let s3 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s3, W::one());

    fst.add_arc(s0, Some('a'), Some('a'), s1, w1);
    fst.add_arc(s0, Some('b'), Some('b'), s2, w2);
    fst.add_arc(s1, Some('c'), Some('c'), s3, w3);
    fst.add_arc(s2, Some('d'), Some('d'), s3, w4);

    fst
}

/// Create a cyclic WFST with a self-loop.
///
/// ```text
/// (0) [start] ──a:a──► (1) ──┐
///                       ↑    │ b:b
///                       └────┘
///                       ↓
///                     (2) [final]
/// ```
pub fn cyclic_wfst<W>() -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s2, W::one());

    fst.add_arc(s0, Some('a'), Some('a'), s1, W::one());
    fst.add_arc(s1, Some('b'), Some('b'), s1, W::one()); // Self-loop
    fst.add_arc(s1, Some('c'), Some('c'), s2, W::one());

    fst
}

/// Create a WFST with epsilon transitions.
///
/// ```text
/// (0) [start] ──ε:ε──► (1) ──a:a──► (2) ──ε:ε──► (3) [final]
/// ```
pub fn epsilon_wfst<W>() -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    let s3 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s3, W::one());

    fst.add_epsilon(s0, s1, W::one());
    fst.add_arc(s1, Some('a'), Some('a'), s2, W::one());
    fst.add_epsilon(s2, s3, W::one());

    fst
}

/// Create a WFST with multiple epsilon transitions (epsilon-rich).
///
/// ```text
///              ┌────ε:ε────┐
/// (0) [start] ─┤           ├──► (2) ──a:a──► (3) [final]
///              └───ε:ε──► (1) ──ε:ε──┘
/// ```
pub fn epsilon_rich_wfst<W>() -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    let s3 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s3, W::one());

    fst.add_epsilon(s0, s1, W::one());
    fst.add_epsilon(s0, s2, W::one());
    fst.add_epsilon(s1, s2, W::one());
    fst.add_arc(s2, Some('a'), Some('a'), s3, W::one());

    fst
}

/// Create a non-deterministic WFST.
///
/// ```text
///              ┌───a:x───► (1) [final]
/// (0) [start] ─┤
///              └───a:y───► (2) [final]
/// ```
pub fn nondeterministic_wfst<W>() -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s1, W::one());
    fst.set_final(s2, W::one());

    // Same input label 'a' but different outputs
    fst.add_arc(s0, Some('a'), Some('x'), s1, W::one());
    fst.add_arc(s0, Some('a'), Some('y'), s2, W::one());

    fst
}

/// Create a transducer (different input/output labels).
///
/// ```text
/// (0) [start] ──a:x──► (1) ──b:y──► (2) [final]
/// ```
pub fn transducer<W>() -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s2, W::one());

    fst.add_arc(s0, Some('a'), Some('x'), s1, W::one());
    fst.add_arc(s1, Some('b'), Some('y'), s2, W::one());

    fst
}

/// Create a transducer with custom input/output mappings.
pub fn transducer_custom<L, W>(mappings: &[(L, L, W)]) -> VectorWfst<L, W>
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    let num_states = mappings.len() + 1;
    let mut fst = VectorWfst::new();

    for _ in 0..num_states {
        fst.add_state();
    }

    if num_states > 0 {
        fst.set_start(0);
        fst.set_final((num_states - 1) as StateId, W::one());

        for (i, (input, output, weight)) in mappings.iter().enumerate() {
            fst.add_arc(
                i as StateId,
                Some(input.clone()),
                Some(output.clone()),
                (i + 1) as StateId,
                *weight,
            );
        }
    }

    fst
}

/// Create an empty WFST (no states).
pub fn empty_wfst<L, W>() -> VectorWfst<L, W>
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    VectorWfst::new()
}

/// Create a WFST with multiple final states.
///
/// ```text
///              ┌───a:a───► (1) [final, w1]
/// (0) [start] ─┤
///              └───b:b───► (2) [final, w2]
/// ```
pub fn multi_final_wfst<W>(final_weights: &[W]) -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::new();

    let start = fst.add_state();
    fst.set_start(start);

    for (i, &weight) in final_weights.iter().enumerate() {
        let state = fst.add_state();
        let label = char::from(b'a' + (i % 26) as u8);
        fst.add_arc(start, Some(label), Some(label), state, W::one());
        fst.set_final(state, weight);
    }

    fst
}

/// Create a complete DFA over an alphabet.
///
/// Creates a DFA with `num_states` states where every state has a transition
/// for every label in the alphabet.
pub fn complete_dfa<W>(num_states: usize, alphabet: &[char]) -> VectorWfst<char, W>
where
    W: Semiring,
{
    let mut fst = VectorWfst::with_capacity(num_states);

    for _ in 0..num_states {
        fst.add_state();
    }

    if num_states > 0 {
        fst.set_start(0);
        fst.set_final((num_states - 1) as StateId, W::one());

        for from in 0..num_states {
            for (i, &label) in alphabet.iter().enumerate() {
                let to = (from + i + 1) % num_states;
                fst.add_arc(
                    from as StateId,
                    Some(label),
                    Some(label),
                    to as StateId,
                    W::one(),
                );
            }
        }
    }

    fst
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::test_utils::assertions::{has_no_epsilon, is_acyclic, is_deterministic};
    use crate::wfst::Wfst;

    #[test]
    fn test_single_state_wfst() {
        let fst: VectorWfst<char, TropicalWeight> = single_state_wfst();
        assert_eq!(fst.num_states(), 1);
        assert_eq!(fst.start(), 0);
        assert!(fst.is_final(0));
    }

    #[test]
    fn test_linear_wfst() {
        let fst: VectorWfst<char, TropicalWeight> = linear_wfst(5);
        assert_eq!(fst.num_states(), 5);
        assert_eq!(fst.start(), 0);
        assert!(fst.is_final(4));
        assert!(is_acyclic(&fst));
        assert!(is_deterministic(&fst));
    }

    #[test]
    fn test_diamond_wfst() {
        let fst: VectorWfst<char, TropicalWeight> = diamond_wfst();
        assert_eq!(fst.num_states(), 4);
        assert!(is_acyclic(&fst));
        assert!(has_no_epsilon(&fst));
    }

    #[test]
    fn test_cyclic_wfst() {
        let fst: VectorWfst<char, TropicalWeight> = cyclic_wfst();
        assert!(!is_acyclic(&fst));
    }

    #[test]
    fn test_epsilon_wfst() {
        let fst: VectorWfst<char, TropicalWeight> = epsilon_wfst();
        assert!(!has_no_epsilon(&fst));
    }

    #[test]
    fn test_nondeterministic_wfst() {
        let fst: VectorWfst<char, TropicalWeight> = nondeterministic_wfst();
        assert!(!is_deterministic(&fst));
    }

    #[test]
    fn test_transducer() {
        let fst: VectorWfst<char, TropicalWeight> = transducer();
        assert_eq!(fst.num_states(), 3);
        // Check that input and output labels differ
        let trans = fst.transitions(0);
        assert_eq!(trans.len(), 1);
        assert_ne!(trans[0].input, trans[0].output);
    }
}
