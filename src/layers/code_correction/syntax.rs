//! Syntax recovery layer for error recovery in code correction.
//!
//! This layer implements syntax error recovery by inserting or deleting
//! tokens to produce a parseable token sequence.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::backend::LatticeBackend;
use crate::lattice::{EdgeMetadata, Lattice, LatticeBuilder};
use crate::semiring::{Semiring, TropicalWeight};

use super::super::{CorrectionLayer, LayerResult};

/// Strategy for syntax error recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStrategy {
    /// Insert missing tokens (e.g., missing closing bracket).
    Insertion,
    /// Delete unexpected tokens (e.g., extra semicolon).
    Deletion,
    /// Replace tokens (e.g., wrong bracket type).
    Replacement,
    /// All strategies combined.
    All,
}

impl Default for RecoveryStrategy {
    fn default() -> Self {
        Self::All
    }
}

/// Configuration for syntax recovery.
#[derive(Clone, Debug)]
pub struct SyntaxRecoveryConfig {
    /// Recovery strategies to use.
    pub strategies: Vec<RecoveryStrategy>,

    /// Cost for inserting a token.
    pub insertion_cost: f64,

    /// Cost for deleting a token.
    pub deletion_cost: f64,

    /// Cost for replacing a token.
    pub replacement_cost: f64,

    /// Maximum number of consecutive insertions.
    pub max_insertions: usize,

    /// Maximum number of consecutive deletions.
    pub max_deletions: usize,

    /// Tokens that can be inserted for recovery.
    pub insertable_tokens: HashSet<Arc<str>>,

    /// Tokens that can be deleted for recovery (typically noise tokens).
    pub deletable_tokens: HashSet<Arc<str>>,

    /// Token pairs for bracket matching (open -> close).
    pub bracket_pairs: HashMap<Arc<str>, Arc<str>>,

    /// Whether to balance brackets.
    pub balance_brackets: bool,

    /// Whether to add missing semicolons.
    pub add_semicolons: bool,

    /// Language hint for recovery strategies.
    pub language_hint: Option<String>,
}

impl Default for SyntaxRecoveryConfig {
    fn default() -> Self {
        // Default insertable tokens (common syntax elements)
        let insertable: HashSet<Arc<str>> = ["(", ")", "[", "]", "{", "}", ";", ",", ":", "."]
            .iter()
            .map(|s| Arc::from(*s))
            .collect();

        // Default deletable tokens (typically typos or extra punctuation)
        let deletable: HashSet<Arc<str>> = [";", ",", ".", "(", ")", "[", "]", "{", "}"]
            .iter()
            .map(|s| Arc::from(*s))
            .collect();

        // Standard bracket pairs
        let mut brackets = HashMap::new();
        brackets.insert(Arc::from("("), Arc::from(")"));
        brackets.insert(Arc::from("["), Arc::from("]"));
        brackets.insert(Arc::from("{"), Arc::from("}"));
        brackets.insert(Arc::from("<"), Arc::from(">"));

        Self {
            strategies: vec![RecoveryStrategy::All],
            insertion_cost: 2.0,
            deletion_cost: 1.5,
            replacement_cost: 1.0,
            max_insertions: 3,
            max_deletions: 2,
            insertable_tokens: insertable,
            deletable_tokens: deletable,
            bracket_pairs: brackets,
            balance_brackets: true,
            add_semicolons: false,
            language_hint: None,
        }
    }
}

impl SyntaxRecoveryConfig {
    /// Create a new configuration with the specified strategies.
    pub fn new(strategies: Vec<RecoveryStrategy>) -> Self {
        Self {
            strategies,
            ..Default::default()
        }
    }

    /// Set insertion cost.
    pub fn with_insertion_cost(mut self, cost: f64) -> Self {
        self.insertion_cost = cost;
        self
    }

    /// Set deletion cost.
    pub fn with_deletion_cost(mut self, cost: f64) -> Self {
        self.deletion_cost = cost;
        self
    }

    /// Set replacement cost.
    pub fn with_replacement_cost(mut self, cost: f64) -> Self {
        self.replacement_cost = cost;
        self
    }

    /// Set maximum consecutive insertions.
    pub fn with_max_insertions(mut self, max: usize) -> Self {
        self.max_insertions = max;
        self
    }

    /// Set maximum consecutive deletions.
    pub fn with_max_deletions(mut self, max: usize) -> Self {
        self.max_deletions = max;
        self
    }

    /// Add tokens that can be inserted.
    pub fn with_insertable_tokens<I, S>(mut self, tokens: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for tok in tokens {
            self.insertable_tokens.insert(Arc::from(tok.as_ref()));
        }
        self
    }

    /// Add tokens that can be deleted.
    pub fn with_deletable_tokens<I, S>(mut self, tokens: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for tok in tokens {
            self.deletable_tokens.insert(Arc::from(tok.as_ref()));
        }
        self
    }

    /// Add a bracket pair.
    pub fn with_bracket_pair(mut self, open: &str, close: &str) -> Self {
        self.bracket_pairs.insert(Arc::from(open), Arc::from(close));
        self
    }

    /// Set whether to balance brackets.
    pub fn with_bracket_balancing(mut self, balance: bool) -> Self {
        self.balance_brackets = balance;
        self
    }

    /// Set whether to add missing semicolons.
    pub fn with_semicolon_insertion(mut self, add: bool) -> Self {
        self.add_semicolons = add;
        self
    }

    /// Set language hint.
    pub fn with_language(mut self, language: &str) -> Self {
        self.language_hint = Some(language.to_string());
        self
    }

    /// Check if insertion is enabled.
    pub fn allows_insertion(&self) -> bool {
        self.strategies
            .iter()
            .any(|s| matches!(s, RecoveryStrategy::Insertion | RecoveryStrategy::All))
    }

    /// Check if deletion is enabled.
    pub fn allows_deletion(&self) -> bool {
        self.strategies
            .iter()
            .any(|s| matches!(s, RecoveryStrategy::Deletion | RecoveryStrategy::All))
    }

    /// Check if replacement is enabled.
    pub fn allows_replacement(&self) -> bool {
        self.strategies
            .iter()
            .any(|s| matches!(s, RecoveryStrategy::Replacement | RecoveryStrategy::All))
    }
}

/// Syntax recovery layer for code correction.
///
/// This layer adds recovery edges to the lattice to handle syntax errors:
/// - Insert missing tokens (brackets, semicolons, etc.)
/// - Delete unexpected tokens
/// - Replace mismatched tokens (bracket types)
#[derive(Clone, Debug)]
pub struct SyntaxRecoveryLayer {
    config: SyntaxRecoveryConfig,
}

impl SyntaxRecoveryLayer {
    /// Create a new syntax recovery layer.
    pub fn new(config: SyntaxRecoveryConfig) -> Self {
        Self { config }
    }

    /// Get the configuration.
    pub fn config(&self) -> &SyntaxRecoveryConfig {
        &self.config
    }

    /// Estimate the expansion factor (how many paths are added).
    pub fn estimated_expansion(&self) -> f64 {
        // Insertion typically adds a few alternative paths
        let insertion_factor = if self.config.allows_insertion() {
            1.1
        } else {
            1.0
        };
        // Deletion might slightly reduce paths
        let deletion_factor = if self.config.allows_deletion() {
            0.95
        } else {
            1.0
        };

        insertion_factor * deletion_factor
    }

    /// Apply the layer to a lattice (internal implementation).
    fn apply_impl<W, B>(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>>
    where
        W: Semiring + From<TropicalWeight>,
        B: LatticeBackend + Clone,
    {
        // Handle empty lattice
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // Clone the backend for the new lattice
        let mut backend = lattice.backend().clone();
        let mut builder = LatticeBuilder::new(backend.clone());

        // Track bracket state for balancing
        let mut bracket_stack: Vec<Arc<str>> = Vec::new();

        // First pass: copy all original edges
        for edge in lattice.edges() {
            let source = edge.source.value() as usize;
            let target = edge.target.value() as usize;

            builder.add_correction_by_id(
                source,
                target,
                edge.label,
                edge.weight.clone(),
                edge.metadata.clone(),
            );

            // Track brackets for balancing
            if self.config.balance_brackets {
                if let Some(word) = lattice.word(edge.label) {
                    let word_arc = Arc::from(word);
                    if self.config.bracket_pairs.contains_key(&word_arc) {
                        bracket_stack.push(word_arc);
                    } else if let Some((open, _)) = self
                        .config
                        .bracket_pairs
                        .iter()
                        .find(|(_, close)| **close == word_arc)
                    {
                        if bracket_stack.last() == Some(open) {
                            bracket_stack.pop();
                        }
                    }
                }
            }
        }

        // Second pass: add recovery edges

        // Add insertion edges at each node
        if self.config.allows_insertion() {
            for node_id in lattice.node_ids() {
                let pos = node_id.value() as usize;

                // For each insertable token, add an epsilon transition
                for token in &self.config.insertable_tokens {
                    // Intern the token
                    let vocab_id = backend.intern(token);

                    // Create an insertion edge (same source/target with the inserted token)
                    // This creates a "free" insertion that the path can take
                    let weight = W::from(TropicalWeight::new(self.config.insertion_cost));
                    let mut metadata = EdgeMetadata::default();
                    metadata.is_original = false;

                    // Add self-loop for insertion
                    builder.add_correction_by_id(pos, pos, vocab_id, weight, metadata);
                }
            }
        }

        // Add deletion edges (skip edges)
        if self.config.allows_deletion() {
            for edge in lattice.edges() {
                if let Some(word) = lattice.word(edge.label) {
                    let word_arc = Arc::from(word);

                    // Check if this token is deletable
                    if self.config.deletable_tokens.contains(&word_arc) {
                        let source = edge.source.value() as usize;
                        let target = edge.target.value() as usize;

                        // Add a skip edge that bypasses this token
                        // This is implemented by adding an epsilon edge
                        let weight = W::from(TropicalWeight::new(self.config.deletion_cost));
                        let mut metadata = EdgeMetadata::default();
                        metadata.is_original = false;

                        // Intern epsilon (empty token) for deletion
                        let epsilon_id = backend.intern("");

                        // Add skip edge from source to target (bypassing the original token)
                        builder.add_correction_by_id(source, target, epsilon_id, weight, metadata);
                    }
                }
            }
        }

        // Balance unclosed brackets at the end
        if self.config.balance_brackets && !bracket_stack.is_empty() {
            let end_pos = lattice.end().value() as usize;

            // Add closing brackets for each unclosed opening bracket
            for open in bracket_stack.iter().rev() {
                if let Some(close) = self.config.bracket_pairs.get(open) {
                    let vocab_id = backend.intern(close);
                    let weight = W::from(TropicalWeight::new(self.config.insertion_cost));
                    let mut metadata = EdgeMetadata::default();
                    metadata.is_original = false;

                    // Add closing bracket at the end
                    builder.add_correction_by_id(end_pos, end_pos, vocab_id, weight, metadata);
                }
            }
        }

        // Build the new lattice
        let num_nodes = lattice.num_nodes();
        Ok(builder.build(num_nodes))
    }
}

impl<W, B> CorrectionLayer<W, B> for SyntaxRecoveryLayer
where
    W: Semiring + From<TropicalWeight>,
    B: LatticeBackend + Clone,
{
    fn name(&self) -> &str {
        "syntax-recovery"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        self.apply_impl(lattice)
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        true
    }

    fn estimated_reduction(&self) -> f64 {
        self.estimated_expansion()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;

    fn build_test_lattice() -> Lattice<TropicalWeight, HashMapBackend> {
        let mut backend = HashMapBackend::new();
        let def = backend.intern("def");
        let foo = backend.intern("foo");
        let lparen = backend.intern("(");
        // Note: missing closing paren

        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        builder.add_correction_by_id(0, 1, def, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, foo, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(2, 3, lparen, TropicalWeight::one(), EdgeMetadata::default());
        builder.build(3)
    }

    #[test]
    fn test_recovery_strategy_default() {
        assert_eq!(RecoveryStrategy::default(), RecoveryStrategy::All);
    }

    #[test]
    fn test_config_default() {
        let config = SyntaxRecoveryConfig::default();
        assert!(config.allows_insertion());
        assert!(config.allows_deletion());
        assert!(config.allows_replacement());
        assert!(config.balance_brackets);
        assert!(config.insertable_tokens.contains(&Arc::from("(")));
        assert!(config.bracket_pairs.contains_key(&Arc::from("(")));
    }

    #[test]
    fn test_config_builder() {
        let config = SyntaxRecoveryConfig::new(vec![RecoveryStrategy::Insertion])
            .with_insertion_cost(3.0)
            .with_max_insertions(5)
            .with_bracket_balancing(false);

        assert!((config.insertion_cost - 3.0).abs() < 0.001);
        assert_eq!(config.max_insertions, 5);
        assert!(!config.balance_brackets);
        assert!(config.allows_insertion());
        assert!(!config.allows_deletion());
    }

    #[test]
    fn test_config_insertable_tokens() {
        let config = SyntaxRecoveryConfig::default().with_insertable_tokens(vec!["async", "await"]);

        assert!(config.insertable_tokens.contains(&Arc::from("async")));
        assert!(config.insertable_tokens.contains(&Arc::from("await")));
    }

    #[test]
    fn test_config_bracket_pair() {
        let config = SyntaxRecoveryConfig::default().with_bracket_pair("<<", ">>");

        assert!(config.bracket_pairs.contains_key(&Arc::from("<<")));
        assert_eq!(
            config.bracket_pairs.get(&Arc::from("<<")),
            Some(&Arc::from(">>"))
        );
    }

    #[test]
    fn test_layer_creation() {
        let config = SyntaxRecoveryConfig::default();
        let layer = SyntaxRecoveryLayer::new(config.clone());

        assert_eq!(layer.config().insertion_cost, config.insertion_cost);
    }

    #[test]
    fn test_layer_name() {
        let layer = SyntaxRecoveryLayer::new(SyntaxRecoveryConfig::default());
        assert_eq!(
            <SyntaxRecoveryLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::name(&layer),
            "syntax-recovery"
        );
    }

    #[test]
    fn test_layer_apply() {
        let layer = SyntaxRecoveryLayer::new(SyntaxRecoveryConfig::default());
        let lattice = build_test_lattice();

        let result =
            <SyntaxRecoveryLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
                &layer, &lattice,
            );
        assert!(result.is_ok());

        let recovered = result.expect("should apply");
        // Should have more edges than original (recovery edges added)
        assert!(recovered.num_edges() >= lattice.num_edges());
    }

    #[test]
    fn test_layer_empty_lattice() {
        let layer = SyntaxRecoveryLayer::new(SyntaxRecoveryConfig::default());

        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let empty_lattice = builder.build(0);

        let result =
            <SyntaxRecoveryLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
                &layer,
                &empty_lattice,
            );
        assert!(result.is_ok());
    }

    #[test]
    fn test_layer_estimated_expansion() {
        let layer = SyntaxRecoveryLayer::new(SyntaxRecoveryConfig::default());
        let expansion = layer.estimated_expansion();

        // Should be close to 1.0 (slight increase from insertions, slight decrease from deletions)
        assert!(expansion > 0.9 && expansion < 1.2);
    }

    #[test]
    fn test_insertion_only() {
        let config = SyntaxRecoveryConfig::new(vec![RecoveryStrategy::Insertion]);
        let layer = SyntaxRecoveryLayer::new(config);
        let lattice = build_test_lattice();

        let result =
            <SyntaxRecoveryLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
                &layer, &lattice,
            );
        assert!(result.is_ok());
    }

    #[test]
    fn test_deletion_only() {
        let config = SyntaxRecoveryConfig::new(vec![RecoveryStrategy::Deletion]);
        let layer = SyntaxRecoveryLayer::new(config);
        let lattice = build_test_lattice();

        let result =
            <SyntaxRecoveryLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
                &layer, &lattice,
            );
        assert!(result.is_ok());
    }

    #[test]
    fn test_no_bracket_balancing() {
        let config = SyntaxRecoveryConfig::default().with_bracket_balancing(false);
        let layer = SyntaxRecoveryLayer::new(config);
        let lattice = build_test_lattice();

        let result =
            <SyntaxRecoveryLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
                &layer, &lattice,
            );
        assert!(result.is_ok());
    }
}
