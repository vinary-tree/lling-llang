//! MathML semantic correction layer.
//!
//! Provides semantic type checking and homoglyph disambiguation for
//! mathematical expressions in LaTeX.

use std::sync::Mutex;

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticeBuilder};
use crate::semiring::Semiring;

use super::super::traits::{CorrectionLayer, LayerError, LayerResult};
use super::checker::{MathTypeChecker, TypeCheckerConfig};
use super::homoglyph::{DisambiguatorConfig, GlyphMeaning, HomoglyphDisambiguator, MathContext};
use super::types::{MathType, TypeErrorKind, TypeWarningKind};

/// Configuration for the MathML semantic layer.
#[derive(Clone)]
pub struct MathMLSemanticConfig {
    /// Whether to perform type checking.
    pub check_types: bool,
    /// Whether to disambiguate homoglyphs.
    pub disambiguate_homoglyphs: bool,
    /// Whether to prune paths with type errors.
    pub prune_type_errors: bool,
    /// Whether to normalize homoglyphs.
    pub normalize_homoglyphs: bool,
    /// Minimum confidence for disambiguation.
    pub disambiguation_threshold: f32,
    /// Whether to track warnings.
    pub track_warnings: bool,
}

impl Default for MathMLSemanticConfig {
    fn default() -> Self {
        Self {
            check_types: true,
            disambiguate_homoglyphs: true,
            prune_type_errors: true,
            normalize_homoglyphs: false,
            disambiguation_threshold: 0.5,
            track_warnings: true,
        }
    }
}

impl MathMLSemanticConfig {
    /// Create a strict configuration that aggressively prunes invalid paths.
    pub fn strict() -> Self {
        Self {
            check_types: true,
            disambiguate_homoglyphs: true,
            prune_type_errors: true,
            normalize_homoglyphs: true,
            disambiguation_threshold: 0.7,
            track_warnings: true,
        }
    }

    /// Create a lenient configuration that keeps more paths.
    pub fn lenient() -> Self {
        Self {
            check_types: true,
            disambiguate_homoglyphs: true,
            prune_type_errors: false,
            normalize_homoglyphs: false,
            disambiguation_threshold: 0.3,
            track_warnings: true,
        }
    }

    /// Create a minimal configuration for fast processing.
    pub fn minimal() -> Self {
        Self {
            check_types: false,
            disambiguate_homoglyphs: true,
            prune_type_errors: false,
            normalize_homoglyphs: true,
            disambiguation_threshold: 0.5,
            track_warnings: false,
        }
    }
}

/// Semantic issue found during analysis.
#[derive(Debug, Clone)]
pub struct SemanticIssue {
    /// Kind of issue.
    pub kind: SemanticIssueKind,
    /// Issue message.
    pub message: String,
    /// Position in token sequence.
    pub position: Option<usize>,
    /// Severity level.
    pub severity: IssueSeverity,
}

impl SemanticIssue {
    /// Create a new semantic issue.
    pub fn new(kind: SemanticIssueKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            position: None,
            severity: IssueSeverity::Error,
        }
    }

    /// Set position.
    pub fn at(mut self, pos: usize) -> Self {
        self.position = Some(pos);
        self
    }

    /// Set severity.
    pub fn with_severity(mut self, severity: IssueSeverity) -> Self {
        self.severity = severity;
        self
    }
}

/// Kind of semantic issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticIssueKind {
    /// Type mismatch in expression.
    TypeMismatch,
    /// Wrong arity for function/operator.
    ArityMismatch,
    /// Undefined variable.
    UndefinedVariable,
    /// Division by zero.
    DivisionByZero,
    /// Ambiguous homoglyph.
    AmbiguousGlyph,
    /// Invalid expression structure.
    InvalidStructure,
}

/// Severity of a semantic issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Informational only.
    Info,
    /// Warning (non-fatal).
    Warning,
    /// Error (may cause pruning).
    Error,
}

/// Result from semantic analysis.
#[derive(Debug, Clone, Default)]
pub struct SemanticResult {
    /// Whether the expression is semantically valid.
    pub is_valid: bool,
    /// Inferred type.
    pub inferred_type: Option<MathType>,
    /// Issues found.
    pub issues: Vec<SemanticIssue>,
    /// Disambiguation decisions made.
    pub disambiguations: Vec<DisambiguationDecision>,
}

impl SemanticResult {
    /// Create a valid result.
    pub fn ok(ty: MathType) -> Self {
        Self {
            is_valid: true,
            inferred_type: Some(ty),
            issues: Vec::new(),
            disambiguations: Vec::new(),
        }
    }

    /// Create an invalid result.
    pub fn invalid(issue: SemanticIssue) -> Self {
        Self {
            is_valid: false,
            inferred_type: None,
            issues: vec![issue],
            disambiguations: Vec::new(),
        }
    }

    /// Check if there are errors.
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error)
    }

    /// Get error issues.
    pub fn errors(&self) -> impl Iterator<Item = &SemanticIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
    }

    /// Get warning issues.
    pub fn warnings(&self) -> impl Iterator<Item = &SemanticIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
    }

    /// Add an issue.
    pub fn add_issue(&mut self, issue: SemanticIssue) {
        if issue.severity == IssueSeverity::Error {
            self.is_valid = false;
        }
        self.issues.push(issue);
    }

    /// Add a disambiguation decision.
    pub fn add_disambiguation(&mut self, decision: DisambiguationDecision) {
        self.disambiguations.push(decision);
    }
}

/// A disambiguation decision for a homoglyph.
#[derive(Debug, Clone)]
pub struct DisambiguationDecision {
    /// Original glyph.
    pub original: char,
    /// Chosen meaning.
    pub meaning: GlyphMeaning,
    /// Confidence in the decision.
    pub confidence: f32,
    /// Position in input.
    pub position: usize,
}

/// MathML semantic correction layer.
///
/// Filters lattice paths based on semantic type checking and homoglyph disambiguation.
///
/// # Example
///
/// ```ignore
/// use lling_llang::layers::mathml::{MathMLSemanticLayer, MathMLSemanticConfig};
///
/// let layer = MathMLSemanticLayer::new();
/// let filtered = layer.apply(&lattice)?;
/// ```
pub struct MathMLSemanticLayer {
    /// Type checker for mathematical expressions.
    type_checker: Mutex<MathTypeChecker>,
    /// Homoglyph disambiguator.
    disambiguator: HomoglyphDisambiguator,
    /// Configuration.
    config: MathMLSemanticConfig,
    /// Results from last apply.
    last_results: Mutex<Vec<SemanticResult>>,
}

impl MathMLSemanticLayer {
    /// Create a new MathML semantic layer with default configuration.
    pub fn new() -> Self {
        Self {
            type_checker: Mutex::new(MathTypeChecker::new()),
            disambiguator: HomoglyphDisambiguator::new(),
            config: MathMLSemanticConfig::default(),
            last_results: Mutex::new(Vec::new()),
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: MathMLSemanticConfig) -> Self {
        Self {
            type_checker: Mutex::new(MathTypeChecker::new()),
            disambiguator: HomoglyphDisambiguator::new(),
            config,
            last_results: Mutex::new(Vec::new()),
        }
    }

    /// Create with custom type checker configuration.
    pub fn with_type_checker_config(mut self, config: TypeCheckerConfig) -> Self {
        self.type_checker = Mutex::new(MathTypeChecker::with_config(config));
        self
    }

    /// Create with custom disambiguator configuration.
    pub fn with_disambiguator_config(mut self, config: DisambiguatorConfig) -> Self {
        self.disambiguator = HomoglyphDisambiguator::with_config(config);
        self
    }

    /// Get the current configuration.
    pub fn config(&self) -> &MathMLSemanticConfig {
        &self.config
    }

    /// Get the results from the last apply call.
    pub fn last_results(&self) -> Vec<SemanticResult> {
        self.last_results
            .lock()
            .expect("layers/mathml/semantic.rs: required value was None/Err")
            .clone()
    }

    /// Analyze a token sequence for semantic validity.
    pub fn analyze(&self, tokens: &[&str]) -> SemanticResult {
        let mut result = SemanticResult {
            is_valid: true,
            inferred_type: None,
            issues: Vec::new(),
            disambiguations: Vec::new(),
        };

        // Build context for disambiguation
        let mut context = MathContext::default();
        context.in_math_mode = true; // Assume math mode for MathML layer

        // Phase 1: Disambiguate homoglyphs
        if self.config.disambiguate_homoglyphs {
            self.disambiguate_tokens(tokens, &mut context, &mut result);
        }

        // Phase 2: Type check
        if self.config.check_types {
            self.type_check_tokens(tokens, &mut result);
        }

        result
    }

    /// Disambiguate homoglyphs in token sequence.
    fn disambiguate_tokens(
        &self,
        tokens: &[&str],
        context: &mut MathContext,
        result: &mut SemanticResult,
    ) {
        for (pos, token) in tokens.iter().enumerate() {
            for c in token.chars() {
                if self.disambiguator.is_ambiguous(c) {
                    let meaning = self.disambiguator.disambiguate(c, context);

                    let confidence = self.disambiguation_confidence(c, &meaning, context);

                    // Record decision
                    result.add_disambiguation(DisambiguationDecision {
                        original: c,
                        meaning: meaning.clone(),
                        confidence,
                        position: pos,
                    });

                    // Add warning if confidence is low
                    if confidence < self.config.disambiguation_threshold {
                        result.add_issue(
                            SemanticIssue::new(
                                SemanticIssueKind::AmbiguousGlyph,
                                format!("Ambiguous glyph '{}' with low confidence", c),
                            )
                            .at(pos)
                            .with_severity(IssueSeverity::Warning),
                        );
                    }
                }
            }

            // Update context for next token
            self.update_context(context, token);
        }
    }

    /// Update context based on current token.
    fn update_context(&self, context: &mut MathContext, token: &str) {
        // Check if token is a number
        context.prev_was_number = token.parse::<f64>().is_ok();

        // Check if token is an operator
        context.prev_was_operator =
            matches!(token, "+" | "-" | "*" | "/" | "=" | "<" | ">" | "^" | "_")
                || token.starts_with('\\')
                    && matches!(token, "\\pm" | "\\mp" | "\\times" | "\\div" | "\\cdot");

        // Store previous token
        context.prev_token = Some(token.to_string());
    }

    /// Type check token sequence.
    fn type_check_tokens(&self, tokens: &[&str], result: &mut SemanticResult) {
        let mut checker = self
            .type_checker
            .lock()
            .expect("layers/mathml/semantic.rs: required value was None/Err");
        let type_result = checker.check(tokens);

        // Set inferred type
        result.inferred_type = Some(type_result.inferred_type.clone());

        // Convert type errors to semantic issues
        for error in &type_result.errors {
            let kind = match error.kind {
                TypeErrorKind::TypeMismatch => SemanticIssueKind::TypeMismatch,
                TypeErrorKind::ArityMismatch => SemanticIssueKind::ArityMismatch,
                TypeErrorKind::UndefinedVariable => SemanticIssueKind::UndefinedVariable,
                TypeErrorKind::DivisionByZero => SemanticIssueKind::DivisionByZero,
                TypeErrorKind::InvalidStructure => SemanticIssueKind::InvalidStructure,
                TypeErrorKind::InvalidOperator => SemanticIssueKind::InvalidStructure,
                TypeErrorKind::AmbiguousType => SemanticIssueKind::AmbiguousGlyph,
            };

            let mut issue = SemanticIssue::new(kind, &error.message);
            if let Some(pos) = error.position {
                issue = issue.at(pos);
            }
            result.add_issue(issue);
        }

        // Convert type warnings
        if self.config.track_warnings {
            for warning in &type_result.warnings {
                let mut issue =
                    SemanticIssue::new(warning_issue_kind(warning.kind), &warning.message)
                        .with_severity(IssueSeverity::Warning);
                if let Some(pos) = warning.position {
                    issue = issue.at(pos);
                }
                result.add_issue(issue);
            }
        }
    }

    /// Check if a token sequence should be pruned based on semantic analysis.
    fn should_prune(&self, result: &SemanticResult) -> bool {
        self.config.prune_type_errors && result.has_errors()
    }

    fn disambiguation_confidence(
        &self,
        glyph: char,
        meaning: &GlyphMeaning,
        context: &MathContext,
    ) -> f32 {
        if matches!(meaning, GlyphMeaning::Unknown) {
            return 0.15;
        }

        let Some(set) = self.disambiguator.get_confusion_set(glyph) else {
            return 1.0;
        };

        if set.meanings.len() == 1 {
            return 0.95;
        }

        let mut confidence = 0.55f32;
        confidence -= ((set.meanings.len().saturating_sub(2)) as f32 * 0.05).min(0.20);

        match meaning {
            GlyphMeaning::Multiplication => {
                if matches!(glyph, '×' | '⋅' | '∙' | '✕' | '✖' | '⨯') {
                    confidence += 0.25;
                }
                if context.prev_was_number || context.prev_token.as_deref() == Some(")") {
                    confidence += 0.15;
                }
                if context.in_math_mode {
                    confidence += 0.10;
                }
            }
            GlyphMeaning::Variable(_) => {
                if glyph.is_alphabetic() {
                    confidence += 0.20;
                }
                if context.prev_was_operator || context.prev_token.is_none() {
                    confidence += 0.10;
                }
            }
            GlyphMeaning::Subtraction => {
                if context.prev_was_number || context.prev_token.as_deref() == Some(")") {
                    confidence += 0.25;
                }
            }
            GlyphMeaning::UnaryMinus => {
                if context.prev_was_operator || context.prev_token.is_none() {
                    confidence += 0.25;
                }
            }
            GlyphMeaning::Digit(_) => {
                if glyph.is_ascii_digit() {
                    confidence += 0.20;
                }
                if context.prev_was_number {
                    confidence += 0.15;
                }
            }
            GlyphMeaning::DecimalPoint => {
                if context.prev_was_number
                    && context
                        .next_token
                        .as_deref()
                        .and_then(|next| next.chars().next())
                        .map(|c| c.is_ascii_digit())
                        .unwrap_or(false)
                {
                    confidence += 0.30;
                }
            }
            GlyphMeaning::Prime => {
                if context.in_math_mode && !context.prev_was_number && !context.prev_was_operator {
                    confidence += 0.25;
                }
            }
            _ => {
                if context.in_math_mode {
                    confidence += 0.05;
                }
            }
        }

        confidence.clamp(0.05, 0.99)
    }

    /// Normalize a token by replacing homoglyphs with canonical forms.
    pub fn normalize_token(&self, token: &str) -> String {
        if self.config.normalize_homoglyphs {
            self.disambiguator.normalize(token)
        } else {
            token.to_string()
        }
    }
}

fn warning_issue_kind(kind: TypeWarningKind) -> SemanticIssueKind {
    match kind {
        TypeWarningKind::ImplicitCoercion => SemanticIssueKind::TypeMismatch,
        TypeWarningKind::UnusedVariable => SemanticIssueKind::UndefinedVariable,
        TypeWarningKind::Ambiguity => SemanticIssueKind::AmbiguousGlyph,
        TypeWarningKind::Deprecated => SemanticIssueKind::InvalidStructure,
    }
}

impl Default for MathMLSemanticLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for MathMLSemanticLayer {
    fn name(&self) -> &str {
        "mathml-semantic"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        // Clear previous results
        self.last_results
            .lock()
            .expect("layers/mathml/semantic.rs: required value was None/Err")
            .clear();

        // Handle empty lattice
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // Extract token sequence from edges
        let tokens: Vec<String> = lattice
            .edges()
            .iter()
            .filter_map(|e| lattice.backend().lookup(e.label).map(|s| s.to_string()))
            .collect();

        let token_refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();

        // Analyze the token sequence
        let analysis = self.analyze(&token_refs);

        // Store result
        self.last_results
            .lock()
            .expect("layers/mathml/semantic.rs: required value was None/Err")
            .push(analysis.clone());

        // Check if we should prune
        if self.should_prune(&analysis) {
            let error_msg = analysis
                .errors()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(LayerError::ParseError(format!(
                "Semantic analysis failed: {}",
                error_msg
            )));
        }

        // If normalizing homoglyphs, rebuild lattice with normalized tokens
        if self.config.normalize_homoglyphs {
            let mut new_builder = LatticeBuilder::new(lattice.backend().clone());

            for edge in lattice.edges() {
                let original = lattice.backend().lookup(edge.label);
                if let Some(token) = original {
                    let normalized = self.normalize_token(token);
                    // If normalized is different, intern the new string
                    let label = if normalized != token {
                        new_builder.backend_mut().intern(&normalized)
                    } else {
                        edge.label
                    };
                    new_builder.add_correction_by_id(
                        edge.source.0 as usize,
                        edge.target.0 as usize,
                        label,
                        edge.weight,
                        edge.metadata.clone(),
                    );
                } else {
                    // Keep original edge if lookup fails
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
            return Ok(new_builder.build(end_pos));
        }

        Ok(lattice.clone())
    }

    fn can_apply(&self, lattice: &Lattice<W, B>) -> bool {
        // Can apply if lattice is non-empty or is a valid empty lattice
        !lattice.is_empty() || lattice.start() == lattice.end()
    }

    fn estimated_reduction(&self) -> f64 {
        // Semantic analysis typically provides moderate filtering
        if self.config.prune_type_errors {
            0.20
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::EdgeMetadata;
    use crate::semiring::TropicalWeight;

    fn build_test_lattice(tokens: &[&str]) -> Lattice<TropicalWeight, HashMapBackend> {
        let mut backend = HashMapBackend::new();
        let ids: Vec<_> = tokens.iter().map(|t| backend.intern(t)).collect();

        let mut builder = LatticeBuilder::new(backend);
        for (i, &id) in ids.iter().enumerate() {
            builder.add_correction_by_id(
                i,
                i + 1,
                id,
                TropicalWeight::one(),
                EdgeMetadata::default(),
            );
        }

        builder.build(tokens.len())
    }

    #[test]
    fn test_layer_name() {
        let layer = MathMLSemanticLayer::new();

        type L = MathMLSemanticLayer;
        type W = TropicalWeight;
        type B = HashMapBackend;

        assert_eq!(
            <L as CorrectionLayer<W, B>>::name(&layer),
            "mathml-semantic"
        );
    }

    #[test]
    fn test_layer_creation() {
        let layer = MathMLSemanticLayer::new();

        assert!(layer.config.check_types);
        assert!(layer.config.disambiguate_homoglyphs);
        assert!(layer.config.prune_type_errors);
    }

    #[test]
    fn test_config_presets() {
        let strict = MathMLSemanticConfig::strict();
        assert!(strict.prune_type_errors);
        assert!(strict.normalize_homoglyphs);

        let lenient = MathMLSemanticConfig::lenient();
        assert!(!lenient.prune_type_errors);
        assert!(!lenient.normalize_homoglyphs);

        let minimal = MathMLSemanticConfig::minimal();
        assert!(!minimal.check_types);
        assert!(minimal.disambiguate_homoglyphs);
    }

    #[test]
    fn test_analyze_valid_expression() {
        let layer = MathMLSemanticLayer::new();

        let result = layer.analyze(&["\\sin", "{", "x", "}"]);
        // Should be valid since \sin is a known function
        assert!(result.is_valid || !result.has_errors());
    }

    #[test]
    fn test_analyze_number() {
        let layer = MathMLSemanticLayer::new();

        let result = layer.analyze(&["42"]);
        assert!(result.is_valid);
        assert_eq!(result.inferred_type, Some(MathType::Number));
    }

    #[test]
    fn test_analyze_greek_letter() {
        let layer = MathMLSemanticLayer::new();

        let result = layer.analyze(&["\\alpha"]);
        assert!(result.is_valid);
        assert_eq!(result.inferred_type, Some(MathType::Variable));
    }

    #[test]
    fn test_disambiguate_x() {
        let layer = MathMLSemanticLayer::new();

        // After number, x should be multiplication
        let result = layer.analyze(&["2", "x", "3"]);
        assert!(!result.disambiguations.is_empty());
    }

    #[test]
    fn test_estimated_reduction_prune_mode() {
        let layer = MathMLSemanticLayer::new();

        type L = MathMLSemanticLayer;
        type W = TropicalWeight;
        type B = HashMapBackend;

        let reduction = <L as CorrectionLayer<W, B>>::estimated_reduction(&layer);
        assert!((reduction - 0.20).abs() < 0.01);
    }

    #[test]
    fn test_estimated_reduction_no_prune_mode() {
        let config = MathMLSemanticConfig::lenient();
        let layer = MathMLSemanticLayer::with_config(config);

        type L = MathMLSemanticLayer;
        type W = TropicalWeight;
        type B = HashMapBackend;

        let reduction = <L as CorrectionLayer<W, B>>::estimated_reduction(&layer);
        assert!((reduction - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_can_apply_empty_lattice() {
        let layer = MathMLSemanticLayer::new();

        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let empty_lattice = builder.build(0);

        assert!(layer.can_apply(&empty_lattice));
    }

    #[test]
    fn test_apply_empty_lattice() {
        let layer = MathMLSemanticLayer::new();

        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let empty_lattice = builder.build(0);

        let result = layer.apply(&empty_lattice);
        assert!(result.is_ok());
    }

    #[test]
    fn test_apply_simple_lattice() {
        let layer = MathMLSemanticLayer::with_config(MathMLSemanticConfig::lenient());

        let lattice = build_test_lattice(&["\\sin", "{", "x", "}"]);
        let result = layer.apply(&lattice);

        assert!(result.is_ok());
    }

    #[test]
    fn test_normalize_token() {
        let config = MathMLSemanticConfig {
            normalize_homoglyphs: true,
            ..Default::default()
        };
        let layer = MathMLSemanticLayer::with_config(config);

        // Should normalize multiplication sign to x
        let normalized = layer.normalize_token("2×3");
        assert_eq!(normalized, "2x3");
    }

    #[test]
    fn test_normalize_disabled() {
        let config = MathMLSemanticConfig {
            normalize_homoglyphs: false,
            ..Default::default()
        };
        let layer = MathMLSemanticLayer::with_config(config);

        // Should not normalize when disabled
        let normalized = layer.normalize_token("2×3");
        assert_eq!(normalized, "2×3");
    }

    #[test]
    fn test_last_results_initially_empty() {
        let layer = MathMLSemanticLayer::new();
        assert!(layer.last_results().is_empty());
    }

    #[test]
    fn test_config_access() {
        let config = MathMLSemanticConfig::strict();
        let layer = MathMLSemanticLayer::with_config(config);

        assert!(layer.config().prune_type_errors);
        assert!(layer.config().normalize_homoglyphs);
    }

    #[test]
    fn test_semantic_issue() {
        let issue = SemanticIssue::new(SemanticIssueKind::TypeMismatch, "test error")
            .at(5)
            .with_severity(IssueSeverity::Warning);

        assert_eq!(issue.kind, SemanticIssueKind::TypeMismatch);
        assert_eq!(issue.position, Some(5));
        assert_eq!(issue.severity, IssueSeverity::Warning);
    }

    #[test]
    fn test_semantic_result() {
        let mut result = SemanticResult::ok(MathType::Number);
        assert!(result.is_valid);
        assert!(!result.has_errors());

        result.add_issue(SemanticIssue::new(SemanticIssueKind::TypeMismatch, "error"));
        assert!(!result.is_valid);
        assert!(result.has_errors());
    }

    #[test]
    fn test_disambiguation_decision() {
        let decision = DisambiguationDecision {
            original: 'x',
            meaning: GlyphMeaning::Multiplication,
            confidence: 0.8,
            position: 2,
        };

        assert_eq!(decision.original, 'x');
        assert!(matches!(decision.meaning, GlyphMeaning::Multiplication));
    }

    #[test]
    fn test_disambiguation_confidence_uses_context() {
        let layer = MathMLSemanticLayer::new();
        let numeric_context = MathContext {
            in_math_mode: true,
            prev_was_number: true,
            prev_token: Some("2".to_string()),
            ..Default::default()
        };
        let operator_context = MathContext {
            in_math_mode: true,
            prev_was_operator: true,
            prev_token: Some("+".to_string()),
            ..Default::default()
        };

        let multiplication =
            layer.disambiguation_confidence('×', &GlyphMeaning::Multiplication, &numeric_context);
        let variable = layer.disambiguation_confidence(
            'x',
            &GlyphMeaning::Variable("x".into()),
            &operator_context,
        );

        assert!(multiplication > 0.8);
        assert!(variable > 0.7);
    }

    #[test]
    fn test_warning_issue_kind_mapping() {
        assert_eq!(
            warning_issue_kind(TypeWarningKind::ImplicitCoercion),
            SemanticIssueKind::TypeMismatch
        );
        assert_eq!(
            warning_issue_kind(TypeWarningKind::Deprecated),
            SemanticIssueKind::InvalidStructure
        );
    }
}
