//! `SymbolicConstrainedDecoder` — behaviorally/structurally-constrained decoding
//! driven by a `lling_llang::symbolic` SFA (Task #22 §4-C.1).
//!
//! **The materialize-φ mandate.** To drive a token mask from a symbolic state you must
//! *materialize* the guard predicate `φ` into a [`TokenMask`]. This decoder does so at
//! **build time**: for every vocabulary token whose domain element satisfies a state's
//! outgoing transition guard, the token is set in that state's mask and recorded in the
//! state's δ table. At **decode time** the mask is an O(1) table lookup — never a live
//! SAT/SMT call in the hot loop. It slots beside [`super::WfstConstraint`] /
//! [`super::CompressedFsmConstraint`] and composes with [`super::ConstrainedBeamSearch`].
//!
//! Because the precomputed tables are independent of the algebra `A`, the decoder type
//! itself is non-generic — `build` accepts an SFA over *any* effective Boolean algebra
//! (interval, char-class, SMT `TheoryAlgebra<Z3Theory>`, …) and a vocabulary mapping
//! `TokenId → A::Domain`.

use std::collections::HashMap;

use crate::symbolic::{BooleanAlgebra, SymbolicAutomaton};
use crate::wfst::StateId;

use super::{ConstrainedDecoder, DecoderState, TokenId, TokenMask};

/// A constrained decoder whose admissible tokens at each automaton state are the
/// materialized minterm of that state's symbolic guards.
pub struct SymbolicConstrainedDecoder {
    /// Per-state precomputed mask of admissible tokens.
    state_masks: Vec<TokenMask>,
    /// Per-state transition table `token → next state` (first matching guard wins,
    /// making the materialized automaton deterministic on the vocabulary).
    deltas: Vec<HashMap<TokenId, StateId>>,
    /// The (single) initial state.
    initial: StateId,
    /// Per-state accepting flag.
    accepting: Vec<bool>,
    /// Vocabulary size (for empty-mask fallback).
    vocab_size: usize,
}

impl SymbolicConstrainedDecoder {
    /// Build the decoder by materializing `sfa`'s guards against `vocab` (the mapping
    /// `TokenId → A::Domain`, indexed by token id). O(states · |vocab|) once; O(1) per
    /// decode step thereafter.
    pub fn build<A: BooleanAlgebra>(sfa: &SymbolicAutomaton<A>, vocab: &[A::Domain]) -> Self {
        let n = sfa.states.len();
        let vocab_size = vocab.len();
        let mut state_masks = vec![TokenMask::new(vocab_size); n];
        let mut deltas: Vec<HashMap<TokenId, StateId>> = vec![HashMap::new(); n];

        for t in &sfa.transitions {
            if t.from >= n {
                continue;
            }
            for (tok, elem) in vocab.iter().enumerate() {
                if sfa.algebra.evaluate(&t.guard, elem) {
                    let tid = tok as TokenId;
                    state_masks[t.from].set(tid);
                    deltas[t.from].entry(tid).or_insert(t.to as StateId);
                }
            }
        }

        let initial = sfa.initial_states.iter().copied().min().unwrap_or(0) as StateId;
        let accepting = (0..n).map(|i| sfa.accepting_states.contains(&i)).collect();
        SymbolicConstrainedDecoder {
            state_masks,
            deltas,
            initial,
            accepting,
            vocab_size,
        }
    }
}

impl ConstrainedDecoder for SymbolicConstrainedDecoder {
    fn valid_tokens(&self, state: &DecoderState) -> TokenMask {
        self.state_masks
            .get(state.automaton_state as usize)
            .cloned()
            .unwrap_or_else(|| TokenMask::new(self.vocab_size))
    }

    fn advance(&self, state: &DecoderState, token: TokenId) -> Option<DecoderState> {
        self.deltas
            .get(state.automaton_state as usize)
            .and_then(|d| d.get(&token))
            .map(|&next| DecoderState {
                automaton_state: next,
                stack: state.stack.clone(),
            })
    }

    fn is_accepting(&self, state: &DecoderState) -> bool {
        self.accepting
            .get(state.automaton_state as usize)
            .copied()
            .unwrap_or(false)
    }

    fn initial_state(&self) -> DecoderState {
        DecoderState {
            automaton_state: self.initial,
            stack: Vec::new(),
        }
    }

    fn vocab_size(&self) -> usize {
        self.vocab_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::ConstrainedDecoder;
    use crate::symbolic::{IntervalAlgebra, IntervalPred};

    #[test]
    fn materializes_only_tokens_satisfying_the_guard() {
        // SFA: s0 --[10,20)--> s1(accepting). vocab token id t ↦ value t.
        let mut sfa = SymbolicAutomaton::new(IntervalAlgebra::new(0, 100));
        let s0 = sfa.add_state(false, None);
        let s1 = sfa.add_state(true, None);
        sfa.set_initial(s0);
        sfa.add_transition(s0, s1, IntervalPred::Range(10, 20));

        let vocab: Vec<i64> = (0..30).collect();
        let dec = SymbolicConstrainedDecoder::build(&sfa, &vocab);

        let st = dec.initial_state();
        let mask = dec.valid_tokens(&st);
        assert!(mask.is_valid(15), "token 15 ∈ [10,20) must be admissible");
        assert!(!mask.is_valid(5), "token 5 ∉ [10,20)");
        assert!(!mask.is_valid(25), "token 25 ∉ [10,20)");

        let next = dec.advance(&st, 15).expect("token 15 advances the automaton");
        assert!(dec.is_accepting(&next));
        assert!(dec.advance(&st, 5).is_none(), "a masked-out token cannot advance");
    }
}
