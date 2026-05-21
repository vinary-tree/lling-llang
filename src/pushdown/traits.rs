//! Traits for weighted pushdown automata.

use std::hash::Hash;

use super::{PdaTransition, StackSymbol};
use crate::semiring::Semiring;
use crate::wfst::StateId;

/// How a PDA accepts input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PdaAcceptMode {
    /// Accept by reaching a final state.
    #[default]
    FinalState,
    /// Accept by emptying the stack.
    EmptyStack,
    /// Accept by either final state or empty stack.
    Both,
}

/// A configuration (instantaneous description) of a PDA.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PdaConfiguration<L> {
    /// Current state.
    pub state: StateId,
    /// Remaining input.
    pub remaining_input: Vec<L>,
    /// Current stack (top at end).
    pub stack: Vec<StackSymbol>,
}

impl<L> PdaConfiguration<L> {
    /// Create a new configuration.
    pub fn new(state: StateId, remaining_input: Vec<L>, stack: Vec<StackSymbol>) -> Self {
        Self {
            state,
            remaining_input,
            stack,
        }
    }

    /// Create the initial configuration for a PDA.
    pub fn initial(start: StateId, input: Vec<L>, initial_stack: StackSymbol) -> Self {
        Self {
            state: start,
            remaining_input: input,
            stack: vec![initial_stack],
        }
    }

    /// Check if input is exhausted.
    pub fn input_exhausted(&self) -> bool {
        self.remaining_input.is_empty()
    }

    /// Check if stack is empty.
    pub fn stack_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Get the current stack top (if any).
    pub fn stack_top(&self) -> Option<StackSymbol> {
        self.stack.last().copied()
    }

    /// Get the next input symbol (if any).
    pub fn next_input(&self) -> Option<&L> {
        self.remaining_input.first()
    }
}

impl<L: Clone> PdaConfiguration<L> {
    /// Apply a transition to get a new configuration.
    pub fn apply_transition(&self, transition: &PdaTransition<L, impl Semiring>) -> Option<Self>
    where
        L: PartialEq,
    {
        // Check stack top matches
        let stack_top = self.stack_top()?;
        if stack_top != transition.stack_top {
            return None;
        }

        // Check input matches
        let mut new_input = self.remaining_input.clone();
        match &transition.input {
            Some(expected) => {
                if self.next_input() != Some(expected) {
                    return None;
                }
                new_input.remove(0); // Consume input
            }
            None => {
                // Epsilon transition - don't consume input
            }
        }

        // Apply stack action
        let mut new_stack = self.stack.clone();
        if !transition.stack_action.apply(&mut new_stack) {
            return None;
        }

        Some(Self {
            state: transition.to,
            remaining_input: new_input,
            stack: new_stack,
        })
    }
}

/// Trait for weighted pushdown automata.
pub trait WeightedPda<L, W: Semiring>: Clone + Send + Sync {
    /// Get the start state.
    fn start(&self) -> StateId;

    /// Get the initial stack symbol.
    fn initial_stack(&self) -> StackSymbol;

    /// Check if a state is final.
    fn is_final(&self, state: StateId) -> bool;

    /// Get the final weight of a state.
    fn final_weight(&self, state: StateId) -> W;

    /// Get the acceptance mode.
    fn accept_mode(&self) -> PdaAcceptMode;

    /// Check if this PDA accepts by empty stack.
    fn accepts_empty_stack(&self) -> bool {
        matches!(
            self.accept_mode(),
            PdaAcceptMode::EmptyStack | PdaAcceptMode::Both
        )
    }

    /// Check if this PDA accepts by final state.
    fn accepts_final_state(&self) -> bool {
        matches!(
            self.accept_mode(),
            PdaAcceptMode::FinalState | PdaAcceptMode::Both
        )
    }

    /// Get transitions from a state.
    fn transitions(&self, state: StateId) -> &[PdaTransition<L, W>];

    /// Get transitions matching a specific input and stack top.
    fn matching_transitions(
        &self,
        state: StateId,
        input: Option<&L>,
        stack_top: StackSymbol,
    ) -> Vec<&PdaTransition<L, W>>
    where
        L: PartialEq,
    {
        self.transitions(state)
            .iter()
            .filter(|t| t.matches(input, stack_top))
            .collect()
    }

    /// Get epsilon transitions from a state with a given stack top.
    fn epsilon_transitions(
        &self,
        state: StateId,
        stack_top: StackSymbol,
    ) -> Vec<&PdaTransition<L, W>>
    where
        L: PartialEq,
    {
        self.transitions(state)
            .iter()
            .filter(|t| t.is_epsilon() && t.stack_top == stack_top)
            .collect()
    }

    /// Get the number of states.
    fn num_states(&self) -> usize;

    /// Get the number of transitions.
    fn num_transitions(&self) -> usize;

    /// Iterate over all states.
    fn states(&self) -> impl Iterator<Item = StateId>;

    /// Iterate over final states.
    fn final_states(&self) -> impl Iterator<Item = StateId>;

    /// Check if the PDA is empty (no states).
    fn is_empty(&self) -> bool {
        self.num_states() == 0
    }

    /// Check if a configuration is accepting.
    fn is_accepting(&self, config: &PdaConfiguration<L>) -> bool {
        if !config.input_exhausted() {
            return false;
        }

        match self.accept_mode() {
            PdaAcceptMode::FinalState => self.is_final(config.state),
            PdaAcceptMode::EmptyStack => config.stack_empty(),
            PdaAcceptMode::Both => self.is_final(config.state) || config.stack_empty(),
        }
    }

    /// Get the accepting weight for a configuration.
    fn accepting_weight(&self, config: &PdaConfiguration<L>) -> Option<W>
    where
        W: Clone,
    {
        if !config.input_exhausted() {
            return None;
        }

        match self.accept_mode() {
            PdaAcceptMode::FinalState => {
                if self.is_final(config.state) {
                    Some(self.final_weight(config.state))
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
                if self.is_final(config.state) {
                    Some(self.final_weight(config.state))
                } else if config.stack_empty() {
                    Some(W::one())
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pushdown::stack::StackAction;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_accept_mode_default() {
        assert_eq!(PdaAcceptMode::default(), PdaAcceptMode::FinalState);
    }

    #[test]
    fn test_configuration_initial() {
        let config: PdaConfiguration<char> =
            PdaConfiguration::initial(0, vec!['a', 'b', 'c'], StackSymbol::BOTTOM);

        assert_eq!(config.state, 0);
        assert_eq!(config.remaining_input, vec!['a', 'b', 'c']);
        assert_eq!(config.stack, vec![StackSymbol::BOTTOM]);
    }

    #[test]
    fn test_configuration_stack_top() {
        let config: PdaConfiguration<char> =
            PdaConfiguration::new(0, vec![], vec![StackSymbol::BOTTOM, StackSymbol::new(1)]);

        assert_eq!(config.stack_top(), Some(StackSymbol::new(1)));
    }

    #[test]
    fn test_configuration_empty_stack() {
        let config: PdaConfiguration<char> = PdaConfiguration::new(0, vec![], vec![]);

        assert!(config.stack_empty());
        assert_eq!(config.stack_top(), None);
    }

    #[test]
    fn test_configuration_input_exhausted() {
        let config: PdaConfiguration<char> =
            PdaConfiguration::new(0, vec![], vec![StackSymbol::BOTTOM]);

        assert!(config.input_exhausted());
    }

    #[test]
    fn test_configuration_next_input() {
        let config: PdaConfiguration<char> =
            PdaConfiguration::new(0, vec!['a', 'b'], vec![StackSymbol::BOTTOM]);

        assert_eq!(config.next_input(), Some(&'a'));
    }

    #[test]
    fn test_apply_transition_consuming() {
        let config: PdaConfiguration<char> =
            PdaConfiguration::new(0, vec!['a', 'b'], vec![StackSymbol::BOTTOM]);

        let trans = PdaTransition::<char, TropicalWeight>::new(
            0,
            Some('a'),
            StackSymbol::BOTTOM,
            StackAction::Push(vec![StackSymbol::BOTTOM, StackSymbol::new(1)]),
            1,
            TropicalWeight::one(),
        );

        let new_config = config
            .apply_transition(&trans)
            .expect("pushdown/traits.rs: required value was None/Err");

        assert_eq!(new_config.state, 1);
        assert_eq!(new_config.remaining_input, vec!['b']);
        assert_eq!(
            new_config.stack,
            vec![StackSymbol::BOTTOM, StackSymbol::new(1)]
        );
    }

    #[test]
    fn test_apply_transition_epsilon() {
        let config: PdaConfiguration<char> =
            PdaConfiguration::new(0, vec!['a'], vec![StackSymbol::BOTTOM]);

        let trans = PdaTransition::<char, TropicalWeight>::epsilon(
            0,
            StackSymbol::BOTTOM,
            StackAction::Noop,
            1,
            TropicalWeight::one(),
        );

        let new_config = config
            .apply_transition(&trans)
            .expect("pushdown/traits.rs: required value was None/Err");

        assert_eq!(new_config.state, 1);
        assert_eq!(new_config.remaining_input, vec!['a']); // Not consumed
        assert_eq!(new_config.stack, vec![StackSymbol::BOTTOM]);
    }

    #[test]
    fn test_apply_transition_wrong_stack() {
        let config: PdaConfiguration<char> =
            PdaConfiguration::new(0, vec!['a'], vec![StackSymbol::BOTTOM]);

        let trans = PdaTransition::<char, TropicalWeight>::new(
            0,
            Some('a'),
            StackSymbol::new(1), // Wrong stack top
            StackAction::Pop,
            1,
            TropicalWeight::one(),
        );

        assert!(config.apply_transition(&trans).is_none());
    }

    #[test]
    fn test_apply_transition_wrong_input() {
        let config: PdaConfiguration<char> =
            PdaConfiguration::new(0, vec!['a'], vec![StackSymbol::BOTTOM]);

        let trans = PdaTransition::<char, TropicalWeight>::new(
            0,
            Some('b'), // Wrong input
            StackSymbol::BOTTOM,
            StackAction::Pop,
            1,
            TropicalWeight::one(),
        );

        assert!(config.apply_transition(&trans).is_none());
    }
}
