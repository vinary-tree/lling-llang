//! LaTeX syntax correction layer.
//!
//! Provides CFG-based filtering for LaTeX documents, combining grammar parsing
//! with structural validation and optional repair suggestions.

use std::sync::Arc;

use crate::backend::LatticeBackend;
use crate::cfg::EarleyParser;
use crate::lattice::{Lattice, LatticeBuilder};
use crate::semiring::Semiring;

use super::grammar::LatexGrammar;
use super::repair::{CompositeRepairStrategy, RepairStrategy, RepairSuggestion};
use super::validator::{LatexValidator, ValidationResult};
use crate::layers::traits::{CorrectionLayer, LayerError, LayerResult};

/// Configuration for the LaTeX syntax layer.
#[derive(Clone)]
pub struct LatexSyntaxConfig {
    /// Whether to prune edges that don't parse.
    pub prune_ungrammatical: bool,
    /// Whether to run structural validation after parsing.
    pub validate_structure: bool,
    /// Whether to generate repair suggestions for errors.
    pub generate_repairs: bool,
    /// Maximum number of repair suggestions per issue.
    pub max_repairs_per_issue: usize,
    /// Whether to apply high-confidence repairs automatically.
    pub auto_repair: bool,
    /// Minimum confidence threshold for auto-repair.
    pub auto_repair_threshold: f32,
}

impl Default for LatexSyntaxConfig {
    fn default() -> Self {
        Self {
            prune_ungrammatical: true,
            validate_structure: true,
            generate_repairs: true,
            max_repairs_per_issue: 3,
            auto_repair: false,
            auto_repair_threshold: 0.9,
        }
    }
}

impl LatexSyntaxConfig {
    /// Create a strict configuration that prunes aggressively.
    pub fn strict() -> Self {
        Self {
            prune_ungrammatical: true,
            validate_structure: true,
            generate_repairs: true,
            max_repairs_per_issue: 5,
            auto_repair: false,
            auto_repair_threshold: 0.95,
        }
    }

    /// Create a lenient configuration that keeps more paths.
    pub fn lenient() -> Self {
        Self {
            prune_ungrammatical: false,
            validate_structure: true,
            generate_repairs: true,
            max_repairs_per_issue: 3,
            auto_repair: true,
            auto_repair_threshold: 0.85,
        }
    }

    /// Create a minimal configuration for fast processing.
    pub fn minimal() -> Self {
        Self {
            prune_ungrammatical: true,
            validate_structure: false,
            generate_repairs: false,
            max_repairs_per_issue: 0,
            auto_repair: false,
            auto_repair_threshold: 1.0,
        }
    }
}

/// LaTeX syntax correction layer.
///
/// Filters lattice paths based on LaTeX grammar rules and structural constraints.
/// Optionally generates repair suggestions for invalid paths.
///
/// # Example
///
/// ```ignore
/// use lling_llang::layers::latex::{LatexSyntaxLayer, LatexGrammar, LatexSyntaxConfig};
///
/// let grammar = LatexGrammar::standard()?;
/// let layer = LatexSyntaxLayer::new(grammar);
///
/// let filtered = layer.apply(&lattice)?;
/// ```
pub struct LatexSyntaxLayer {
    /// The LaTeX grammar for parsing.
    grammar: LatexGrammar,
    /// Structural validator.
    validator: LatexValidator,
    /// Repair strategy for generating suggestions.
    repair_strategy: Option<Arc<dyn RepairStrategy>>,
    /// Configuration options.
    config: LatexSyntaxConfig,
    /// Cached repair suggestions from last apply.
    last_repairs: std::sync::Mutex<Vec<RepairSuggestion>>,
}

impl LatexSyntaxLayer {
    /// Create a new LaTeX syntax layer with default configuration.
    pub fn new(grammar: LatexGrammar) -> Self {
        Self {
            grammar,
            validator: LatexValidator::new(),
            repair_strategy: Some(Arc::new(CompositeRepairStrategy::all())),
            config: LatexSyntaxConfig::default(),
            last_repairs: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Create a new layer with custom configuration.
    pub fn with_config(grammar: LatexGrammar, config: LatexSyntaxConfig) -> Self {
        let repair_strategy = if config.generate_repairs {
            Some(Arc::new(CompositeRepairStrategy::all()) as Arc<dyn RepairStrategy>)
        } else {
            None
        };

        Self {
            grammar,
            validator: LatexValidator::new(),
            repair_strategy,
            config,
            last_repairs: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Set a custom validator.
    pub fn with_validator(mut self, validator: LatexValidator) -> Self {
        self.validator = validator;
        self
    }

    /// Set a custom repair strategy.
    pub fn with_repair_strategy<S: RepairStrategy + 'static>(mut self, strategy: S) -> Self {
        self.repair_strategy = Some(Arc::new(strategy));
        self
    }

    /// Disable repair suggestions.
    pub fn without_repairs(mut self) -> Self {
        self.repair_strategy = None;
        self.config.generate_repairs = false;
        self
    }

    /// Get the grammar used by this layer.
    pub fn grammar(&self) -> &LatexGrammar {
        &self.grammar
    }

    /// Get the current configuration.
    pub fn config(&self) -> &LatexSyntaxConfig {
        &self.config
    }

    /// Get repair suggestions from the last apply call.
    pub fn last_repairs(&self) -> Vec<RepairSuggestion> {
        self.last_repairs
            .lock()
            .expect("layers/latex/syntax.rs: required value was None/Err")
            .clone()
    }

    /// Validate a token sequence using the structural validator.
    pub fn validate_tokens(&self, tokens: &[&str]) -> ValidationResult {
        self.validator.validate(tokens)
    }

    /// Generate repair suggestions for validation issues.
    fn generate_repairs(
        &self,
        validation: &ValidationResult,
        context: &[&str],
    ) -> Vec<RepairSuggestion> {
        let Some(strategy) = &self.repair_strategy else {
            return Vec::new();
        };

        let mut all_repairs = Vec::new();

        for issue in &validation.issues {
            let mut repairs = strategy.suggest(issue, context);
            repairs.truncate(self.config.max_repairs_per_issue);
            all_repairs.extend(repairs);
        }

        // Sort by confidence
        all_repairs.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        all_repairs
    }
}

// Implement Send + Sync for thread safety
unsafe impl Send for LatexSyntaxLayer {}
unsafe impl Sync for LatexSyntaxLayer {}

impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for LatexSyntaxLayer {
    fn name(&self) -> &str {
        "latex-syntax"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        // Clear previous repairs
        self.last_repairs
            .lock()
            .expect("layers/latex/syntax.rs: required value was None/Err")
            .clear();

        // Handle empty lattice
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // Phase 1: CFG-based parsing
        let parser = EarleyParser::new(self.grammar.grammar());
        let parse_result = parser.parse_lattice(lattice);

        let (filtered_lattice, _used_edges) = match parse_result {
            Ok(forest) => {
                // Collect edges used in valid parses
                let used_edges = forest.collect_used_edges();

                if !self.config.prune_ungrammatical {
                    // Keep all edges
                    (lattice.clone(), None)
                } else {
                    // Build lattice with only used edges
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

                    let end_pos = lattice.end().0 as usize;
                    (new_builder.build(end_pos), Some(used_edges))
                }
            }
            Err(e) => {
                // Parse failed - handle based on configuration
                if self.config.prune_ungrammatical {
                    return Err(LayerError::ParseError(format!(
                        "LaTeX parse failed: {:?}",
                        e
                    )));
                }
                // Keep original lattice if not pruning
                (lattice.clone(), None)
            }
        };

        // Phase 2: Structural validation (optional)
        if self.config.validate_structure {
            // Extract token sequence from best path for validation
            // This is a simplified approach - a more sophisticated implementation
            // would validate all paths or representative paths
            let tokens: Vec<String> = filtered_lattice
                .edges()
                .iter()
                .filter_map(|e| {
                    filtered_lattice
                        .backend()
                        .lookup(e.label)
                        .map(|s| s.to_string())
                })
                .collect();

            let token_refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
            let validation = self.validator.validate(&token_refs);

            // Generate repairs if there are issues
            if !validation.is_valid && self.config.generate_repairs {
                let repairs = self.generate_repairs(&validation, &token_refs);
                *self
                    .last_repairs
                    .lock()
                    .expect("layers/latex/syntax.rs: required value was None/Err") = repairs;
            }

            // For strict mode with errors, report them
            if !validation.is_valid && self.config.prune_ungrammatical && validation.has_errors() {
                let error_msg = validation
                    .errors()
                    .map(|e| e.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(LayerError::ParseError(format!(
                    "LaTeX validation failed: {}",
                    error_msg
                )));
            }
        }

        Ok(filtered_lattice)
    }

    fn can_apply(&self, lattice: &Lattice<W, B>) -> bool {
        // Can apply if lattice is non-empty or is a valid empty lattice
        !lattice.is_empty() || lattice.start() == lattice.end()
    }

    fn estimated_reduction(&self) -> f64 {
        // LaTeX syntax filtering typically reduces paths moderately
        // More conservative than general CFG filtering since LaTeX is structured
        if self.config.prune_ungrammatical {
            0.15
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_layer_name() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let layer = LatexSyntaxLayer::new(grammar);

        type L = LatexSyntaxLayer;
        type W = TropicalWeight;
        type B = HashMapBackend;

        assert_eq!(<L as CorrectionLayer<W, B>>::name(&layer), "latex-syntax");
    }

    #[test]
    fn test_layer_creation() {
        let grammar = LatexGrammar::standard().expect("grammar should build");
        let layer = LatexSyntaxLayer::new(grammar);

        assert!(layer.config.prune_ungrammatical);
        assert!(layer.config.validate_structure);
        assert!(layer.config.generate_repairs);
    }

    #[test]
    fn test_config_presets() {
        let strict = LatexSyntaxConfig::strict();
        assert!(strict.prune_ungrammatical);
        assert!(!strict.auto_repair);

        let lenient = LatexSyntaxConfig::lenient();
        assert!(!lenient.prune_ungrammatical);
        assert!(lenient.auto_repair);

        let minimal = LatexSyntaxConfig::minimal();
        assert!(minimal.prune_ungrammatical);
        assert!(!minimal.validate_structure);
        assert!(!minimal.generate_repairs);
    }

    #[test]
    fn test_with_custom_validator() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let validator = LatexValidator::new()
            .with_environment_validation(false)
            .with_nested_math(true);

        let layer = LatexSyntaxLayer::new(grammar).with_validator(validator);

        // Layer should be created successfully
        type L = LatexSyntaxLayer;
        type W = TropicalWeight;
        type B = HashMapBackend;
        assert_eq!(<L as CorrectionLayer<W, B>>::name(&layer), "latex-syntax");
    }

    #[test]
    fn test_without_repairs() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let layer = LatexSyntaxLayer::new(grammar).without_repairs();

        assert!(layer.repair_strategy.is_none());
        assert!(!layer.config.generate_repairs);
    }

    #[test]
    fn test_estimated_reduction_prune_mode() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let layer = LatexSyntaxLayer::new(grammar);

        type L = LatexSyntaxLayer;
        type W = TropicalWeight;
        type B = HashMapBackend;

        let reduction = <L as CorrectionLayer<W, B>>::estimated_reduction(&layer);
        assert!((reduction - 0.15).abs() < 0.01);
    }

    #[test]
    fn test_estimated_reduction_no_prune_mode() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let config = LatexSyntaxConfig::lenient();
        let layer = LatexSyntaxLayer::with_config(grammar, config);

        type L = LatexSyntaxLayer;
        type W = TropicalWeight;
        type B = HashMapBackend;

        let reduction = <L as CorrectionLayer<W, B>>::estimated_reduction(&layer);
        assert!((reduction - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_can_apply_empty_lattice() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let layer = LatexSyntaxLayer::new(grammar);

        // Empty lattice (start == end)
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let empty_lattice = builder.build(0);

        assert!(layer.can_apply(&empty_lattice));
    }

    #[test]
    fn test_apply_empty_lattice() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let layer = LatexSyntaxLayer::new(grammar);

        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let empty_lattice = builder.build(0);

        let result = layer.apply(&empty_lattice);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_tokens() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let layer = LatexSyntaxLayer::new(grammar);

        // Valid braces
        let valid = layer.validate_tokens(&["{", "content", "}"]);
        assert!(valid.is_valid);

        // Invalid braces
        let invalid = layer.validate_tokens(&["{", "content"]);
        assert!(!invalid.is_valid);
    }

    #[test]
    fn test_last_repairs_initially_empty() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let layer = LatexSyntaxLayer::new(grammar);

        assert!(layer.last_repairs().is_empty());
    }

    #[test]
    fn test_config_access() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let config = LatexSyntaxConfig::strict();
        let layer = LatexSyntaxLayer::with_config(grammar, config);

        assert!(layer.config().prune_ungrammatical);
        assert!(layer.config().validate_structure);
    }

    #[test]
    fn test_grammar_access() {
        let grammar = LatexGrammar::minimal().expect("grammar should build");
        let layer = LatexSyntaxLayer::new(grammar);

        // Should be able to access the grammar
        assert!(layer.grammar().grammar().num_productions() > 0);
    }
}
