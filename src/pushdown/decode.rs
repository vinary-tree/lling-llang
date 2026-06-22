//! Incremental, configuration-driven decode surface over a weighted pushdown
//! automaton.
//!
//! Where [`VectorPda::accepts`](super::VectorPda::accepts) answers the
//! *whole-string* recognition question with a breadth-first search, this module
//! exposes the *incremental* analogue used for grammar-constrained generation:
//! given a live [`PdaConfiguration`], enumerate the terminals that may legally
//! follow, advance the configuration by one terminal, and test acceptance. This
//! is the pushdown (nested) counterpart of the flat-axis
//! [`crate::llm::WfstConstraint`] / [`crate::llm::ConstrainedDecoder`]: it
//! mirrors `valid_tokens` ([`PdaDecoder::legal_next`]), `advance`
//! ([`PdaDecoder::advance`]), `is_accepting` ([`PdaDecoder::is_accepting`]), and
//! the token-mask projection ([`PdaDecoder::legal_next_mask`]).
//!
//! # Epsilon closure
//!
//! A PDA may interpose ε-transitions — transitions with no input symbol that
//! merely reshape the stack and/or change state — between two terminal reads.
//! For example the `a^n b^n` grammar uses an ε-transition to switch from the
//! "reading `a`s" phase to the "reading `b`s" phase, and the balanced-bracket
//! grammar uses an ε-transition to return to its accepting state once the stack
//! is rebalanced. To surface the correct terminal frontier we must first chase
//! every ε-transition reachable from the current configuration. We compute the
//! **ε-closure** over `(state, stack)` pairs: the set of configurations
//! reachable by applying zero or more ε-transitions, visiting each `(state,
//! stack)` pair at most once.
//!
//! ## Termination: bounded even on ε-push-cycles
//!
//! The closure is bounded two ways. First, it deduplicates on the exact
//! `(state, stack)` pair, so a *self-returning* ε-cycle (one that leaves the
//! configuration unchanged) terminates. Second — because ε-transitions whose
//! net stack change is positive (a `Push` with no input) could otherwise grow
//! the stack without bound if they form a cycle — the closure refuses to
//! descend past a configurable **maximum stack depth**
//! ([`PdaDecoder::with_max_stack_depth`], default
//! [`PdaDecoder::DEFAULT_MAX_STACK_DEPTH`]): any ε-successor deeper than the
//! bound is simply not enqueued. Together these guarantee the closure is finite
//! for *every* automaton — including an adversarial ε-`Push` cycle — while
//! leaving every well-formed grammar (balanced brackets, `a^n b^n`, palindrome,
//! the tape-DSL), whose reachable ε-closure never approaches the bound,
//! completely unaffected.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};

use super::{PdaAcceptMode, PdaConfiguration, StackSymbol, VectorPda};
use crate::llm::{TokenId, TokenMask};
use crate::semiring::Semiring;

/// Incremental decode surface over a [`VectorPda`].
///
/// Borrows the automaton immutably; all decode state lives in the
/// caller-owned [`PdaConfiguration`] threaded through [`legal_next`],
/// [`advance`], and [`is_accepting`]. This keeps the decoder itself stateless
/// and trivially shareable, exactly as [`crate::llm::WfstConstraint`] keeps its
/// per-step state in [`crate::llm::DecoderState`].
///
/// [`legal_next`]: PdaDecoder::legal_next
/// [`advance`]: PdaDecoder::advance
/// [`is_accepting`]: PdaDecoder::is_accepting
#[derive(Debug, Clone, Copy)]
pub struct PdaDecoder<'a, L, W: Semiring> {
    /// The automaton whose configurations are being decoded.
    pda: &'a VectorPda<L, W>,
    /// Maximum stack depth the ε-closure will build before refusing to descend
    /// further (see the module-level "Termination" note). Guarantees the
    /// closure terminates even for an automaton with an ε-`Push` cycle.
    max_stack_depth: usize,
}

impl<'a, L: Clone + PartialEq, W: Semiring> PdaDecoder<'a, L, W> {
    /// Default ε-closure [`max_stack_depth`](Self::with_max_stack_depth) bound:
    /// deep enough that no well-formed DSL grammar approaches it, shallow enough
    /// that an adversarial ε-push-cycle is cut off promptly.
    pub const DEFAULT_MAX_STACK_DEPTH: usize = 4096;

    /// Wrap an automaton for incremental decoding, using the default
    /// [`DEFAULT_MAX_STACK_DEPTH`](Self::DEFAULT_MAX_STACK_DEPTH) ε-closure
    /// bound.
    pub fn new(pda: &'a VectorPda<L, W>) -> Self {
        Self { pda, max_stack_depth: Self::DEFAULT_MAX_STACK_DEPTH }
    }

    /// Wrap an automaton with an explicit ε-closure stack-depth bound. Use a
    /// smaller bound to cut off pathological ε-push-cycles sooner, or a larger
    /// one for grammars with genuinely deep nesting.
    pub fn with_max_stack_depth(pda: &'a VectorPda<L, W>, max_stack_depth: usize) -> Self {
        Self { pda, max_stack_depth }
    }

    /// The initial configuration: start state, empty remaining input (terminals
    /// are fed one at a time via [`advance`](Self::advance)), and the
    /// automaton's initial stack symbol on the bottom of the stack.
    pub fn initial_config(&self) -> PdaConfiguration<L> {
        PdaConfiguration::initial(self.pda.get_start(), Vec::new(), self.pda.get_initial_stack())
    }

    /// Enumerate the terminals that may legally be read next from `cfg`.
    ///
    /// The ε-closure of `cfg` is traversed first so that terminals reachable
    /// only after one or more stack-reshaping ε-transitions (e.g. the phase
    /// switch in `a^n b^n`) are included. Duplicate terminals are collapsed;
    /// the returned order follows first discovery during the closure walk.
    pub fn legal_next(&self, cfg: &PdaConfiguration<L>) -> Vec<L> {
        let closure = self.epsilon_closure(cfg);
        // Upper bound: at most one terminal per outgoing transition across the
        // whole closure. Preallocating avoids repeated growth. Deduplication is
        // a linear `contains` scan because the frozen surface bounds only
        // `L: Clone + PartialEq` (no `Eq + Hash`); the legal frontier is small.
        let capacity: usize = closure
            .iter()
            .map(|c| self.pda.get_transitions(c.state).len())
            .sum();
        let mut out: Vec<L> = Vec::with_capacity(capacity);
        for config in &closure {
            let Some(stack_top) = config.stack_top() else {
                continue;
            };
            for trans in self.pda.get_transitions(config.state) {
                if trans.stack_top != stack_top {
                    continue;
                }
                if let Some(label) = &trans.input {
                    if !out.contains(label) {
                        out.push(label.clone());
                    }
                }
            }
        }
        out
    }

    /// Advance `cfg` by reading the terminal `sym`, returning the resulting
    /// configuration, or `None` if `sym` is not a legal continuation.
    ///
    /// The ε-closure of `cfg` is searched for a consuming transition matching
    /// `sym` and the current stack top; the matching transition's stack action
    /// is then applied via [`PdaConfiguration::apply_transition`]. The returned
    /// configuration again carries empty remaining input, ready for the next
    /// call. If several ε-reachable configurations admit `sym`, the first found
    /// during the closure walk wins (the grammars consumed here are
    /// unambiguous at the terminal frontier).
    pub fn advance(&self, cfg: &PdaConfiguration<L>, sym: &L) -> Option<PdaConfiguration<L>> {
        for config in self.epsilon_closure(cfg) {
            let Some(stack_top) = config.stack_top() else {
                continue;
            };
            for trans in self.pda.get_transitions(config.state) {
                if trans.is_epsilon() || trans.stack_top != stack_top {
                    continue;
                }
                if trans.input.as_ref() != Some(sym) {
                    continue;
                }
                // Feed exactly `sym` as the remaining input so the shared
                // `apply_transition` consumes it and applies the stack action.
                let staged = PdaConfiguration::new(
                    config.state,
                    vec![sym.clone()],
                    config.stack.clone(),
                );
                if let Some(mut next) = staged.apply_transition(trans) {
                    // Leave the cursor with empty remaining input; the caller
                    // re-closes on the next step.
                    next.remaining_input.clear();
                    return Some(next);
                }
            }
        }
        None
    }

    /// Test whether `cfg` (or any configuration in its ε-closure) is accepting,
    /// honoring the automaton's [`PdaAcceptMode`].
    ///
    /// Because acceptance may require traversing a trailing ε-transition (e.g.
    /// `a^n b^n` reaches its final state via an ε-move once the stack is back
    /// to `Z₀`), the whole ε-closure is consulted: `cfg` is accepting if any
    /// ε-reachable configuration satisfies the accept mode.
    pub fn is_accepting(&self, cfg: &PdaConfiguration<L>) -> bool {
        let mode = self.pda.get_accept_mode();
        self.epsilon_closure(cfg)
            .iter()
            .any(|config| Self::config_accepts(self.pda, mode, config))
    }

    /// Decide acceptance of a single configuration under `mode`, ignoring
    /// remaining input (the incremental decoder never pre-loads input, so a
    /// configuration is "at end of input" by construction).
    fn config_accepts(
        pda: &VectorPda<L, W>,
        mode: PdaAcceptMode,
        config: &PdaConfiguration<L>,
    ) -> bool {
        match mode {
            PdaAcceptMode::FinalState => pda.get_is_final(config.state),
            PdaAcceptMode::EmptyStack => config.stack_empty(),
            PdaAcceptMode::Both => pda.get_is_final(config.state) || config.stack_empty(),
        }
    }

    /// Compute the ε-closure of `cfg`: every configuration reachable by zero or
    /// more ε-transitions, with each distinct `(state, stack)` pair visited
    /// once.
    ///
    /// Termination relies on the ε-acyclicity assumption documented at the
    /// module level: the `(state, stack)` dedup set bounds any closure whose
    /// reachable set is finite. The walk is a standard work-list BFS seeded
    /// with `cfg` itself, so the closure always contains `cfg`.
    fn epsilon_closure(&self, cfg: &PdaConfiguration<L>) -> Vec<PdaConfiguration<L>> {
        let mut visited: HashSet<(crate::wfst::StateId, Vec<StackSymbol>)> = HashSet::new();
        let mut closure: Vec<PdaConfiguration<L>> = Vec::new();
        let mut work: Vec<PdaConfiguration<L>> = Vec::new();

        let seed = PdaConfiguration::new(cfg.state, Vec::new(), cfg.stack.clone());
        visited.insert((seed.state, seed.stack.clone()));
        work.push(seed);

        while let Some(config) = work.pop() {
            let Some(stack_top) = config.stack_top() else {
                closure.push(config);
                continue;
            };
            // Expand ε-transitions out of this configuration.
            for trans in self.pda.get_transitions(config.state) {
                if !trans.is_epsilon() || trans.stack_top != stack_top {
                    continue;
                }
                // ε-transition: empty remaining input, apply the stack action.
                let staged =
                    PdaConfiguration::new(config.state, Vec::new(), config.stack.clone());
                if let Some(next) = staged.apply_transition(trans) {
                    // Bound the descent: an ε-`Push` cycle would otherwise build
                    // an unboundedly deep stack (every depth is a fresh, unseen
                    // `(state, stack)` pair, so the visited-set alone cannot stop
                    // it). Refusing to enqueue past `max_stack_depth` keeps the
                    // closure finite for every automaton.
                    if next.stack.len() > self.max_stack_depth {
                        continue;
                    }
                    let key = (next.state, next.stack.clone());
                    if visited.insert(key) {
                        work.push(next);
                    }
                }
            }
            closure.push(config);
        }
        closure
    }

    /// The ε-closure of `cfg` paired with the best (`⊕`-combined) weight of an
    /// ε-only path from `cfg` to each reachable configuration; the seed has
    /// weight [`Semiring::one`]. Bounded identically to
    /// [`epsilon_closure`](Self::epsilon_closure) (the same `max_stack_depth`),
    /// so it terminates even on ε-push-cycles. Backs the weighted surfaces
    /// [`legal_next_weighted`](Self::legal_next_weighted) and
    /// [`acceptance_weight`](Self::acceptance_weight); the unweighted
    /// [`legal_next`](Self::legal_next) / [`is_accepting`](Self::is_accepting)
    /// keep their own (cheaper, weight-free) closure.
    fn weighted_epsilon_closure(
        &self,
        cfg: &PdaConfiguration<L>,
    ) -> Vec<(PdaConfiguration<L>, W)> {
        let mut best: HashMap<(crate::wfst::StateId, Vec<StackSymbol>), W> = HashMap::new();
        let mut queue: VecDeque<PdaConfiguration<L>> = VecDeque::new();

        let seed = PdaConfiguration::new(cfg.state, Vec::new(), cfg.stack.clone());
        best.insert((seed.state, seed.stack.clone()), W::one());
        queue.push_back(seed);

        while let Some(config) = queue.pop_front() {
            let current = *best
                .get(&(config.state, config.stack.clone()))
                .expect("a queued configuration always has a recorded best weight");
            let Some(stack_top) = config.stack_top() else {
                continue;
            };
            for trans in self.pda.get_transitions(config.state) {
                if !trans.is_epsilon() || trans.stack_top != stack_top {
                    continue;
                }
                let staged =
                    PdaConfiguration::new(config.state, Vec::new(), config.stack.clone());
                let Some(next) = staged.apply_transition(trans) else {
                    continue;
                };
                if next.stack.len() > self.max_stack_depth {
                    continue;
                }
                let candidate = current.times(&trans.weight);
                match best.entry((next.state, next.stack.clone())) {
                    Entry::Vacant(slot) => {
                        slot.insert(candidate);
                        queue.push_back(next);
                    }
                    Entry::Occupied(mut slot) => {
                        // ⊕-relax: keep the best weight; re-enqueue only when it
                        // strictly improves so the fixpoint is reached finitely
                        // (⊕ is idempotent; tropical cost is bounded below).
                        let merged = slot.get().plus(&candidate);
                        if merged != *slot.get() {
                            slot.insert(merged);
                            queue.push_back(next);
                        }
                    }
                }
            }
        }

        best.into_iter()
            .map(|((state, stack), weight)| {
                (PdaConfiguration::new(state, Vec::new(), stack), weight)
            })
            .collect()
    }

    /// Like [`legal_next`](Self::legal_next), but pairs each legal terminal with
    /// the best (`⊕`) weight of reading it next: over every ε-path to an
    /// enabling configuration `⊗` that terminal transition's own weight,
    /// `⊕`-combined. This is the surface a *weighted* constrained decoder ranks
    /// continuations with; [`legal_next`](Self::legal_next) is exactly its
    /// terminal projection. Terminal order follows first discovery.
    pub fn legal_next_weighted(&self, cfg: &PdaConfiguration<L>) -> Vec<(L, W)> {
        let closure = self.weighted_epsilon_closure(cfg);
        let mut out: Vec<(L, W)> = Vec::new();
        for (config, path_weight) in &closure {
            let Some(stack_top) = config.stack_top() else {
                continue;
            };
            for trans in self.pda.get_transitions(config.state) {
                if trans.is_epsilon() || trans.stack_top != stack_top {
                    continue;
                }
                let Some(label) = &trans.input else {
                    continue;
                };
                let weight = path_weight.times(&trans.weight);
                if let Some(entry) = out.iter_mut().find(|entry| &entry.0 == label) {
                    entry.1 = entry.1.plus(&weight);
                } else {
                    out.push((label.clone(), weight));
                }
            }
        }
        out
    }

    /// The best (`⊕`) weight with which a complete, in-grammar string may end at
    /// `cfg`, or [`Semiring::zero`] if no ε-reachable configuration accepts.
    /// Each accepting ε-reachable configuration contributes its ε-path weight
    /// `⊗` the accepting weight implied by the automaton's [`PdaAcceptMode`]
    /// (the state's final weight for accepting-state modes, [`Semiring::one`]
    /// for empty-stack acceptance); the contributions are `⊕`-combined. This is
    /// the weighted counterpart of [`is_accepting`](Self::is_accepting).
    pub fn acceptance_weight(&self, cfg: &PdaConfiguration<L>) -> W {
        let mode = self.pda.get_accept_mode();
        let mut total = W::zero();
        let mut accepted = false;
        for (config, path_weight) in self.weighted_epsilon_closure(cfg) {
            if !Self::config_accepts(self.pda, mode, &config) {
                continue;
            }
            let final_weight = match mode {
                PdaAcceptMode::FinalState => self.pda.get_final_weight(config.state),
                PdaAcceptMode::EmptyStack => W::one(),
                PdaAcceptMode::Both => {
                    if self.pda.get_is_final(config.state) {
                        self.pda.get_final_weight(config.state)
                    } else {
                        W::one()
                    }
                }
            };
            let contribution = path_weight.times(&final_weight);
            total = if accepted { total.plus(&contribution) } else { contribution };
            accepted = true;
        }
        if accepted {
            total
        } else {
            W::zero()
        }
    }

    /// Whether *any* terminal continuation is legal from `cfg` (after ε-closure).
    /// `false` is a hard dead-end: the only valid move is to stop — which is
    /// in-grammar iff [`is_accepting`](Self::is_accepting) — otherwise the
    /// partial string cannot be completed within the grammar at all. Lets a
    /// decoder distinguish "must end here" from "stuck".
    pub fn has_legal_continuation(&self, cfg: &PdaConfiguration<L>) -> bool {
        self.epsilon_closure(cfg).iter().any(|config| {
            let Some(stack_top) = config.stack_top() else {
                return false;
            };
            self.pda.get_transitions(config.state).iter().any(|trans| {
                !trans.is_epsilon() && trans.stack_top == stack_top && trans.input.is_some()
            })
        })
    }
}

impl<L: Clone + PartialEq, W: Semiring> PdaDecoder<'_, L, W> {
    /// Project [`legal_next`](Self::legal_next) onto a [`TokenMask`] of width
    /// `vocab`, valid only when each terminal maps to a [`TokenId`].
    ///
    /// This is the pushdown analogue of
    /// [`ConstrainedDecoder::valid_tokens`](crate::llm::ConstrainedDecoder::valid_tokens):
    /// every legal-next terminal whose id is `< vocab` has its bit set; all
    /// other bits stay clear, masking every byte/token the grammar forbids.
    pub fn legal_next_mask(&self, cfg: &PdaConfiguration<L>, vocab: usize) -> TokenMask
    where
        L: Copy + Into<TokenId>,
    {
        let mut mask = TokenMask::new(vocab);
        for label in self.legal_next(cfg) {
            let id: TokenId = label.into();
            if (id as usize) < vocab {
                mask.set(id);
            }
        }
        mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pushdown::{PdaBuilder, StackAction};
    use crate::semiring::TropicalWeight;

    /// Balanced-bracket decoder over `char` terminals.
    fn brackets() -> VectorPda<char, TropicalWeight> {
        PdaBuilder::balanced_brackets('(', ')', TropicalWeight::one())
    }

    /// Balanced-bracket decoder over `u8` (byte) terminals, for the mask test.
    fn brackets_bytes() -> VectorPda<u8, TropicalWeight> {
        PdaBuilder::balanced_brackets(b'(', b')', TropicalWeight::one())
    }

    /// `{ a^n b^n | n >= 1 }` decoder over `char` terminals.
    fn a_n_b_n() -> VectorPda<char, TropicalWeight> {
        PdaBuilder::a_n_b_n('a', 'b', TropicalWeight::one())
    }

    #[test]
    fn legal_next_on_balanced_brackets() {
        let pda = brackets();
        let dec = PdaDecoder::new(&pda);

        // Initial: only '(' is legal; ')' is not; the empty string is accepting.
        let c0 = dec.initial_config();
        let next0 = dec.legal_next(&c0);
        assert!(next0.contains(&'('), "'(' must be legal initially");
        assert!(!next0.contains(&')'), "')' must not be legal initially");
        assert!(dec.is_accepting(&c0), "empty bracket string is accepting");

        // After '(': both '(' (nest deeper) and ')' (close) are legal.
        let c1 = dec
            .advance(&c0, &'(')
            .expect("'(' must advance from the initial config");
        let next1 = dec.legal_next(&c1);
        assert!(next1.contains(&'('), "'(' must be legal after '('");
        assert!(next1.contains(&')'), "')' must be legal after '('");

        // With one unmatched '(', the config is NOT accepting (non-empty stack /
        // non-final), but after closing it the config IS accepting again.
        assert!(!dec.is_accepting(&c1), "'(' alone is not balanced");
        let c2 = dec.advance(&c1, &')').expect("')' must close the '('");
        assert!(dec.is_accepting(&c2), "'()' is balanced and accepting");
    }

    #[test]
    fn advance_threads_stack() {
        let pda = brackets();
        let dec = PdaDecoder::new(&pda);

        // "(())" accepts: thread the stack push/pop through advance().
        let mut cfg = dec.initial_config();
        for sym in ['(', '(', ')', ')'] {
            cfg = dec
                .advance(&cfg, &sym)
                .unwrap_or_else(|| panic!("'{sym}' must advance while decoding \"(())\""));
        }
        assert!(dec.is_accepting(&cfg), "\"(())\" must accept");

        // "())" has no legal continuation at the 3rd symbol: after "()" the
        // stack is back to Z₀ and a further ')' has no transition.
        let c0 = dec.initial_config();
        let c1 = dec.advance(&c0, &'(').expect("'(' advances");
        let c2 = dec.advance(&c1, &')').expect("')' closes to balanced");
        assert!(
            dec.advance(&c2, &')').is_none(),
            "a 3rd ')' in \"())\" must have no legal continuation"
        );
        assert!(
            !dec.legal_next(&c2).contains(&')'),
            "')' must not be in the legal frontier of the balanced config"
        );
    }

    #[test]
    fn legal_next_mask_matches_legal_next() {
        let pda = brackets_bytes();
        let dec = PdaDecoder::new(&pda);

        // For a byte-labeled PDA, the set bits of the mask must equal the
        // Vec<u8> returned by legal_next, at every reachable configuration.
        let check = |cfg: &PdaConfiguration<u8>| {
            let legal = dec.legal_next(cfg);
            let mask = dec.legal_next_mask(cfg, 256);
            let from_mask: Vec<u8> = mask.iter_valid().map(|t| t as u8).collect();
            let mut legal_sorted = legal.clone();
            legal_sorted.sort_unstable();
            let mut mask_sorted = from_mask.clone();
            mask_sorted.sort_unstable();
            assert_eq!(
                legal_sorted, mask_sorted,
                "mask bits must equal legal_next bytes"
            );
            assert_eq!(mask.count_valid(), legal.len());
        };

        let c0 = dec.initial_config();
        check(&c0); // expects only b'('
        assert_eq!(dec.legal_next(&c0), vec![b'(']);

        let c1 = dec.advance(&c0, &b'(').expect("b'(' advances");
        check(&c1); // expects b'(' and b')'
    }

    #[test]
    fn epsilon_closure_traverses_push_pop() {
        // A grammar with an ε category-entry rule: the start state has NO
        // consuming transition of its own; it must take an ε-transition (which
        // pushes a category marker) before any inner terminal becomes legal.
        //
        //   start --ε, Z0 / push[Z0, C]--> inner
        //   inner --'x', C / Noop--> accept   (final)
        //
        // legal_next(initial) must surface 'x' purely through the ε-closure.
        let mut builder: PdaBuilder<char, TropicalWeight> = PdaBuilder::new();
        let start = builder.add_state();
        let inner = builder.add_state();
        let accept = builder.add_final_state(TropicalWeight::one());
        builder.set_start(start);

        let z0 = builder.initial_stack();
        let cat = builder.add_stack_symbol();

        // ε category-entry: push the category marker.
        builder.add_epsilon_transition(
            start,
            z0,
            inner,
            StackAction::Push(vec![z0, cat]),
            TropicalWeight::one(),
        );
        // Inner terminal 'x' on the category marker.
        builder.add_read_transition(inner, 'x', cat, accept, TropicalWeight::one());

        let pda = builder.build();
        let dec = PdaDecoder::new(&pda);

        let c0 = dec.initial_config();
        let next0 = dec.legal_next(&c0);
        assert!(
            next0.contains(&'x'),
            "inner terminal 'x' must surface through the ε category-entry rule"
        );
        // And we can actually advance over it to an accepting config.
        let c1 = dec.advance(&c0, &'x').expect("'x' must advance via ε-closure");
        assert!(dec.is_accepting(&c1), "reading 'x' must reach the final state");
    }

    #[test]
    fn nested_grammar_a_n_b_n() {
        let pda = a_n_b_n();
        let dec = PdaDecoder::new(&pda);

        // "aaabbb" accepts.
        let mut cfg = dec.initial_config();
        for sym in ['a', 'a', 'a', 'b', 'b', 'b'] {
            cfg = dec
                .advance(&cfg, &sym)
                .unwrap_or_else(|| panic!("'{sym}' must advance while decoding \"aaabbb\""));
        }
        assert!(dec.is_accepting(&cfg), "\"aaabbb\" must accept");

        // "aabbb" is rejected mid-stream: after "aabb" the stack is back to Z0,
        // the ε-move to the final state is available, and no further 'b' is
        // legal — so the 5th symbol 'b' has no legal continuation.
        let mut cfg = dec.initial_config();
        for sym in ['a', 'a', 'b', 'b'] {
            cfg = dec
                .advance(&cfg, &sym)
                .unwrap_or_else(|| panic!("'{sym}' must advance while decoding \"aabb...\""));
        }
        assert!(
            !dec.legal_next(&cfg).contains(&'b'),
            "no 5th 'b' may follow \"aabb\" (would be \"aabbb\")"
        );
        assert!(
            dec.advance(&cfg, &'b').is_none(),
            "\"aabbb\" must be rejected mid-stream by advance"
        );

        // "aaabb" is rejected: it is consumable symbol-by-symbol but the final
        // config is NOT accepting (one 'a' marker remains on the stack, more
        // 'b's are still required).
        let mut cfg = dec.initial_config();
        for sym in ['a', 'a', 'a', 'b', 'b'] {
            cfg = dec
                .advance(&cfg, &sym)
                .unwrap_or_else(|| panic!("'{sym}' must advance while decoding \"aaabb\""));
        }
        assert!(
            !dec.is_accepting(&cfg),
            "\"aaabb\" must not be accepting (a 'b' is still required)"
        );
        assert!(
            dec.legal_next(&cfg).contains(&'b'),
            "a further 'b' must still be legal after \"aaabb\""
        );
    }

    #[test]
    fn epsilon_push_cycle_terminates_via_depth_bound() {
        // An adversarial automaton whose only moves are ε-`Push` self-loops:
        // each fires without input and grows the stack by one, so every
        // reachable configuration is a fresh, deeper `(state, stack)` pair. The
        // visited-set alone cannot bound the closure — only `max_stack_depth`
        // can. Without the bound these queries would loop forever / OOM.
        let mut builder: PdaBuilder<char, TropicalWeight> = PdaBuilder::new();
        let q = builder.add_final_state(TropicalWeight::one());
        builder.set_start(q);
        let z0 = builder.initial_stack();
        let cat = builder.add_stack_symbol();
        // ε, Z₀ / push[Z₀, cat] → q   (…Z₀ ↦ …Z₀ cat)
        builder.add_epsilon_transition(
            q,
            z0,
            q,
            StackAction::Push(vec![z0, cat]),
            TropicalWeight::one(),
        );
        // ε, cat / push[cat, cat] → q  (…cat ↦ …cat cat — unbounded growth)
        builder.add_epsilon_transition(
            q,
            cat,
            q,
            StackAction::Push(vec![cat, cat]),
            TropicalWeight::one(),
        );
        let pda = builder.build();

        let dec = PdaDecoder::with_max_stack_depth(&pda, 8);
        let c0 = dec.initial_config();

        // The headline guarantee: every query MUST return (not hang) despite the
        // ε-push cycle.
        assert!(
            dec.legal_next(&c0).is_empty(),
            "a pure ε-push cycle has no terminal moves"
        );
        assert!(
            !dec.has_legal_continuation(&c0),
            "a pure ε-push cycle is a terminal dead-end"
        );
        assert!(dec.is_accepting(&c0), "q is final with the bottom stack");
        assert_eq!(
            dec.acceptance_weight(&c0),
            TropicalWeight::one(),
            "acceptance_weight must also terminate and report the accepting weight"
        );
    }

    #[test]
    fn acceptance_weight_tracks_completability() {
        let pda = brackets();
        let dec = PdaDecoder::new(&pda);
        let c0 = dec.initial_config();
        // The empty string is balanced/accepting: weight one (zero tropical cost).
        assert_eq!(dec.acceptance_weight(&c0), TropicalWeight::one());
        // After a single '(' the string is incomplete: no accepting config → zero.
        let c1 = dec.advance(&c0, &'(').expect("'(' advances");
        assert_eq!(dec.acceptance_weight(&c1), TropicalWeight::zero());
        // Closing it is accepting again.
        let c2 = dec.advance(&c1, &')').expect("')' closes");
        assert_eq!(dec.acceptance_weight(&c2), TropicalWeight::one());
    }

    #[test]
    fn legal_next_weighted_projects_to_legal_next() {
        let pda = brackets();
        let dec = PdaDecoder::new(&pda);
        let c0 = dec.initial_config();

        let weighted = dec.legal_next_weighted(&c0);
        // The terminal projection equals the unweighted frontier...
        let projected: Vec<char> = weighted.iter().map(|(l, _)| *l).collect();
        assert_eq!(projected, dec.legal_next(&c0));
        // ...and every weight is one() (all bracket transitions cost zero).
        assert!(weighted.iter().all(|(_, w)| *w == TropicalWeight::one()));

        // After '(' both '(' and ')' are legal, each with weight one().
        let c1 = dec.advance(&c0, &'(').expect("'(' advances");
        let weighted1 = dec.legal_next_weighted(&c1);
        let mut terms: Vec<char> = weighted1.iter().map(|(l, _)| *l).collect();
        terms.sort_unstable();
        assert_eq!(terms, vec!['(', ')']);
        assert!(weighted1.iter().all(|(_, w)| *w == TropicalWeight::one()));
    }

    #[test]
    fn has_legal_continuation_detects_dead_end() {
        let pda = a_n_b_n();
        let dec = PdaDecoder::new(&pda);
        // From the initial config 'a' is a legal continuation.
        let c0 = dec.initial_config();
        assert!(dec.has_legal_continuation(&c0));
        // After "ab" the string is complete and the stack is back to Z₀; the
        // only legal move is to stop — no further terminal continuation exists.
        let c1 = dec.advance(&c0, &'a').expect("'a' advances");
        let c2 = dec.advance(&c1, &'b').expect("'b' advances");
        assert!(dec.is_accepting(&c2), "\"ab\" is accepting");
        assert!(
            !dec.has_legal_continuation(&c2),
            "\"ab\" is a complete dead-end (the decoder must stop here)"
        );
    }
}
