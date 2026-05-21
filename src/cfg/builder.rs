//! Grammar builder for convenient grammar construction.

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use super::grammar::{Grammar, GrammarError, Production};
use super::types::{NonTerminal, RuleId, Symbol, Terminal};

/// Builder for constructing grammars with a fluent API.
///
/// # Example
///
/// ```rust
/// use lling_llang::cfg::GrammarBuilder;
///
/// let grammar = GrammarBuilder::new()
///     .non_terminal("S")
///     .non_terminal("NP")
///     .non_terminal("VP")
///     .rule("S", &["NP", "VP"])
///     .rule("NP", &["the", "dog"])
///     .start("S")
///     .build()
///     .expect("valid grammar");
/// ```
#[derive(Debug, Default)]
pub struct GrammarBuilder {
    /// Non-terminal name to index mapping.
    nt_names: FxHashMap<String, NonTerminal>,
    /// Terminal name to ID mapping.
    terminal_names: FxHashMap<String, Terminal>,
    /// Next non-terminal index.
    next_nt: u16,
    /// Next terminal ID.
    next_terminal: u32,
    /// Productions being built.
    productions: Vec<Production>,
    /// Start symbol name.
    start: Option<String>,
}

impl GrammarBuilder {
    /// Create a new grammar builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a non-terminal symbol.
    ///
    /// This is optional - non-terminals used in rules are auto-declared.
    pub fn non_terminal(mut self, name: &str) -> Self {
        if !self.nt_names.contains_key(name) {
            let nt = NonTerminal::new(self.next_nt);
            self.nt_names.insert(name.to_string(), nt);
            self.next_nt += 1;
        }
        self
    }

    /// Declare multiple non-terminals.
    pub fn non_terminals(mut self, names: &[&str]) -> Self {
        for name in names {
            self = self.non_terminal(name);
        }
        self
    }

    /// Declare a terminal symbol with auto-generated ID.
    pub fn terminal(mut self, name: &str) -> Self {
        if !self.terminal_names.contains_key(name) {
            let t = Terminal::new(self.next_terminal);
            self.terminal_names.insert(name.to_string(), t);
            self.next_terminal += 1;
        }
        self
    }

    /// Declare a terminal symbol with a specific vocabulary ID.
    pub fn terminal_with_id(mut self, name: &str, vocab_id: u32) -> Self {
        let t = Terminal::new(vocab_id);
        self.terminal_names.insert(name.to_string(), t);
        self
    }

    /// Set the start symbol.
    pub fn start(mut self, name: &str) -> Self {
        self.start = Some(name.to_string());
        self
    }

    /// Add a production rule.
    ///
    /// The LHS is auto-declared as a non-terminal.
    /// RHS symbols starting with uppercase are treated as non-terminals,
    /// lowercase symbols are treated as terminals.
    pub fn rule(self, lhs: &str, rhs: &[&str]) -> Self {
        self.rule_with_prob(lhs, rhs, 0.0)
    }

    /// Add a production rule with a log probability.
    pub fn rule_with_prob(mut self, lhs: &str, rhs: &[&str], log_prob: f32) -> Self {
        // Ensure LHS is a non-terminal
        self = self.non_terminal(lhs);
        let lhs_nt = self.nt_names[lhs];

        // Build RHS
        let mut rhs_symbols: SmallVec<[Symbol; 4]> = SmallVec::new();

        for sym in rhs {
            if sym.is_empty() || *sym == "ε" {
                rhs_symbols.push(Symbol::epsilon());
            } else if self.is_non_terminal_name(sym) {
                // Non-terminal (uppercase start)
                self = self.non_terminal(sym);
                rhs_symbols.push(Symbol::NonTerminal(self.nt_names[*sym]));
            } else {
                // Terminal (lowercase start)
                self = self.terminal(sym);
                rhs_symbols.push(Symbol::Terminal(self.terminal_names[*sym]));
            }
        }

        let rule_id = RuleId::new(self.productions.len() as u32);
        self.productions.push(Production::with_prob(
            rule_id,
            lhs_nt,
            rhs_symbols,
            log_prob,
        ));
        self
    }

    /// Add an epsilon production (A → ε).
    pub fn epsilon_rule(self, lhs: &str) -> Self {
        self.rule(lhs, &["ε"])
    }

    /// Build the grammar.
    pub fn build(self) -> Result<Grammar, GrammarError> {
        let start_name = self.start.as_deref().ok_or(GrammarError::NoStartSymbol)?;
        let start_nt = self
            .nt_names
            .get(start_name)
            .copied()
            .ok_or_else(|| GrammarError::UndefinedNonTerminal(NonTerminal::new(0)))?;

        let mut grammar = Grammar::new(start_nt, self.productions, self.next_nt as usize)?;

        // Register names
        for (name, nt) in &self.nt_names {
            grammar.set_nt_name(*nt, name.clone());
        }

        for (name, terminal) in &self.terminal_names {
            grammar.register_terminal(name.clone(), *terminal);
        }

        Ok(grammar)
    }

    /// Check if a symbol name should be treated as a non-terminal.
    ///
    /// By default, symbols starting with uppercase are non-terminals.
    fn is_non_terminal_name(&self, name: &str) -> bool {
        name.chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
    }

    /// Get the non-terminal for a name, if it exists.
    pub fn get_non_terminal(&self, name: &str) -> Option<NonTerminal> {
        self.nt_names.get(name).copied()
    }

    /// Get the terminal for a name, if it exists.
    pub fn get_terminal(&self, name: &str) -> Option<Terminal> {
        self.terminal_names.get(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_simple() {
        let grammar = GrammarBuilder::new()
            .start("S")
            .rule("S", &["NP", "VP"])
            .rule("NP", &["Det", "N"])
            .rule("VP", &["V", "NP"])
            .rule("Det", &["the"])
            .rule("N", &["dog"])
            .rule("V", &["saw"])
            .build()
            .expect("valid grammar");

        assert_eq!(grammar.num_productions(), 6);
        assert_eq!(grammar.nt_name(grammar.start()), Some("S"));
    }

    #[test]
    fn test_builder_non_terminals() {
        let builder = GrammarBuilder::new().non_terminals(&["S", "A", "B"]);

        assert!(builder.get_non_terminal("S").is_some());
        assert!(builder.get_non_terminal("A").is_some());
        assert!(builder.get_non_terminal("B").is_some());
        assert!(builder.get_non_terminal("C").is_none());
    }

    #[test]
    fn test_builder_epsilon_rule() {
        let grammar = GrammarBuilder::new()
            .start("S")
            .rule("S", &["A"])
            .epsilon_rule("A")
            .build()
            .expect("valid grammar");

        assert_eq!(grammar.num_productions(), 2);

        // Find epsilon production
        let a_nt = grammar.terminal_by_name("A");
        assert!(a_nt.is_none()); // A is a non-terminal, not terminal
    }

    #[test]
    fn test_builder_with_probability() {
        let grammar = GrammarBuilder::new()
            .start("S")
            .rule_with_prob("S", &["a"], -0.5)
            .rule_with_prob("S", &["b"], -1.0)
            .build()
            .expect("valid grammar");

        let prods: Vec<_> = grammar.productions_for(grammar.start()).collect();
        assert_eq!(prods[0].log_prob, -0.5);
        assert_eq!(prods[1].log_prob, -1.0);
    }

    #[test]
    fn test_builder_terminal_with_id() {
        let grammar = GrammarBuilder::new()
            .terminal_with_id("the", 100)
            .terminal_with_id("dog", 200)
            .start("S")
            .rule("S", &["the", "dog"])
            .build()
            .expect("valid grammar");

        assert_eq!(grammar.terminal_by_name("the"), Some(Terminal::new(100)));
        assert_eq!(grammar.terminal_by_name("dog"), Some(Terminal::new(200)));
    }

    #[test]
    fn test_builder_auto_declaration() {
        // Non-terminals should be auto-declared from rules
        let grammar = GrammarBuilder::new()
            .start("S")
            .rule("S", &["A", "B"])
            .rule("A", &["a"])
            .rule("B", &["b"])
            .build()
            .expect("valid grammar");

        // Check A and B were auto-declared
        assert!(
            grammar.nt_name(NonTerminal::new(1)).is_some()
                || grammar.nt_name(NonTerminal::new(2)).is_some()
        );
    }

    #[test]
    fn test_builder_no_start_error() {
        let result = GrammarBuilder::new().rule("S", &["a"]).build();

        assert!(matches!(result, Err(GrammarError::NoStartSymbol)));
    }

    #[test]
    fn test_builder_terminal_names() {
        let grammar = GrammarBuilder::new()
            .start("S")
            .rule("S", &["the", "dog"])
            .build()
            .expect("valid grammar");

        assert!(grammar.terminal_by_name("the").is_some());
        assert!(grammar.terminal_by_name("dog").is_some());
    }
}
