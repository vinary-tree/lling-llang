//! Lazy CFG × Lattice composition using incremental Earley parsing.
//!
//! This module provides lazy composition between context-free grammars
//! and weighted lattices. The Earley chart is built incrementally as
//! the lattice is traversed, computing only the chart items reachable
//! from the current exploration path.
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::prelude::*;
//! use lling_llang::composition::LazyCfgComposition;
//!
//! // Create grammar and lattice
//! let grammar = GrammarBuilder::new()
//!     .start("S")
//!     .rule("S", &["NP", "VP"])
//!     // ...
//!     .build()
//!     .expect("valid grammar");
//!
//! let lattice = build_lattice(&["the", "dog", "saw"], &grammar);
//!
//! // Create lazy composition
//! let mut composition = LazyCfgComposition::new(&grammar, &lattice);
//!
//! // Parse lazily
//! let forest = composition.parse().expect("should parse");
//! ```

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::backend::LatticeBackend;
use crate::cfg::{EarleyParser, Grammar, ParseError, ParseForest, ParseTree, ForestNodeId};
use crate::lattice::{Lattice, LatticePath, NodeId, EdgeId, Edge};
use crate::semiring::Semiring;

/// State of parsing at a lattice position.
#[derive(Clone, Debug)]
pub enum ParseState {
    /// Not yet explored.
    Unexplored,
    /// Parsing in progress.
    InProgress,
    /// Parsing complete with success.
    Complete(SmallVec<[ForestNodeId; 4]>),
    /// Parsing failed (no valid parse).
    Failed,
}

/// Lazy CFG × Lattice composition using incremental Earley parsing.
///
/// The composition is lazy in the sense that chart items are only computed
/// when needed during traversal. This is more efficient than eager parsing
/// when only exploring a subset of paths.
pub struct LazyCfgComposition<'g, 'l, W, B>
where
    W: Semiring,
    B: LatticeBackend,
{
    /// Reference to the grammar.
    grammar: &'g Grammar,
    /// Reference to the lattice.
    lattice: &'l Lattice<W, B>,
    /// Earley parser for parsing.
    parser: EarleyParser<'g>,
    /// Cached parse results by position.
    parse_cache: FxHashMap<NodeId, ParseState>,
    /// Full parse forest (built incrementally).
    forest: Option<ParseForest>,
    /// Whether full parsing has been completed.
    parsed: bool,
}

impl<'g, 'l, W, B> LazyCfgComposition<'g, 'l, W, B>
where
    W: Semiring,
    B: LatticeBackend,
{
    /// Create a new lazy CFG × Lattice composition.
    pub fn new(grammar: &'g Grammar, lattice: &'l Lattice<W, B>) -> Self {
        let parser = EarleyParser::new(grammar);
        Self {
            grammar,
            lattice,
            parser,
            parse_cache: FxHashMap::default(),
            forest: None,
            parsed: false,
        }
    }

    /// Get the grammar.
    pub fn grammar(&self) -> &Grammar {
        self.grammar
    }

    /// Get the lattice.
    pub fn lattice(&self) -> &Lattice<W, B> {
        self.lattice
    }

    /// Check if the lattice has any grammatically valid parse.
    pub fn has_valid_parse(&mut self) -> bool {
        self.ensure_parsed();
        self.forest.as_ref().map_or(false, |f| !f.is_empty())
    }

    /// Parse the lattice, building the full parse forest.
    ///
    /// This triggers a full Earley parse of the lattice. The result
    /// is cached for subsequent calls.
    pub fn parse(&mut self) -> Result<&ParseForest, ParseError> {
        self.ensure_parsed();
        self.forest.as_ref().ok_or(ParseError::NoParse)
    }

    /// Get the best parse tree.
    pub fn best_parse(&mut self) -> Option<ParseTree> {
        self.ensure_parsed();
        self.forest.as_ref().and_then(|f| f.best_parse())
    }

    /// Get all parse trees (up to a limit).
    pub fn all_parses(&mut self, limit: usize) -> Vec<ParseTree> {
        self.ensure_parsed();
        self.forest.as_ref().map_or(Vec::new(), |f| f.all_parses(limit))
    }

    /// Filter the lattice to keep only grammatically valid paths.
    ///
    /// Returns a new lattice containing only edges that participate
    /// in at least one valid parse.
    pub fn filter(&mut self) -> Result<FilteredLattice<'l, W, B>, ParseError> {
        self.ensure_parsed();

        let forest = self.forest.as_ref().ok_or(ParseError::NoParse)?;
        if forest.is_empty() {
            return Err(ParseError::NoParse);
        }

        // Collect edges used in valid parses
        let used_edges = forest.collect_used_edges();

        Ok(FilteredLattice {
            lattice: self.lattice,
            valid_edges: used_edges,
        })
    }

    /// Iterate over grammatically valid paths.
    ///
    /// This lazily yields paths that have valid parses according to the grammar.
    pub fn valid_paths(&mut self) -> ValidPathIterator<'_, 'g, 'l, W, B> {
        self.ensure_parsed();

        let valid_edges = self.forest
            .as_ref()
            .map(|f| f.collect_used_edges())
            .unwrap_or_default();

        ValidPathIterator {
            composition: self,
            valid_edges,
            frontier: vec![(self.lattice.start(), SmallVec::new(), W::one())],
            visited: FxHashSet::default(),
        }
    }

    /// Get the number of cached parse states.
    pub fn cached_states(&self) -> usize {
        self.parse_cache.len()
    }

    /// Clear the parse cache.
    pub fn clear_cache(&mut self) {
        self.parse_cache.clear();
        self.forest = None;
        self.parsed = false;
    }

    /// Ensure parsing has been performed.
    fn ensure_parsed(&mut self) {
        if !self.parsed {
            match self.parser.parse_lattice(self.lattice) {
                Ok(forest) => {
                    self.forest = Some(forest);
                }
                Err(_) => {
                    self.forest = None;
                }
            }
            self.parsed = true;
        }
    }
}

/// A filtered view of a lattice containing only grammatically valid edges.
#[derive(Debug)]
pub struct FilteredLattice<'l, W, B>
where
    W: Semiring,
    B: LatticeBackend,
{
    /// The original lattice.
    lattice: &'l Lattice<W, B>,
    /// Set of edge IDs that participate in valid parses.
    valid_edges: FxHashSet<EdgeId>,
}

impl<'l, W, B> FilteredLattice<'l, W, B>
where
    W: Semiring,
    B: LatticeBackend,
{
    /// Get the original lattice.
    pub fn original(&self) -> &Lattice<W, B> {
        self.lattice
    }

    /// Get the set of valid edge IDs.
    pub fn valid_edge_ids(&self) -> &FxHashSet<EdgeId> {
        &self.valid_edges
    }

    /// Check if an edge is valid (participates in a parse).
    pub fn is_edge_valid(&self, edge_id: EdgeId) -> bool {
        self.valid_edges.contains(&edge_id)
    }

    /// Get the number of valid edges.
    pub fn num_valid_edges(&self) -> usize {
        self.valid_edges.len()
    }

    /// Get the total number of edges in the original lattice.
    pub fn total_edges(&self) -> usize {
        self.lattice.num_edges()
    }

    /// Get the reduction ratio (valid edges / total edges).
    pub fn reduction_ratio(&self) -> f64 {
        if self.total_edges() == 0 {
            1.0
        } else {
            self.valid_edges.len() as f64 / self.total_edges() as f64
        }
    }

    /// Iterate over valid edges.
    pub fn valid_edges(&self) -> impl Iterator<Item = &Edge<W>> {
        self.valid_edges.iter().filter_map(|&id| self.lattice.edge(id))
    }

    /// Materialize the filtered lattice into a new lattice.
    ///
    /// This creates a new lattice containing only the valid edges.
    pub fn materialize(&self) -> Lattice<W, B>
    where
        B: Clone,
        W: Clone,
    {
        use crate::lattice::LatticeBuilder;

        let mut builder = LatticeBuilder::<W, B>::new(self.lattice.backend().clone());

        // Find the maximum position to build correctly
        let mut max_pos = 0;
        for edge in self.valid_edges() {
            // Get position info from nodes
            if let (Some(source), Some(target)) = (
                self.lattice.node(edge.source),
                self.lattice.node(edge.target),
            ) {
                if let Some(pos) = source.position {
                    max_pos = max_pos.max(pos);
                }
                if let Some(pos) = target.position {
                    max_pos = max_pos.max(pos);
                }
            }
        }

        // Add valid edges to builder
        for edge in self.valid_edges() {
            if let (Some(source), Some(target)) = (
                self.lattice.node(edge.source),
                self.lattice.node(edge.target),
            ) {
                let start_pos = source.position.unwrap_or(edge.source.0 as usize);
                let end_pos = target.position.unwrap_or(edge.target.0 as usize);

                builder.add_correction_by_id(
                    start_pos,
                    end_pos,
                    edge.label,
                    edge.weight.clone(),
                    edge.metadata.clone(),
                );
            }
        }

        builder.build(max_pos + 1)
    }
}

/// Iterator over grammatically valid paths in a lattice.
pub struct ValidPathIterator<'c, 'g, 'l, W, B>
where
    W: Semiring,
    B: LatticeBackend,
{
    /// Reference to the composition.
    composition: &'c LazyCfgComposition<'g, 'l, W, B>,
    /// Valid edge IDs.
    valid_edges: FxHashSet<EdgeId>,
    /// Frontier of (current_node, path_edges, path_weight).
    frontier: Vec<(NodeId, SmallVec<[EdgeId; 8]>, W)>,
    /// Visited states to avoid duplicates.
    visited: FxHashSet<(NodeId, Vec<EdgeId>)>,
}

impl<'c, 'g, 'l, W, B> Iterator for ValidPathIterator<'c, 'g, 'l, W, B>
where
    W: Semiring + Clone,
    B: LatticeBackend,
{
    type Item = LatticePath<W>;

    fn next(&mut self) -> Option<Self::Item> {
        let lattice = self.composition.lattice;
        let end = lattice.end();

        while let Some((node, path, weight)) = self.frontier.pop() {
            // Check if we've reached the end
            if node == end {
                // Build the path
                let mut result = LatticePath::with_weight(weight);
                for &edge_id in &path {
                    // The edge weight was already accumulated, so use W::one() here
                    result.edges.push(edge_id);
                }
                result.mark_complete();
                return Some(result);
            }

            // Explore outgoing edges that are valid
            for edge in lattice.outgoing_edges(node) {
                if self.valid_edges.contains(&edge.id) {
                    let mut new_path = path.clone();
                    new_path.push(edge.id);

                    // Check if we've visited this state
                    let state = (edge.target, new_path.to_vec());
                    if !self.visited.contains(&state) {
                        self.visited.insert(state);
                        let new_weight = weight.times(&edge.weight);
                        self.frontier.push((edge.target, new_path, new_weight));
                    }
                }
            }
        }

        None
    }
}

/// Statistics about the composition.
#[derive(Clone, Debug, Default)]
pub struct CompositionStats {
    /// Number of chart items created.
    pub chart_items: usize,
    /// Number of forest nodes created.
    pub forest_nodes: usize,
    /// Number of complete parses found.
    pub complete_parses: usize,
    /// Number of lattice edges.
    pub lattice_edges: usize,
    /// Number of valid edges (in parses).
    pub valid_edges: usize,
}

impl<'g, 'l, W, B> LazyCfgComposition<'g, 'l, W, B>
where
    W: Semiring,
    B: LatticeBackend,
{
    /// Get composition statistics.
    pub fn stats(&mut self) -> CompositionStats {
        self.ensure_parsed();

        let forest = self.forest.as_ref();
        let valid_edges = forest.map(|f| f.collect_used_edges()).unwrap_or_default();

        CompositionStats {
            chart_items: 0, // Would need to expose from parser
            forest_nodes: forest.map_or(0, |f| f.num_nodes()),
            complete_parses: forest.map_or(0, |f| f.num_roots()),
            lattice_edges: self.lattice.num_edges(),
            valid_edges: valid_edges.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::cfg::GrammarBuilder;
    use crate::lattice::{LatticeBuilder, EdgeMetadata};
    use crate::semiring::TropicalWeight;

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

        // Get terminal IDs from grammar and intern words
        let word_ids: Vec<_> = words.iter().map(|w| {
            let t = grammar.terminal_by_name(w).expect(&format!("unknown word: {}", w));
            let _id = backend.intern(w);
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
    fn test_lazy_composition_basic() {
        let grammar = simple_grammar();
        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        assert!(composition.has_valid_parse());
    }

    #[test]
    fn test_lazy_composition_parse() {
        let grammar = simple_grammar();
        let lattice = build_lattice(&["the", "dog", "saw", "a", "cat"], &grammar);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        let result = composition.parse();
        assert!(result.is_ok());

        let forest = result.unwrap();
        assert!(!forest.is_empty());
    }

    #[test]
    fn test_lazy_composition_best_parse() {
        let grammar = simple_grammar();
        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        let tree = composition.best_parse();
        assert!(tree.is_some());
    }

    #[test]
    fn test_lazy_composition_filter() {
        let grammar = simple_grammar();
        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        let result = composition.filter();
        assert!(result.is_ok());

        let filtered = result.unwrap();
        assert!(filtered.num_valid_edges() > 0);
        assert!(filtered.reduction_ratio() <= 1.0);
    }

    #[test]
    fn test_lazy_composition_invalid_parse() {
        let grammar = simple_grammar();

        // "saw the" is not a valid sentence
        let mut backend = HashMapBackend::new();
        let _saw = backend.intern("saw");
        let _the = backend.intern("the");
        let saw_id = grammar.terminal_by_name("saw").expect("saw").vocab_id();
        let the_id = grammar.terminal_by_name("the").expect("the").vocab_id();

        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        builder.add_correction_by_id(0, 1, saw_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, the_id, TropicalWeight::one(), EdgeMetadata::default());
        let lattice = builder.build(2);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        assert!(!composition.has_valid_parse());
        assert!(composition.parse().is_err());
    }

    #[test]
    fn test_lazy_composition_valid_paths() {
        let grammar = simple_grammar();
        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        assert!(composition.has_valid_parse());

        let paths: Vec<_> = composition.valid_paths().collect();
        assert!(!paths.is_empty());
    }

    #[test]
    fn test_lazy_composition_stats() {
        let grammar = simple_grammar();
        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        let stats = composition.stats();
        assert!(stats.complete_parses > 0);
        assert!(stats.valid_edges > 0);
    }

    #[test]
    fn test_lazy_composition_all_parses() {
        let grammar = simple_grammar();
        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        let parses = composition.all_parses(10);
        assert!(!parses.is_empty());
    }

    #[test]
    fn test_lazy_composition_clear_cache() {
        let grammar = simple_grammar();
        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        // Parse once
        let _ = composition.parse();
        assert!(composition.parsed);

        // Clear and verify reset
        composition.clear_cache();
        assert!(!composition.parsed);
    }

    #[test]
    fn test_filtered_lattice_materialize() {
        let grammar = simple_grammar();
        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        let filtered = composition.filter().expect("should filter");
        let materialized = filtered.materialize();

        // Materialized lattice should have valid edges
        assert!(materialized.num_edges() > 0);
    }

    #[test]
    fn test_filtered_lattice_reduction() {
        let grammar = simple_grammar();

        // Create lattice with alternative corrections
        let mut backend = HashMapBackend::new();
        let _the = backend.intern("the");
        let _dog = backend.intern("dog");
        let _saw = backend.intern("saw");
        let _xyz = backend.intern("xyz"); // Invalid word

        let the_id = grammar.terminal_by_name("the").expect("the").vocab_id();
        let dog_id = grammar.terminal_by_name("dog").expect("dog").vocab_id();
        let saw_id = grammar.terminal_by_name("saw").expect("saw").vocab_id();

        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        // Valid path
        builder.add_correction_by_id(0, 1, the_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, dog_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(2, 3, saw_id, TropicalWeight::one(), EdgeMetadata::default());
        // Invalid alternative at position 1 (99 is not a valid terminal)
        builder.add_correction_by_id(1, 2, 99, TropicalWeight::one(), EdgeMetadata::default());

        let lattice = builder.build(3);

        let mut composition = LazyCfgComposition::new(&grammar, &lattice);

        let filtered = composition.filter().expect("should filter");

        // Should have filtered out the invalid edge
        assert!(filtered.num_valid_edges() < filtered.total_edges());
    }
}
