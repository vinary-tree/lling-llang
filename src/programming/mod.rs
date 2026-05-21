//! Programming language support for syntax analysis and repair.
//!
//! This module provides:
//! - **Parser Backend Abstraction**: Unified interface for different parsing technologies
//! - **Syntax Error Recovery**: WFST-based repair of syntax errors
//! - **Token Patterns**: Matching and rewriting token sequences
//!
//! ## Parser Backends
//!
//! The `ParserBackend` trait provides a unified interface for integrating different
//! parsing technologies:
//! - Tree-sitter for incremental, error-tolerant parsing
//! - LALRPOP for LR(1) parsing
//! - Pest for PEG parsing
//! - Custom parser implementations
//!
//! ## Example
//!
//! ```rust,ignore
//! use lling_llang::programming::*;
//!
//! // Create a syntax repair transducer
//! let repairer = SyntaxRepairBuilder::new()
//!     .add_rule(SyntaxRepairRule::insert_after("}", ";", 0.5))
//!     .add_rule(SyntaxRepairRule::substitute("funciton", "function", 0.1))
//!     .build();
//!
//! // Repair syntax errors
//! let (repaired, repairs) = repairer.repair("funciton foo() {}");
//! ```

mod api_migration;
mod repair;
mod token;
mod traits;

pub use api_migration::{
    ApiMigrationBuilder, ApiMigrationRule, ApiMigrationTransducer, MigrationResult, MigrationStats,
    MigrationType, Version, VersionRange,
};
pub use repair::{
    RepairAction, RepairCandidate, SyntaxRepairBuilder, SyntaxRepairCosts, SyntaxRepairRule,
    SyntaxRepairTransducer,
};
pub use token::{
    PatternMatcher, ReplacementAction, Token, TokenKind, TokenPattern, TokenPredicate,
    TokenReplacement,
};
pub use traits::{
    NodeKind, ParseResult, ParserBackend, ParserError, Position, Range, SyntaxNode, SyntaxNodeRef,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_imports() {
        // Verify all public types are accessible
        let _: TokenKind = TokenKind::Keyword;
        let _: Position = Position {
            line: 0,
            column: 0,
            byte_offset: 0,
        };
    }
}
