//! Vector-based weighted pushdown automaton implementation.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

use super::{
    PdaAcceptMode, PdaConfiguration, PdaTransition, StackAction, StackSymbol, WeightedPda,
};
use crate::semiring::Semiring;
use crate::wfst::StateId;

/// State information for a PDA.
#[derive(Debug, Clone)]
pub struct PdaState<L, W: Semiring> {
    /// Whether this is a final state.
    pub is_final: bool,
    /// Final weight (only meaningful if is_final).
    pub final_weight: W,
    /// Outgoing transitions.
    pub transitions: Vec<PdaTransition<L, W>>,
}

impl<L, W: Semiring> PdaState<L, W> {
    /// Create a non-final state.
    pub fn non_final() -> Self {
        Self {
            is_final: false,
            final_weight: W::zero(),
            transitions: Vec::new(),
        }
    }

    /// Create a final state with the given weight.
    pub fn final_with_weight(weight: W) -> Self {
        Self {
            is_final: true,
            final_weight: weight,
            transitions: Vec::new(),
        }
    }
}

impl<L, W: Semiring> Default for PdaState<L, W> {
    fn default() -> Self {
        Self::non_final()
    }
}

/// Vector-based implementation of a weighted pushdown automaton.
#[derive(Debug, Clone)]
pub struct VectorPda<L, W: Semiring> {
    /// States indexed by ID.
    states: Vec<PdaState<L, W>>,
    /// Initial state.
    start: StateId,
    /// Initial stack symbol.
    initial_stack: StackSymbol,
    /// Acceptance mode.
    accept_mode: PdaAcceptMode,
    /// Total number of transitions.
    num_transitions: usize,
}

impl<L, W: Semiring> VectorPda<L, W> {
    /// Get the initial stack symbol (inherent method).
    pub fn get_initial_stack(&self) -> StackSymbol {
        self.initial_stack
    }

    /// Get the start state (inherent method).
    pub fn get_start(&self) -> StateId {
        self.start
    }

    /// Get the acceptance mode (inherent method).
    pub fn get_accept_mode(&self) -> PdaAcceptMode {
        self.accept_mode
    }

    /// Get the number of states (inherent method).
    pub fn get_num_states(&self) -> usize {
        self.states.len()
    }

    /// Get the number of transitions (inherent method).
    pub fn get_num_transitions(&self) -> usize {
        self.num_transitions
    }

    /// Get transitions from a state (inherent method).
    pub fn get_transitions(&self, state: StateId) -> &[PdaTransition<L, W>] {
        self.states
            .get(state as usize)
            .map(|s| s.transitions.as_slice())
            .unwrap_or(&[])
    }

    /// Check if a state is final (inherent method).
    pub fn get_is_final(&self, state: StateId) -> bool {
        self.states
            .get(state as usize)
            .map(|s| s.is_final)
            .unwrap_or(false)
    }

    /// Get the final weight of a state (inherent method).
    pub fn get_final_weight(&self, state: StateId) -> W
    where
        W: Clone,
    {
        self.states
            .get(state as usize)
            .map(|s| s.final_weight.clone())
            .unwrap_or_else(W::zero)
    }
}

impl<L: Clone + Eq + Hash, W: Semiring + Clone> VectorPda<L, W> {
    /// Create a new empty PDA.
    pub fn new() -> Self {
        Self {
            states: Vec::new(),
            start: 0,
            initial_stack: StackSymbol::BOTTOM,
            accept_mode: PdaAcceptMode::FinalState,
            num_transitions: 0,
        }
    }

    /// Create a PDA with a specific acceptance mode.
    pub fn with_accept_mode(accept_mode: PdaAcceptMode) -> Self {
        Self {
            states: Vec::new(),
            start: 0,
            initial_stack: StackSymbol::BOTTOM,
            accept_mode,
            num_transitions: 0,
        }
    }

    /// Add a state and return its ID.
    pub fn add_state(&mut self) -> StateId {
        let id = self.states.len() as StateId;
        self.states.push(PdaState::non_final());
        id
    }

    /// Add a final state with the given weight.
    pub fn add_final_state(&mut self, weight: W) -> StateId {
        let id = self.states.len() as StateId;
        self.states.push(PdaState::final_with_weight(weight));
        id
    }

    /// Set the start state.
    pub fn set_start(&mut self, state: StateId) {
        self.start = state;
    }

    /// Set the initial stack symbol.
    pub fn set_initial_stack(&mut self, symbol: StackSymbol) {
        self.initial_stack = symbol;
    }

    /// Set the acceptance mode.
    pub fn set_accept_mode(&mut self, mode: PdaAcceptMode) {
        self.accept_mode = mode;
    }

    /// Make a state final.
    pub fn set_final(&mut self, state: StateId, weight: W) {
        if let Some(s) = self.states.get_mut(state as usize) {
            s.is_final = true;
            s.final_weight = weight;
        }
    }

    /// Remove final status from a state.
    pub fn unset_final(&mut self, state: StateId) {
        if let Some(s) = self.states.get_mut(state as usize) {
            s.is_final = false;
            s.final_weight = W::zero();
        }
    }

    /// Add a transition.
    pub fn add_transition(&mut self, transition: PdaTransition<L, W>) {
        if let Some(s) = self.states.get_mut(transition.from as usize) {
            s.transitions.push(transition);
            self.num_transitions += 1;
        }
    }

    /// Add a transition with explicit parameters.
    pub fn add_transition_parts(
        &mut self,
        from: StateId,
        input: Option<L>,
        stack_top: StackSymbol,
        stack_action: StackAction,
        to: StateId,
        weight: W,
    ) {
        self.add_transition(PdaTransition::new(
            from,
            input,
            stack_top,
            stack_action,
            to,
            weight,
        ));
    }

    /// Add an epsilon transition.
    pub fn add_epsilon_transition(
        &mut self,
        from: StateId,
        stack_top: StackSymbol,
        stack_action: StackAction,
        to: StateId,
        weight: W,
    ) {
        self.add_transition(PdaTransition::epsilon(
            from,
            stack_top,
            stack_action,
            to,
            weight,
        ));
    }

    /// Get mutable access to a state.
    pub fn state_mut(&mut self, state: StateId) -> Option<&mut PdaState<L, W>> {
        self.states.get_mut(state as usize)
    }

    /// Reserve capacity for states.
    pub fn reserve_states(&mut self, additional: usize) {
        self.states.reserve(additional);
    }

    /// Check if the PDA accepts the given input using breadth-first search.
    ///
    /// This is a simple recognition algorithm that may not terminate
    /// for PDAs with epsilon cycles. For production use, consider
    /// implementing a more sophisticated algorithm with cycle detection.
    pub fn accepts<I>(&self, input: I) -> bool
    where
        I: IntoIterator<Item = L>,
        L: PartialEq,
    {
        let input_vec: Vec<L> = input.into_iter().collect();
        let initial = PdaConfiguration::initial(self.start, input_vec, self.initial_stack);

        let mut visited: HashSet<(StateId, usize, Vec<StackSymbol>)> = HashSet::new();
        let mut queue: VecDeque<PdaConfiguration<L>> = VecDeque::new();
        queue.push_back(initial);

        while let Some(config) = queue.pop_front() {
            // Create a key for cycle detection
            let key = (
                config.state,
                config.remaining_input.len(),
                config.stack.clone(),
            );
            if visited.contains(&key) {
                continue;
            }
            visited.insert(key);

            // Check if we're in an accepting configuration
            if self.is_config_accepting(&config) {
                return true;
            }

            // Get stack top
            let stack_top = match config.stack_top() {
                Some(st) => st,
                None => continue, // Empty stack, no transitions possible
            };

            // Try epsilon transitions first
            for trans in self.get_epsilon_transitions(config.state, stack_top) {
                if let Some(new_config) = config.apply_transition(trans) {
                    queue.push_back(new_config);
                }
            }

            // Try consuming transitions
            if let Some(next_input) = config.next_input() {
                for trans in
                    self.get_matching_transitions(config.state, Some(next_input), stack_top)
                {
                    if !trans.is_epsilon() {
                        if let Some(new_config) = config.apply_transition(trans) {
                            queue.push_back(new_config);
                        }
                    }
                }
            }
        }

        false
    }

    /// Check if a configuration is accepting (inherent method).
    fn is_config_accepting(&self, config: &PdaConfiguration<L>) -> bool {
        if !config.input_exhausted() {
            return false;
        }

        match self.accept_mode {
            PdaAcceptMode::FinalState => self.get_is_final(config.state),
            PdaAcceptMode::EmptyStack => config.stack_empty(),
            PdaAcceptMode::Both => self.get_is_final(config.state) || config.stack_empty(),
        }
    }

    /// Get epsilon transitions from a state with a given stack top (inherent method).
    fn get_epsilon_transitions(
        &self,
        state: StateId,
        stack_top: StackSymbol,
    ) -> Vec<&PdaTransition<L, W>>
    where
        L: PartialEq,
    {
        self.get_transitions(state)
            .iter()
            .filter(|t| t.is_epsilon() && t.stack_top == stack_top)
            .collect()
    }

    /// Get transitions matching a specific input and stack top (inherent method).
    fn get_matching_transitions(
        &self,
        state: StateId,
        input: Option<&L>,
        stack_top: StackSymbol,
    ) -> Vec<&PdaTransition<L, W>>
    where
        L: PartialEq,
    {
        self.get_transitions(state)
            .iter()
            .filter(|t| t.matches(input, stack_top))
            .collect()
    }

    /// Get the accepting weight for a configuration (inherent method).
    fn get_accepting_weight(&self, config: &PdaConfiguration<L>) -> Option<W> {
        if !config.input_exhausted() {
            return None;
        }

        match self.accept_mode {
            PdaAcceptMode::FinalState => {
                if self.get_is_final(config.state) {
                    Some(self.get_final_weight(config.state))
                } else {
                    None
                }
            }
            PdaAcceptMode::EmptyStack => {
                if config.stack_empty() {
                    Some(W::one())
                } else {
                    None
                }
            }
            PdaAcceptMode::Both => {
                if self.get_is_final(config.state) {
                    Some(self.get_final_weight(config.state))
                } else if config.stack_empty() {
                    Some(W::one())
                } else {
                    None
                }
            }
        }
    }

    /// Compute the total weight of all accepting paths for the given input.
    ///
    /// Returns None if the input is not accepted.
    pub fn total_weight<I>(&self, input: I) -> Option<W>
    where
        I: IntoIterator<Item = L>,
        L: PartialEq,
    {
        let input_vec: Vec<L> = input.into_iter().collect();
        let initial = PdaConfiguration::initial(self.start, input_vec, self.initial_stack);

        // Map from configurations to accumulated weights
        let mut weights: HashMap<(StateId, usize, Vec<StackSymbol>), W> = HashMap::new();
        let mut queue: VecDeque<(PdaConfiguration<L>, W)> = VecDeque::new();
        queue.push_back((initial, W::one()));

        let mut total = W::zero();

        while let Some((config, path_weight)) = queue.pop_front() {
            let key = (
                config.state,
                config.remaining_input.len(),
                config.stack.clone(),
            );

            // Update weight for this configuration
            let current_weight = weights.entry(key.clone()).or_insert_with(W::zero);
            *current_weight = current_weight.clone().plus(&path_weight);

            // Check if we're in an accepting configuration
            if let Some(accept_weight) = self.get_accepting_weight(&config) {
                total = total.plus(&path_weight.clone().times(&accept_weight));
            }

            // Get stack top
            let stack_top = match config.stack_top() {
                Some(st) => st,
                None => continue,
            };

            // Try epsilon transitions
            for trans in self.get_epsilon_transitions(config.state, stack_top) {
                if let Some(new_config) = config.apply_transition(trans) {
                    let new_weight = path_weight.clone().times(&trans.weight);
                    queue.push_back((new_config, new_weight));
                }
            }

            // Try consuming transitions
            if let Some(next_input) = config.next_input() {
                for trans in
                    self.get_matching_transitions(config.state, Some(next_input), stack_top)
                {
                    if !trans.is_epsilon() {
                        if let Some(new_config) = config.apply_transition(trans) {
                            let new_weight = path_weight.clone().times(&trans.weight);
                            queue.push_back((new_config, new_weight));
                        }
                    }
                }
            }
        }

        if total == W::zero() {
            None
        } else {
            Some(total)
        }
    }

    /// Approximate this PDA as a finite state transducer with bounded stack depth.
    ///
    /// This creates an FST where states are (PDA state, stack content) pairs,
    /// limited to stacks of at most `max_depth` symbols.
    pub fn approximate_fst(&self, max_depth: usize) -> crate::wfst::VectorWfst<L, W>
    where
        L: Send + Sync,
    {
        use crate::wfst::{MutableWfst, VectorWfst, WeightedTransition};

        let mut fst: VectorWfst<L, W> = VectorWfst::new();

        // Map from (state, stack) to FST state
        let mut state_map: HashMap<(StateId, Vec<StackSymbol>), StateId> = HashMap::new();
        let mut queue: VecDeque<(StateId, Vec<StackSymbol>)> = VecDeque::new();

        // Initial FST state
        let initial_stack = vec![self.initial_stack];
        let initial_key = (self.start, initial_stack.clone());
        let initial_fst_state = fst.add_state();
        state_map.insert(initial_key.clone(), initial_fst_state);
        fst.set_start(initial_fst_state);
        queue.push_back((self.start, initial_stack));

        while let Some((pda_state, stack)) = queue.pop_front() {
            let key = (pda_state, stack.clone());
            let fst_state = *state_map.get(&key).expect("state should exist");

            // Check if accepting
            let config = PdaConfiguration::new(pda_state, vec![], stack.clone());
            if let Some(accept_weight) = self.get_accepting_weight(&config) {
                fst.set_final(fst_state, accept_weight);
            }

            // Get stack top
            let stack_top = match stack.last() {
                Some(&st) => st,
                None => continue,
            };

            // Process transitions
            for trans in self.get_transitions(pda_state) {
                if trans.stack_top != stack_top {
                    continue;
                }

                // Apply stack action
                let mut new_stack = stack.clone();
                if !trans.stack_action.apply(&mut new_stack) {
                    continue;
                }

                // Enforce depth limit
                if new_stack.len() > max_depth {
                    continue;
                }

                // Get or create target FST state
                let target_key = (trans.to, new_stack.clone());
                let target_fst_state = if let Some(&existing) = state_map.get(&target_key) {
                    existing
                } else {
                    let new_state = fst.add_state();
                    state_map.insert(target_key.clone(), new_state);
                    queue.push_back((trans.to, new_stack));
                    new_state
                };

                // Add FST transition
                fst.add_transition(WeightedTransition::new(
                    fst_state,
                    trans.input.clone(),
                    trans.input.clone(),
                    target_fst_state,
                    trans.weight.clone(),
                ));
            }
        }

        fst
    }
}

impl<L: Clone + Eq + Hash, W: Semiring + Clone> Default for VectorPda<L, W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L, W> WeightedPda<L, W> for VectorPda<L, W>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    fn start(&self) -> StateId {
        self.start
    }

    fn initial_stack(&self) -> StackSymbol {
        self.initial_stack
    }

    fn is_final(&self, state: StateId) -> bool {
        self.states
            .get(state as usize)
            .map(|s| s.is_final)
            .unwrap_or(false)
    }

    fn final_weight(&self, state: StateId) -> W {
        self.states
            .get(state as usize)
            .map(|s| s.final_weight.clone())
            .unwrap_or_else(W::zero)
    }

    fn accept_mode(&self) -> PdaAcceptMode {
        self.accept_mode
    }

    fn transitions(&self, state: StateId) -> &[PdaTransition<L, W>] {
        self.states
            .get(state as usize)
            .map(|s| s.transitions.as_slice())
            .unwrap_or(&[])
    }

    fn num_states(&self) -> usize {
        self.states.len()
    }

    fn num_transitions(&self) -> usize {
        self.num_transitions
    }

    fn states(&self) -> impl Iterator<Item = StateId> {
        0..self.states.len() as StateId
    }

    fn final_states(&self) -> impl Iterator<Item = StateId> {
        self.states
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_final)
            .map(|(i, _)| i as StateId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::wfst::Wfst;

    #[test]
    fn test_empty_pda() {
        let pda: VectorPda<char, TropicalWeight> = VectorPda::new();
        assert_eq!(pda.num_states(), 0);
        assert_eq!(pda.num_transitions(), 0);
        assert!(pda.is_empty());
    }

    #[test]
    fn test_add_states() {
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        let s0 = pda.add_state();
        let s1 = pda.add_final_state(TropicalWeight::one());

        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(pda.num_states(), 2);
        assert!(!pda.is_final(s0));
        assert!(pda.is_final(s1));
    }

    #[test]
    fn test_set_start() {
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        let s0 = pda.add_state();
        let s1 = pda.add_state();

        pda.set_start(s0);
        assert_eq!(pda.start(), s0);

        pda.set_start(s1);
        assert_eq!(pda.start(), s1);
    }

    #[test]
    fn test_set_final() {
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        let s0 = pda.add_state();
        assert!(!pda.is_final(s0));

        pda.set_final(s0, TropicalWeight::new(2.0));
        assert!(pda.is_final(s0));
        assert_eq!(pda.final_weight(s0).value(), 2.0);

        pda.unset_final(s0);
        assert!(!pda.is_final(s0));
    }

    #[test]
    fn test_add_transitions() {
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        let s0 = pda.add_state();
        let s1 = pda.add_state();

        pda.add_transition_parts(
            s0,
            Some('a'),
            StackSymbol::BOTTOM,
            StackAction::Push(vec![StackSymbol::BOTTOM, StackSymbol::new(1)]),
            s1,
            TropicalWeight::one(),
        );

        assert_eq!(pda.num_transitions(), 1);
        assert_eq!(pda.transitions(s0).len(), 1);
        assert_eq!(pda.transitions(s1).len(), 0);
    }

    #[test]
    fn test_epsilon_transition() {
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        let s0 = pda.add_state();
        let s1 = pda.add_state();

        pda.add_epsilon_transition(
            s0,
            StackSymbol::BOTTOM,
            StackAction::Noop,
            s1,
            TropicalWeight::one(),
        );

        let trans = &pda.transitions(s0)[0];
        assert!(trans.is_epsilon());
    }

    #[test]
    fn test_balanced_parentheses_pda() {
        // PDA for balanced parentheses: { (^n )^n | n >= 0 }
        // Uses two states: s0 (start/final when balanced), s1 (processing with markers)
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        let s0 = pda.add_final_state(TropicalWeight::one()); // Accept when stack = [Z₀]
        let s1 = pda.add_state(); // Processing state (not final)
        pda.set_start(s0);

        let z0 = StackSymbol::BOTTOM;
        let left = StackSymbol::new(1);

        // From s0 (balanced state): On '(', push marker and go to s1
        pda.add_transition_parts(
            s0,
            Some('('),
            z0,
            StackAction::Push(vec![z0, left]),
            s1,
            TropicalWeight::one(),
        );

        // From s1: On '(', push another marker
        pda.add_transition_parts(
            s1,
            Some('('),
            left,
            StackAction::Push(vec![left, left]),
            s1,
            TropicalWeight::one(),
        );

        // From s1: On ')', pop marker. If z0 is revealed, go back to s0
        pda.add_transition_parts(
            s1,
            Some(')'),
            left,
            StackAction::Pop,
            s1,
            TropicalWeight::one(),
        );

        // Epsilon transition: when in s1 with z0 on top, go to s0
        pda.add_epsilon_transition(s1, z0, StackAction::Noop, s0, TropicalWeight::one());

        // Test acceptance
        assert!(pda.accepts("".chars()));
        assert!(pda.accepts("()".chars()));
        assert!(pda.accepts("(())".chars()));
        assert!(pda.accepts("((()))".chars()));
        assert!(!pda.accepts("(".chars()));
        assert!(!pda.accepts(")".chars()));
        assert!(!pda.accepts("(()".chars()));
        assert!(!pda.accepts("())".chars()));
        assert!(!pda.accepts(")(()".chars()));
    }

    #[test]
    fn test_a_n_b_n_pda() {
        // PDA for { a^n b^n | n >= 1 }
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        let s0 = pda.add_state(); // Start, reading a's
        let s1 = pda.add_state(); // Reading b's
        let s2 = pda.add_final_state(TropicalWeight::one()); // Accepting

        pda.set_start(s0);

        let z0 = StackSymbol::BOTTOM;
        let a_marker = StackSymbol::new(1);

        // Read first 'a', push marker
        pda.add_transition_parts(
            s0,
            Some('a'),
            z0,
            StackAction::Push(vec![z0, a_marker]),
            s0,
            TropicalWeight::one(),
        );

        // Read more 'a's, push markers
        pda.add_transition_parts(
            s0,
            Some('a'),
            a_marker,
            StackAction::Push(vec![a_marker, a_marker]),
            s0,
            TropicalWeight::one(),
        );

        // Switch to reading 'b's (epsilon transition)
        pda.add_epsilon_transition(s0, a_marker, StackAction::Noop, s1, TropicalWeight::one());

        // Read 'b', pop marker
        pda.add_transition_parts(
            s1,
            Some('b'),
            a_marker,
            StackAction::Pop,
            s1,
            TropicalWeight::one(),
        );

        // Accept when stack has only z0 (epsilon transition to final)
        pda.add_epsilon_transition(s1, z0, StackAction::Noop, s2, TropicalWeight::one());

        // Test acceptance
        assert!(pda.accepts("ab".chars()));
        assert!(pda.accepts("aabb".chars()));
        assert!(pda.accepts("aaabbb".chars()));
        assert!(!pda.accepts("".chars()));
        assert!(!pda.accepts("a".chars()));
        assert!(!pda.accepts("b".chars()));
        assert!(!pda.accepts("aab".chars()));
        assert!(!pda.accepts("abb".chars()));
        assert!(!pda.accepts("ba".chars()));
    }

    #[test]
    fn test_accept_mode_empty_stack() {
        // PDA accepting by empty stack
        let mut pda: VectorPda<char, TropicalWeight> =
            VectorPda::with_accept_mode(PdaAcceptMode::EmptyStack);

        let s0 = pda.add_state();
        pda.set_start(s0);

        let z0 = StackSymbol::BOTTOM;

        // Read 'a', pop the stack
        pda.add_transition_parts(
            s0,
            Some('a'),
            z0,
            StackAction::Pop,
            s0,
            TropicalWeight::one(),
        );

        // Should accept "a" (empties the stack)
        assert!(pda.accepts("a".chars()));
        assert!(!pda.accepts("".chars())); // Stack not empty
        assert!(!pda.accepts("aa".chars())); // Can't process second 'a'
    }

    #[test]
    fn test_states_iterator() {
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        pda.add_state();
        pda.add_state();
        pda.add_state();

        let states: Vec<_> = pda.states().collect();
        assert_eq!(states, vec![0, 1, 2]);
    }

    #[test]
    fn test_final_states_iterator() {
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        pda.add_state(); // not final
        pda.add_final_state(TropicalWeight::one()); // final
        pda.add_state(); // not final
        pda.add_final_state(TropicalWeight::one()); // final

        let final_states: Vec<_> = pda.final_states().collect();
        assert_eq!(final_states, vec![1, 3]);
    }

    #[test]
    fn test_approximate_fst() {
        // Simple PDA: accepts "ab"
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        let s0 = pda.add_state();
        let s1 = pda.add_state();
        let s2 = pda.add_final_state(TropicalWeight::one());

        pda.set_start(s0);

        let z0 = StackSymbol::BOTTOM;
        let marker = StackSymbol::new(1);

        // Read 'a', push marker
        pda.add_transition_parts(
            s0,
            Some('a'),
            z0,
            StackAction::Push(vec![z0, marker]),
            s1,
            TropicalWeight::one(),
        );

        // Read 'b', pop marker
        pda.add_transition_parts(
            s1,
            Some('b'),
            marker,
            StackAction::Pop,
            s2,
            TropicalWeight::one(),
        );

        // Approximate as FST with max depth 5
        let fst = pda.approximate_fst(5);

        // Should have 3 states (one for each PDA state with appropriate stack)
        assert!(fst.num_states() >= 3);
    }

    #[test]
    fn test_matching_transitions() {
        let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();

        let s0 = pda.add_state();
        let s1 = pda.add_state();

        let z0 = StackSymbol::BOTTOM;
        let marker = StackSymbol::new(1);

        // Add several transitions
        pda.add_transition_parts(
            s0,
            Some('a'),
            z0,
            StackAction::Noop,
            s1,
            TropicalWeight::one(),
        );
        pda.add_transition_parts(
            s0,
            Some('b'),
            z0,
            StackAction::Noop,
            s1,
            TropicalWeight::one(),
        );
        pda.add_transition_parts(
            s0,
            Some('a'),
            marker,
            StackAction::Noop,
            s1,
            TropicalWeight::one(),
        );
        pda.add_epsilon_transition(s0, z0, StackAction::Noop, s1, TropicalWeight::one());

        // Test matching
        let matches = pda.matching_transitions(s0, Some(&'a'), z0);
        assert_eq!(matches.len(), 2); // 'a' on z0 and epsilon on z0

        let matches = pda.matching_transitions(s0, Some(&'b'), z0);
        assert_eq!(matches.len(), 2); // 'b' on z0 and epsilon on z0

        let matches = pda.matching_transitions(s0, Some(&'c'), z0);
        assert_eq!(matches.len(), 1); // Only epsilon

        let matches = pda.matching_transitions(s0, Some(&'a'), marker);
        assert_eq!(matches.len(), 1); // 'a' on marker
    }
}
