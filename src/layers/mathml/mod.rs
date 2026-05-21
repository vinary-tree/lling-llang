//! MathML semantic correction layer for lling-llang.
//!
//! This module provides semantic type checking and homoglyph disambiguation
//! for mathematical expressions based on Content MathML semantics.
//!
//! # Overview
//!
//! The MathML semantic layer consists of several components:
//!
//! - **Type System** (`types`): Defines mathematical types like `Number`, `Function`,
//!   `Relation`, etc., along with type environments and type checking results.
//!
//! - **Type Checker** (`checker`): Performs type inference and checking on mathematical
//!   expressions, validating operator arities and argument types.
//!
//! - **Homoglyph Disambiguator** (`homoglyph`): Handles visually similar characters
//!   with different meanings (e.g., `x` vs `×`, `0` vs `O`).
//!
//! - **Semantic Layer** (`semantic`): The main `CorrectionLayer` implementation that
//!   combines type checking and homoglyph disambiguation for lattice filtering.
//!
//! # Example
//!
//! ```ignore
//! use lling_llang::layers::mathml::{MathMLSemanticLayer, MathMLSemanticConfig};
//!
//! // Create a semantic layer with default configuration
//! let layer = MathMLSemanticLayer::new();
//!
//! // Or with custom configuration
//! let layer = MathMLSemanticLayer::with_config(MathMLSemanticConfig::strict());
//!
//! // Apply to a lattice
//! let result = layer.apply(&lattice)?;
//!
//! // Check analysis results
//! for result in layer.last_results() {
//!     if !result.is_valid {
//!         for issue in result.errors() {
//!             println!("Error: {}", issue.message);
//!         }
//!     }
//! }
//! ```
//!
//! # Type System
//!
//! The type system supports common mathematical types:
//!
//! ```ignore
//! use lling_llang::layers::mathml::types::{MathType, Arity};
//!
//! // Function type: sin : Number -> Number
//! let sin_type = MathType::Function {
//!     arity: Arity::Unary,
//!     domain: vec![MathType::Number],
//!     codomain: Box::new(MathType::Number),
//! };
//!
//! // Vector type: R^n
//! let vector_type = MathType::Vector {
//!     element: Box::new(MathType::Number),
//!     dimension: Some(3),
//! };
//! ```
//!
//! # Homoglyph Disambiguation
//!
//! The disambiguator uses context to determine the meaning of ambiguous characters:
//!
//! ```ignore
//! use lling_llang::layers::mathml::homoglyph::{HomoglyphDisambiguator, MathContext};
//!
//! let disambiguator = HomoglyphDisambiguator::new();
//!
//! // After a number, 'x' is likely multiplication
//! let context = MathContext {
//!     prev_was_number: true,
//!     in_math_mode: true,
//!     ..Default::default()
//! };
//!
//! let meaning = disambiguator.disambiguate('x', &context);
//! // Returns GlyphMeaning::Multiplication
//! ```
//!
//! # Configuration
//!
//! The layer supports several configuration presets:
//!
//! - `MathMLSemanticConfig::default()`: Balanced configuration
//! - `MathMLSemanticConfig::strict()`: Aggressive pruning, normalization enabled
//! - `MathMLSemanticConfig::lenient()`: Keep more paths, no normalization
//! - `MathMLSemanticConfig::minimal()`: Fast processing, minimal checking

pub mod checker;
pub mod homoglyph;
pub mod semantic;
pub mod types;

// Re-export main types
pub use types::{
    Arity, MathType, SemanticCategory, TypeEnvironment, TypeError, TypeErrorKind, TypeResult,
    TypeSignature, TypeWarning, TypeWarningKind,
};

pub use checker::{MathTypeChecker, TypeCheckerConfig};

pub use homoglyph::{
    DisambiguatorConfig, GlyphMeaning, HomoglyphDisambiguator, HomoglyphSet, MathContext,
    MathDomain,
};

pub use semantic::{
    DisambiguationDecision, IssueSeverity, MathMLSemanticConfig, MathMLSemanticLayer,
    SemanticIssue, SemanticIssueKind, SemanticResult,
};
