//! LaTeX syntax correction layer.
//!
//! This module provides a correction layer for LaTeX documents, filtering and
//! repairing lattice paths based on LaTeX grammar rules.
//!
//! # Features
//!
//! - CFG-based filtering for LaTeX document structure
//! - Environment begin/end matching
//! - Brace and delimiter balancing
//! - Math mode validation
//! - Syntax repair suggestions
//!
//! # Architecture
//!
//! The LaTeX layer uses a multi-pass approach:
//!
//! 1. **Grammar Filtering**: Uses Earley parsing to filter paths that don't
//!    conform to LaTeX grammar.
//!
//! 2. **Structural Validation**: Checks begin/end pairing, brace matching,
//!    and delimiter balance.
//!
//! 3. **Repair Generation**: For invalid paths, generates repair suggestions
//!    (insert/delete/replace) that would make them valid.
//!
//! # Example
//!
//! ```ignore
//! use lling_llang::layers::latex::{LatexSyntaxLayer, LatexGrammar};
//!
//! let grammar = LatexGrammar::standard();
//! let layer = LatexSyntaxLayer::new(grammar);
//!
//! let corrected = layer.apply(&lattice)?;
//! ```

mod grammar;
mod repair;
mod syntax;
mod validator;

pub use grammar::{LatexGrammar, LatexGrammarBuilder, LatexGrammarError};
pub use repair::{RepairKind, RepairStrategy, RepairSuggestion};
pub use syntax::{LatexSyntaxConfig, LatexSyntaxLayer};
pub use validator::{IssueSeverity, LatexValidator, ValidationIssue, ValidationResult};
