//! Earley parser for lattice input.
//!
//! The Earley parser is modified to work with lattices instead of strings:
//! - Scanner follows lattice edges instead of string positions
//! - Multiple edges at a position are handled naturally
//! - Chart positions correspond to lattice nodes

use std::collections::VecDeque;
use std::fmt;
use std::hash::{Hash, Hasher};

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use super::types::{NonTerminal, Terminal, RuleId, Symbol};
use super::grammar::Grammar;
use super::forest::{ParseForest, ForestNodeId, ForestNode, ForestChild};

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, NodeId, EdgeId};
use crate::semiring::Semiring;

/// An Earley item (chart state).
///
/// Represents a partially-matched production rule.
///
/// Note: Hash and Eq are implemented manually to only compare (rule, dot, start).
/// The forest_node, terminal_edges, and child_nodes fields are metadata and don't affect state identity.
#[derive(Clone, Debug)]
pub struct EarleyState {
    /// The production rule.
    pub rule: RuleId,
    /// Position of the dot (how much has been matched).
    pub dot: usize,
    /// Starting position (lattice node) of this rule.
    pub start: NodeId,
    /// Parse forest back-pointers for completed constituents (deprecated, kept for compatibility).
    pub forest_node: Option<ForestNodeId>,
    /// Terminal edges consumed during scanning (in order of consumption).
    pub terminal_edges: SmallVec<[EdgeId; 4]>,
    /// Child forest nodes accumulated as we match RHS symbols (non-terminals and terminals).
    pub child_nodes: SmallVec<[ForestChild; 4]>,
}

impl PartialEq for EarleyState {
    fn eq(&self, other: &Self) -> bool {
        // State identity is based on (rule, dot, start) only
        self.rule == other.rule && self.dot == other.dot && self.start == other.start
    }
}

impl Eq for EarleyState {}

impl Hash for EarleyState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash only (rule, dot, start) for chart deduplication
        self.rule.hash(state);
        self.dot.hash(state);
        self.start.hash(state);
    }
}

impl EarleyState {
    /// Create a new Earley state.
    pub fn new(rule: RuleId, dot: usize, start: NodeId) -> Self {
        Self {
            rule,
            dot,
            start,
            forest_node: None,
            terminal_edges: SmallVec::new(),
            child_nodes: SmallVec::new(),
        }
    }

    /// Create a state with a forest node.
    pub fn with_forest(rule: RuleId, dot: usize, start: NodeId, forest: ForestNodeId) -> Self {
        Self {
            rule,
            dot,
            start,
            forest_node: Some(forest),
            terminal_edges: SmallVec::new(),
            child_nodes: SmallVec::new(),
        }
    }

    /// Advance the dot by one position.
    pub fn advance(&self) -> Self {
        Self {
            rule: self.rule,
            dot: self.dot + 1,
            start: self.start,
            forest_node: self.forest_node,
            terminal_edges: self.terminal_edges.clone(),
            child_nodes: self.child_nodes.clone(),
        }
    }

    /// Advance the dot by one position after consuming a terminal edge.
    pub fn advance_with_terminal(&self, edge_id: EdgeId) -> Self {
        let mut terminal_edges = self.terminal_edges.clone();
        terminal_edges.push(edge_id);
        let mut child_nodes = self.child_nodes.clone();
        child_nodes.push(ForestChild::Terminal(edge_id));
        Self {
            rule: self.rule,
            dot: self.dot + 1,
            start: self.start,
            forest_node: self.forest_node,
            terminal_edges,
            child_nodes,
        }
    }

    /// Advance the dot by one position after matching a non-terminal.
    pub fn advance_with_nonterminal(&self, child_forest_node: ForestNodeId) -> Self {
        let mut child_nodes = self.child_nodes.clone();
        child_nodes.push(ForestChild::Derivation(smallvec::smallvec![child_forest_node]));
        Self {
            rule: self.rule,
            dot: self.dot + 1,
            start: self.start,
            forest_node: Some(child_forest_node),
            terminal_edges: self.terminal_edges.clone(),
            child_nodes,
        }
    }

    /// Check if the rule is complete (dot at end).
    pub fn is_complete(&self, grammar: &Grammar) -> bool {
        let prod = grammar.production(self.rule).expect("valid rule");
        self.dot >= prod.rhs_len()
    }

    /// Get the symbol after the dot, if any.
    pub fn next_symbol<'a>(&self, grammar: &'a Grammar) -> Option<&'a Symbol> {
        let prod = grammar.production(self.rule)?;
        prod.rhs_at(self.dot)
    }

    /// Get the LHS of this state's rule.
    pub fn lhs(&self, grammar: &Grammar) -> NonTerminal {
        grammar.production(self.rule).expect("valid rule").lhs
    }
}

impl fmt::Display for EarleyState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}, {}, {}, {:?}, edges: {}]",
            self.rule, self.dot, self.start.0, self.forest_node, self.terminal_edges.len())
    }
}

/// Key for Earley chart lookup (uses rule, dot, start for identity).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct EarleyKey {
    rule: RuleId,
    dot: usize,
    start: NodeId,
}

impl From<&EarleyState> for EarleyKey {
    fn from(state: &EarleyState) -> Self {
        Self {
            rule: state.rule,
            dot: state.dot,
            start: state.start,
        }
    }
}

/// Earley chart: collection of items indexed by position.
#[derive(Clone, Debug)]
pub struct EarleyChart {
    /// Items at each position (lattice node), keyed for deduplication with child merging.
    positions: FxHashMap<NodeId, FxHashMap<EarleyKey, EarleyState>>,
    /// Agenda for processing.
    agenda: VecDeque<(NodeId, EarleyState)>,
    /// Set of (pos, key) pairs already processed to avoid re-adding to agenda.
    processed: FxHashSet<(NodeId, EarleyKey)>,
}

impl EarleyChart {
    /// Create a new empty chart.
    pub fn new() -> Self {
        Self {
            positions: FxHashMap::default(),
            agenda: VecDeque::new(),
            processed: FxHashSet::default(),
        }
    }

    /// Add an item to the chart at a position.
    ///
    /// Returns true if the item was new (not a duplicate).
    /// If the item already exists, children are merged.
    pub fn add(&mut self, pos: NodeId, state: EarleyState) -> bool {
        let key = EarleyKey::from(&state);
        let map = self.positions.entry(pos).or_default();

        if let Some(existing) = map.get_mut(&key) {
            // Merge children from the new state into the existing one
            for child in state.child_nodes {
                if !existing.child_nodes.contains(&child) {
                    existing.child_nodes.push(child);
                }
            }
            for edge in state.terminal_edges {
                if !existing.terminal_edges.contains(&edge) {
                    existing.terminal_edges.push(edge);
                }
            }
            // Update forest_node if the new one is set
            if state.forest_node.is_some() && existing.forest_node.is_none() {
                existing.forest_node = state.forest_node;
            }
            false // Not a new state
        } else {
            map.insert(key.clone(), state.clone());
            // Only add to agenda if not already processed
            if !self.processed.contains(&(pos, key.clone())) {
                self.processed.insert((pos, key));
                self.agenda.push_back((pos, state));
            }
            true
        }
    }

    /// Get all items at a position.
    pub fn at(&self, pos: NodeId) -> impl Iterator<Item = &EarleyState> {
        self.positions.get(&pos).into_iter().flat_map(|s| s.values())
    }

    /// Pop the next item from the agenda.
    pub fn pop(&mut self) -> Option<(NodeId, EarleyState)> {
        self.agenda.pop_front()
    }

    /// Check if the agenda is empty.
    pub fn is_agenda_empty(&self) -> bool {
        self.agenda.is_empty()
    }

    /// Get the number of items in the chart.
    pub fn len(&self) -> usize {
        self.positions.values().map(|s| s.len()).sum()
    }

    /// Check if the chart is empty.
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

impl Default for EarleyChart {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// No complete parse found.
    NoParse,
    /// Empty lattice.
    EmptyLattice,
    /// Grammar error.
    GrammarError(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NoParse => write!(f, "no complete parse found"),
            ParseError::EmptyLattice => write!(f, "empty lattice"),
            ParseError::GrammarError(msg) => write!(f, "grammar error: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

/// Earley parser for lattices.
pub struct EarleyParser<'g> {
    grammar: &'g Grammar,
    /// Nullable non-terminals (can derive epsilon).
    nullable: Vec<bool>,
}

impl<'g> EarleyParser<'g> {
    /// Create a new parser for the given grammar.
    pub fn new(grammar: &'g Grammar) -> Self {
        let nullable = grammar.compute_nullable();
        Self { grammar, nullable }
    }

    /// Parse a lattice and return a parse forest.
    pub fn parse_lattice<W: Semiring, B: LatticeBackend>(
        &self,
        lattice: &Lattice<W, B>,
    ) -> Result<ParseForest, ParseError> {
        if lattice.is_empty() && lattice.start() != lattice.end() {
            return Err(ParseError::EmptyLattice);
        }

        let mut chart = EarleyChart::new();
        let mut forest = ParseForest::new();

        let start = lattice.start();

        // Initialize: Add S → •α for all S-productions
        self.predict(&mut chart, start, self.grammar.start());

        // Process agenda
        while let Some((pos, state)) = chart.pop() {
            if state.is_complete(self.grammar) {
                // Completer
                self.complete(&mut chart, &mut forest, lattice, pos, &state);
            } else {
                let next_sym = state.next_symbol(self.grammar);
                match next_sym {
                    Some(Symbol::NonTerminal(nt)) => {
                        // Predictor
                        self.predict(&mut chart, pos, *nt);

                        // Handle nullable non-terminals (epsilon completion)
                        if self.nullable[nt.index() as usize] {
                            let advanced = state.advance();
                            chart.add(pos, advanced);
                        }
                    }
                    Some(Symbol::Terminal(terminal)) => {
                        // Scanner
                        self.scan(&mut chart, lattice, pos, &state, *terminal);
                    }
                    Some(Symbol::Epsilon) => {
                        // Skip epsilon
                        let advanced = state.advance();
                        chart.add(pos, advanced);
                    }
                    None => {
                        // Already complete, handled above
                    }
                }
            }
        }

        // Check for successful parse by looking at the forest roots
        // (roots are now added in complete() when start-symbol rules finish)
        if !forest.is_empty() {
            Ok(forest)
        } else {
            Err(ParseError::NoParse)
        }
    }

    /// Predictor: Add items for productions of a non-terminal.
    fn predict(&self, chart: &mut EarleyChart, pos: NodeId, nt: NonTerminal) {
        for prod in self.grammar.productions_for(nt) {
            let state = EarleyState::new(prod.id, 0, pos);
            chart.add(pos, state);
        }
    }

    /// Scanner: Match a terminal against lattice edges.
    fn scan<W: Semiring, B: LatticeBackend>(
        &self,
        chart: &mut EarleyChart,
        lattice: &Lattice<W, B>,
        pos: NodeId,
        state: &EarleyState,
        terminal: Terminal,
    ) {
        // Look for matching edges
        for edge in lattice.outgoing_edges(pos) {
            if edge.label == terminal.vocab_id() {
                // Advance and track the edge that was consumed
                let advanced = state.advance_with_terminal(edge.id);
                chart.add(edge.target, advanced);
            }
        }
    }

    /// Completer: When a rule completes, advance waiting rules.
    fn complete<W: Semiring, B: LatticeBackend>(
        &self,
        chart: &mut EarleyChart,
        forest: &mut ParseForest,
        lattice: &Lattice<W, B>,
        pos: NodeId,
        completed: &EarleyState,
    ) {
        let completed_nt = completed.lhs(self.grammar);

        // Create forest node for completed rule
        let mut node = ForestNode::new(
            completed.rule,
            completed.start,
            pos,
        );

        // Add all children accumulated during parsing this rule
        node.children = completed.child_nodes.clone();

        let forest_node = forest.add_node(node);

        // If this is a start-symbol rule spanning the entire input, add as root
        if completed_nt == self.grammar.start() &&
           completed.start == lattice.start() &&
           pos == lattice.end() {
            forest.add_root(forest_node);
        }

        // Find items waiting for this non-terminal
        let waiting: Vec<_> = chart.at(completed.start)
            .filter(|s| {
                !s.is_complete(self.grammar) &&
                matches!(s.next_symbol(self.grammar), Some(Symbol::NonTerminal(nt)) if *nt == completed_nt)
            })
            .cloned()
            .collect();

        for waiter in waiting {
            // Advance with the completed non-terminal's forest node
            let advanced = waiter.advance_with_nonterminal(forest_node);
            chart.add(pos, advanced);
        }
    }

    /// Check if the grammar accepts the lattice.
    pub fn accepts<W: Semiring, B: LatticeBackend>(
        &self,
        lattice: &Lattice<W, B>,
    ) -> bool {
        self.parse_lattice(lattice).is_ok()
    }

    /// Get the grammar.
    pub fn grammar(&self) -> &Grammar {
        self.grammar
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::{LatticeBuilder, EdgeMetadata};
    use crate::semiring::TropicalWeight;
    use crate::cfg::GrammarBuilder;

    fn simple_grammar() -> Grammar {
        // S → NP VP
        // NP → Det N
        // VP → V NP | V
        // Det → "the" | "a"
        // N → "dog" | "cat"
        // V → "saw" | "chased"
        GrammarBuilder::new()
            .start("S")
            .rule("S", &["NP", "VP"])
            .rule("NP", &["Det", "N"])
            .rule("VP", &["V", "NP"])
            .rule("VP", &["V"])
            .rule("Det", &["the"])
            .rule("Det", &["a"])
            .rule("N", &["dog"])
            .rule("N", &["cat"])
            .rule("V", &["saw"])
            .rule("V", &["chased"])
            .build()
            .expect("valid grammar")
    }

    fn build_lattice(words: &[&str], grammar: &Grammar) -> Lattice<TropicalWeight, HashMapBackend> {
        let mut backend = HashMapBackend::new();

        // Get terminal IDs from grammar and also intern words in backend
        let word_ids: Vec<_> = words.iter().map(|w| {
            // Get the terminal ID from grammar (this is what the parser expects)
            let t = grammar.terminal_by_name(w).expect(&format!("unknown word: {}", w));
            // Intern in backend for lookup purposes
            let _id = backend.intern(w);
            // Use grammar's terminal ID for the edge
            t.vocab_id()
        }).collect();

        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        for (i, &id) in word_ids.iter().enumerate() {
            builder.add_correction_by_id(
                i,
                i + 1,
                id,
                TropicalWeight::one(),
                EdgeMetadata::default(),
            );
        }

        builder.build(words.len())
    }

    #[test]
    fn test_earley_state() {
        let state = EarleyState::new(RuleId::new(0), 1, NodeId(0));
        assert_eq!(state.rule, RuleId::new(0));
        assert_eq!(state.dot, 1);
        assert_eq!(state.start, NodeId(0));
        assert!(state.forest_node.is_none());

        let advanced = state.advance();
        assert_eq!(advanced.dot, 2);
    }

    #[test]
    fn test_earley_chart() {
        let mut chart = EarleyChart::new();
        assert!(chart.is_empty());

        let state = EarleyState::new(RuleId::new(0), 0, NodeId(0));
        assert!(chart.add(NodeId(0), state.clone()));
        assert!(!chart.add(NodeId(0), state.clone())); // Duplicate

        assert_eq!(chart.len(), 1);
        assert!(!chart.is_agenda_empty());

        let (pos, s) = chart.pop().expect("item");
        assert_eq!(pos, NodeId(0));
        assert_eq!(s.rule, RuleId::new(0));
    }

    #[test]
    fn test_parse_simple_sentence() {
        let grammar = simple_grammar();
        let parser = EarleyParser::new(&grammar);

        // "the dog saw a cat"
        let lattice = build_lattice(&["the", "dog", "saw", "a", "cat"], &grammar);
        let result = parser.parse_lattice(&lattice);

        assert!(result.is_ok(), "Parse should succeed: {:?}", result);
    }

    #[test]
    fn test_parse_intransitive() {
        let grammar = simple_grammar();
        let parser = EarleyParser::new(&grammar);

        // "the dog saw" (intransitive use)
        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);
        let result = parser.parse_lattice(&lattice);

        assert!(result.is_ok(), "Parse should succeed: {:?}", result);
    }

    #[test]
    fn test_parse_failure() {
        let grammar = simple_grammar();
        let parser = EarleyParser::new(&grammar);

        // "saw the" - not a valid sentence
        let mut backend = HashMapBackend::new();
        // Intern for backend lookup
        let _saw_interned = backend.intern("saw");
        let _the_interned = backend.intern("the");

        // Use grammar's terminal IDs for edges (what the parser expects)
        let saw_id = grammar.terminal_by_name("saw").expect("saw").vocab_id();
        let the_id = grammar.terminal_by_name("the").expect("the").vocab_id();

        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        builder.add_correction_by_id(0, 1, saw_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, the_id, TropicalWeight::one(), EdgeMetadata::default());
        let lattice = builder.build(2);

        let result = parser.parse_lattice(&lattice);
        assert!(matches!(result, Err(ParseError::NoParse)));
    }

    #[test]
    fn test_accepts() {
        let grammar = simple_grammar();
        let parser = EarleyParser::new(&grammar);

        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);
        assert!(parser.accepts(&lattice));
    }

    #[test]
    fn test_nullable_handling() {
        // Grammar with nullable: S → A B, A → ε | "a"
        let grammar = GrammarBuilder::new()
            .start("S")
            .rule("S", &["A", "B"])
            .epsilon_rule("A")
            .rule("A", &["a"])
            .rule("B", &["b"])
            .build()
            .expect("valid grammar");

        let parser = EarleyParser::new(&grammar);

        // "b" should parse (A → ε)
        let mut backend = HashMapBackend::new();
        let _b_interned = backend.intern("b");
        // Use grammar's terminal ID for the edge
        let b_id = grammar.terminal_by_name("b").expect("b").vocab_id();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        builder.add_correction_by_id(0, 1, b_id, TropicalWeight::one(), EdgeMetadata::default());
        let lattice = builder.build(1);

        let result = parser.parse_lattice(&lattice);
        assert!(result.is_ok(), "Parse with nullable should succeed: {:?}", result);
    }
}
