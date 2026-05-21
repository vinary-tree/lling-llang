//! Stack symbols and actions for pushdown automata.

use std::fmt::{self, Debug, Display};

/// A symbol in the stack alphabet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StackSymbol(pub u32);

impl StackSymbol {
    /// The bottom-of-stack marker (Z₀).
    pub const BOTTOM: StackSymbol = StackSymbol(0);

    /// Create a new stack symbol with the given ID.
    pub fn new(id: u32) -> Self {
        StackSymbol(id)
    }

    /// Get the ID of this symbol.
    pub fn id(&self) -> u32 {
        self.0
    }

    /// Check if this is the bottom-of-stack marker.
    pub fn is_bottom(&self) -> bool {
        *self == Self::BOTTOM
    }
}

impl Display for StackSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_bottom() {
            write!(f, "Z₀")
        } else {
            write!(f, "γ{}", self.0)
        }
    }
}

impl From<u32> for StackSymbol {
    fn from(id: u32) -> Self {
        StackSymbol(id)
    }
}

/// An action to perform on the stack during a transition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StackAction {
    /// Pop the top symbol (the matched symbol).
    Pop,

    /// Push symbols onto the stack (rightmost ends up on top).
    /// After matching and popping the stack top, push these symbols.
    Push(Vec<StackSymbol>),

    /// Replace the top symbol with a sequence of symbols.
    /// Equivalent to Pop followed by Push, but more efficient.
    Replace(Vec<StackSymbol>),

    /// Leave the stack unchanged.
    /// The matched symbol remains on top.
    Noop,
}

impl StackAction {
    /// Create a push action with a single symbol.
    pub fn push_one(symbol: StackSymbol) -> Self {
        StackAction::Push(vec![symbol])
    }

    /// Create a push action with two symbols.
    pub fn push_two(bottom: StackSymbol, top: StackSymbol) -> Self {
        StackAction::Push(vec![bottom, top])
    }

    /// Create a replace action with a single symbol.
    pub fn replace_one(symbol: StackSymbol) -> Self {
        StackAction::Replace(vec![symbol])
    }

    /// Check if this action pops the stack.
    pub fn pops(&self) -> bool {
        match self {
            StackAction::Pop => true,
            StackAction::Push(_) | StackAction::Replace(_) => true,
            StackAction::Noop => false,
        }
    }

    /// Check if this is a no-op.
    pub fn is_noop(&self) -> bool {
        matches!(self, StackAction::Noop)
    }

    /// Get the net stack change (negative = shrinks, positive = grows).
    pub fn net_change(&self) -> i32 {
        match self {
            StackAction::Pop => -1,
            StackAction::Push(symbols) => symbols.len() as i32 - 1,
            StackAction::Replace(symbols) => symbols.len() as i32 - 1,
            StackAction::Noop => 0,
        }
    }

    /// Apply this action to a stack (represented as a Vec with top at end).
    pub fn apply(&self, stack: &mut Vec<StackSymbol>) -> bool {
        if stack.is_empty() {
            return false;
        }

        match self {
            StackAction::Pop => {
                stack.pop();
                true
            }
            StackAction::Push(symbols) => {
                stack.pop(); // Pop the matched symbol
                stack.extend(symbols.iter().cloned());
                true
            }
            StackAction::Replace(symbols) => {
                stack.pop(); // Pop the matched symbol
                stack.extend(symbols.iter().cloned());
                true
            }
            StackAction::Noop => {
                // Leave stack unchanged
                true
            }
        }
    }
}

impl Display for StackAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StackAction::Pop => write!(f, "pop"),
            StackAction::Push(symbols) => {
                write!(f, "push[")?;
                for (i, s) in symbols.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", s)?;
                }
                write!(f, "]")
            }
            StackAction::Replace(symbols) => {
                write!(f, "replace[")?;
                for (i, s) in symbols.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", s)?;
                }
                write!(f, "]")
            }
            StackAction::Noop => write!(f, "noop"),
        }
    }
}

impl Default for StackAction {
    fn default() -> Self {
        StackAction::Noop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_symbol_creation() {
        let sym = StackSymbol::new(5);
        assert_eq!(sym.id(), 5);
        assert!(!sym.is_bottom());
    }

    #[test]
    fn test_stack_symbol_bottom() {
        let bottom = StackSymbol::BOTTOM;
        assert!(bottom.is_bottom());
        assert_eq!(bottom.id(), 0);
    }

    #[test]
    fn test_stack_symbol_display() {
        assert_eq!(format!("{}", StackSymbol::BOTTOM), "Z₀");
        assert_eq!(format!("{}", StackSymbol::new(3)), "γ3");
    }

    #[test]
    fn test_stack_action_pop() {
        let mut stack = vec![StackSymbol::BOTTOM, StackSymbol::new(1)];
        let action = StackAction::Pop;

        assert!(action.apply(&mut stack));
        assert_eq!(stack, vec![StackSymbol::BOTTOM]);
        assert_eq!(action.net_change(), -1);
    }

    #[test]
    fn test_stack_action_push() {
        let mut stack = vec![StackSymbol::BOTTOM];
        let action = StackAction::Push(vec![StackSymbol::new(1), StackSymbol::new(2)]);

        assert!(action.apply(&mut stack));
        assert_eq!(stack, vec![StackSymbol::new(1), StackSymbol::new(2)]);
        assert_eq!(action.net_change(), 1);
    }

    #[test]
    fn test_stack_action_replace() {
        let mut stack = vec![StackSymbol::BOTTOM, StackSymbol::new(1)];
        let action = StackAction::Replace(vec![StackSymbol::new(2), StackSymbol::new(3)]);

        assert!(action.apply(&mut stack));
        assert_eq!(
            stack,
            vec![
                StackSymbol::BOTTOM,
                StackSymbol::new(2),
                StackSymbol::new(3)
            ]
        );
    }

    #[test]
    fn test_stack_action_noop() {
        let mut stack = vec![StackSymbol::BOTTOM, StackSymbol::new(1)];
        let original = stack.clone();
        let action = StackAction::Noop;

        assert!(action.apply(&mut stack));
        assert_eq!(stack, original);
        assert_eq!(action.net_change(), 0);
    }

    #[test]
    fn test_stack_action_display() {
        assert_eq!(format!("{}", StackAction::Pop), "pop");
        assert_eq!(format!("{}", StackAction::Noop), "noop");
        assert_eq!(
            format!("{}", StackAction::Push(vec![StackSymbol::new(1)])),
            "push[γ1]"
        );
    }

    #[test]
    fn test_net_change() {
        assert_eq!(StackAction::Pop.net_change(), -1);
        assert_eq!(StackAction::Noop.net_change(), 0);
        assert_eq!(
            StackAction::Push(vec![StackSymbol::new(1), StackSymbol::new(2)]).net_change(),
            1
        );
        assert_eq!(
            StackAction::Replace(vec![StackSymbol::new(1)]).net_change(),
            0
        );
    }

    #[test]
    fn test_empty_stack() {
        let mut stack: Vec<StackSymbol> = vec![];
        assert!(!StackAction::Pop.apply(&mut stack));
    }

    #[test]
    fn test_push_one() {
        let action = StackAction::push_one(StackSymbol::new(5));
        assert_eq!(action, StackAction::Push(vec![StackSymbol::new(5)]));
    }

    #[test]
    fn test_push_two() {
        let action = StackAction::push_two(StackSymbol::new(1), StackSymbol::new(2));
        assert_eq!(
            action,
            StackAction::Push(vec![StackSymbol::new(1), StackSymbol::new(2)])
        );
    }
}
