//! Context-free grammar representation.

use std::fmt;

use smallvec::SmallVec;
use rustc_hash::FxHashMap;

use super::types::{NonTerminal, Terminal, RuleId, Symbol};

/// A production rule in the grammar.
///
/// Represents a rule of the form: LHS → RHS₁ RHS₂ ... RHSₙ
#[derive(Clone, Debug)]
pub struct Production {
    /// Rule identifier.
    pub id: RuleId,
    /// Left-hand side (non-terminal being defined).
    pub lhs: NonTerminal,
    /// Right-hand side (sequence of symbols).
    pub rhs: SmallVec<[Symbol; 4]>,
    /// Log probability of this production (for PCFG).
    pub log_prob: f32,
}

impl Production {
    /// Create a new production.
    pub fn new(id: RuleId, lhs: NonTerminal, rhs: SmallVec<[Symbol; 4]>) -> Self {
        Self {
            id,
            lhs,
            rhs,
            log_prob: 0.0,
        }
    }

    /// Create a production with a probability.
    pub fn with_prob(id: RuleId, lhs: NonTerminal, rhs: SmallVec<[Symbol; 4]>, log_prob: f32) -> Self {
        Self {
            id,
            lhs,
            rhs,
            log_prob,
        }
    }

    /// Check if this is an epsilon production (A → ε).
    pub fn is_epsilon(&self) -> bool {
        self.rhs.is_empty() || (self.rhs.len() == 1 && self.rhs[0].is_epsilon())
    }

    /// Get the length of the right-hand side.
    pub fn rhs_len(&self) -> usize {
        if self.is_epsilon() {
            0
        } else {
            self.rhs.len()
        }
    }

    /// Get the symbol at a given position in the RHS.
    pub fn rhs_at(&self, pos: usize) -> Option<&Symbol> {
        self.rhs.get(pos)
    }
}

impl fmt::Display for Production {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} →", self.lhs)?;
        if self.rhs.is_empty() {
            write!(f, " ε")?;
        } else {
            for sym in &self.rhs {
                write!(f, " {}", sym)?;
            }
        }
        Ok(())
    }
}

/// Error type for grammar operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrammarError {
    /// No start symbol defined.
    NoStartSymbol,
    /// Undefined non-terminal referenced.
    UndefinedNonTerminal(NonTerminal),
    /// Empty grammar (no productions).
    EmptyGrammar,
    /// Duplicate rule ID.
    DuplicateRuleId(RuleId),
}

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GrammarError::NoStartSymbol => write!(f, "no start symbol defined"),
            GrammarError::UndefinedNonTerminal(nt) => write!(f, "undefined non-terminal: {}", nt),
            GrammarError::EmptyGrammar => write!(f, "empty grammar (no productions)"),
            GrammarError::DuplicateRuleId(id) => write!(f, "duplicate rule ID: {}", id),
        }
    }
}

impl std::error::Error for GrammarError {}

/// A context-free grammar.
///
/// Stores productions and provides efficient lookup by LHS non-terminal.
#[derive(Clone, Debug)]
pub struct Grammar {
    /// Start symbol.
    pub start: NonTerminal,
    /// All productions indexed by rule ID.
    productions: Vec<Production>,
    /// Productions indexed by LHS non-terminal.
    by_lhs: Vec<SmallVec<[RuleId; 8]>>,
    /// Non-terminal name mapping (for display).
    nt_names: FxHashMap<NonTerminal, String>,
    /// Terminal to vocabulary ID mapping.
    terminal_vocab: FxHashMap<String, Terminal>,
    /// Vocabulary ID to terminal name mapping.
    vocab_names: FxHashMap<Terminal, String>,
    /// Number of non-terminals.
    num_non_terminals: usize,
}

impl Grammar {
    /// Create a new grammar.
    pub fn new(
        start: NonTerminal,
        productions: Vec<Production>,
        num_non_terminals: usize,
    ) -> Result<Self, GrammarError> {
        if productions.is_empty() {
            return Err(GrammarError::EmptyGrammar);
        }

        // Build by_lhs index
        let mut by_lhs: Vec<SmallVec<[RuleId; 8]>> = vec![SmallVec::new(); num_non_terminals];

        for prod in &productions {
            let nt_idx = prod.lhs.index() as usize;
            if nt_idx >= num_non_terminals {
                return Err(GrammarError::UndefinedNonTerminal(prod.lhs));
            }
            by_lhs[nt_idx].push(prod.id);
        }

        Ok(Self {
            start,
            productions,
            by_lhs,
            nt_names: FxHashMap::default(),
            terminal_vocab: FxHashMap::default(),
            vocab_names: FxHashMap::default(),
            num_non_terminals,
        })
    }

    /// Get the start symbol.
    pub fn start(&self) -> NonTerminal {
        self.start
    }

    /// Get a production by rule ID.
    pub fn production(&self, id: RuleId) -> Option<&Production> {
        self.productions.get(id.index() as usize)
    }

    /// Get all productions for a given LHS non-terminal.
    pub fn productions_for(&self, lhs: NonTerminal) -> impl Iterator<Item = &Production> {
        let nt_idx = lhs.index() as usize;
        self.by_lhs
            .get(nt_idx)
            .into_iter()
            .flat_map(|rules| rules.iter())
            .filter_map(|&id| self.production(id))
    }

    /// Get all productions.
    pub fn productions(&self) -> &[Production] {
        &self.productions
    }

    /// Get the number of productions.
    pub fn num_productions(&self) -> usize {
        self.productions.len()
    }

    /// Get the number of non-terminals.
    pub fn num_non_terminals(&self) -> usize {
        self.num_non_terminals
    }

    /// Set the name for a non-terminal (for display).
    pub fn set_nt_name(&mut self, nt: NonTerminal, name: String) {
        self.nt_names.insert(nt, name);
    }

    /// Get the name of a non-terminal.
    pub fn nt_name(&self, nt: NonTerminal) -> Option<&str> {
        self.nt_names.get(&nt).map(|s| s.as_str())
    }

    /// Register a terminal with its vocabulary ID and name.
    pub fn register_terminal(&mut self, name: String, terminal: Terminal) {
        self.terminal_vocab.insert(name.clone(), terminal);
        self.vocab_names.insert(terminal, name);
    }

    /// Look up a terminal by name.
    pub fn terminal_by_name(&self, name: &str) -> Option<Terminal> {
        self.terminal_vocab.get(name).copied()
    }

    /// Get the name of a terminal.
    pub fn terminal_name(&self, terminal: Terminal) -> Option<&str> {
        self.vocab_names.get(&terminal).map(|s| s.as_str())
    }

    /// Check if this grammar has nullable non-terminals (can derive ε).
    pub fn compute_nullable(&self) -> Vec<bool> {
        let mut nullable = vec![false; self.num_non_terminals];
        let mut changed = true;

        while changed {
            changed = false;
            for prod in &self.productions {
                if nullable[prod.lhs.index() as usize] {
                    continue;
                }

                // Check if RHS is nullable
                let rhs_nullable = prod.rhs.iter().all(|sym| match sym {
                    Symbol::Epsilon => true,
                    Symbol::Terminal(_) => false,
                    Symbol::NonTerminal(nt) => nullable[nt.index() as usize],
                });

                if rhs_nullable {
                    nullable[prod.lhs.index() as usize] = true;
                    changed = true;
                }
            }
        }

        nullable
    }

    /// Compute FIRST sets for all non-terminals.
    pub fn compute_first_sets(&self) -> Vec<FxHashMap<Terminal, bool>> {
        let nullable = self.compute_nullable();
        let mut first: Vec<FxHashMap<Terminal, bool>> = vec![FxHashMap::default(); self.num_non_terminals];
        let mut changed = true;

        while changed {
            changed = false;

            for prod in &self.productions {
                let lhs_idx = prod.lhs.index() as usize;

                for sym in &prod.rhs {
                    match sym {
                        Symbol::Terminal(t) => {
                            if !first[lhs_idx].contains_key(t) {
                                first[lhs_idx].insert(*t, true);
                                changed = true;
                            }
                            break; // Terminal found, stop
                        }
                        Symbol::NonTerminal(nt) => {
                            let nt_idx = nt.index() as usize;
                            // Add FIRST(nt) to FIRST(lhs)
                            for (&t, _) in &first[nt_idx].clone() {
                                if !first[lhs_idx].contains_key(&t) {
                                    first[lhs_idx].insert(t, true);
                                    changed = true;
                                }
                            }
                            // If nt is not nullable, stop
                            if !nullable[nt_idx] {
                                break;
                            }
                        }
                        Symbol::Epsilon => {
                            // Skip epsilon
                        }
                    }
                }
            }
        }

        first
    }
}

impl fmt::Display for Grammar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Grammar (start: {})", self.start)?;
        for prod in &self.productions {
            writeln!(f, "  {}", prod)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_grammar() -> Grammar {
        // S → NP VP
        // NP → Det N
        // VP → V NP
        let productions = vec![
            Production::new(
                RuleId::new(0),
                NonTerminal::new(0), // S
                smallvec::smallvec![Symbol::non_terminal(1), Symbol::non_terminal(2)], // NP VP
            ),
            Production::new(
                RuleId::new(1),
                NonTerminal::new(1), // NP
                smallvec::smallvec![Symbol::non_terminal(3), Symbol::non_terminal(4)], // Det N
            ),
            Production::new(
                RuleId::new(2),
                NonTerminal::new(2), // VP
                smallvec::smallvec![Symbol::non_terminal(5), Symbol::non_terminal(1)], // V NP
            ),
        ];

        Grammar::new(NonTerminal::new(0), productions, 6).expect("valid grammar")
    }

    #[test]
    fn test_grammar_creation() {
        let grammar = simple_grammar();
        assert_eq!(grammar.start(), NonTerminal::new(0));
        assert_eq!(grammar.num_productions(), 3);
        assert_eq!(grammar.num_non_terminals(), 6);
    }

    #[test]
    fn test_production_access() {
        let grammar = simple_grammar();

        let prod = grammar.production(RuleId::new(0)).expect("rule 0");
        assert_eq!(prod.lhs, NonTerminal::new(0));
        assert_eq!(prod.rhs.len(), 2);
    }

    #[test]
    fn test_productions_for_lhs() {
        let grammar = simple_grammar();

        let s_prods: Vec<_> = grammar.productions_for(NonTerminal::new(0)).collect();
        assert_eq!(s_prods.len(), 1);
        assert_eq!(s_prods[0].id, RuleId::new(0));
    }

    #[test]
    fn test_epsilon_production() {
        let prod = Production::new(
            RuleId::new(0),
            NonTerminal::new(0),
            SmallVec::new(),
        );
        assert!(prod.is_epsilon());
        assert_eq!(prod.rhs_len(), 0);

        let prod2 = Production::new(
            RuleId::new(1),
            NonTerminal::new(0),
            smallvec::smallvec![Symbol::epsilon()],
        );
        assert!(prod2.is_epsilon());
    }

    #[test]
    fn test_production_display() {
        let prod = Production::new(
            RuleId::new(0),
            NonTerminal::new(0),
            smallvec::smallvec![Symbol::non_terminal(1), Symbol::terminal(5)],
        );
        let display = format!("{}", prod);
        assert!(display.contains("NT(0)"));
        assert!(display.contains("→"));
    }

    #[test]
    fn test_empty_grammar_error() {
        let result = Grammar::new(NonTerminal::new(0), vec![], 1);
        assert!(matches!(result, Err(GrammarError::EmptyGrammar)));
    }

    #[test]
    fn test_nt_names() {
        let mut grammar = simple_grammar();
        grammar.set_nt_name(NonTerminal::new(0), "S".to_string());
        grammar.set_nt_name(NonTerminal::new(1), "NP".to_string());

        assert_eq!(grammar.nt_name(NonTerminal::new(0)), Some("S"));
        assert_eq!(grammar.nt_name(NonTerminal::new(1)), Some("NP"));
        assert_eq!(grammar.nt_name(NonTerminal::new(5)), None);
    }

    #[test]
    fn test_terminal_registration() {
        let mut grammar = simple_grammar();
        grammar.register_terminal("the".to_string(), Terminal::new(100));

        assert_eq!(grammar.terminal_by_name("the"), Some(Terminal::new(100)));
        assert_eq!(grammar.terminal_name(Terminal::new(100)), Some("the"));
        assert_eq!(grammar.terminal_by_name("unknown"), None);
    }

    #[test]
    fn test_nullable_computation() {
        // S → A B
        // A → ε
        // B → b
        let productions = vec![
            Production::new(
                RuleId::new(0),
                NonTerminal::new(0), // S
                smallvec::smallvec![Symbol::non_terminal(1), Symbol::non_terminal(2)],
            ),
            Production::new(
                RuleId::new(1),
                NonTerminal::new(1), // A → ε
                smallvec::smallvec![Symbol::epsilon()],
            ),
            Production::new(
                RuleId::new(2),
                NonTerminal::new(2), // B → b
                smallvec::smallvec![Symbol::terminal(0)],
            ),
        ];

        let grammar = Grammar::new(NonTerminal::new(0), productions, 3).expect("valid grammar");
        let nullable = grammar.compute_nullable();

        assert!(!nullable[0]); // S not nullable
        assert!(nullable[1]);  // A is nullable
        assert!(!nullable[2]); // B not nullable
    }
}
