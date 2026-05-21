//! Transitions for pushdown automata.

use std::fmt::{self, Debug};
use std::hash::Hash;

use super::{StackAction, StackSymbol};
use crate::semiring::Semiring;
use crate::wfst::StateId;

/// A transition in a weighted pushdown automaton.
#[derive(Clone, PartialEq)]
pub struct PdaTransition<L, W: Semiring> {
    /// Source state.
    pub from: StateId,
    /// Input symbol to consume (None for ε-transition).
    pub input: Option<L>,
    /// Required stack top symbol.
    pub stack_top: StackSymbol,
    /// Stack action to perform.
    pub stack_action: StackAction,
    /// Target state.
    pub to: StateId,
    /// Transition weight.
    pub weight: W,
}

impl<L, W: Semiring> PdaTransition<L, W> {
    /// Create a new transition.
    pub fn new(
        from: StateId,
        input: Option<L>,
        stack_top: StackSymbol,
        stack_action: StackAction,
        to: StateId,
        weight: W,
    ) -> Self {
        Self {
            from,
            input,
            stack_top,
            stack_action,
            to,
            weight,
        }
    }

    /// Create an epsilon transition.
    pub fn epsilon(
        from: StateId,
        stack_top: StackSymbol,
        stack_action: StackAction,
        to: StateId,
        weight: W,
    ) -> Self {
        Self::new(from, None, stack_top, stack_action, to, weight)
    }

    /// Check if this is an epsilon transition.
    pub fn is_epsilon(&self) -> bool {
        self.input.is_none()
    }

    /// Check if this transition matches the given input and stack top.
    pub fn matches(&self, input: Option<&L>, stack_top: StackSymbol) -> bool
    where
        L: PartialEq,
    {
        if self.stack_top != stack_top {
            return false;
        }

        match (&self.input, input) {
            (None, _) => true, // Epsilon matches anything
            (Some(a), Some(b)) => a == b,
            (Some(_), None) => false,
        }
    }

    /// Get the source state.
    pub fn source(&self) -> StateId {
        self.from
    }

    /// Get the target state.
    pub fn target(&self) -> StateId {
        self.to
    }

    /// Get the net stack change.
    pub fn net_stack_change(&self) -> i32 {
        self.stack_action.net_change()
    }
}

impl<L: Clone, W: Semiring + Clone> PdaTransition<L, W> {
    /// Map the input label using a function.
    pub fn map_input<F, M>(&self, f: F) -> PdaTransition<M, W>
    where
        F: FnOnce(&L) -> M,
    {
        PdaTransition {
            from: self.from,
            input: self.input.as_ref().map(f),
            stack_top: self.stack_top,
            stack_action: self.stack_action.clone(),
            to: self.to,
            weight: self.weight.clone(),
        }
    }
}

impl<L: Debug, W: Semiring + Debug> Debug for PdaTransition<L, W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PdaTransition {{ {} --", self.from)?;
        match &self.input {
            Some(i) => write!(f, "{:?}", i)?,
            None => write!(f, "ε")?,
        }
        write!(
            f,
            "/{} {}-- {} (w={:?}) }}",
            self.stack_top, self.stack_action, self.to, self.weight
        )
    }
}

impl<L: Eq + Hash, W: Semiring + PartialEq> Eq for PdaTransition<L, W> {}

impl<L: Hash, W: Semiring + Hash> Hash for PdaTransition<L, W> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.from.hash(state);
        self.input.hash(state);
        self.stack_top.hash(state);
        self.stack_action.hash(state);
        self.to.hash(state);
        self.weight.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_transition_creation() {
        let trans: PdaTransition<char, TropicalWeight> = PdaTransition::new(
            0,
            Some('a'),
            StackSymbol::BOTTOM,
            StackAction::Push(vec![StackSymbol::BOTTOM, StackSymbol::new(1)]),
            1,
            TropicalWeight::one(),
        );

        assert_eq!(trans.from, 0);
        assert_eq!(trans.to, 1);
        assert_eq!(trans.input, Some('a'));
        assert!(!trans.is_epsilon());
    }

    #[test]
    fn test_epsilon_transition() {
        let trans: PdaTransition<char, TropicalWeight> = PdaTransition::epsilon(
            0,
            StackSymbol::BOTTOM,
            StackAction::Noop,
            1,
            TropicalWeight::one(),
        );

        assert!(trans.is_epsilon());
        assert_eq!(trans.input, None);
    }

    #[test]
    fn test_matches() {
        let trans: PdaTransition<char, TropicalWeight> = PdaTransition::new(
            0,
            Some('a'),
            StackSymbol::new(1),
            StackAction::Pop,
            1,
            TropicalWeight::one(),
        );

        // Correct input and stack top
        assert!(trans.matches(Some(&'a'), StackSymbol::new(1)));

        // Wrong input
        assert!(!trans.matches(Some(&'b'), StackSymbol::new(1)));

        // Wrong stack top
        assert!(!trans.matches(Some(&'a'), StackSymbol::new(2)));

        // No input
        assert!(!trans.matches(None, StackSymbol::new(1)));
    }

    #[test]
    fn test_epsilon_matches() {
        let trans: PdaTransition<char, TropicalWeight> = PdaTransition::epsilon(
            0,
            StackSymbol::new(1),
            StackAction::Pop,
            1,
            TropicalWeight::one(),
        );

        // Epsilon matches any input with correct stack top
        assert!(trans.matches(Some(&'a'), StackSymbol::new(1)));
        assert!(trans.matches(Some(&'b'), StackSymbol::new(1)));
        assert!(trans.matches(None, StackSymbol::new(1)));

        // Wrong stack top still fails
        assert!(!trans.matches(Some(&'a'), StackSymbol::new(2)));
    }

    #[test]
    fn test_source_target() {
        let trans: PdaTransition<char, TropicalWeight> = PdaTransition::new(
            5,
            Some('x'),
            StackSymbol::BOTTOM,
            StackAction::Noop,
            10,
            TropicalWeight::one(),
        );

        assert_eq!(trans.source(), 5);
        assert_eq!(trans.target(), 10);
    }

    #[test]
    fn test_net_stack_change() {
        let trans1: PdaTransition<char, TropicalWeight> = PdaTransition::new(
            0,
            Some('a'),
            StackSymbol::BOTTOM,
            StackAction::Pop,
            1,
            TropicalWeight::one(),
        );
        assert_eq!(trans1.net_stack_change(), -1);

        let trans2: PdaTransition<char, TropicalWeight> = PdaTransition::new(
            0,
            Some('a'),
            StackSymbol::BOTTOM,
            StackAction::Push(vec![StackSymbol::new(1), StackSymbol::new(2)]),
            1,
            TropicalWeight::one(),
        );
        assert_eq!(trans2.net_stack_change(), 1);
    }

    #[test]
    fn test_map_input() {
        let trans: PdaTransition<i32, TropicalWeight> = PdaTransition::new(
            0,
            Some(5),
            StackSymbol::BOTTOM,
            StackAction::Noop,
            1,
            TropicalWeight::one(),
        );

        let mapped = trans.map_input(|&x| x * 2);
        assert_eq!(mapped.input, Some(10));
    }

    #[test]
    fn test_debug_format() {
        let trans: PdaTransition<char, TropicalWeight> = PdaTransition::new(
            0,
            Some('a'),
            StackSymbol::BOTTOM,
            StackAction::Pop,
            1,
            TropicalWeight::new(1.0),
        );

        let debug = format!("{:?}", trans);
        assert!(debug.contains("PdaTransition"));
        assert!(debug.contains("'a'"));
    }
}
