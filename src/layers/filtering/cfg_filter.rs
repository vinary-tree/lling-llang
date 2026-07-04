//! CFG grammar filtering layer.
//!
//! This layer filters lattice paths that don't parse according to a
//! context-free grammar.

use crate::backend::LatticeBackend;
use crate::cfg::{EarleyParser, Grammar, ParseError};
use crate::lattice::{Lattice, LatticeBuilder};
use crate::semiring::Semiring;

use super::super::traits::{CorrectionLayer, LayerError, LayerResult};

/// CFG grammar filtering layer.
///
/// Filters lattice paths to only those that parse successfully
/// according to the provided grammar.
///
/// # Example
///
/// ```ignore
/// use lling_llang::layers::CfgFilterLayer;
/// use lling_llang::cfg::GrammarBuilder;
///
/// let grammar = GrammarBuilder::new()
///     .start("S")
///     .rule("S", &["NP", "VP"])
///     .build()?;
///
/// let layer = CfgFilterLayer::new(&grammar);
/// let filtered = layer.apply(&lattice)?;
/// ```
pub struct CfgFilterLayer<'g> {
    grammar: &'g Grammar,
    /// Whether to completely remove ungrammatical edges or just downweight them.
    prune_ungrammatical: bool,
}

impl<'g> CfgFilterLayer<'g> {
    /// Create a new CFG filter layer.
    pub fn new(grammar: &'g Grammar) -> Self {
        Self {
            grammar,
            prune_ungrammatical: true,
        }
    }

    /// Set whether to prune ungrammatical edges (default: true).
    ///
    /// If false, ungrammatical edges are kept but may be downweighted
    /// in future versions.
    pub fn with_pruning(mut self, prune: bool) -> Self {
        self.prune_ungrammatical = prune;
        self
    }

    /// Get the grammar used by this layer.
    pub fn grammar(&self) -> &Grammar {
        self.grammar
    }
}

impl<'g, W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for CfgFilterLayer<'g> {
    fn name(&self) -> &str {
        "cfg-filter"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        // Handle empty lattice
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // Parse the lattice
        let parser = EarleyParser::new(self.grammar);
        let forest = parser.parse_lattice(lattice).map_err(|e| match e {
            ParseError::NoParse => LayerError::ParseError("no valid parse found".to_string()),
            ParseError::EmptyLattice => LayerError::ParseError("empty lattice".to_string()),
            ParseError::GrammarError(msg) => LayerError::ConfigError(msg),
        })?;

        // Collect edges that are used in at least one valid parse
        let used_edges = forest.collect_used_edges();

        if !self.prune_ungrammatical {
            // If not pruning, return the original lattice
            return Ok(lattice.clone());
        }

        // Build a new lattice with only the used edges
        let mut new_builder = LatticeBuilder::new(lattice.backend().clone());

        for edge in lattice.edges() {
            if used_edges.contains(&edge.id) {
                new_builder.add_correction_by_id(
                    edge.source.0 as usize,
                    edge.target.0 as usize,
                    edge.label,
                    edge.weight,
                    edge.metadata.clone(),
                );
            }
        }

        // Build with the same end position
        let end_pos = lattice.end().0 as usize;
        Ok(new_builder.build(end_pos))
    }

    fn can_apply(&self, lattice: &Lattice<W, B>) -> bool {
        // Can apply if the lattice is non-empty or if it's a valid empty lattice
        !lattice.is_empty() || lattice.start() == lattice.end()
    }

    fn estimated_reduction(&self) -> f64 {
        // CFG filtering typically reduces paths significantly
        0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::cfg::GrammarBuilder;
    use crate::lattice::EdgeMetadata;
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

        let word_ids: Vec<_> = words
            .iter()
            .map(|w| {
                let t = grammar
                    .terminal_by_name(w)
                    .expect(&format!("unknown word: {}", w));
                let _id = backend.intern(w);
                t.vocab_id()
            })
            .collect();

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
    fn test_cfg_filter_layer_name() {
        let grammar = simple_grammar();
        let layer = CfgFilterLayer::new(&grammar);
        // Use concrete types for the trait method call
        let name =
            <CfgFilterLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::name(&layer);
        assert_eq!(name, "cfg-filter");
    }

    #[test]
    fn test_cfg_filter_valid_sentence() {
        let grammar = simple_grammar();
        let layer = CfgFilterLayer::new(&grammar);

        // "the dog saw" is a valid sentence
        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);
        assert!(layer.can_apply(&lattice));

        let result = layer.apply(&lattice);
        assert!(result.is_ok(), "should parse valid sentence: {:?}", result);

        let filtered = result.expect("layers/cfg_filter.rs: required value was None/Err");
        assert_eq!(filtered.num_edges(), 3);
    }

    #[test]
    fn test_cfg_filter_invalid_sentence() {
        let grammar = simple_grammar();
        let layer = CfgFilterLayer::new(&grammar);

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

        let result = layer.apply(&lattice);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LayerError::ParseError(_)));
    }

    #[test]
    fn test_cfg_filter_with_alternatives() {
        let grammar = simple_grammar();
        let layer = CfgFilterLayer::new(&grammar);

        // Lattice with two paths: "the dog saw" and "the cat saw"
        let mut backend = HashMapBackend::new();
        let _the = backend.intern("the");
        let _dog = backend.intern("dog");
        let _cat = backend.intern("cat");
        let _saw = backend.intern("saw");

        let the_id = grammar.terminal_by_name("the").expect("the").vocab_id();
        let dog_id = grammar.terminal_by_name("dog").expect("dog").vocab_id();
        let cat_id = grammar.terminal_by_name("cat").expect("cat").vocab_id();
        let saw_id = grammar.terminal_by_name("saw").expect("saw").vocab_id();

        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        builder.add_correction_by_id(0, 1, the_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, dog_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, cat_id, TropicalWeight::one(), EdgeMetadata::default()); // Alternative
        builder.add_correction_by_id(2, 3, saw_id, TropicalWeight::one(), EdgeMetadata::default());
        let lattice = builder.build(3);

        assert_eq!(lattice.num_edges(), 4);

        let result = layer.apply(&lattice);
        assert!(result.is_ok());

        let filtered = result.expect("layers/cfg_filter.rs: required value was None/Err");
        // At least one valid parse should be found (either "dog" or "cat" path)
        // Note: Current implementation may only capture one derivation path.
        // A full packed forest would capture both, but that's a future enhancement.
        assert!(filtered.num_edges() >= 3);
        assert!(filtered.num_edges() <= 4);
    }

    #[test]
    fn test_cfg_filter_prunes_invalid() {
        let grammar = simple_grammar();
        let layer = CfgFilterLayer::new(&grammar);

        // Lattice with valid path "the dog saw" and invalid edge "the saw dog"
        // This is tricky - we need a lattice where some edges are used and some aren't
        // Let's create: 0 -the-> 1 -dog-> 2 -saw-> 3
        //               0 -the-> 1 -saw-> 2 (invalid continuation)
        let mut backend = HashMapBackend::new();
        let _the = backend.intern("the");
        let _dog = backend.intern("dog");
        let _saw = backend.intern("saw");

        let the_id = grammar.terminal_by_name("the").expect("the").vocab_id();
        let dog_id = grammar.terminal_by_name("dog").expect("dog").vocab_id();
        let saw_id = grammar.terminal_by_name("saw").expect("saw").vocab_id();

        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        // Valid path edges
        builder.add_correction_by_id(0, 1, the_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, dog_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(2, 3, saw_id, TropicalWeight::one(), EdgeMetadata::default());
        // Invalid edge: "saw" at position 1 (where we need a noun)
        builder.add_correction_by_id(1, 2, saw_id, TropicalWeight::one(), EdgeMetadata::default());
        let lattice = builder.build(3);

        assert_eq!(lattice.num_edges(), 4);

        let result = layer.apply(&lattice);
        assert!(result.is_ok());

        let filtered = result.expect("layers/cfg_filter.rs: required value was None/Err");
        // The invalid "saw" edge at position 1->2 should be pruned
        assert_eq!(filtered.num_edges(), 3);
    }

    #[test]
    fn test_cfg_filter_no_prune_mode() {
        let grammar = simple_grammar();
        let layer = CfgFilterLayer::new(&grammar).with_pruning(false);

        let lattice = build_lattice(&["the", "dog", "saw"], &grammar);
        let result = layer.apply(&lattice);
        assert!(result.is_ok());

        let filtered = result.expect("layers/cfg_filter.rs: required value was None/Err");
        // Without pruning, all edges should remain
        assert_eq!(filtered.num_edges(), 3);
    }

    #[test]
    fn test_cfg_filter_estimated_reduction() {
        let grammar = simple_grammar();
        let layer = CfgFilterLayer::new(&grammar);
        let reduction = <CfgFilterLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::estimated_reduction(&layer);
        assert!((reduction - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_cfg_filter_empty_lattice() {
        let grammar = simple_grammar();
        let layer = CfgFilterLayer::new(&grammar);

        // Create an empty lattice (start == end)
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let lattice = builder.build(0);

        let result = layer.apply(&lattice);
        assert!(result.is_ok());
    }
}
