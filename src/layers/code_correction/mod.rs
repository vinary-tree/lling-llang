//! Pattern-aware code correction layer for programming language correction.
//!
//! This module provides syntax error recovery and grammar-constrained code
//! completion using WFST lattices combined with learned patterns from subtree mining.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Code Correction Layer Stack                          │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Pattern-Aware Layer                                                    │
//! │     ↑ Uses patterns from subtree mining to boost common idioms          │
//! │  Syntax Recovery Layer                                                  │
//! │     ↑ Uses grammar to insert/delete tokens for error recovery           │
//! │  Token Correction Layer                                                 │
//! │     ↑ Uses edit distance + pattern boost for token-level corrections    │
//! │  [Input Lattice]                                                        │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use lling_llang::layers::{LayerPipeline, CodeCorrectionLayer};
//! use lling_llang::layers::code_correction::{CodeCorrectionConfig, SyntaxRecoveryConfig};
//!
//! // Create a code correction layer with pattern support
//! let config = CodeCorrectionConfig::new("python")
//!     .with_syntax_recovery(SyntaxRecoveryConfig::default())
//!     .with_max_corrections(5);
//!
//! let layer = CodeCorrectionLayer::new(config);
//! let mut pipeline = LayerPipeline::new();
//! pipeline.add_layer(layer);
//!
//! let corrected = pipeline.apply(&code_lattice)?;
//! ```

mod config;
mod pattern;
mod syntax;

pub use config::{CodeCorrectionConfig, CodeCorrectionLanguage};
pub use pattern::{PatternAwareConfig, PatternAwareLayer, PatternBoost};
pub use syntax::{RecoveryStrategy, SyntaxRecoveryConfig, SyntaxRecoveryLayer};

use std::marker::PhantomData;

use crate::backend::LatticeBackend;
use crate::lattice::Lattice;
use crate::semiring::{Semiring, TropicalWeight};

use super::{CorrectionLayer, LayerResult, LayerStats};

/// Combined code correction layer that applies multiple correction strategies.
///
/// This layer chains together:
/// 1. Token-level corrections (via edit distance, delegated to caller)
/// 2. Syntax recovery (inserting/deleting tokens to fix parse errors)
/// 3. Pattern-aware boosting (using mined idioms to prefer common patterns)
///
/// # Type Parameters
///
/// * `W` - Weight semiring type (typically `TropicalWeight`)
/// * `B` - Lattice backend type (vocabulary interning)
pub struct CodeCorrectionLayer<W: Semiring, B: LatticeBackend> {
    config: CodeCorrectionConfig,
    syntax_layer: SyntaxRecoveryLayer,
    pattern_layer: Option<PatternAwareLayer>,
    _phantom: PhantomData<(W, B)>,
}

impl<W: Semiring, B: LatticeBackend> CodeCorrectionLayer<W, B> {
    /// Create a new code correction layer with the given configuration.
    pub fn new(config: CodeCorrectionConfig) -> Self {
        let syntax_layer =
            SyntaxRecoveryLayer::new(config.syntax_config.clone().unwrap_or_default());

        let pattern_layer = config.pattern_config.clone().map(PatternAwareLayer::new);

        Self {
            config,
            syntax_layer,
            pattern_layer,
            _phantom: PhantomData,
        }
    }

    /// Create with default configuration for the given language.
    pub fn for_language(language: &str) -> Self {
        Self::new(CodeCorrectionConfig::new(language))
    }

    /// Get the configuration.
    pub fn config(&self) -> &CodeCorrectionConfig {
        &self.config
    }

    /// Get the syntax recovery layer.
    pub fn syntax_layer(&self) -> &SyntaxRecoveryLayer {
        &self.syntax_layer
    }

    /// Get the pattern-aware layer, if configured.
    pub fn pattern_layer(&self) -> Option<&PatternAwareLayer> {
        self.pattern_layer.as_ref()
    }

    /// Check if pattern-aware correction is enabled.
    pub fn has_patterns(&self) -> bool {
        self.pattern_layer.is_some()
    }
}

impl<W, B> CorrectionLayer<W, B> for CodeCorrectionLayer<W, B>
where
    W: Semiring + From<TropicalWeight>,
    B: LatticeBackend + Clone,
{
    fn name(&self) -> &str {
        "code-correction"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        // Handle empty lattice
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // Stage 1: Syntax recovery (insert/delete tokens to fix parse errors)
        let after_syntax = self.syntax_layer.apply(lattice)?;

        // Stage 2: Pattern-aware boosting (if patterns are loaded)
        let result = match &self.pattern_layer {
            Some(pattern) => pattern.apply(&after_syntax)?,
            None => after_syntax,
        };

        Ok(result)
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        // Can apply to any non-empty lattice
        true
    }

    fn estimated_reduction(&self) -> f64 {
        // This layer typically doesn't reduce paths much, but may add recovery paths
        // Estimate based on syntax recovery adding ~10% more paths
        let syntax_factor = self.syntax_layer.estimated_expansion();
        let pattern_factor = self
            .pattern_layer
            .as_ref()
            .map(|p| p.estimated_reduction())
            .unwrap_or(1.0);

        syntax_factor * pattern_factor
    }

    fn apply_with_stats(
        &self,
        lattice: &Lattice<W, B>,
    ) -> LayerResult<(Lattice<W, B>, LayerStats)> {
        let start = std::time::Instant::now();
        let input_edges = lattice.num_edges();

        let result = self.apply(lattice)?;

        let output_edges = result.num_edges();
        let elapsed = start.elapsed();

        let stats = LayerStats {
            input_paths: 0, // Would need path counting
            output_paths: 0,
            input_edges,
            output_edges,
            time_us: elapsed.as_micros() as u64,
        };

        Ok((result, stats))
    }
}

impl<W: Semiring, B: LatticeBackend> Clone for CodeCorrectionLayer<W, B> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            syntax_layer: self.syntax_layer.clone(),
            pattern_layer: self.pattern_layer.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<W: Semiring, B: LatticeBackend> std::fmt::Debug for CodeCorrectionLayer<W, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeCorrectionLayer")
            .field("config", &self.config)
            .field("has_patterns", &self.has_patterns())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::{EdgeMetadata, LatticeBuilder};
    use crate::semiring::TropicalWeight;

    fn build_simple_lattice() -> Lattice<TropicalWeight, HashMapBackend> {
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
    fn test_code_correction_layer_creation() {
        let config = CodeCorrectionConfig::new("python");
        let layer: CodeCorrectionLayer<TropicalWeight, HashMapBackend> =
            CodeCorrectionLayer::new(config);

        assert_eq!(layer.config().language.as_str(), "python");
    }

    #[test]
    fn test_code_correction_layer_for_language() {
        let layer: CodeCorrectionLayer<TropicalWeight, HashMapBackend> =
            CodeCorrectionLayer::for_language("rust");

        assert_eq!(layer.config().language.as_str(), "rust");
    }

    #[test]
    fn test_code_correction_layer_name() {
        let layer: CodeCorrectionLayer<TropicalWeight, HashMapBackend> =
            CodeCorrectionLayer::for_language("python");

        assert_eq!(
            <CodeCorrectionLayer<TropicalWeight, HashMapBackend> as CorrectionLayer<
                TropicalWeight,
                HashMapBackend,
            >>::name(&layer),
            "code-correction"
        );
    }

    #[test]
    fn test_code_correction_layer_apply() {
        let layer: CodeCorrectionLayer<TropicalWeight, HashMapBackend> =
            CodeCorrectionLayer::for_language("python");

        let lattice = build_simple_lattice();
        let result = layer.apply(&lattice);

        assert!(result.is_ok());
        let corrected = result.expect("should apply");
        // Layer should not lose edges (it may add recovery paths)
        assert!(corrected.num_edges() >= lattice.num_edges());
    }

    #[test]
    fn test_code_correction_layer_empty_lattice() {
        let layer: CodeCorrectionLayer<TropicalWeight, HashMapBackend> =
            CodeCorrectionLayer::for_language("python");

        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let empty_lattice = builder.build(0);

        let result = layer.apply(&empty_lattice);
        assert!(result.is_ok());
    }

    #[test]
    fn test_code_correction_layer_with_stats() {
        let layer: CodeCorrectionLayer<TropicalWeight, HashMapBackend> =
            CodeCorrectionLayer::for_language("python");

        let lattice = build_simple_lattice();
        let result = layer.apply_with_stats(&lattice);

        assert!(result.is_ok());
        let (corrected, stats) = result.expect("should apply");
        assert_eq!(stats.input_edges, 5);
        assert!(stats.output_edges >= 5);
        assert!(corrected.num_edges() >= 5);
    }

    #[test]
    fn test_code_correction_layer_clone() {
        let config = CodeCorrectionConfig::new("python").with_max_corrections(10);
        let layer: CodeCorrectionLayer<TropicalWeight, HashMapBackend> =
            CodeCorrectionLayer::new(config);

        let cloned = layer.clone();
        assert_eq!(cloned.config().max_corrections_per_token, 10);
    }

    #[test]
    fn test_code_correction_layer_debug() {
        let layer: CodeCorrectionLayer<TropicalWeight, HashMapBackend> =
            CodeCorrectionLayer::for_language("python");

        let debug_str = format!("{:?}", layer);
        assert!(debug_str.contains("CodeCorrectionLayer"));
        assert!(debug_str.contains("has_patterns"));
    }
}
