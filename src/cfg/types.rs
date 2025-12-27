//! Core types for context-free grammars.

use std::fmt;

/// Non-terminal symbol identifier.
///
/// Non-terminals represent syntactic categories (e.g., S, NP, VP).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NonTerminal(pub u16);

impl NonTerminal {
    /// Create a new non-terminal with the given index.
    pub fn new(index: u16) -> Self {
        Self(index)
    }

    /// Get the index of this non-terminal.
    pub fn index(&self) -> u16 {
        self.0
    }
}

impl fmt::Display for NonTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NT({})", self.0)
    }
}

/// Terminal symbol identifier.
///
/// Terminals represent actual words/tokens in the input.
/// The value is a vocabulary ID from the lattice backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Terminal(pub u32);

impl Terminal {
    /// Create a new terminal with the given vocabulary ID.
    pub fn new(vocab_id: u32) -> Self {
        Self(vocab_id)
    }

    /// Get the vocabulary ID of this terminal.
    pub fn vocab_id(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for Terminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T({})", self.0)
    }
}

/// Production rule identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuleId(pub u32);

impl RuleId {
    /// Create a new rule ID.
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the rule index.
    pub fn index(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "R{}", self.0)
    }
}

/// Kind of a grammar symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// Non-terminal symbol.
    NonTerminal,
    /// Terminal symbol.
    Terminal,
    /// Epsilon (empty string).
    Epsilon,
}

/// A symbol in a production rule's right-hand side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Symbol {
    /// Non-terminal symbol.
    NonTerminal(NonTerminal),
    /// Terminal symbol (word/token).
    Terminal(Terminal),
    /// Epsilon (empty production).
    Epsilon,
}

impl Symbol {
    /// Create a non-terminal symbol.
    pub fn non_terminal(index: u16) -> Self {
        Symbol::NonTerminal(NonTerminal::new(index))
    }

    /// Create a terminal symbol.
    pub fn terminal(vocab_id: u32) -> Self {
        Symbol::Terminal(Terminal::new(vocab_id))
    }

    /// Create an epsilon symbol.
    pub fn epsilon() -> Self {
        Symbol::Epsilon
    }

    /// Get the kind of this symbol.
    pub fn kind(&self) -> SymbolKind {
        match self {
            Symbol::NonTerminal(_) => SymbolKind::NonTerminal,
            Symbol::Terminal(_) => SymbolKind::Terminal,
            Symbol::Epsilon => SymbolKind::Epsilon,
        }
    }

    /// Check if this is a non-terminal.
    pub fn is_non_terminal(&self) -> bool {
        matches!(self, Symbol::NonTerminal(_))
    }

    /// Check if this is a terminal.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Symbol::Terminal(_))
    }

    /// Check if this is epsilon.
    pub fn is_epsilon(&self) -> bool {
        matches!(self, Symbol::Epsilon)
    }

    /// Get the non-terminal, if this is one.
    pub fn as_non_terminal(&self) -> Option<NonTerminal> {
        match self {
            Symbol::NonTerminal(nt) => Some(*nt),
            _ => None,
        }
    }

    /// Get the terminal, if this is one.
    pub fn as_terminal(&self) -> Option<Terminal> {
        match self {
            Symbol::Terminal(t) => Some(*t),
            _ => None,
        }
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Symbol::NonTerminal(nt) => write!(f, "{}", nt),
            Symbol::Terminal(t) => write!(f, "{}", t),
            Symbol::Epsilon => write!(f, "ε"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_terminal() {
        let nt = NonTerminal::new(5);
        assert_eq!(nt.index(), 5);
        assert_eq!(format!("{}", nt), "NT(5)");
    }

    #[test]
    fn test_terminal() {
        let t = Terminal::new(42);
        assert_eq!(t.vocab_id(), 42);
        assert_eq!(format!("{}", t), "T(42)");
    }

    #[test]
    fn test_rule_id() {
        let r = RuleId::new(10);
        assert_eq!(r.index(), 10);
        assert_eq!(format!("{}", r), "R10");
    }

    #[test]
    fn test_symbol_creation() {
        let nt = Symbol::non_terminal(3);
        let t = Symbol::terminal(7);
        let eps = Symbol::epsilon();

        assert!(nt.is_non_terminal());
        assert!(!nt.is_terminal());
        assert!(!nt.is_epsilon());

        assert!(!t.is_non_terminal());
        assert!(t.is_terminal());
        assert!(!t.is_epsilon());

        assert!(!eps.is_non_terminal());
        assert!(!eps.is_terminal());
        assert!(eps.is_epsilon());
    }

    #[test]
    fn test_symbol_kind() {
        assert_eq!(Symbol::non_terminal(0).kind(), SymbolKind::NonTerminal);
        assert_eq!(Symbol::terminal(0).kind(), SymbolKind::Terminal);
        assert_eq!(Symbol::epsilon().kind(), SymbolKind::Epsilon);
    }

    #[test]
    fn test_symbol_extraction() {
        let nt = Symbol::NonTerminal(NonTerminal::new(5));
        let t = Symbol::Terminal(Terminal::new(10));

        assert_eq!(nt.as_non_terminal(), Some(NonTerminal::new(5)));
        assert_eq!(nt.as_terminal(), None);

        assert_eq!(t.as_non_terminal(), None);
        assert_eq!(t.as_terminal(), Some(Terminal::new(10)));
    }

    #[test]
    fn test_symbol_display() {
        assert_eq!(format!("{}", Symbol::non_terminal(1)), "NT(1)");
        assert_eq!(format!("{}", Symbol::terminal(2)), "T(2)");
        assert_eq!(format!("{}", Symbol::epsilon()), "ε");
    }
}
