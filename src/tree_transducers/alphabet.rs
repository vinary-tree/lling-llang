//! Ranked alphabets for tree transducers.
//!
//! A ranked alphabet assigns an arity (number of children) to each symbol.

use std::collections::HashMap;
use std::hash::Hash;

/// Trait for ranked alphabet symbols.
///
/// A ranked alphabet is a finite set of symbols where each symbol
/// has an associated arity (number of children).
pub trait RankedAlphabet: Clone + Eq + Hash + Send + Sync {
    /// Get the arity (number of children) for this symbol.
    fn arity(&self) -> usize;

    /// Check if this is a constant (arity 0).
    fn is_constant(&self) -> bool {
        self.arity() == 0
    }
}

/// A simple symbol with explicit arity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol<L> {
    /// The symbol label.
    pub label: L,
    /// The arity (number of children).
    pub arity: usize,
}

impl<L> Symbol<L> {
    /// Create a new symbol with the given label and arity.
    pub fn new(label: L, arity: usize) -> Self {
        Self { label, arity }
    }

    /// Create a constant symbol (arity 0).
    pub fn constant(label: L) -> Self {
        Self { label, arity: 0 }
    }

    /// Create a unary symbol (arity 1).
    pub fn unary(label: L) -> Self {
        Self { label, arity: 1 }
    }

    /// Create a binary symbol (arity 2).
    pub fn binary(label: L) -> Self {
        Self { label, arity: 2 }
    }
}

impl<L: Clone + Eq + Hash + Send + Sync> RankedAlphabet for Symbol<L> {
    fn arity(&self) -> usize {
        self.arity
    }
}

/// A simple alphabet that stores arities in a map.
#[derive(Debug, Clone)]
pub struct SimpleAlphabet<L: Eq + Hash> {
    /// Map from label to arity.
    arities: HashMap<L, usize>,
    /// Default arity for unknown symbols.
    default_arity: usize,
}

impl<L: Eq + Hash + Clone> SimpleAlphabet<L> {
    /// Create a new empty alphabet.
    pub fn new() -> Self {
        Self {
            arities: HashMap::new(),
            default_arity: 0,
        }
    }

    /// Create with a default arity for unknown symbols.
    pub fn with_default_arity(default_arity: usize) -> Self {
        Self {
            arities: HashMap::new(),
            default_arity,
        }
    }

    /// Add a symbol with its arity.
    pub fn add(&mut self, label: L, arity: usize) {
        self.arities.insert(label, arity);
    }

    /// Get the arity of a symbol.
    pub fn arity(&self, label: &L) -> usize {
        *self.arities.get(label).unwrap_or(&self.default_arity)
    }

    /// Check if the alphabet contains a symbol.
    pub fn contains(&self, label: &L) -> bool {
        self.arities.contains_key(label)
    }

    /// Get all symbols in the alphabet.
    pub fn symbols(&self) -> impl Iterator<Item = (&L, &usize)> {
        self.arities.iter()
    }

    /// Get the number of symbols.
    pub fn len(&self) -> usize {
        self.arities.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.arities.is_empty()
    }
}

impl<L: Eq + Hash + Clone> Default for SimpleAlphabet<L> {
    fn default() -> Self {
        Self::new()
    }
}

/// Implement RankedAlphabet for String (assumes arity stored elsewhere).
impl RankedAlphabet for String {
    fn arity(&self) -> usize {
        // Default implementation - arity should be tracked separately
        // or derived from context
        0
    }
}

/// Implement RankedAlphabet for &str.
impl RankedAlphabet for &str {
    fn arity(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_creation() {
        let s = Symbol::new("S", 2);
        assert_eq!(s.label, "S");
        assert_eq!(s.arity(), 2);
        assert!(!s.is_constant());
    }

    #[test]
    fn test_symbol_constant() {
        let c = Symbol::constant("x");
        assert_eq!(c.arity(), 0);
        assert!(c.is_constant());
    }

    #[test]
    fn test_symbol_unary_binary() {
        let u = Symbol::unary("NOT");
        assert_eq!(u.arity(), 1);

        let b = Symbol::binary("AND");
        assert_eq!(b.arity(), 2);
    }

    #[test]
    fn test_simple_alphabet() {
        let mut alphabet: SimpleAlphabet<&str> = SimpleAlphabet::new();
        alphabet.add("S", 2);
        alphabet.add("NP", 2);
        alphabet.add("VP", 1);
        alphabet.add("N", 0);

        assert_eq!(alphabet.arity(&"S"), 2);
        assert_eq!(alphabet.arity(&"N"), 0);
        assert_eq!(alphabet.arity(&"unknown"), 0); // default arity
    }

    #[test]
    fn test_alphabet_with_default() {
        let mut alphabet: SimpleAlphabet<&str> = SimpleAlphabet::with_default_arity(1);
        alphabet.add("S", 2);

        assert_eq!(alphabet.arity(&"S"), 2);
        assert_eq!(alphabet.arity(&"unknown"), 1);
    }

    #[test]
    fn test_alphabet_symbols() {
        let mut alphabet: SimpleAlphabet<&str> = SimpleAlphabet::new();
        alphabet.add("A", 0);
        alphabet.add("B", 1);
        alphabet.add("C", 2);

        assert_eq!(alphabet.len(), 3);
        assert!(!alphabet.is_empty());
        assert!(alphabet.contains(&"A"));
        assert!(!alphabet.contains(&"D"));
    }
}
