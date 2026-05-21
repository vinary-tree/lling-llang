//! Builder for weighted pushdown automata.

use std::hash::Hash;

#[cfg(test)]
use super::WeightedPda;
use super::{PdaAcceptMode, PdaTransition, StackAction, StackSymbol, VectorPda};
use crate::semiring::Semiring;
use crate::wfst::StateId;

/// Builder for constructing weighted pushdown automata.
#[derive(Debug, Clone)]
pub struct PdaBuilder<L, W: Semiring> {
    /// The PDA being built.
    pda: VectorPda<L, W>,
    /// Next stack symbol ID.
    next_stack_symbol: u32,
}

impl<L: Clone + Eq + Hash, W: Semiring + Clone> PdaBuilder<L, W> {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            pda: VectorPda::new(),
            next_stack_symbol: 1, // 0 is reserved for BOTTOM
        }
    }

    /// Create a builder with a specific acceptance mode.
    pub fn with_accept_mode(mode: PdaAcceptMode) -> Self {
        Self {
            pda: VectorPda::with_accept_mode(mode),
            next_stack_symbol: 1,
        }
    }

    /// Add a state and return its ID.
    pub fn add_state(&mut self) -> StateId {
        self.pda.add_state()
    }

    /// Add a final state with the given weight.
    pub fn add_final_state(&mut self, weight: W) -> StateId {
        self.pda.add_final_state(weight)
    }

    /// Set the start state.
    pub fn set_start(&mut self, state: StateId) {
        self.pda.set_start(state);
    }

    /// Get the initial stack symbol (Z₀).
    pub fn initial_stack(&self) -> StackSymbol {
        self.pda.get_initial_stack()
    }

    /// Set a custom initial stack symbol.
    pub fn set_initial_stack(&mut self, symbol: StackSymbol) {
        self.pda.set_initial_stack(symbol);
    }

    /// Allocate a new stack symbol.
    pub fn add_stack_symbol(&mut self) -> StackSymbol {
        let id = self.next_stack_symbol;
        self.next_stack_symbol += 1;
        StackSymbol::new(id)
    }

    /// Make a state final.
    pub fn set_final(&mut self, state: StateId, weight: W) {
        self.pda.set_final(state, weight);
    }

    /// Add a transition.
    pub fn add_transition(
        &mut self,
        from: StateId,
        input: Option<L>,
        stack_top: StackSymbol,
        to: StateId,
        stack_action: StackAction,
        weight: W,
    ) -> &mut Self {
        self.pda.add_transition(PdaTransition::new(
            from,
            input,
            stack_top,
            stack_action,
            to,
            weight,
        ));
        self
    }

    /// Add an epsilon transition.
    pub fn add_epsilon_transition(
        &mut self,
        from: StateId,
        stack_top: StackSymbol,
        to: StateId,
        stack_action: StackAction,
        weight: W,
    ) -> &mut Self {
        self.pda
            .add_epsilon_transition(from, stack_top, stack_action, to, weight);
        self
    }

    /// Add a transition that only reads input (no stack change).
    pub fn add_read_transition(
        &mut self,
        from: StateId,
        input: L,
        stack_top: StackSymbol,
        to: StateId,
        weight: W,
    ) -> &mut Self {
        self.add_transition(from, Some(input), stack_top, to, StackAction::Noop, weight)
    }

    /// Add a transition that pushes a symbol.
    pub fn add_push_transition(
        &mut self,
        from: StateId,
        input: Option<L>,
        stack_top: StackSymbol,
        push_symbols: Vec<StackSymbol>,
        to: StateId,
        weight: W,
    ) -> &mut Self {
        self.add_transition(
            from,
            input,
            stack_top,
            to,
            StackAction::Push(push_symbols),
            weight,
        )
    }

    /// Add a transition that pops the stack.
    pub fn add_pop_transition(
        &mut self,
        from: StateId,
        input: Option<L>,
        stack_top: StackSymbol,
        to: StateId,
        weight: W,
    ) -> &mut Self {
        self.add_transition(from, input, stack_top, to, StackAction::Pop, weight)
    }

    /// Add a transition that replaces the stack top.
    pub fn add_replace_transition(
        &mut self,
        from: StateId,
        input: Option<L>,
        stack_top: StackSymbol,
        replace_symbols: Vec<StackSymbol>,
        to: StateId,
        weight: W,
    ) -> &mut Self {
        self.add_transition(
            from,
            input,
            stack_top,
            to,
            StackAction::Replace(replace_symbols),
            weight,
        )
    }

    /// Build the PDA.
    pub fn build(self) -> VectorPda<L, W> {
        self.pda
    }

    /// Get the number of states.
    pub fn num_states(&self) -> usize {
        self.pda.get_num_states()
    }

    /// Get the number of transitions.
    pub fn num_transitions(&self) -> usize {
        self.pda.get_num_transitions()
    }
}

impl<L: Clone + Eq + Hash, W: Semiring + Clone> Default for PdaBuilder<L, W> {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience methods for building common PDA patterns.
impl<L: Clone + Eq + Hash, W: Semiring + Clone> PdaBuilder<L, W> {
    /// Build a PDA for balanced brackets.
    ///
    /// Recognizes strings of balanced open/close pairs.
    /// Uses two states: s0 (final, balanced) and s1 (processing, unbalanced).
    pub fn balanced_brackets(open: L, close: L, weight_one: W) -> VectorPda<L, W> {
        let mut builder = Self::new();

        // s0 is the accepting state (only reachable when brackets are balanced)
        let s0 = builder.add_final_state(weight_one.clone());
        // s1 is the processing state (when we have unmatched open brackets)
        let s1 = builder.add_state();
        builder.set_start(s0);

        let z0 = builder.initial_stack();
        let bracket = builder.add_stack_symbol();

        // From s0 (balanced): on open bracket, push marker and go to s1
        builder.add_push_transition(
            s0,
            Some(open.clone()),
            z0,
            vec![z0, bracket],
            s1,
            weight_one.clone(),
        );

        // From s1 (unbalanced): on open bracket, push another marker
        builder.add_push_transition(
            s1,
            Some(open),
            bracket,
            vec![bracket, bracket],
            s1,
            weight_one.clone(),
        );

        // From s1: on close bracket, pop marker (stay in s1)
        builder.add_pop_transition(s1, Some(close), bracket, s1, weight_one.clone());

        // From s1: when z0 is revealed (all brackets matched), epsilon to s0
        builder.add_epsilon_transition(s1, z0, s0, StackAction::Noop, weight_one);

        builder.build()
    }

    /// Build a PDA for { a^n b^n | n >= 1 }.
    pub fn a_n_b_n(a: L, b: L, weight_one: W) -> VectorPda<L, W> {
        let mut builder = Self::new();

        let reading_as = builder.add_state();
        let reading_bs = builder.add_state();
        let accepting = builder.add_final_state(weight_one.clone());

        builder.set_start(reading_as);

        let z0 = builder.initial_stack();
        let marker = builder.add_stack_symbol();

        // Read first 'a', push marker
        builder.add_push_transition(
            reading_as,
            Some(a.clone()),
            z0,
            vec![z0, marker],
            reading_as,
            weight_one.clone(),
        );

        // Read more 'a's, push markers
        builder.add_push_transition(
            reading_as,
            Some(a),
            marker,
            vec![marker, marker],
            reading_as,
            weight_one.clone(),
        );

        // Switch to reading 'b's
        builder.add_epsilon_transition(
            reading_as,
            marker,
            reading_bs,
            StackAction::Noop,
            weight_one.clone(),
        );

        // Read 'b', pop marker
        builder.add_pop_transition(reading_bs, Some(b), marker, reading_bs, weight_one.clone());

        // Accept when stack has only z0
        builder.add_epsilon_transition(reading_bs, z0, accepting, StackAction::Noop, weight_one);

        builder.build()
    }

    /// Build a PDA for palindromes (with a center marker).
    ///
    /// Recognizes strings of the form w # w^R where w is over the alphabet
    /// and # is a center marker.
    pub fn palindrome_with_center(alphabet: &[L], center: L, weight_one: W) -> VectorPda<L, W>
    where
        L: Clone + Eq + Hash,
    {
        let mut builder = Self::new();

        let reading_first_half = builder.add_state();
        let reading_second_half = builder.add_state();
        let accepting = builder.add_final_state(weight_one.clone());

        builder.set_start(reading_first_half);

        let z0 = builder.initial_stack();

        // Create stack symbols for each alphabet symbol
        let mut symbol_map: std::collections::HashMap<L, StackSymbol> =
            std::collections::HashMap::new();
        for sym in alphabet {
            let stack_sym = builder.add_stack_symbol();
            symbol_map.insert(sym.clone(), stack_sym);
        }

        // First half: read symbols and push them (from z0)
        for sym in alphabet {
            let stack_sym = symbol_map[sym];
            builder.add_push_transition(
                reading_first_half,
                Some(sym.clone()),
                z0,
                vec![z0, stack_sym],
                reading_first_half,
                weight_one.clone(),
            );
        }

        // First half: read symbols and push them (from any stack symbol)
        for sym in alphabet {
            let stack_sym = symbol_map[sym];
            for other_sym in alphabet {
                let other_stack_sym = symbol_map[other_sym];
                builder.add_push_transition(
                    reading_first_half,
                    Some(sym.clone()),
                    other_stack_sym,
                    vec![other_stack_sym, stack_sym],
                    reading_first_half,
                    weight_one.clone(),
                );
            }
        }

        // Read center marker, transition to second half
        for &stack_sym in symbol_map.values() {
            builder.add_read_transition(
                reading_first_half,
                center.clone(),
                stack_sym,
                reading_second_half,
                weight_one.clone(),
            );
        }

        // Also allow center from z0 (empty first half)
        builder.add_read_transition(
            reading_first_half,
            center,
            z0,
            reading_second_half,
            weight_one.clone(),
        );

        // Second half: read symbols and pop matching ones
        for sym in alphabet {
            let stack_sym = symbol_map[sym];
            builder.add_pop_transition(
                reading_second_half,
                Some(sym.clone()),
                stack_sym,
                reading_second_half,
                weight_one.clone(),
            );
        }

        // Accept when stack is empty (only z0 remains)
        builder.add_epsilon_transition(
            reading_second_half,
            z0,
            accepting,
            StackAction::Noop,
            weight_one,
        );

        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_builder_basic() {
        let mut builder: PdaBuilder<char, TropicalWeight> = PdaBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_final_state(TropicalWeight::one());

        builder.set_start(s0);

        let z0 = builder.initial_stack();
        let marker = builder.add_stack_symbol();

        builder.add_push_transition(
            s0,
            Some('a'),
            z0,
            vec![z0, marker],
            s1,
            TropicalWeight::one(),
        );

        let pda = builder.build();

        assert_eq!(pda.num_states(), 2);
        assert_eq!(pda.num_transitions(), 1);
    }

    #[test]
    fn test_add_stack_symbol() {
        let mut builder: PdaBuilder<char, TropicalWeight> = PdaBuilder::new();

        let sym1 = builder.add_stack_symbol();
        let sym2 = builder.add_stack_symbol();
        let sym3 = builder.add_stack_symbol();

        assert_eq!(sym1.id(), 1);
        assert_eq!(sym2.id(), 2);
        assert_eq!(sym3.id(), 3);
    }

    #[test]
    fn test_balanced_brackets() {
        let pda = PdaBuilder::balanced_brackets('(', ')', TropicalWeight::one());

        assert!(pda.accepts("".chars()));
        assert!(pda.accepts("()".chars()));
        assert!(pda.accepts("(())".chars()));
        assert!(pda.accepts("((()))".chars()));
        assert!(!pda.accepts("(".chars()));
        assert!(!pda.accepts(")".chars()));
        assert!(!pda.accepts("(()".chars()));
    }

    #[test]
    fn test_a_n_b_n() {
        let pda = PdaBuilder::a_n_b_n('a', 'b', TropicalWeight::one());

        assert!(pda.accepts("ab".chars()));
        assert!(pda.accepts("aabb".chars()));
        assert!(pda.accepts("aaabbb".chars()));
        assert!(!pda.accepts("".chars()));
        assert!(!pda.accepts("a".chars()));
        assert!(!pda.accepts("b".chars()));
        assert!(!pda.accepts("aab".chars()));
        assert!(!pda.accepts("abb".chars()));
    }

    #[test]
    fn test_palindrome_with_center() {
        let alphabet = vec!['a', 'b'];
        let pda = PdaBuilder::palindrome_with_center(&alphabet, '#', TropicalWeight::one());

        assert!(pda.accepts("#".chars())); // Empty palindrome
        assert!(pda.accepts("a#a".chars()));
        assert!(pda.accepts("b#b".chars()));
        assert!(pda.accepts("ab#ba".chars()));
        assert!(pda.accepts("aba#aba".chars()));
        assert!(pda.accepts("aab#baa".chars()));
        assert!(!pda.accepts("a#b".chars()));
        assert!(!pda.accepts("ab#ab".chars()));
        assert!(!pda.accepts("ab#".chars()));
    }

    #[test]
    fn test_builder_chaining() {
        let mut builder: PdaBuilder<char, TropicalWeight> = PdaBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();
        let s2 = builder.add_final_state(TropicalWeight::one());

        builder.set_start(s0);

        let z0 = builder.initial_stack();
        let marker = builder.add_stack_symbol();

        builder
            .add_push_transition(
                s0,
                Some('a'),
                z0,
                vec![z0, marker],
                s1,
                TropicalWeight::one(),
            )
            .add_pop_transition(s1, Some('b'), marker, s2, TropicalWeight::one());

        let pda = builder.build();

        assert!(pda.accepts("ab".chars()));
        assert!(!pda.accepts("a".chars()));
        assert!(!pda.accepts("b".chars()));
    }

    #[test]
    fn test_accept_mode_builder() {
        let builder: PdaBuilder<char, TropicalWeight> =
            PdaBuilder::with_accept_mode(PdaAcceptMode::EmptyStack);

        let pda = builder.build();
        assert_eq!(pda.accept_mode(), PdaAcceptMode::EmptyStack);
    }

    #[test]
    fn test_read_transition() {
        let mut builder: PdaBuilder<char, TropicalWeight> = PdaBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_final_state(TropicalWeight::one());

        builder.set_start(s0);

        let z0 = builder.initial_stack();

        builder.add_read_transition(s0, 'a', z0, s1, TropicalWeight::one());

        let pda = builder.build();

        let trans = &pda.transitions(s0)[0];
        assert!(trans.stack_action.is_noop());
        assert_eq!(trans.input, Some('a'));
    }

    #[test]
    fn test_replace_transition() {
        let mut builder: PdaBuilder<char, TropicalWeight> = PdaBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_state();

        builder.set_start(s0);

        let z0 = builder.initial_stack();
        let marker1 = builder.add_stack_symbol();
        let marker2 = builder.add_stack_symbol();

        builder.add_replace_transition(
            s0,
            Some('a'),
            z0,
            vec![marker1, marker2],
            s1,
            TropicalWeight::one(),
        );

        let pda = builder.build();

        let trans = &pda.transitions(s0)[0];
        match &trans.stack_action {
            StackAction::Replace(symbols) => {
                assert_eq!(symbols.len(), 2);
                assert_eq!(symbols[0], marker1);
                assert_eq!(symbols[1], marker2);
            }
            _ => panic!("Expected Replace action"),
        }
    }
}
