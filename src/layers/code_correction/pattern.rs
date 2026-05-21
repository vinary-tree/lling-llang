//! Pattern-aware correction layer using mined code idioms.
//!
//! This layer uses patterns discovered by subtree mining (e.g., TreeminerD)
//! to boost corrections that match common code idioms.

use std::collections::HashMap;
use std::sync::Arc;

use crate::backend::LatticeBackend;
use crate::lattice::{EdgeMetadata, Lattice, LatticeBuilder};
use crate::semiring::{Semiring, TropicalWeight};

use super::super::{CorrectionLayer, LayerResult};

/// A pattern boost entry for a token sequence.
#[derive(Clone, Debug)]
pub struct PatternBoost {
    /// The token sequence pattern.
    pub pattern: Vec<Arc<str>>,
    /// Boost value (negative cost = bonus).
    pub boost: f64,
    /// Pattern ID for tracking.
    pub pattern_id: u64,
    /// Support (how many times this pattern appears in the corpus).
    pub support: usize,
    /// Pattern name/description for debugging.
    pub name: Option<String>,
}

impl PatternBoost {
    /// Create a new pattern boost.
    pub fn new<I, S>(pattern: I, boost: f64) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            pattern: pattern.into_iter().map(|s| Arc::from(s.as_ref())).collect(),
            boost,
            pattern_id: 0,
            support: 0,
            name: None,
        }
    }

    /// Set the pattern ID.
    pub fn with_id(mut self, id: u64) -> Self {
        self.pattern_id = id;
        self
    }

    /// Set the support count.
    pub fn with_support(mut self, support: usize) -> Self {
        self.support = support;
        self
    }

    /// Set the pattern name.
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    /// Get the pattern length.
    pub fn len(&self) -> usize {
        self.pattern.len()
    }

    /// Check if the pattern is empty.
    pub fn is_empty(&self) -> bool {
        self.pattern.is_empty()
    }
}

/// Configuration for pattern-aware correction.
#[derive(Clone, Debug)]
pub struct PatternAwareConfig {
    /// Patterns with their boost values.
    pub patterns: Vec<PatternBoost>,

    /// Minimum pattern length to consider.
    pub min_pattern_length: usize,

    /// Maximum pattern length to consider.
    pub max_pattern_length: usize,

    /// Default boost for patterns without explicit boost.
    pub default_boost: f64,

    /// Whether to use longest matching pattern only.
    pub longest_match_only: bool,

    /// Maximum boost to apply (caps total boost).
    pub max_boost: f64,

    /// Whether patterns must match at token boundaries.
    pub token_boundary_only: bool,

    /// Index for quick pattern lookup (prefix -> pattern indices).
    pattern_index: HashMap<Arc<str>, Vec<usize>>,
}

impl Default for PatternAwareConfig {
    fn default() -> Self {
        Self {
            patterns: Vec::new(),
            min_pattern_length: 2,
            max_pattern_length: 10,
            default_boost: 0.5,
            longest_match_only: true,
            max_boost: 5.0,
            token_boundary_only: true,
            pattern_index: HashMap::new(),
        }
    }
}

impl PatternAwareConfig {
    /// Create a new configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a pattern with explicit boost.
    pub fn with_pattern<I, S>(mut self, pattern: I, boost: f64) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let pattern_boost = PatternBoost::new(pattern, boost);
        self.add_pattern_internal(pattern_boost);
        self
    }

    /// Add a pre-built pattern boost.
    pub fn with_pattern_boost(mut self, pattern: PatternBoost) -> Self {
        self.add_pattern_internal(pattern);
        self
    }

    /// Add multiple patterns.
    pub fn with_patterns(mut self, patterns: Vec<PatternBoost>) -> Self {
        for pattern in patterns {
            self.add_pattern_internal(pattern);
        }
        self
    }

    fn add_pattern_internal(&mut self, pattern: PatternBoost) {
        let idx = self.patterns.len();
        if let Some(first) = pattern.pattern.first() {
            self.pattern_index
                .entry(Arc::clone(first))
                .or_default()
                .push(idx);
        }
        self.patterns.push(pattern);
    }

    /// Set minimum pattern length.
    pub fn with_min_length(mut self, len: usize) -> Self {
        self.min_pattern_length = len;
        self
    }

    /// Set maximum pattern length.
    pub fn with_max_length(mut self, len: usize) -> Self {
        self.max_pattern_length = len;
        self
    }

    /// Set default boost value.
    pub fn with_default_boost(mut self, boost: f64) -> Self {
        self.default_boost = boost;
        self
    }

    /// Set whether to use longest match only.
    pub fn with_longest_match_only(mut self, longest: bool) -> Self {
        self.longest_match_only = longest;
        self
    }

    /// Set maximum boost cap.
    pub fn with_max_boost(mut self, max: f64) -> Self {
        self.max_boost = max;
        self
    }

    /// Get patterns starting with the given token.
    pub fn patterns_starting_with(&self, token: &str) -> impl Iterator<Item = &PatternBoost> {
        let token_arc = Arc::from(token);
        self.pattern_index
            .get(&token_arc)
            .into_iter()
            .flatten()
            .filter_map(|&idx| self.patterns.get(idx))
    }

    /// Find the best matching pattern for a token sequence.
    pub fn find_best_pattern(&self, tokens: &[&str]) -> Option<&PatternBoost> {
        if tokens.is_empty() {
            return None;
        }

        let first = tokens[0];
        let mut best: Option<&PatternBoost> = None;
        let mut best_len = 0;

        for pattern in self.patterns_starting_with(first) {
            if pattern.len() > tokens.len() {
                continue;
            }

            // Check if pattern matches
            let matches = pattern
                .pattern
                .iter()
                .zip(tokens.iter())
                .all(|(p, t)| p.as_ref() == *t);

            if matches && pattern.len() > best_len {
                best = Some(pattern);
                best_len = pattern.len();
            }
        }

        best
    }

    /// Create common patterns for Python.
    pub fn python_patterns() -> Self {
        Self::new()
            .with_pattern(vec!["def", "foo", "(", ")"], 1.0)
            .with_pattern(vec!["if", "_", ":"], 0.8)
            .with_pattern(vec!["for", "_", "in", "_", ":"], 1.0)
            .with_pattern(vec!["class", "_", ":"], 0.9)
            .with_pattern(vec!["return", "_"], 0.5)
            .with_pattern(vec!["import", "_"], 0.5)
            .with_pattern(vec!["from", "_", "import", "_"], 0.8)
    }

    /// Create common patterns for Rust.
    pub fn rust_patterns() -> Self {
        Self::new()
            .with_pattern(vec!["fn", "_", "(", ")"], 1.0)
            .with_pattern(vec!["let", "_", "="], 0.8)
            .with_pattern(vec!["let", "mut", "_", "="], 0.9)
            .with_pattern(vec!["impl", "_", "for", "_"], 1.0)
            .with_pattern(vec!["struct", "_", "{"], 0.9)
            .with_pattern(vec!["enum", "_", "{"], 0.9)
            .with_pattern(vec!["match", "_", "{"], 0.8)
            .with_pattern(vec!["if", "let", "Some", "(", "_", ")", "="], 1.0)
            .with_pattern(vec!["->", "Result", "<"], 0.7)
    }

    /// Create common patterns for Rholang.
    pub fn rholang_patterns() -> Self {
        Self::new()
            .with_pattern(vec!["new", "_", "in"], 1.0)
            .with_pattern(vec!["contract", "_", "(", ")"], 1.0)
            .with_pattern(vec!["for", "(", "_", "<-", "_", ")"], 1.0)
            .with_pattern(vec!["match", "_", "{"], 0.8)
            .with_pattern(vec!["|"], 0.3)
    }

    /// Create common patterns for MeTTa.
    pub fn metta_patterns() -> Self {
        Self::new()
            .with_pattern(vec!["(", "=", "_", "_", ")"], 1.0)
            .with_pattern(vec!["(", ":", "_", "_", ")"], 0.9)
            .with_pattern(vec!["(", "match", "_", "_", "_", ")"], 1.0)
            .with_pattern(vec!["(", "let", "_", "_", "_", ")"], 0.8)
            .with_pattern(vec!["!", "(", "_", ")"], 0.7)
    }
}

/// Pattern-aware correction layer.
///
/// This layer boosts lattice paths that match common code patterns,
/// making idiomatically correct code more likely to be selected.
#[derive(Clone, Debug)]
pub struct PatternAwareLayer {
    config: PatternAwareConfig,
}

impl PatternAwareLayer {
    /// Create a new pattern-aware layer.
    pub fn new(config: PatternAwareConfig) -> Self {
        Self { config }
    }

    /// Create for Python with default patterns.
    pub fn python() -> Self {
        Self::new(PatternAwareConfig::python_patterns())
    }

    /// Create for Rust with default patterns.
    pub fn rust() -> Self {
        Self::new(PatternAwareConfig::rust_patterns())
    }

    /// Create for Rholang with default patterns.
    pub fn rholang() -> Self {
        Self::new(PatternAwareConfig::rholang_patterns())
    }

    /// Create for MeTTa with default patterns.
    pub fn metta() -> Self {
        Self::new(PatternAwareConfig::metta_patterns())
    }

    /// Get the configuration.
    pub fn config(&self) -> &PatternAwareConfig {
        &self.config
    }

    /// Get the number of patterns.
    pub fn num_patterns(&self) -> usize {
        self.config.patterns.len()
    }

    /// Get estimated reduction factor.
    pub fn estimated_reduction(&self) -> f64 {
        // Pattern boosting typically doesn't reduce paths, it reweights them
        1.0
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

        // If no patterns, just return the original
        if self.config.patterns.is_empty() {
            return Ok(lattice.clone());
        }

        // Collect tokens from the lattice for pattern matching
        let tokens: Vec<(usize, usize, String, W, EdgeMetadata)> = lattice
            .edges()
            .iter()
            .filter_map(|edge| {
                let word = lattice.word(edge.label)?;
                Some((
                    edge.source.value() as usize,
                    edge.target.value() as usize,
                    word.to_string(),
                    edge.weight.clone(),
                    edge.metadata.clone(),
                ))
            })
            .collect();

        // Build token sequence for pattern matching
        // Note: This is a simplified approach - a full implementation would
        // enumerate paths through the lattice
        let token_strings: Vec<&str> = tokens.iter().map(|(_, _, t, _, _)| t.as_str()).collect();

        // Find patterns that match in this token sequence
        let mut boosts: HashMap<(usize, usize), f64> = HashMap::new();

        for i in 0..token_strings.len() {
            let remaining = &token_strings[i..];
            if let Some(pattern) = self.config.find_best_pattern(remaining) {
                // Apply boost to all edges in this pattern
                for j in 0..pattern.len().min(remaining.len()) {
                    if let Some((source, target, _, _, _)) = tokens.get(i + j) {
                        let key = (*source, *target);
                        let current = boosts.entry(key).or_insert(0.0);
                        *current = (*current + pattern.boost).min(self.config.max_boost);
                    }
                }
            }
        }

        // Rebuild the lattice with boosted weights
        let backend = lattice.backend().clone();
        let mut builder = LatticeBuilder::new(backend);

        for (source, target, _, weight, metadata) in &tokens {
            let boost = boosts.get(&(*source, *target)).copied().unwrap_or(0.0);

            // Apply boost as negative cost (in tropical semiring)
            let boosted_weight = if boost > 0.0 {
                let boost_weight = W::from(TropicalWeight::new(-boost));
                weight.clone().times(&boost_weight)
            } else {
                weight.clone()
            };

            // Re-add the token with its original label
            if let Some(edge) = lattice.edges().iter().find(|e| {
                e.source.value() as usize == *source && e.target.value() as usize == *target
            }) {
                builder.add_correction_by_id(
                    *source,
                    *target,
                    edge.label,
                    boosted_weight,
                    metadata.clone(),
                );
            }
        }

        // Build the new lattice
        let num_nodes = lattice.num_nodes();
        Ok(builder.build(num_nodes))
    }
}

impl<W, B> CorrectionLayer<W, B> for PatternAwareLayer
where
    W: Semiring + From<TropicalWeight>,
    B: LatticeBackend + Clone,
{
    fn name(&self) -> &str {
        "pattern-aware"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        self.apply_impl(lattice)
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        // Can always apply, even with no patterns (will be a no-op)
        true
    }

    fn estimated_reduction(&self) -> f64 {
        PatternAwareLayer::estimated_reduction(self)
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
        let rparen = backend.intern(")");
        let colon = backend.intern(":");

        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        builder.add_correction_by_id(0, 1, def, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, foo, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(2, 3, lparen, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(3, 4, rparen, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(4, 5, colon, TropicalWeight::one(), EdgeMetadata::default());
        builder.build(5)
    }

    #[test]
    fn test_pattern_boost_creation() {
        let pattern = PatternBoost::new(vec!["def", "foo", "(", ")"], 1.0)
            .with_id(42)
            .with_support(100)
            .with_name("function_def");

        assert_eq!(pattern.len(), 4);
        assert!(!pattern.is_empty());
        assert_eq!(pattern.pattern_id, 42);
        assert_eq!(pattern.support, 100);
        assert_eq!(pattern.name, Some("function_def".to_string()));
    }

    #[test]
    fn test_config_default() {
        let config = PatternAwareConfig::default();
        assert!(config.patterns.is_empty());
        assert_eq!(config.min_pattern_length, 2);
        assert_eq!(config.max_pattern_length, 10);
        assert!(config.longest_match_only);
    }

    #[test]
    fn test_config_with_patterns() {
        let config = PatternAwareConfig::new()
            .with_pattern(vec!["def", "foo"], 0.5)
            .with_pattern(vec!["class", "bar"], 0.8);

        assert_eq!(config.patterns.len(), 2);
    }

    #[test]
    fn test_config_find_best_pattern() {
        let config = PatternAwareConfig::new()
            .with_pattern(vec!["def", "foo"], 0.5)
            .with_pattern(vec!["def", "foo", "(", ")"], 1.0)
            .with_pattern(vec!["class", "bar"], 0.8);

        let tokens = vec!["def", "foo", "(", ")"];
        let best = config.find_best_pattern(&tokens);

        assert!(best.is_some());
        let pattern = best.expect("layers/code_correction/pattern.rs: required value was None/Err");
        assert_eq!(pattern.len(), 4); // Should find the longest match
        assert!((pattern.boost - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_config_patterns_starting_with() {
        let config = PatternAwareConfig::new()
            .with_pattern(vec!["def", "foo"], 0.5)
            .with_pattern(vec!["def", "bar"], 0.6)
            .with_pattern(vec!["class", "baz"], 0.7);

        let def_patterns: Vec<_> = config.patterns_starting_with("def").collect();
        assert_eq!(def_patterns.len(), 2);

        let class_patterns: Vec<_> = config.patterns_starting_with("class").collect();
        assert_eq!(class_patterns.len(), 1);
    }

    #[test]
    fn test_python_patterns() {
        let config = PatternAwareConfig::python_patterns();
        assert!(!config.patterns.is_empty());

        // Should have patterns for common Python constructs
        let def_patterns: Vec<_> = config.patterns_starting_with("def").collect();
        assert!(!def_patterns.is_empty());
    }

    #[test]
    fn test_rust_patterns() {
        let config = PatternAwareConfig::rust_patterns();
        assert!(!config.patterns.is_empty());

        let fn_patterns: Vec<_> = config.patterns_starting_with("fn").collect();
        assert!(!fn_patterns.is_empty());
    }

    #[test]
    fn test_rholang_patterns() {
        let config = PatternAwareConfig::rholang_patterns();
        assert!(!config.patterns.is_empty());

        let new_patterns: Vec<_> = config.patterns_starting_with("new").collect();
        assert!(!new_patterns.is_empty());
    }

    #[test]
    fn test_metta_patterns() {
        let config = PatternAwareConfig::metta_patterns();
        assert!(!config.patterns.is_empty());
    }

    #[test]
    fn test_layer_creation() {
        let layer = PatternAwareLayer::new(PatternAwareConfig::python_patterns());
        assert!(layer.num_patterns() > 0);
    }

    #[test]
    fn test_layer_factory_methods() {
        let python = PatternAwareLayer::python();
        assert!(python.num_patterns() > 0);

        let rust = PatternAwareLayer::rust();
        assert!(rust.num_patterns() > 0);

        let rholang = PatternAwareLayer::rholang();
        assert!(rholang.num_patterns() > 0);

        let metta = PatternAwareLayer::metta();
        assert!(metta.num_patterns() > 0);
    }

    #[test]
    fn test_layer_name() {
        let layer = PatternAwareLayer::python();
        assert_eq!(
            <PatternAwareLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::name(&layer),
            "pattern-aware"
        );
    }

    #[test]
    fn test_layer_apply() {
        let layer = PatternAwareLayer::new(
            PatternAwareConfig::new().with_pattern(vec!["def", "foo", "(", ")"], 1.0),
        );

        let lattice = build_test_lattice();
        let result = <PatternAwareLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );

        assert!(result.is_ok());
        let boosted = result.expect("should apply");
        // Should have same number of edges (boosting doesn't add/remove)
        assert_eq!(boosted.num_edges(), lattice.num_edges());
    }

    #[test]
    fn test_layer_apply_empty() {
        let layer = PatternAwareLayer::python();

        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let empty_lattice = builder.build(0);

        let result = <PatternAwareLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer,
            &empty_lattice,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_layer_apply_no_patterns() {
        let layer = PatternAwareLayer::new(PatternAwareConfig::new());
        let lattice = build_test_lattice();

        let result = <PatternAwareLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );
        assert!(result.is_ok());

        let unchanged = result.expect("should apply");
        assert_eq!(unchanged.num_edges(), lattice.num_edges());
    }

    #[test]
    fn test_layer_estimated_reduction() {
        let layer = PatternAwareLayer::python();
        assert!((layer.estimated_reduction() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_max_boost_cap() {
        let config = PatternAwareConfig::new()
            .with_max_boost(2.0)
            .with_pattern(vec!["def", "foo"], 10.0); // Very high boost

        assert!((config.max_boost - 2.0).abs() < 0.001);
    }
}
